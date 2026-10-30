//! `wglane status`: active country, exit IP, tailnet check, bypass routes.

use std::fmt::Write as _;

use color_eyre::Result;
use wglane_core::ports::{ExitIpProbe, TailnetProbe, Tunnel};
use wglane_core::{CountryCode, TailnetHealth, TunnelStatus};
use wglane_macos::{HelperClient, SystemRunner, TailscalePing};
use wglane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Show country, exit IP, tailnet health and bypass routes",
    long_about = "\
Show the active tunnel, the exit IP the internet sees, whether a tailnet
peer answers, and how many bypass routes are installed.

`protected` is Nord's own verdict on the connection (a public, unauthenticated
lookup; shown as `unknown` when offline). A warning is printed when the tunnel
is up but Nord says it is not protected, or when the exit country differs from
the connected one.

With --json, prints one object for scripts and menu-bar plugins:
  state          \"up\" | \"down\" | \"unknown\" (helper unreadable)
  country        connected country code, e.g. \"ZA\", or null
  exit_ip        public IP, or null if the lookup failed
  protected      true | false | null (unknown)
  tailnet        {ok: bool, peer: string|null, ms: number|null}
  bypass_routes  number of bypass routes installed
Keys are stable; new keys may be added, none removed or renamed.",
    after_help = "\
Examples:
  wglane status
  wglane st --json | jq .protected",
    visible_alias = "st"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, _args: &Args) -> Result<()> {
    let runner = SystemRunner;
    let (status, known) = gather(
        &HelperClient::new(runner),
        &NordApi::new(),
        &TailscalePing::new(runner),
        ctx.recorded_routes(),
    );
    println!("{}", render(&status, known, ctx.json)?);
    Ok(())
}

/// A failing probe is a field value, not a command failure. The flag is false
/// when the helper could not say which tunnel is up (missing helper or sudo rule).
pub(crate) fn gather<T: Tunnel, E: ExitIpProbe, N: TailnetProbe>(
    tunnel: &T,
    exit: &E,
    tailnet: &N,
    bypass_routes: usize,
) -> (TunnelStatus, bool) {
    let exit = exit.exit().ok();
    let active = tunnel.active();
    let known = active.is_ok();
    let active = active.unwrap_or_default();
    let status = TunnelStatus {
        bypass_routes: if active.is_empty() { 0 } else { bypass_routes },
        active,
        exit_ip: exit.as_ref().map(|e| e.ip),
        exit_country: exit.as_ref().and_then(|e| e.country),
        protected: exit.as_ref().and_then(|e| e.protected),
        tailnet: tailnet
            .tailnet()
            .unwrap_or_else(|e| TailnetHealth::Unreachable {
                reason: e.to_string(),
            }),
    };
    (status, known)
}

/// Country of the active tunnel (tunnel names are lower-case codes).
fn connected(s: &TunnelStatus) -> Option<CountryCode> {
    s.active.first().and_then(|n| n.parse().ok())
}

/// The documented `--json` shape; see `long_about`.
fn report(s: &TunnelStatus, known: bool) -> serde_json::Value {
    let (ok, peer, ms) = match &s.tailnet {
        TailnetHealth::Reachable { peer, latency_ms } => {
            (true, Some(peer.as_str()), Some(*latency_ms))
        }
        _ => (false, None, None),
    };
    let state = match (known, s.active.is_empty()) {
        (false, _) => "unknown",
        (true, true) => "down",
        (true, false) => "up",
    };
    serde_json::json!({
        "state": state,
        "country": connected(s),
        "exit_ip": s.exit_ip,
        "protected": s.protected,
        "tailnet": { "ok": ok, "peer": peer, "ms": ms },
        "bypass_routes": s.bypass_routes,
    })
}

/// Leak warnings; only meaningful while a tunnel is up.
fn warnings(s: &TunnelStatus) -> Vec<String> {
    let Some(want) = connected(s) else {
        return vec![];
    };
    let mut w = vec![];
    if s.protected == Some(false) {
        w.push("warning: tunnel is up but Nord says this connection is not protected".to_owned());
    }
    if let Some(got) = s.exit_country.filter(|g| *g != want) {
        w.push(format!("warning: exit country {got}, expected {want}"));
    }
    w
}

