use std::io::ErrorKind;

use serde_json::Value;
use wglane_core::ports::TailnetProbe;
use wglane_core::{Error, Result, TailnetHealth};

use crate::CommandRunner;

/// [`TailnetProbe`] via `tailscale ping`.
#[derive(Debug)]
pub struct TailscalePing<R> {
    runner: R,
}

impl<R: CommandRunner> TailscalePing<R> {
    /// Uses `runner` to call the `tailscale` CLI.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    fn probe(&self) -> Result<TailnetHealth> {
        let unreachable = |reason: String| Ok(TailnetHealth::Unreachable { reason });
        let status = self.runner.run("tailscale", &["status", "--json"])?;
        if !status.success() {
            return unreachable(format!("tailscale status failed: {}", status.stderr.trim()));
        }
        let Some((peer, ip)) = online_peer(&status.stdout) else {
            return unreachable("no online tailnet peer to ping".into());
        };
        let ping = self
            .runner
            .run("tailscale", &["ping", "-c", "1", "--timeout", "3s", &ip])?;
        match ping.success().then(|| latency_ms(&ping.stdout)).flatten() {
            Some(latency_ms) => Ok(TailnetHealth::Reachable { peer, latency_ms }),
            None => unreachable(format!("no reply from {peer}")),
        }
    }
}

impl<R: CommandRunner> TailnetProbe for TailscalePing<R> {
    fn tailnet(&self) -> Result<TailnetHealth> {
        match self.probe() {
            Err(Error::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {
                Ok(TailnetHealth::NotInstalled)
            }
            other => other,
        }
    }
}

/// First online peer's MagicDNS name and first Tailscale IP (`status --json`).
/// `tailscale ping` rejects the DNS name ("no matching peer"), so ping the IP.
fn online_peer(status_json: &str) -> Option<(String, String)> {
    let status: Value = serde_json::from_str(status_json).ok()?;
    status["Peer"]
        .as_object()?
        .values()
        .filter(|p| p["Online"].as_bool() == Some(true))
        .find_map(|p| Some((p["DNSName"].as_str()?, p["TailscaleIPs"][0].as_str()?)))
        .map(|(n, ip)| (n.trim_end_matches('.').to_owned(), ip.to_owned()))
}

/// `23` from `pong from host (100.64.0.2) via DERP(nyc) in 23ms`.
fn latency_ms(ping_out: &str) -> Option<u64> {
    let (_, rest) = ping_out
        .lines()
        .find(|l| l.starts_with("pong"))?
        .rsplit_once(" in ")?;
    rest.trim().strip_suffix("ms")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use crate::testing::FakeRunner;

    // Synthetic; shape follows `tailscale status --json`.
    const STATUS: &str = r#"{"Peer":{
        "nodekey:aa":{"DNSName":"off.tail.example.","TailscaleIPs":["100.64.0.1"],"Online":false},
        "nodekey:bb":{"DNSName":"box.tail.example.","TailscaleIPs":["100.64.0.2"],"Online":true}}}"#;
    const PONG: &str = "pong from box (100.64.0.2) via DERP(nyc) in 23ms\n";

    #[test]
    fn pings_first_online_peer() {
        let fake = FakeRunner::new();
        fake.push_stdout(STATUS).push_stdout(PONG);
        let probe = TailscalePing::new(fake);
        let health = probe.tailnet().expect("probe");
        assert_eq!(
            health,
            TailnetHealth::Reachable {
                peer: "box.tail.example".into(),
                latency_ms: 23
            }
        );
        assert_eq!(
            probe.runner.calls()[1],
            [
                "tailscale",
                "ping",
                "-c",
                "1",
                "--timeout",
                "3s",
                "100.64.0.2"
            ]
        );
    }

    #[test]
    fn no_online_peer_or_failed_ping_is_unreachable() {
        let fake = FakeRunner::new();
        fake.push_stdout(r#"{"Peer":{}}"#);
        assert!(matches!(
            TailscalePing::new(fake).tailnet().expect("probe"),
            TailnetHealth::Unreachable { .. }
        ));

        let fake = FakeRunner::new();
        fake.push_stdout(STATUS).push(CommandOutput {
            code: Some(1),
            stdout: "ping \"box\" timed out\n".into(),
            ..CommandOutput::default()
        });
        assert!(matches!(
            TailscalePing::new(fake).tailnet().expect("probe"),
            TailnetHealth::Unreachable { .. }
        ));
    }

    #[test]
    fn missing_binary_is_not_installed() {
        struct Missing;
        impl CommandRunner for Missing {
            fn run(&self, _: &str, _: &[&str]) -> Result<CommandOutput> {
                Err(Error::io("running `tailscale`", ErrorKind::NotFound.into()))
            }
        }
        assert_eq!(
            TailscalePing::new(Missing).tailnet().expect("probe"),
            TailnetHealth::NotInstalled
        );
    }
}
