//! `wglane up <CC>`: connect, regenerating the config if missing or stale.

use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use wglane_core::ports::{BypassProvider, RouteBypass, ServerCatalog, TokenSource, Tunnel};
use wglane_core::{
    BypassSet, CountryCode, Error, MAX_BYPASS, Overrides, generate_config, is_stale, render_wg_conf,
};
use wglane_macos::{HelperClient, SystemRunner, TailscaleBypass};
use wglane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Connect to a country (switches if already connected)",
    long_about = "\
Connect to the least-loaded NordLynx server in COUNTRY.

If the config for COUNTRY is missing or older than 7 days it is regenerated
first (no separate `gen` step). An existing wglane tunnel is brought down.

Why Tailscale breaks, and what wglane does:
  A full tunnel (AllowedIPs 0.0.0.0/0) also captures Tailscale's underlay:
  the control server and DERP relays. netcheck then reports UDP false and the
  tailnet dies, although 100.64.0.0/10 still routes. wglane adds host routes
  for those IPs via your physical gateway, so the tailnet keeps working and
  everything else exits through the VPN. The config has no DNS= line, so
  MagicDNS keeps working too.",
    after_help = "\
Examples:
  wglane up ZA          South Africa
  wglane up us          case does not matter
  wglane up             use default_country from config.toml (or WGLANE_COUNTRY)
  wglane -v up DE       show each step"
)]
pub(crate) struct Args {
    /// Two-letter country code, e.g. US, DE, ZA (not UK: use GB).
    /// Default: `default_country` from `config.toml` or `WGLANE_COUNTRY`.
    pub(crate) country: Option<CountryCode>,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let flags = Overrides {
        country: args.country,
        ..Overrides::default()
    };
    let cc = ctx.settings(flags)?.default_country.value.ok_or_else(|| {
        eyre!(
            "no country given: run `wglane up ZA`, or set default_country in {} (see `wglane config init`)",
            ctx.config_file().display()
        )
    })?;
    println!("{}", connect_real(ctx, cc)?.summary(cc));
    Ok(())
}

/// `up` against the real adapters. Prints nothing, so the TUI can call it.
pub(crate) fn connect_real(ctx: &Context, cc: CountryCode) -> Result<Connected> {
    let helper = HelperClient::new(SystemRunner);
    let tailscale = TailscaleBypass::new(SystemRunner);
    let static_routes = ctx.static_bypass()?;
    connect_recorded(
        ctx,
        &helper,
        &helper,
        &[&tailscale, &static_routes],
        &NordApi::new(),
        &ctx.token_source(),
        cc,
    )
}

/// The helper takes at most [`MAX_BYPASS`] routes; say which setting to trim.
pub(crate) fn check_limit(set: &BypassSet) -> wglane_core::Result<()> {
    if set.len() > MAX_BYPASS {
        return Err(Error::Config(format!(
            "{} bypass routes, the limit is {MAX_BYPASS}: shorten bypass_extra in config.toml",
            set.len()
        )));
    }
    Ok(())
}

/// `connect` plus the config store and the route-count record `status` reads.
pub(crate) fn connect_recorded<T: Tunnel, R: RouteBypass, C: ServerCatalog, S: TokenSource>(
    ctx: &Context,
    tunnel: &T,
    routes: &R,
    providers: &[&dyn BypassProvider],
    catalog: &C,
    token: &S,
    cc: CountryCode,
) -> Result<Connected> {
    let store = ctx.conf_store();
    let done = connect(
        tunnel,
        routes,
        providers,
        cc,
        store.dir(),
        store.age(cc)?,
        || regenerate(catalog, token, cc, |body| Ok(store.write(cc, body)?)),
    )?;
    ctx.record_routes(done.routes)?;
    Ok(done)
}

/// What `connect` did, for the one-line report.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Connected {
    regenerated: bool,
    /// Other tunnels taken down first.
    switched_from: Vec<String>,
    /// The tunnel for this country was already up and kept.
    already_up: bool,
    pub(crate) routes: usize,
}

