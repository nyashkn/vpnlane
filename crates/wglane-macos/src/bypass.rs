use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

use ipnet::{IpNet, Ipv4Net};
use serde_json::Value;
use wglane_core::ports::BypassProvider;
use wglane_core::{BypassSet, Result};

use crate::CommandRunner;

/// Tailscale/Headscale underlay: the control server and every DERP relay.
///
/// Without host routes for these the tailnet dies under a full tunnel
/// (netcheck reports UDP false), even though 100.64.0.0/10 still routes.
#[derive(Debug)]
pub struct TailscaleBypass<R> {
    runner: R,
}

impl<R: CommandRunner> TailscaleBypass<R> {
    /// Uses `runner` to call the `tailscale` CLI.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Stdout of `tailscale debug <what>` as JSON; `None` on any failure.
    fn debug_json(&self, what: &str) -> Option<Value> {
        let out = self.runner.run("tailscale", &["debug", what]).ok()?;
        out.success()
            .then(|| serde_json::from_str(&out.stdout).ok())?
    }
}

impl<R: CommandRunner> BypassProvider for TailscaleBypass<R> {
    /// IPv4 only, like the reference script. Anything that fails (no
    /// `tailscale`, bad JSON, DNS) is skipped so the rest still lands.
    fn collect(&self) -> Result<BypassSet> {
        let mut set = BypassSet::new();
        if let Some(prefs) = self.debug_json("prefs") {
            let control = prefs["ControlURL"].as_str().and_then(url_host);
            let resolved = control.and_then(|h| (h, 443).to_socket_addrs().ok());
            for addr in resolved.into_iter().flatten() {
                if let IpAddr::V4(ip) = addr.ip() {
                    set.insert(host_route(ip));
                }
            }
        }
        if let Some(map) = self.debug_json("derp-map") {
            let nodes = map["Regions"]
                .as_object()
                .into_iter()
                .flat_map(|regions| regions.values())
                .flat_map(|region| region["Nodes"].as_array().into_iter().flatten());
            for ip in nodes.filter_map(|n| n["IPv4"].as_str()?.parse().ok()) {
                set.insert(host_route(ip));
            }
        }
        Ok(set)
    }
}

fn host_route(ip: Ipv4Addr) -> IpNet {
    IpNet::V4(Ipv4Net::from(ip))
}

/// Host part of `https://host:port/path`.
fn url_host(url: &str) -> Option<&str> {
    let authority = url.split_once("//")?.1.split('/').next()?;
    let host = authority.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// User-listed hosts and CIDRs from config, so wglane is not Tailscale-only.
#[derive(Clone, Debug, Default)]
pub struct StaticBypass(pub BypassSet);

impl BypassProvider for StaticBypass {
    fn collect(&self) -> Result<BypassSet> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use crate::testing::FakeRunner;

    // Synthetic; shape follows `tailscale debug prefs` / `debug derp-map`.
    const PREFS: &str = r#"{"ControlURL":"https://192.0.2.10:8443/","RouteAll":false}"#;
    const DERP_MAP: &str = r#"{"Regions":{
        "1":{"RegionID":1,"Nodes":[
            {"Name":"1a","IPv4":"198.51.100.1","IPv6":"2001:db8::1"},
            {"Name":"1b","IPv4":"198.51.100.2"}]},
        "2":{"RegionID":2,"Nodes":[{"Name":"2a","STUNOnly":true},{"Name":"2b","IPv4":"198.51.100.1"}]}}}"#;

    fn nets(set: &BypassSet) -> Vec<String> {
        set.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn collects_control_host_and_derp_v4s_deduped() {
        let fake = FakeRunner::new();
        fake.push_stdout(PREFS).push_stdout(DERP_MAP);
        let bypass = TailscaleBypass::new(fake);
        let set = bypass.collect().expect("collect");
        assert_eq!(
            nets(&set),
            ["192.0.2.10/32", "198.51.100.1/32", "198.51.100.2/32"]
        );
        assert_eq!(
            bypass.runner.calls(),
            [
                ["tailscale", "debug", "prefs"],
                ["tailscale", "debug", "derp-map"]
            ]
        );
    }

    #[test]
    fn partial_failure_keeps_what_was_found() {
        let fake = FakeRunner::new();
        fake.push(CommandOutput {
            code: Some(1),
            ..CommandOutput::default()
        })
        .push_stdout(DERP_MAP);
        let set = TailscaleBypass::new(fake).collect().expect("collect");
        assert_eq!(nets(&set), ["198.51.100.1/32", "198.51.100.2/32"]);

        let fake = FakeRunner::new();
        fake.push_stdout("not json")
            .push_stdout(r#"{"Regions":null}"#);
        assert!(
            TailscaleBypass::new(fake)
                .collect()
                .expect("collect")
                .is_empty()
        );
    }

    #[test]
    fn url_host_strips_scheme_port_path() {
        assert_eq!(url_host("https://a.example:443/x"), Some("a.example"));
        assert_eq!(url_host("http://192.0.2.1"), Some("192.0.2.1"));
        assert_eq!(url_host("nonsense"), None);
    }

    #[test]
    fn static_bypass_returns_its_set() {
        let set: BypassSet = ["203.0.113.0/24".parse().expect("cidr")]
            .into_iter()
            .collect();
        assert_eq!(StaticBypass(set.clone()).collect().expect("collect"), set);
    }
}
