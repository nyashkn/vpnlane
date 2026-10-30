//! `wglane watch`: re-apply bypass routes on every network change.

use color_eyre::Result;
use wglane_core::BypassSet;
use wglane_core::ports::{BypassProvider, NetworkWatcher, RouteBypass, Tunnel};
use wglane_macos::{HelperClient, RouteMonitor, SystemRunner, TailscaleBypass};

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Keep bypass routes correct across network changes",
    long_about = "\
Runs in the foreground. On each network change (Wi-Fi switch, new
gateway) it re-reads the physical gateway and the bypass list and
re-installs the routes, so the tailnet survives roaming. Event-driven,
not polling.",
    after_help = "\
Examples:
  wglane watch
  wglane -v watch      log every change"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, _args: &Args) -> Result<()> {
    let static_routes = ActiveBypass(ctx);
    watch_loop(
        &mut RouteMonitor::new(),
        &HelperClient::new(SystemRunner),
        &HelperClient::new(SystemRunner),
        &[&TailscaleBypass::new(SystemRunner), &static_routes],
        |n| ctx.record_routes(n),
    )
}

/// The `bypass_extra` the last `up` applied (base plus preset), read afresh
/// on every change so a connect made after `watch` started is honored.
struct ActiveBypass<'a>(&'a Context);

impl BypassProvider for ActiveBypass<'_> {
    fn collect(&self) -> wglane_core::Result<BypassSet> {
        self.0
            .applied_bypass()
            .map(|set| set.0)
            .map_err(|e| wglane_core::Error::Config(e.to_string()))
    }
}

/// Re-applies the bypass routes after each network change while a tunnel is
/// up. Ends only when the watcher fails; a failed re-apply is logged and the
/// next change retries, because roaming leaves the network briefly gatewayless.
fn watch_loop<W, T, R>(
    watcher: &mut W,
    tunnel: &T,
    routes: &R,
    bypass: &[&dyn BypassProvider],
    record: impl Fn(usize) -> Result<()>,
) -> Result<()>
where
    W: NetworkWatcher,
    T: Tunnel,
    R: RouteBypass,
{
    loop {
        watcher.wait_for_change()?;
        match reapply(tunnel, routes, bypass) {
            Ok(Some(n)) => {
                tracing::info!("network changed: re-applied {n} bypass routes");
                // status reads this copy; a failed write only leaves the count stale.
                if let Err(e) = record(n) {
                    tracing::warn!("could not record the bypass route count: {e}");
                }
            }
            Ok(None) => tracing::debug!("network changed: no tunnel up, nothing to do"),
            Err(e) => tracing::warn!("network changed: could not re-apply bypass routes: {e}"),
        }
    }
}

