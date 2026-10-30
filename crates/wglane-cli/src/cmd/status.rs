//! `wglane status`: active country, exit IP, tailnet check, bypass routes.

use std::fmt::Write as _;
use std::net::IpAddr;

use color_eyre::Result;
use wglane_core::ports::{ExitIpProbe, RouteBypass, TailnetProbe, Tunnel};
use wglane_core::{TailnetHealth, TunnelStatus};
use wglane_macos::{CommandRunner, RouteTable, SystemRunner, TailscalePing, WgQuick};
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
    let status = gather(
        &WgQuick::new(runner),
        &NordApi::new(),
        &TailscalePing::new(runner),
        RouteTable::new(runner)
            .physical_gateway()
            .map_or(0, |gw| bypass_route_count(&runner, gw)),
    );
    println!("{}", render(&status, ctx.json)?);
    Ok(())
}

/// A failing probe is a field value, not a command failure.
fn gather<T: Tunnel, E: ExitIpProbe, N: TailnetProbe>(
    tunnel: &T,
    exit: &E,
    tailnet: &N,
    bypass_routes: usize,
) -> TunnelStatus {
    TunnelStatus {
        active: tunnel.active().unwrap_or_default(),
        exit_ip: exit.exit_ip().ok(),
        tailnet: tailnet
            .tailnet()
            .unwrap_or_else(|e| TailnetHealth::Unreachable {
                reason: e.to_string(),
            }),
        bypass_routes,
    }
}

/// IPv4 routes (other than `default`) whose next hop is the physical gateway.
/// 0 when the routing table cannot be read.
// shortcut: counts every route via the gateway, not only wglane's; exact once the helper tags its routes.
fn bypass_route_count<R: CommandRunner>(runner: &R, gw: IpAddr) -> usize {
    let Ok(out) = runner.run_ok("netstat", &["-rn", "-f", "inet"]) else {
        return 0;
    };
    let gw = gw.to_string();
    out.stdout
        .lines()
        .filter(|l| {
            let mut f = l.split_whitespace();
            f.next().is_some_and(|d| d != "default") && f.next() == Some(gw.as_str())
        })
        .count()
}

fn render(s: &TunnelStatus, json: bool) -> Result<String> {
    if json {
        return Ok(serde_json::to_string_pretty(s)?);
    }
    let tunnel = if s.active.is_empty() {
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
    use wglane_macos::testing::FakeRunner;

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
    }

    #[test]
    fn text_and_json_are_stable() {
        insta::assert_snapshot!(render(&status(), false).unwrap());
        insta::assert_snapshot!(render(&status(), true).unwrap());
    }

    #[test]
    fn down_and_unreachable_render_as_values() {
        let s = TunnelStatus {
            active: vec![],
            exit_ip: None,
            tailnet: TailnetHealth::NotInstalled,
            bypass_routes: 0,
        };
        insta::assert_snapshot!(render(&s, false).unwrap());
        insta::assert_snapshot!(render(&s, true).unwrap());
    }

    #[test]
    fn counts_non_default_routes_via_physical_gateway() {
        let netstat = "\
Destination        Gateway            Flags               Netif Expire
default            192.168.1.1        UGScg                 en0
default            link#25            UCSIg               utun4
198.51.100.9/32    192.168.1.1        UGSc                  en0
203.0.113.0/24     192.168.1.1        UGSc                  en0
192.168.1/24       link#6             UCS                   en0
";
        let r = FakeRunner::new();
        r.push_stdout(netstat);
        assert_eq!(bypass_route_count(&r, "192.168.1.1".parse().unwrap()), 2);
    }
}
