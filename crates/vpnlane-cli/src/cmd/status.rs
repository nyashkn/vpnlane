//! `vpnlane status`: active country, exit IP, tailnet check, bypass routes.

use std::fmt::Write as _;

use color_eyre::Result;
use vpnlane_core::ports::{ExitIpProbe, TailnetProbe, Tunnel};
use vpnlane_core::{CountryCode, Overrides, PresetUse, TailnetHealth, TunnelStatus};
use vpnlane_macos::{HelperClient, SystemRunner, TailscalePing};
use vpnlane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Show country, exit IP, tailnet health and bypass routes",
    long_about = concat!("\
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
  leak_check     \"ok\" (exit IP looked up) | \"disabled\" (exit_ip_check =
                 false) | \"unavailable\" (enabled, but the lookup failed)
  preset         name of the preset the connection was applied from; present
                 only while a tunnel is up and a preset is active
  preset_overridden
                 keys changed after applying it (\"location\", \"pick\"); present
                 only when there are any
Keys are stable; new keys may be added, none removed or renamed.

The leak check is the exit IP lookup behind `protected` and the exit-country
warning. `exit_ip_check = false` (or ", brand_env!("EXIT_IP_CHECK"), "=false) turns it off;
status then prints a `leak check  disabled` line every time, and `", brand_name!(), "
doctor` warns."),
    after_help = concat!("\
Examples:
  ", brand_name!(), " status
  ", brand_name!(), " st --json | jq .protected
  ", brand_name!(), " status --exit-ip-check false   skip the exit ip lookup"),
    visible_alias = "st"
)]
pub(crate) struct Args {
    #[arg(
        long,
        value_name = "BOOL",
        help = concat!(
            "Look up the exit IP (true or false). Default: `exit_ip_check` in `config.toml` or `",
            brand_env!("EXIT_IP_CHECK"),
            "`, else true"
        )
    )]
    exit_ip_check: Option<bool>,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let runner = SystemRunner;
    let flags = Overrides {
        exit_ip_check: args.exit_ip_check,
        ..Overrides::default()
    };
    let exit_ip_check = ctx.settings(flags)?.exit_ip_check.value;
    let (status, known) = gather(
        &HelperClient::new(runner),
        &NordApi::new(),
        &TailscalePing::new(runner),
        ctx.recorded_routes(),
        exit_ip_check,
    );
    let preset = ctx.active_preset().filter(|_| !status.active.is_empty());
    println!(
        "{}",
        render(&status, known, ctx.json, preset.as_ref(), exit_ip_check)?
    );
    Ok(())
}

/// A failing probe is a field value, not a command failure. The flag is false
/// when the helper could not say which tunnel is up (missing helper or sudo rule).
/// With `exit_ip_check` off the exit ip is not looked up and reads as unknown.
pub(crate) fn gather<T: Tunnel, E: ExitIpProbe, N: TailnetProbe>(
    tunnel: &T,
    exit: &E,
    tailnet: &N,
    bypass_routes: usize,
    exit_ip_check: bool,
) -> (TunnelStatus, bool) {
    let exit = exit_ip_check.then(|| exit.exit().ok()).flatten();
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

/// What the leak check (the exit IP lookup) did.
fn leak_check(s: &TunnelStatus, exit_ip_check: bool) -> &'static str {
    match (exit_ip_check, s.exit_ip) {
        (false, _) => "disabled",
        (true, Some(_)) => "ok",
        (true, None) => "unavailable",
    }
}