/// `Some(count)` when routes were re-applied, `None` when no tunnel is active.
fn reapply<T: Tunnel, R: RouteBypass>(
    tunnel: &T,
    routes: &R,
    bypass: &[&dyn BypassProvider],
) -> wglane_core::Result<Option<usize>> {
    if tunnel.active()?.is_empty() {
        return Ok(None);
    }
    let gateway = routes.physical_gateway()?;
    let mut set = BypassSet::new();
    for p in bypass {
        set.merge(p.collect()?);
    }
    super::up::check_limit(&set)?;
    routes.apply(&set, gateway).map(Some)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use wglane_core::testing::{FakeBypassProvider, FakeRoutes, FakeTunnel, FakeWatcher};

    use super::*;

    fn set() -> wglane_core::BypassSet {
        ["203.0.113.5/32".parse().unwrap()].into_iter().collect()
    }

    #[test]
    fn reapplies_on_every_change_while_tunnel_is_up() {
        let tunnel = FakeTunnel::default();
        tunnel.up(Path::new("us.conf")).unwrap();
        let routes = FakeRoutes::new("192.168.1.1".parse().unwrap());
        let mut watcher = FakeWatcher { changes: 2 };
        let recorded = std::cell::RefCell::new(Vec::new());

        let err = watch_loop(
            &mut watcher,
            &tunnel,
            &routes,
            &[&FakeBypassProvider(set())],
            |n| {
                recorded.borrow_mut().push(n);
                Ok(())
            },
        );

        assert!(err.is_err(), "loop ends when the watcher is exhausted");
        assert_eq!(
            *recorded.borrow(),
            [1, 1],
            "count recorded after each re-apply"
        );
        assert_eq!(routes.apply_calls.get(), 2, "one apply per change");
        assert_eq!(routes.installed.borrow().len(), 1);
        assert_eq!(routes.installed.borrow()[0].1.to_string(), "192.168.1.1");
    }

    /// Regression (#29): routes of the applied preset survive a network change.
    /// They now come from the state file's last request, not from the config.
    #[test]
    fn preset_routes_survive_a_refresh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "bypass_extra = [\"203.0.113.0/24\"]\n[presets.work]\nbypass_extra = [\"198.51.100.0/24\"]\n",
        )
        .unwrap();
        let ctx = Context {
            json: false,
            verbose: 0,
            config_dir: dir.path().to_owned(),
        };
        let tunnel = FakeTunnel::default();
        tunnel.up(Path::new("us.conf")).unwrap();
        let routes = FakeRoutes::new("192.168.1.1".parse().unwrap());

        // Nothing applied yet: the top-level set.
        let n = reapply(&tunnel, &routes, &[&ActiveBypass(&ctx)]).unwrap();
        assert_eq!(n, Some(1));

        // `up --preset work` recorded both routes; the refresh re-applies both.
        ctx.record_applied(wglane_core::LastRequest {
            location: Some("US".parse().unwrap()),
            pick: wglane_core::Pick::LeastLoad,
            bypass_extra: vec![
                "203.0.113.0/24".parse().unwrap(),
                "198.51.100.0/24".parse().unwrap(),
            ],
            preset: Some(wglane_core::PresetUse {
                name: "work".into(),
                overridden: vec![],
            }),
            at: 1,
        })
        .unwrap();
        let n = reapply(&tunnel, &routes, &[&ActiveBypass(&ctx)]).unwrap();
        assert_eq!(n, Some(2), "top-level and preset route both re-applied");

        // The set is the applied one, even if the preset has since left the config.
        std::fs::write(dir.path().join("config.toml"), "").unwrap();
        let n = reapply(&tunnel, &routes, &[&ActiveBypass(&ctx)]).unwrap();
        assert_eq!(n, Some(2));
    }

    #[test]
    fn failed_reapply_does_not_end_the_loop() {
        let tunnel = FakeTunnel::default();
        tunnel.up(Path::new("us.conf")).unwrap();
        let routes = FakeRoutes::new("192.168.1.1".parse().unwrap());
        routes.fail_applies.set(1);
        let recorded = std::cell::RefCell::new(Vec::new());

        let _ = watch_loop(
            &mut FakeWatcher { changes: 2 },
            &tunnel,
            &routes,
            &[&FakeBypassProvider(set())],
            |n| {
                recorded.borrow_mut().push(n);
                Ok(())
            },
        );

        assert_eq!(
            routes.apply_calls.get(),
            2,
            "the change after the failure is handled"
        );
        assert_eq!(
            *recorded.borrow(),
            [1],
            "only the successful apply is recorded"
        );
    }

    #[test]
    fn does_nothing_without_a_tunnel() {
        let routes = FakeRoutes::new("192.168.1.1".parse().unwrap());
        let mut watcher = FakeWatcher { changes: 3 };

        let _ = watch_loop(
            &mut watcher,
            &FakeTunnel::default(),
            &routes,
            &[&FakeBypassProvider(set())],
            |_| panic!("nothing re-applied, nothing to record"),
        );

        assert!(routes.installed.borrow().is_empty());
    }
}
