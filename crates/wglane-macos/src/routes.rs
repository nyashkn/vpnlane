use std::net::{IpAddr, Ipv4Addr};

use ipnet::IpNet;
use wglane_core::ports::RouteBypass;
use wglane_core::{BypassSet, Error, Result};

use crate::CommandRunner;

/// [`RouteBypass`] via `/sbin/route`. Needs root, so production use is
/// inside `wglane-helper`.
#[derive(Debug)]
pub struct RouteTable<R> {
    runner: R,
}

impl<R: CommandRunner> RouteTable<R> {
    /// Uses `runner` for every `route`/`netstat` call.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

fn invalid(what: &str) -> Error {
    Error::io(
        format!("refusing route: {what}"),
        std::io::Error::from(std::io::ErrorKind::InvalidInput),
    )
}

/// A gateway must be a real unicast next hop.
fn check_gateway(gw: IpAddr) -> Result<()> {
    if gw.is_unspecified() || gw.is_loopback() || gw.is_multicast() {
        return Err(invalid(&format!("bad gateway {gw}")));
    }
    Ok(())
}

/// A bypass entry must be a specific unicast destination of the gateway's
/// family: never a default route, which would hijack all traffic as root.
fn check_dest(net: &IpNet, gw: IpAddr) -> Result<()> {
    check_dest_family(net)?;
    if net.addr().is_ipv4() != gw.is_ipv4() {
        return Err(invalid(&format!(
            "{net} is not in the gateway's address family"
        )));
    }
    Ok(())
}

fn check_dest_family(net: &IpNet) -> Result<()> {
    let ip = net.addr();
    if net.prefix_len() == 0
        || ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip == IpAddr::V4(Ipv4Addr::BROADCAST)
    {
        return Err(invalid(&format!("bad destination {net}")));
    }
    Ok(())
}

/// `route` argv tail for `net` (everything after the verb), without gateway.
fn target(net: &IpNet) -> Vec<String> {
    let mut a = Vec::new();
    if net.addr().is_ipv6() {
        a.push("-inet6".to_owned());
    }
    if net.prefix_len() == net.max_prefix_len() {
        a.extend(["-host".to_owned(), net.addr().to_string()]);
    } else {
        a.extend(["-net".to_owned(), net.trunc().to_string()]);
    }
    a
}

impl<R: CommandRunner> RouteTable<R> {
    fn route(&self, verb: &str, net: &IpNet, gw: Option<IpAddr>) -> Result<crate::CommandOutput> {
        let mut args = vec!["-n".to_owned(), verb.to_owned()];
        args.extend(target(net));
        args.extend(gw.map(|g| g.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.runner.run("route", &refs)
    }
}

impl<R: CommandRunner> RouteBypass for RouteTable<R> {
    fn physical_gateway(&self) -> Result<IpAddr> {
        let out = self.runner.run_ok("netstat", &["-rn", "-f", "inet"])?;
        parse_physical_gateway(&out.stdout)
            .map(IpAddr::V4)
            .ok_or_else(|| Error::Command {
                program: "netstat".into(),
                status: "no default route".into(),
                stderr: "no non-tunnel IPv4 default gateway found; is the network up?".into(),
            })
    }

    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize> {
        check_gateway(gateway)?;
        // Validate everything first so a bad entry changes nothing.
        for net in set.iter() {
            check_dest(net, gateway)?;
        }
        let mut added = 0;
        for net in set.iter() {
            // Replace semantics: drop any stale route, ignoring "not in table".
            self.route("delete", net, None)?;
            let out = self.route("add", net, Some(gateway))?;
            if !out.success() {
                return Err(Error::Command {
                    program: "route".into(),
                    status: out
                        .code
                        .map_or_else(|| "killed by signal".into(), |c| format!("exit {c}")),
                    stderr: out.stderr.trim().to_owned(),
                });
            }
            added += 1;
        }
        Ok(added)
    }

    fn clear(&self, set: &BypassSet) -> Result<()> {
        for net in set.iter() {
            check_dest_family(net)?;
        }
        for net in set.iter() {
            // Missing routes are fine, so the exit status is ignored.
            self.route("delete", net, None)?;
        }
        Ok(())
    }
}

/// First `default` row of `netstat -rn -f inet` whose interface is not a
/// `utun*` tunnel and whose gateway is an IPv4 address (not `link#N`).
fn parse_physical_gateway(netstat: &str) -> Option<Ipv4Addr> {
    netstat.lines().find_map(|line| {
        let mut f = line.split_whitespace();
        // Columns: Destination Gateway Flags Netif [Expire]
        let (dest, gw, _flags, netif) = (f.next()?, f.next()?, f.next()?, f.next()?);
        (dest == "default" && !netif.starts_with("utun"))
            .then(|| gw.parse().ok())
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use crate::testing::FakeRunner;

    // Synthetic `netstat -rn -f inet` with the VPN up: the tunnel's default
    // is listed first, as it is on a real machine.
    const NETSTAT_VPN_UP: &str = "\
Routing tables

Internet:
Destination        Gateway            Flags               Netif Expire
0/1                10.5.0.2           UGScg               utun4
default            10.5.0.2           UGScg               utun4
default            192.0.2.1          UGScg                 en0
10.5.0.2           10.5.0.2           UH                  utun4
127                127.0.0.1          UCS                   lo0
192.0.2.0/24       link#14            UCS                   en0
";

    const NETSTAT_ONLY_LINK: &str = "\
Internet:
Destination        Gateway            Flags               Netif Expire
default            link#14            UCSIg                 en0
";

    fn set(nets: &[&str]) -> BypassSet {
        nets.iter().map(|n| n.parse().unwrap()).collect()
    }

    fn gw() -> IpAddr {
        "192.0.2.1".parse().unwrap()
    }

    #[test]
    fn gateway_skips_tunnel_default() {
        let t = RouteTable::new(FakeRunner::new());
        t.runner.push_stdout(NETSTAT_VPN_UP);
        assert_eq!(t.physical_gateway().unwrap(), gw());
        assert_eq!(t.runner.calls(), vec![vec!["netstat", "-rn", "-f", "inet"]]);
    }

    #[test]
    fn gateway_missing_is_error() {
        for out in [NETSTAT_ONLY_LINK, ""] {
            let t = RouteTable::new(FakeRunner::new());
            t.runner.push_stdout(out);
            assert!(t.physical_gateway().is_err());
        }
    }

    #[test]
    fn apply_deletes_then_adds_with_exact_argv() {
        let t = RouteTable::new(FakeRunner::new());
        let n = t
            .apply(&set(&["203.0.113.7/32", "198.51.100.0/24"]), gw())
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            t.runner.calls(),
            vec![
                vec!["route", "-n", "delete", "-net", "198.51.100.0/24"],
                vec!["route", "-n", "add", "-net", "198.51.100.0/24", "192.0.2.1"],
                vec!["route", "-n", "delete", "-host", "203.0.113.7"],
                vec!["route", "-n", "add", "-host", "203.0.113.7", "192.0.2.1"],
            ]
        );
    }

    #[test]
    fn apply_ipv6_uses_inet6() {
        let t = RouteTable::new(FakeRunner::new());
        let g: IpAddr = "2001:db8::1".parse().unwrap();
        t.apply(&set(&["2001:db8:1::5/128"]), g).unwrap();
        assert_eq!(
            t.runner.calls()[1],
            vec![
                "route",
                "-n",
                "add",
                "-inet6",
                "-host",
                "2001:db8:1::5",
                "2001:db8::1"
            ]
        );
    }

    #[test]
    fn apply_add_failure_is_error_and_not_counted() {
        let t = RouteTable::new(FakeRunner::new());
        t.runner.push_stdout("");
        t.runner.push(CommandOutput {
            code: Some(1),
            stderr: "route: writing to routing socket: File exists\n".into(),
            ..Default::default()
        });
        assert!(t.apply(&set(&["203.0.113.7/32"]), gw()).is_err());
    }

    #[test]
    fn apply_rejects_bad_input_before_running_anything() {
        let good = "203.0.113.7/32";
        for (bad, g) in [
            ("0.0.0.0/0", gw()),
            ("0.0.0.0/32", gw()),
            ("127.0.0.1/32", gw()),
            ("224.0.0.1/32", gw()),
            ("255.255.255.255/32", gw()),
            ("::/0", "2001:db8::1".parse().unwrap()),
            ("2001:db8::5/128", gw()), // family mismatch
            (good, "0.0.0.0".parse().unwrap()),
            (good, "127.0.0.1".parse().unwrap()),
            (good, "224.0.0.1".parse().unwrap()),
        ] {
            let t = RouteTable::new(FakeRunner::new());
            assert!(t.apply(&set(&[good, bad]), g).is_err(), "{bad} via {g}");
            assert_eq!(t.runner.calls().len(), 0, "{bad} via {g}");
        }
    }

    #[test]
    fn clear_deletes_each_and_ignores_missing() {
        let t = RouteTable::new(FakeRunner::new());
        t.runner.push(CommandOutput {
            code: Some(1),
            stderr: "route: writing to routing socket: not in table\n".into(),
            ..Default::default()
        });
        t.clear(&set(&["203.0.113.7/32", "198.51.100.0/24"]))
            .unwrap();
        assert_eq!(
            t.runner.calls(),
            vec![
                vec!["route", "-n", "delete", "-net", "198.51.100.0/24"],
                vec!["route", "-n", "delete", "-host", "203.0.113.7"],
            ]
        );
    }

    #[test]
    fn clear_rejects_default_route() {
        let t = RouteTable::new(FakeRunner::new());
        assert!(t.clear(&set(&["0.0.0.0/0"])).is_err());
        assert_eq!(t.runner.calls().len(), 0);
    }
}