fn render(s: &TunnelStatus, known: bool, json: bool) -> Result<String> {
    if json {
        return Ok(serde_json::to_string_pretty(&report(s, known))?);
    }
    let tunnel = if !known {
        "unknown: run wglane doctor".to_owned()
    } else if s.active.is_empty() {
        "none".to_owned()
    } else {
        s.active.join(", ")
    };
    let exit_ip = s.exit_ip.map_or("unknown".to_owned(), |ip| ip.to_string());
    let protected = match s.protected {
        Some(true) => "yes (Nord's view)",
        Some(false) => "no (Nord's view)",
        None => "unknown",
    };
    let tailnet = match &s.tailnet {
        TailnetHealth::Reachable { peer, latency_ms } => format!("ok ({peer}, {latency_ms}ms)"),
        TailnetHealth::Unreachable { reason } => format!("unreachable ({reason})"),
        TailnetHealth::NotInstalled => "tailscale not installed".to_owned(),
        _ => "unknown".to_owned(),
    };
    let mut out = String::new();
    let _ = writeln!(out, "tunnel         {tunnel}");
    let _ = writeln!(out, "exit ip        {exit_ip}");
    let _ = writeln!(out, "protected      {protected}");
    let _ = writeln!(out, "tailnet        {tailnet}");
    let _ = write!(out, "bypass routes  {}", s.bypass_routes);
    for line in warnings(s) {
        let _ = write!(out, "\n{line}");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use wglane_core::ExitInsights;
    use wglane_core::testing::{FakeExitIp, FakeTailnet, FakeTunnel};

    use super::*;

    fn status() -> TunnelStatus {
        let tunnel = FakeTunnel::default();
        tunnel.up.borrow_mut().push("za".into());
        gather(
            &tunnel,
            &FakeExitIp(Some(ExitInsights {
                ip: "192.0.2.7".parse().unwrap(),
                country: Some("ZA".parse().unwrap()),
                protected: Some(true),
            })),
            &FakeTailnet(TailnetHealth::Reachable {
                peer: "peer.example.ts.net".into(),
                latency_ms: 23,
            }),
            3,
        )
        .0
    }

    #[test]
    fn text_and_json_are_stable() {
        insta::assert_snapshot!(render(&status(), true, false).unwrap());
        insta::assert_snapshot!(render(&status(), true, true).unwrap());
    }

    #[test]
    fn down_and_unreachable_render_as_values() {
        let s = TunnelStatus {
            active: vec![],
            exit_ip: None,
            exit_country: None,
            protected: None,
            tailnet: TailnetHealth::NotInstalled,
            bypass_routes: 0,
        };
        insta::assert_snapshot!(render(&s, true, false).unwrap());
        insta::assert_snapshot!(render(&s, true, true).unwrap());
    }

    #[test]
    fn mismatches_warn_and_unknown_does_not() {
        let mut s = status();
        assert_eq!(warnings(&s).len(), 0);
        s.protected = Some(false);
        s.exit_country = Some("DE".parse().unwrap());
        insta::assert_snapshot!(render(&s, true, false).unwrap());
        s.protected = None;
        s.exit_country = None;
        assert_eq!(warnings(&s).len(), 0);
        s.active.clear();
        s.protected = Some(false);
        assert_eq!(warnings(&s).len(), 0, "no tunnel, nothing to leak from");
    }

    #[test]
    fn failed_lookup_is_unknown() {
        let (s, _) = gather(
            &FakeTunnel::default(),
            &FakeExitIp(None),
            &FakeTailnet(TailnetHealth::NotInstalled),
            0,
        );
        assert_eq!((s.exit_ip, s.protected), (None, None));
        assert!(
            render(&s, true, false)
                .unwrap()
                .contains("protected      unknown")
        );
    }

    #[test]
    fn no_tunnel_means_no_bypass_routes() {
        let s = gather(
            &FakeTunnel::default(),
            &FakeExitIp::at("192.0.2.7".parse().unwrap()),
            &FakeTailnet(TailnetHealth::NotInstalled),
            5,
        )
        .0;
        assert_eq!(s.bypass_routes, 0);
    }

    #[test]
    fn an_unreadable_tunnel_is_unknown_not_none() {
        struct Broken;
        impl Tunnel for Broken {
            fn up(&self, _: &std::path::Path) -> wglane_core::Result<()> {
                unreachable!()
            }
            fn down(&self, _: &std::path::Path) -> wglane_core::Result<()> {
                unreachable!()
            }
            fn active(&self) -> wglane_core::Result<Vec<String>> {
                Err(wglane_core::Error::Api("no helper".into()))
            }
        }
        let (s, known) = gather(
            &Broken,
            &FakeExitIp::at("192.0.2.7".parse().unwrap()),
            &FakeTailnet(TailnetHealth::NotInstalled),
            0,
        );
        assert!(!known);
        let text = render(&s, known, false).unwrap();
        assert!(text.contains("unknown: run wglane doctor"), "{text}");
        assert!(!text.contains("none"), "{text}");
    }
}
