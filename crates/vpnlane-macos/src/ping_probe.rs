use std::net::IpAddr;
use std::time::Duration;

use vpnlane_core::ports::Prober;
use vpnlane_core::{Error, Result};

use crate::CommandRunner;

/// Absolute path: a minimal `PATH` (no `/sbin`) must not make every probe fail.
const PING: &str = "/sbin/ping";

/// [`Prober`] over the system `ping` (one ICMP echo). Needs no privileges.
///
/// Why ICMP and not a TCP connect: WireGuard endpoints are UDP and expose no
/// TCP listener to rely on, ICMP is what answers on Nord's stations, and
/// `ping` can be pinned to an interface with `-b`.
///
/// While a tunnel is up, every route except the endpoint's goes through it, so
/// an unbound probe would measure the tunnel's exit. Pass the physical
/// interface and the socket is bound to it (`IP_BOUND_IF`); if that interface
/// has no route, the probe fails instead of leaking into the tunnel.
#[derive(Debug)]
pub struct PingProbe<R> {
    runner: R,
    iface: Option<String>,
}

impl<R: CommandRunner> PingProbe<R> {
    /// `iface` is the physical interface to bind to, or `None` when no tunnel
    /// is up (probes then take the normal route).
    pub fn new(runner: R, iface: Option<String>) -> Self {
        Self { runner, iface }
    }

    /// The `ping` arguments for one probe.
    fn argv(&self, ip: IpAddr, timeout: Duration) -> Vec<String> {
        let mut a: Vec<String> = ["-n", "-c", "1"].map(Into::into).into();
        // -W: per-reply wait in ms; -t: hard stop in whole seconds.
        a.extend(["-W".into(), timeout.as_millis().max(1).to_string()]);
        a.extend([
            "-t".into(),
            timeout.as_millis().div_ceil(1000).max(1).to_string(),
        ]);
        if let Some(iface) = &self.iface {
            a.extend(["-b".into(), iface.clone()]);
        }
        a.push(ip.to_string());
        a
    }
}

impl<R: CommandRunner> Prober for PingProbe<R> {
    fn probe(&self, ip: IpAddr, timeout: Duration) -> Option<Duration> {
        let owned = self.argv(ip, timeout);
        let args: Vec<&str> = owned.iter().map(String::as_str).collect();
        let out = self.runner.run(PING, &args).ok()?;
        let rtt = out.success().then(|| parse_rtt(&out.stdout)).flatten()?;
        (rtt <= timeout).then_some(rtt)
    }
}

/// `time=340.267 ms` from the reply line.
fn parse_rtt(stdout: &str) -> Option<Duration> {
    let rest = stdout.split("time=").nth(1)?;
    let ms: f64 = rest.split_whitespace().next()?.parse().ok()?;
    (ms.is_finite() && ms >= 0.0).then(|| Duration::from_secs_f64(ms / 1000.0))
}

/// Interface of the first non-tunnel IPv4 default route, the way
/// `RouteTable::physical_gateway` finds the gateway.
///
/// # Errors
/// When `netstat` fails or lists no such route.
pub fn physical_interface(runner: &impl CommandRunner) -> Result<String> {
    let out = runner.run_ok("netstat", &["-rn", "-f", "inet"])?;
    parse_physical_interface(&out.stdout).ok_or_else(|| Error::Command {
        program: "netstat".into(),
        status: "no default route".into(),
        stderr: "no non-tunnel IPv4 default route found; is the network up?".into(),
    })
}

fn parse_physical_interface(netstat: &str) -> Option<String> {
    netstat.lines().find_map(|line| {
        let mut f = line.split_whitespace();
        // Columns: Destination Gateway Flags Netif [Expire]
        let (dest, _gw, _flags, netif) = (f.next()?, f.next()?, f.next()?, f.next()?);
        let plain = !netif.is_empty() && netif.chars().all(|c| c.is_ascii_alphanumeric());
        (dest == "default" && plain && !netif.starts_with("utun")).then(|| netif.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use crate::testing::FakeRunner;

    const NETSTAT_VPN_UP: &str = "\
Internet:
Destination        Gateway            Flags               Netif Expire
0/1                10.5.0.2           UGScg               utun4
default            10.5.0.2           UGScg               utun4
default            192.0.2.1          UGScg                 en0
default            link#34            UCSIg              utun14
";

    const REPLY: &str = "64 bytes from 203.0.113.9: icmp_seq=0 ttl=55 time=34.267 ms\n";

    fn ip() -> IpAddr {
        "203.0.113.9".parse().unwrap()
    }

    const T: Duration = Duration::from_millis(1500);

    #[test]
    fn ping_is_called_by_absolute_path() {
        assert_eq!(PING, "/sbin/ping");
        let p = PingProbe::new(FakeRunner::new(), None);
        p.probe(ip(), T);
        assert_eq!(p.runner.calls()[0][0], "/sbin/ping");
    }

    #[test]
    fn unbound_argv_when_no_tunnel_is_up() {
        let p = PingProbe::new(FakeRunner::new(), None);
        p.runner.push_stdout(REPLY);
        assert_eq!(p.probe(ip(), T), Some(Duration::from_secs_f64(0.034_267)));
        assert_eq!(
            p.runner.calls(),
            vec![vec![
                PING,
                "-n",
                "-c",
                "1",
                "-W",
                "1500",
                "-t",
                "2",
                "203.0.113.9"
            ]]
        );
    }

    #[test]
    fn bound_to_the_physical_interface_when_a_tunnel_is_up() {
        let p = PingProbe::new(FakeRunner::new(), Some("en0".into()));
        p.probe(ip(), T);
        let call = &p.runner.calls()[0];
        let at = call.iter().position(|a| a == "-b").expect("-b present");
        assert_eq!(call[at + 1], "en0");
        assert_eq!(call.last().map(String::as_str), Some("203.0.113.9"));
    }

    #[test]
    fn no_reply_and_failed_ping_are_none() {
        let p = PingProbe::new(FakeRunner::new(), None);
        p.runner.push(CommandOutput {
            code: Some(2),
            stdout: "1 packets transmitted, 0 packets received, 100.0% packet loss".into(),
            stderr: String::new(),
        });
        assert_eq!(p.probe(ip(), T), None);
        // Exit 0 but nothing parseable, and a reply slower than the timeout.
        p.runner.push_stdout("garbage");
        assert_eq!(p.probe(ip(), T), None);
        p.runner.push_stdout("64 bytes from x: time=2000.0 ms");
        assert_eq!(p.probe(ip(), T), None);
    }

    #[test]
    fn physical_interface_skips_tunnels() {
        let r = FakeRunner::new();
        r.push_stdout(NETSTAT_VPN_UP);
        assert_eq!(physical_interface(&r).unwrap(), "en0");
        assert_eq!(r.calls(), vec![vec!["netstat", "-rn", "-f", "inet"]]);
        let r = FakeRunner::new();
        r.push_stdout("default link#1 UCSIg utun3\n");
        assert!(physical_interface(&r).is_err());
    }
}