/// The documented `--json` shape; see `long_about`.
fn report(
    s: &TunnelStatus,
    known: bool,
    preset: Option<&PresetUse>,
    exit_ip_check: bool,
) -> serde_json::Value {
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
    let mut v = serde_json::json!({
        "state": state,
        "country": connected(s),
        "exit_ip": s.exit_ip,
        "protected": s.protected,
        "tailnet": { "ok": ok, "peer": peer, "ms": ms },
        "bypass_routes": s.bypass_routes,
        "leak_check": leak_check(s, exit_ip_check),
    });
    if let Some(p) = preset {
        v["preset"] = p.name.as_str().into();
        if !p.overridden.is_empty() {
            v["preset_overridden"] = p.overridden.clone().into();
        }
    }
    v
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

fn render(
    s: &TunnelStatus,
    known: bool,
    json: bool,
    preset: Option<&PresetUse>,
    exit_ip_check: bool,
) -> Result<String> {
    if json {
        return Ok(serde_json::to_string_pretty(&report(
            s,
            known,
            preset,
            exit_ip_check,
        ))?);
    }
    let tunnel = if !known {
        concat!("unknown: run ", brand_name!(), " doctor").to_owned()
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
    if let Some(p) = preset {
        let _ = writeln!(out, "preset         {}", p.describe());
    }
    let _ = writeln!(out, "exit ip        {exit_ip}");
    let _ = writeln!(out, "protected      {protected}");
    if !exit_ip_check {
        let _ = writeln!(out, "leak check     disabled (exit_ip_check = false)");
    }
    let _ = writeln!(out, "tailnet        {tailnet}");
    let _ = write!(out, "bypass routes  {}", s.bypass_routes);
    for line in warnings(s) {
        let _ = write!(out, "\n{line}");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use vpnlane_core::ExitInsights;
    use vpnlane_core::testing::{FakeExitIp, FakeTailnet, FakeTunnel};

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
            true,
        )
        .0
    }

    fn work() -> PresetUse {
        PresetUse {
            name: "work".into(),
            overridden: vec![],
        }
    }

    #[test]
    fn text_and_json_are_stable() {
        insta::assert_snapshot!(render(&status(), true, false, None, true).unwrap());
        insta::assert_snapshot!(render(&status(), true, true, None, true).unwrap());
    }

    #[test]
    fn active_preset_is_an_additive_key() {
        let s = status();
        let json = render(&s, true, true, Some(&work()), true).unwrap();
        assert!(json.contains("\"preset\": \"work\""), "{json}");
        assert!(
            !render(&s, true, true, None, true)
                .unwrap()
                .contains("preset")
        );
        let text = render(&s, true, false, Some(&work()), true).unwrap();
        assert!(text.contains("preset         work"), "{text}");
    }

    #[test]
    fn an_overridden_preset_says_so_in_text_and_json() {
        let s = status();
        let p = PresetUse {
            name: "work".into(),
            overridden: vec!["location".into()],
        };
        let text = render(&s, true, false, Some(&p), true).unwrap();
        assert!(
            text.contains("preset         work (location overridden)"),
            "{text}"
        );
        let json = render(&s, true, true, Some(&p), true).unwrap();
        assert!(json.contains("\"preset\": \"work\""), "{json}");
        assert!(json.contains("\"preset_overridden\""), "{json}");
        assert!(
            !render(&s, true, true, Some(&work()), true)
                .unwrap()
                .contains("preset_overridden")
        );
    }

    /// `exit_ip_check = false` is allowed but never silent.
    #[test]
    fn a_disabled_leak_check_is_loud_in_text_and_json() {
        let tunnel = FakeTunnel::default();
        tunnel.up.borrow_mut().push("za".into());
        let gather_with = |check| {
            gather(
                &tunnel,
                &FakeExitIp::at("192.0.2.7".parse().unwrap()),
                &FakeTailnet(TailnetHealth::NotInstalled),
                1,
                check,
            )
            .0
        };
        let off = gather_with(false);
        let text = render(&off, true, false, None, false).unwrap();
        insta::assert_snapshot!("leak_disabled_text", text);
        let json = render(&off, true, true, None, false).unwrap();
        assert!(json.contains("\"leak_check\": \"disabled\""), "{json}");
        // `protected` stays null: with the check off it is unknown, as before.
        assert!(json.contains("\"protected\": null"), "{json}");
        let on = gather_with(true);
        let text = render(&on, true, false, None, true).unwrap();
        assert!(!text.contains("leak check"), "{text}");
        assert!(
            render(&on, true, true, None, true)
                .unwrap()
                .contains("\"leak_check\": \"ok\"")
        );
        // Enabled but the lookup failed.
        let (broken, _) = gather(
            &tunnel,
            &FakeExitIp(None),
            &FakeTailnet(TailnetHealth::NotInstalled),
            0,
            true,
        );
        assert!(
            render(&broken, true, true, None, true)
                .unwrap()
                .contains("\"leak_check\": \"unavailable\"")
        );
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
        insta::assert_snapshot!(render(&s, true, false, None, true).unwrap());
        insta::assert_snapshot!(render(&s, true, true, None, true).unwrap());
    }

    #[test]
    fn mismatches_warn_and_unknown_does_not() {
        let mut s = status();
        assert_eq!(warnings(&s).len(), 0);
        s.protected = Some(false);
        s.exit_country = Some("DE".parse().unwrap());
        insta::assert_snapshot!(render(&s, true, false, None, true).unwrap());
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
            true,
        );
        assert_eq!((s.exit_ip, s.protected), (None, None));
        assert!(
            render(&s, true, false, None, true)
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
            true,
        )
        .0;
        assert_eq!(s.bypass_routes, 0);
    }

    #[test]
    fn an_unreadable_tunnel_is_unknown_not_none() {
        struct Broken;
        impl Tunnel for Broken {
            fn up(&self, _: &std::path::Path) -> vpnlane_core::Result<()> {
                unreachable!()
            }
            fn down(&self, _: &std::path::Path) -> vpnlane_core::Result<()> {
                unreachable!()
            }
            fn active(&self) -> vpnlane_core::Result<Vec<String>> {
                Err(vpnlane_core::Error::Api("no helper".into()))
            }
        }
        let (s, known) = gather(
            &Broken,
            &FakeExitIp::at("192.0.2.7".parse().unwrap()),
            &FakeTailnet(TailnetHealth::NotInstalled),
            0,
            true,
        );
        assert!(!known);
        let text = render(&s, known, false, None, true).unwrap();
        assert!(text.contains("unknown: run vpnlane doctor"), "{text}");
        assert!(!text.contains("none"), "{text}");
    }

    #[test]
    fn exit_ip_check_off_skips_the_lookup() {
        let tunnel = FakeTunnel::default();
        let [on, off] = [true, false].map(|check| {
            gather(
                &tunnel,
                &FakeExitIp::at("192.0.2.7".parse().unwrap()),
                &FakeTailnet(TailnetHealth::NotInstalled),
                0,
                check,
            )
            .0
            .exit_ip
        });
        assert_eq!((on.is_some(), off), (true, None));
    }
}
