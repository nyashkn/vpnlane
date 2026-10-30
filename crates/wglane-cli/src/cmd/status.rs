//! `wglane status`: active country, exit IP, tailnet check, bypass routes.

use std::fmt::Write as _;

use color_eyre::Result;
use wglane_core::ports::{ExitIpProbe, TailnetProbe, Tunnel};
use wglane_core::{TailnetHealth, TunnelStatus};
use wglane_macos::{HelperClient, SystemRunner, TailscalePing};
use wglane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Show country, exit IP, tailnet health and bypass routes",
    long_about = "\
Show the active tunnel, the exit IP the internet sees, whether a tailnet
peer answers, and how many bypass routes are installed.

With --json, prints a TunnelStatus object for scripts and status bars.",
    after_help = "\
Examples:
  wglane status
  wglane st --json | jq .exit_ip",
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
    let active = tunnel.active();
    let known = active.is_ok();
    let active = active.unwrap_or_default();
    let status = TunnelStatus {
        bypass_routes: if active.is_empty() { 0 } else { bypass_routes },
        active,
        exit_ip: exit.exit_ip().ok(),
        tailnet: tailnet
            .tailnet()
            .unwrap_or_else(|e| TailnetHealth::Unreachable {
                reason: e.to_string(),
            }),
    };
    (status, known)
}

fn render(s: &TunnelStatus, known: bool, json: bool) -> Result<String> {
    if json {
        return Ok(serde_json::to_string_pretty(s)?);
    }
    let tunnel = if !known {
        "unknown: run wglane doctor".to_owned()
    } else if s.active.is_empty() {
        "none".to_owned()
    } else {
        s.active.join(", ")
    };
    let exit_ip = s.exit_ip.map_or("unknown".to_owned(), |ip| ip.to_string());
    let tailnet = match &s.tailnet {
        TailnetHealth::Reachable { peer, latency_ms } => format!("ok ({peer}, {latency_ms}ms)"),
        TailnetHealth::Unreachable { reason } => format!("unreachable ({reason})"),
        TailnetHealth::NotInstalled => "tailscale not installed".to_owned(),
        _ => "unknown".to_owned(),
    };
    let mut out = String::new();
    let _ = writeln!(out, "tunnel         {tunnel}");
    let _ = writeln!(out, "exit ip        {exit_ip}");
    let _ = writeln!(out, "tailnet        {tailnet}");
    let _ = write!(out, "bypass routes  {}", s.bypass_routes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use wglane_core::testing::{FakeExitIp, FakeTailnet, FakeTunnel};

    use super::*;

    fn status() -> TunnelStatus {
        let tunnel = FakeTunnel::default();
        tunnel.up.borrow_mut().push("za".into());
        gather(
            &tunnel,
            &FakeExitIp("192.0.2.7".parse().unwrap()),
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
            tailnet: TailnetHealth::NotInstalled,
            bypass_routes: 0,
        };
        insta::assert_snapshot!(render(&s, true, false).unwrap());
        insta::assert_snapshot!(render(&s, true, true).unwrap());
    }

    #[test]
    fn no_tunnel_means_no_bypass_routes() {
        let s = gather(
            &FakeTunnel::default(),
            &FakeExitIp("192.0.2.7".parse().unwrap()),
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
            &FakeExitIp("192.0.2.7".parse().unwrap()),
            &FakeTailnet(TailnetHealth::NotInstalled),
            0,
        );
        assert!(!known);
        let text = render(&s, known, false).unwrap();
        assert!(text.contains("unknown: run wglane doctor"), "{text}");
        assert!(!text.contains("none"), "{text}");
    }
}