impl Connected {
    pub(crate) fn summary(&self, cc: CountryCode) -> String {
        let verb = if self.already_up {
            "already connected to"
        } else if self.switched_from.is_empty() {
            "connected to"
        } else {
            "switched to"
        };
        let regen = if self.regenerated {
            ", config regenerated"
        } else {
            ""
        };
        format!("{verb} {cc}{regen}, {} bypass routes", self.routes)
    }
}

/// Fetches a fresh config for `cc` and hands the rendered body to `write`.
pub(crate) fn regenerate<C: ServerCatalog, T: TokenSource>(
    catalog: &C,
    token: &T,
    cc: CountryCode,
    write: impl FnOnce(&str) -> Result<PathBuf>,
) -> Result<PathBuf> {
    write(&render_wg_conf(&generate_config(catalog, token, cc)?))
}

/// The whole `up` flow against ports. `conf_age` is `None` when the config is
/// missing; `regen` writes a new config and returns its path.
pub(crate) fn connect<T: Tunnel, R: RouteBypass>(
    tunnel: &T,
    routes: &R,
    providers: &[&dyn BypassProvider],
    cc: CountryCode,
    conf_dir: &Path,
    conf_age: Option<Duration>,
    regen: impl FnOnce() -> Result<PathBuf>,
) -> Result<Connected> {
    let regenerated = is_stale(conf_age);
    let conf = if regenerated {
        regen()?
    } else {
        conf_dir.join(format!("{}.conf", cc.to_lowercase()))
    };
    let name = cc.to_lowercase();

    let mut switched_from = Vec::new();
    let mut already_up = false;
    for active in tunnel.active()? {
        // A regenerated config must replace the running tunnel, not keep it.
        if active == name && !regenerated {
            already_up = true;
        } else {
            tunnel.down(&conf_dir.join(format!("{active}.conf")))?;
            if active != name {
                switched_from.push(active);
            }
        }
    }

    // Gateway first: once the tunnel is up, the default route is the tunnel.
    let gateway = routes.physical_gateway()?;
    if !already_up {
        tunnel.up(&conf)?;
    }
    let applied = (|| {
        let mut set = BypassSet::new();
        for p in providers {
            set.merge(p.collect()?);
        }
        check_limit(&set)?;
        routes.apply(&set, gateway)
    })();
    let routes = match applied {
        Ok(n) => n,
        Err(e) => {
            // A full tunnel without its bypass routes kills the tailnet.
            return Err(match tunnel.down(&conf) {
                Ok(()) => eyre!("tunnel {name} was removed because bypass routes failed: {e}"),
                Err(d) => {
                    eyre!("bypass routes failed ({e}) and tunnel {name} could not be removed: {d}")
                }
            });
        }
    };
    Ok(Connected {
        regenerated,
        switched_from,
        already_up,
        routes,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::net::IpAddr;

    use wglane_core::testing::{
        FakeBypassProvider, FakeCatalog, FakeRoutes, FakeToken, FakeTunnel,
    };
    use wglane_core::{Error, MAX_CONF_AGE, PublicKey, Server};

    use super::*;

    const DIR: &str = "/cfg";

    fn cc(s: &str) -> CountryCode {
        s.parse().unwrap()
    }

    fn catalog() -> FakeCatalog {
        FakeCatalog {
            servers: [(
                cc("ZA"),
                vec![Server {
                    hostname: "za3.nordvpn.com".into(),
                    station: "192.0.2.2".parse().unwrap(),
                    load: 9,
                    public_key: PublicKey("pk".into()),
                    country: cc("ZA"),
                }],
            )]
            .into(),
            private_key: Some("priv".into()),
            ..Default::default()
        }
    }

    fn bypass() -> FakeBypassProvider {
        FakeBypassProvider(["198.51.100.1/32".parse().unwrap()].into_iter().collect())
    }

    struct Rig {
        tunnel: FakeTunnel,
        routes: FakeRoutes,
        writes: Cell<u32>,
    }

    impl Rig {
        fn new(up: &[&str]) -> Self {
            let tunnel = FakeTunnel::default();
            tunnel
                .up
                .borrow_mut()
                .extend(up.iter().map(|s| (*s).to_owned()));
            Self {
                tunnel,
                routes: FakeRoutes::new("192.0.2.254".parse::<IpAddr>().unwrap()),
                writes: Cell::new(0),
            }
        }

        fn up(&self, to: &str, age: Option<Duration>, token: Option<&str>) -> Result<Connected> {
            connect(
                &self.tunnel,
                &self.routes,
                &[&bypass()],
                cc(to),
                Path::new(DIR),
                age,
                || {
                    regenerate(
                        &catalog(),
                        &FakeToken(token.map(Into::into)),
                        cc(to),
                        |body| {
                            assert!(body.contains("PrivateKey = priv"));
                            self.writes.set(self.writes.get() + 1);
                            Ok(Path::new(DIR).join(format!("{}.conf", to.to_lowercase())))
                        },
                    )
                },
            )
        }
    }

    const FRESH: Option<Duration> = Some(Duration::from_secs(60));

    #[test]
    fn missing_conf_is_generated_then_connected() {
        let rig = Rig::new(&[]);
        let done = rig.up("ZA", None, Some("tok")).unwrap();
        assert_eq!(rig.writes.get(), 1);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert_eq!(rig.routes.installed.borrow().len(), 1);
        assert_eq!(
            done.summary(cc("ZA")),
            "connected to ZA, config regenerated, 1 bypass routes"
        );
    }

    #[test]
    fn stale_conf_is_regenerated() {
        let rig = Rig::new(&[]);
        rig.up(
            "ZA",
            Some(MAX_CONF_AGE + Duration::from_secs(1)),
            Some("tok"),
        )
        .unwrap();
        assert_eq!(rig.writes.get(), 1);
    }

    #[test]
    fn fresh_conf_is_reused_without_a_token() {
        let rig = Rig::new(&[]);
        let done = rig.up("ZA", FRESH, None).unwrap();
        assert_eq!(rig.writes.get(), 0);
        assert!(!done.regenerated);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
    }

    #[test]
    fn already_connected_keeps_tunnel_and_refreshes_routes() {
        let rig = Rig::new(&["za"]);
        let done = rig.up("ZA", FRESH, None).unwrap();
        assert!(done.already_up);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert_eq!(rig.routes.installed.borrow().len(), 1);
    }

    #[test]
    fn stale_while_connected_bounces_the_tunnel() {
        let rig = Rig::new(&["za"]);
        let done = rig.up("ZA", None, Some("tok")).unwrap();
        assert!(!done.already_up);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
    }

    #[test]
    fn switching_country_downs_the_old_tunnel() {
        let rig = Rig::new(&["us"]);
        let done = rig.up("ZA", None, Some("tok")).unwrap();
        assert_eq!(done.switched_from, ["us"]);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert!(done.summary(cc("ZA")).starts_with("switched to ZA"));
    }

    #[test]
    fn missing_token_fails_before_touching_the_tunnel() {
        let rig = Rig::new(&["us"]);
        let err = rig.up("ZA", None, None).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<Error>(),
            Some(Error::TokenMissing { .. })
        ));
        assert_eq!(*rig.tunnel.up.borrow(), ["us"]);
        assert!(rig.routes.installed.borrow().is_empty());
    }

    #[test]
    fn gateway_is_read_before_the_tunnel_comes_up() {
        let rig = Rig::new(&[]);
        let before: IpAddr = "192.0.2.254".parse().unwrap();
        let gw = rig.routes.gateway.clone();
        // Once the tunnel is up the default route is the tunnel.
        rig.tunnel
            .on_up
            .borrow_mut()
            .push(Box::new(move || gw.set("10.5.0.1".parse().unwrap())));
        rig.up("ZA", FRESH, None).unwrap();
        assert_eq!(rig.routes.installed.borrow()[0].1, before);
    }

    #[test]
    fn failed_bypass_removes_the_tunnel() {
        let rig = Rig::new(&[]);
        rig.routes.fail_applies.set(1);
        let err = rig.up("ZA", FRESH, None).unwrap_err().to_string();
        assert!(err.contains("tunnel za was removed because bypass routes failed"));
        assert!(rig.tunnel.up.borrow().is_empty());
    }
}
