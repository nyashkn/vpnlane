use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, DirBuilder};
use std::io::{self, Read as _};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use ipnet::IpNet;
use wglane_core::ports::{RouteBypass, Tunnel};
use wglane_core::{Error, Result};
use zeroize::Zeroizing;

use crate::helper::{is_cc_stem, rm, write_private};
use crate::privilege::trusted_dir;
use crate::{CommandOutput, CommandRunner, RouteTable};

/// Where `wireguard-go` keeps each interface's control socket (`utunN.sock`);
/// deleting the socket is how a running `wireguard-go` is told to exit.
pub(crate) const WG_RUN_DIR: &str = "/var/run/wireguard";
/// File in the run dir where `wireguard-go` reports which `utunN` it created
/// (`WG_TUN_NAME_FILE`, set by the helper's runner).
pub(crate) const NAME_FILE: &str = "wglane.name";
/// Root-owned record of which `utunN` is which country's tunnel: one
/// `<cc> <utunN> <endpoint-ip|-> <dev>:<ino>:<boot>` line each (the control
/// socket's identity and the boot it was made in). Only these are ever torn
/// down, and only while that socket is still the same file.
const STATE_FILE: &str = "tunnels";
const MAX_CONF_BYTES: u64 = 4096;
/// A full tunnel as two half-routes, so the physical default route survives.
const SPLIT_ROUTES: [&str; 2] = ["0.0.0.0/1", "128.0.0.0/1"];

/// [`Tunnel`] that drives `wireguard-go` and `wg` directly (no helper script, no
/// shell). Must run as root, so in production it is used only inside
/// `wglane-helper`, whose runner allows just these programs.
#[derive(Debug)]
pub struct WgGo<R> {
    runner: R,
    state_dir: PathBuf,
    run_dir: PathBuf,
    /// Seconds since epoch of this boot (`kern.boottime`); a seam for tests.
    boot: fn() -> Result<u64>,
    /// `(top, owner)`: `run_dir` and its ancestors up to `top` must be owned by
    /// `owner` and not writable by others. `None` in tests.
    run_dir_trust: Option<(PathBuf, u32)>,
}

impl<R: CommandRunner> WgGo<R> {
    /// Keeps the tunnel record in `state_dir`; every program runs via `runner`.
    pub fn new(runner: R, state_dir: impl Into<PathBuf>) -> Self {
        let mut engine = Self::with_run_dir(runner, state_dir, WG_RUN_DIR);
        engine.run_dir_trust = Some(("/".into(), 0));
        engine
    }

    /// Like [`new`](Self::new) but with `wireguard-go`'s socket dir elsewhere (tests).
    pub fn with_run_dir(
        runner: R,
        state_dir: impl Into<PathBuf>,
        run_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runner,
            state_dir: state_dir.into(),
            run_dir: run_dir.into(),
            boot: system_boot,
            run_dir_trust: None,
        }
    }
}

/// `kern.boottime` seconds. `/var/run` (where the sockets live) is wiped at
/// boot but the state dir is not, and `utunN` numbers are reused afterwards.
fn system_boot() -> Result<u64> {
    let out = Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.boottime"])
        .env_clear()
        .output()
        .map_err(|e| Error::io("running sysctl kern.boottime", e))?;
    parse_boottime(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| Error::Api("unreadable kern.boottime".into()))
}

/// `{ sec = 1761800000, usec = 123456 } Thu Oct 30 ...` -> `1761800000`.
fn parse_boottime(text: &str) -> Option<u64> {
    let rest = text.split_once("sec = ")?.1;
    rest.split(',').next()?.trim().parse().ok()
}

/// Which file a tunnel's control socket was, and in which boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ident {
    dev: u64,
    ino: u64,
    boot: u64,
}

struct Entry {
    utun: String,
    /// Set once the endpoint host route is installed: only then is it ours to delete.
    endpoint: Option<Ipv4Addr>,
    /// `None` for a record without an identity: never trusted.
    id: Option<Ident>,
}

type State = BTreeMap<String, Entry>;

fn valid_utun(s: &str) -> bool {
    s.strip_prefix("utun")
        .is_some_and(|n| (1..=5).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

fn parse_entry(line: &str) -> Option<(String, Entry)> {
    let mut f = line.split(' ');
    let (cc, utun, ep) = (f.next()?, f.next()?, f.next()?);
    let id = match f.next() {
        None => None,
        Some(id) => {
            let mut n = id.split(':').map(|n| n.parse::<u64>().ok());
            let id = Ident {
                dev: n.next()??,
                ino: n.next()??,
                boot: n.next()??,
            };
            n.next().is_none().then_some(id)?;
            Some(id)
        }
    };
    if f.next().is_some() || !is_cc_stem(cc) || !valid_utun(utun) {
        return None;
    }
    let endpoint = match ep {
        "-" => None,
        ip => Some(ip.parse().ok()?),
    };
    Some((
        cc.to_owned(),
        Entry {
            utun: utun.to_owned(),
            endpoint,
            id,
        },
    ))
}

/// What `up` needs from a config written by `render_wg_conf`.
struct Plan {
    address: Ipv4Addr,
    endpoint: Ipv4Addr,
    /// The config minus `Address =`, which `wg setconf` does not accept.
    setconf: Zeroizing<String>,
}

impl Plan {
    fn parse(text: &str) -> Result<Self> {
        let bad = |why: &str| Error::Api(format!("tunnel config: {why}"));
        let find = |key: &str| text.lines().find_map(|l| l.strip_prefix(key));
        let address = match find("Address = ").map(str::parse::<IpNet>) {
            Some(Ok(IpNet::V4(n))) if n.prefix_len() == 32 => n.addr(),
            _ => return Err(bad("Address must be one IPv4 /32")),
        };
        let endpoint = match find("Endpoint = ").map(str::parse::<SocketAddr>) {
            Some(Ok(SocketAddr::V4(e))) => *e.ip(),
            Some(Ok(SocketAddr::V6(_))) => return Err(bad("IPv6 endpoints are not supported")),
            _ => return Err(bad("Endpoint must be ip:port")),
        };
        // Only the full IPv4 tunnel is implemented; anything else must not half-work.
        if find("AllowedIPs = ") != Some("0.0.0.0/0") {
            return Err(bad("AllowedIPs must be exactly 0.0.0.0/0"));
        }
        let mut setconf = Zeroizing::new(String::new());
        for l in text.lines().filter(|l| !l.starts_with("Address = ")) {
            setconf.push_str(l);
            setconf.push('\n');
        }
        Ok(Self {
            address,
            endpoint,
            setconf,
        })
    }
}

/// Checks `conf` is an absolute `<cc>.conf` path and returns `<cc>`. Runs as
/// root: nothing else from the path reaches argv.
fn checked_conf(conf: &Path) -> Result<&str> {
    let bad = |why: &str| {
        Error::io(
            format!("refusing tunnel config {}: {why}", conf.display()),
            io::Error::from(io::ErrorKind::InvalidInput),
        )
    };
    if !conf.is_absolute() {
        return Err(bad("path must be absolute"));
    }
    if conf.extension().and_then(|e| e.to_str()) != Some("conf") {
        return Err(bad("file must end in .conf"));
    }
    conf.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| is_cc_stem(s))
        .ok_or_else(|| bad("name must be two lowercase letters"))
}

fn command_failed(program: &str, out: &CommandOutput) -> Error {
    Error::Command {
        program: program.into(),
        status: out
            .code
            .map_or_else(|| "killed by signal".into(), |c| format!("exit {c}")),
        stderr: out.stderr.trim().to_owned(),
    }
}

impl<R: CommandRunner> WgGo<R> {
    fn load(&self) -> Result<State> {
        let path = self.state_dir.join(STATE_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(State::new()),
            Err(e) => return Err(Error::io(format!("reading {}", path.display()), e)),
        };
        text.lines()
            .map(|l| {
                parse_entry(l).ok_or_else(|| Error::Api(format!("{} is corrupt", path.display())))
            })
            .collect()
    }

    fn save(&self, state: &State) -> Result<()> {
        let mut text = String::new();
        for (cc, e) in state {
            let ep = e.endpoint.map_or("-".to_owned(), |ip| ip.to_string());
            // `write!` to a String cannot fail.
            let _ = write!(text, "{cc} {} {ep}", e.utun);
            if let Some(i) = e.id {
                let _ = write!(text, " {}:{}:{}", i.dev, i.ino, i.boot);
            }
            text.push('\n');
        }
        write_private(&self.state_dir.join(STATE_FILE), text.as_bytes())
    }

    fn sock(&self, utun: &str) -> PathBuf {
        self.run_dir.join(format!("{utun}.sock"))
    }

    /// Identity of `utun`'s control socket right now.
    fn ident(&self, utun: &str) -> Result<Ident> {
        let path = self.sock(utun);
        let m = fs::symlink_metadata(&path)
            .map_err(|e| Error::io(format!("checking {}", path.display()), e))?;
        Ok(Ident {
            dev: m.dev(),
            ino: m.ino(),
            boot: (self.boot)()?,
        })
    }

    /// Drops every record whose control socket is gone, is another file, or
    /// was made in an earlier boot: the `utunN` may now be someone else's
    /// tunnel, so nothing is run or removed for it.
    fn prune(&self, state: &mut State) -> Result<()> {
        if state.is_empty() {
            return Ok(());
        }
        let boot = (self.boot)()?;
        let before = state.len();
        state.retain(|_, e| {
            let now = fs::symlink_metadata(self.sock(&e.utun)).ok();
            e.id.is_some_and(|i| {
                i.boot == boot && now.is_some_and(|m| (m.dev(), m.ino()) == (i.dev, i.ino))
            })
        });
        if state.len() == before {
            return Ok(());
        }
        self.save(state)
    }

    /// The interface `wireguard-go` just created, from its name file.
    fn take_name(&self) -> Result<String> {
        let path = self.run_dir.join(NAME_FILE);
        let text = fs::read_to_string(&path).map_err(|e| {
            Error::io(
                format!("reading {} (did wireguard-go start?)", path.display()),
                e,
            )
        })?;
        rm(&path)?;
        let name = text.trim();
        if valid_utun(name) {
            Ok(name.to_owned())
        } else {
            Err(Error::Api(format!(
                "wireguard-go reported interface {name:?}"
            )))
        }
    }

    fn configure(
        &self,
        utun: &str,
        plan: &Plan,
        gateway: IpAddr,
        cc: &str,
        state: &mut State,
    ) -> Result<()> {
        let r = &self.runner;
        let out = r.run_with_stdin(
            "wg",
            &["setconf", utun, "/dev/stdin"],
            plan.setconf.as_bytes(),
        )?;
        if !out.success() {
            return Err(command_failed("wg", &out));
        }
        let (address, host) = (plan.address.to_string(), plan.endpoint.to_string());
        r.run_ok(
            "ifconfig",
            &[utun, "inet", &format!("{address}/32"), &address, "alias"],
        )?;
        r.run_ok("ifconfig", &[utun, "up"])?;
        r.run_ok(
            "route",
            &[
                "-q",
                "-n",
                "add",
                "-inet",
                &host,
                "-gateway",
                &gateway.to_string(),
            ],
        )?;
        if let Some(e) = state.get_mut(cc) {
            e.endpoint = Some(plan.endpoint);
        }
        self.save(state)?;
        for net in SPLIT_ROUTES {
            r.run_ok(
                "route",
                &["-q", "-n", "add", "-inet", net, "-interface", utun],
            )?;
        }
        Ok(())
    }

    /// Removes what `up` added for `cc`; a country with no record is a no-op.
    /// Keeps the record if a step fails, so a retry still finds the tunnel.
    fn down_cc(&self, cc: &str) -> Result<()> {
        let mut state = self.load()?;
        self.prune(&mut state)?;
        let Some(entry) = state.get(cc) else {
            return Ok(());
        };
        // A missing route is fine (the interface may already be gone), so statuses are ignored.
        for net in SPLIT_ROUTES {
            self.runner.run(
                "route",
                &[
                    "-q",
                    "-n",
                    "delete",
                    "-inet",
                    net,
                    "-interface",
                    &entry.utun,
                ],
            )?;
        }
        if let Some(ip) = entry.endpoint {
            self.runner
                .run("route", &["-q", "-n", "delete", "-inet", &ip.to_string()])?;
        }
        rm(&self.sock(&entry.utun))?;
        state.remove(cc);
        self.save(&state)
    }
}

impl<R: CommandRunner> Tunnel for WgGo<R> {
    fn up(&self, conf: &Path) -> Result<()> {
        let cc = checked_conf(conf)?;
        let mut text = Zeroizing::new(String::new());
        fs::File::open(conf)
            .and_then(|f| f.take(MAX_CONF_BYTES).read_to_string(&mut text))
            .map_err(|e| Error::io(format!("reading {}", conf.display()), e))?;
        let plan = Plan::parse(&text)?;

        self.down_cc(cc)?;
        let gateway = RouteTable::new(&self.runner).physical_gateway()?;
        DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(&self.run_dir)
            .map_err(|e| Error::io(format!("creating {}", self.run_dir.display()), e))?;
        if let Some((top, owner)) = &self.run_dir_trust {
            trusted_dir(&self.run_dir, top, *owner)?;
        }
        rm(&self.run_dir.join(NAME_FILE))?;
        self.runner.run_ok("wireguard-go", &["utun"])?;
        let utun = self.take_name()?;

        // Recorded before anything else can fail, so `down` can always find it.
        let mut state = self.load()?;
        state.insert(
            cc.to_owned(),
            Entry {
                id: Some(self.ident(&utun)?),
                utun: utun.clone(),
                endpoint: None,
            },
        );
        self.save(&state)?;
        self.configure(&utun, &plan, gateway, cc, &mut state)
            .inspect_err(|_| {
                let _ = self.down_cc(cc);
            })
    }

    fn down(&self, conf: &Path) -> Result<()> {
        self.down_cc(checked_conf(conf)?)
    }

    /// Only tunnels recorded by `up` whose `wireguard-go` is still running.
    fn active(&self) -> Result<Vec<String>> {
        let mut state = self.load()?;
        self.prune(&mut state)?;
        Ok(state.into_keys().collect())
    }

    fn refresh_endpoint(&self, gateway: IpAddr) -> Result<()> {
        let mut state = self.load()?;
        self.prune(&mut state)?;
        for ip in state.values().filter_map(|e| e.endpoint) {
            let host = ip.to_string();
            // Delete then add: `route change` is not reliable across gateway families.
            self.runner
                .run("route", &["-q", "-n", "delete", "-inet", &host])?;
            self.runner.run_ok(
                "route",
                &[
                    "-q",
                    "-n",
                    "add",
                    "-inet",
                    &host,
                    "-gateway",
                    &gateway.to_string(),
                ],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt as _;

    use wglane_core::{PrivateKey, PublicKey, Server, TunnelConfig, render_wg_conf};

    use super::*;
    use crate::testing::FakeRunner;

    const NETSTAT: &str = "\
Internet:
Destination        Gateway            Flags               Netif Expire
default            192.0.2.1          UGScg                 en0
";
    const EP: &str = "198.51.100.7";
    const GW: &str = "192.0.2.1";
    const BOOT: u64 = 1_761_800_000;

    /// [`FakeRunner`] that also plays `wireguard-go`: it reports `utun7` in
    /// the name file and creates the control socket.
    struct Sim {
        inner: FakeRunner,
        run_dir: PathBuf,
        /// Programs (by name) that fail with exit 1.
        failing: RefCell<Vec<&'static str>>,
    }

    impl Sim {
        fn finish(&self, program: &str, mut out: CommandOutput) -> CommandOutput {
            if self.failing.borrow().contains(&program) {
                out.code = Some(1);
                out.stderr = format!("{program} exploded\n");
            }
            out
        }
    }

    impl CommandRunner for Sim {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
            if program == "wireguard-go" {
                fs::write(self.run_dir.join(NAME_FILE), "utun7\n").unwrap();
                fs::write(self.run_dir.join("utun7.sock"), "").unwrap();
            }
            Ok(self.finish(program, self.inner.run(program, args)?))
        }

        fn run_with_stdin(&self, p: &str, a: &[&str], stdin: &[u8]) -> Result<CommandOutput> {
            Ok(self.finish(p, self.inner.run_with_stdin(p, a, stdin)?))
        }
    }

    struct Fixture {
        _t: tempfile::TempDir,
        engine: WgGo<Sim>,
        conf: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            Self::with_netstat(NETSTAT)
        }

        fn with_netstat(netstat: &str) -> Self {
            let t = tempfile::tempdir().unwrap();
            let (state, run) = (t.path().join("state"), t.path().join("run"));
            fs::create_dir(&state).unwrap();
            fs::create_dir(&run).unwrap();
            let sim = Sim {
                inner: FakeRunner::new(),
                run_dir: run.clone(),
                failing: RefCell::default(),
            };
            sim.inner.push_stdout(netstat);
            let conf = state.join("za.conf");
            fs::write(&conf, conf_text(EP)).unwrap();
            let mut engine = WgGo::with_run_dir(sim, &state, &run);
            engine.boot = || Ok(BOOT);
            Self {
                engine,
                conf,
                _t: t,
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.engine.runner.inner.calls()
        }

        fn state_file(&self) -> String {
            fs::read_to_string(self.engine.state_dir.join(STATE_FILE)).unwrap_or_default()
        }
    }

    fn conf_text(endpoint: &str) -> String {
        let server = Server {
            hostname: "x.invalid".into(),
            station: endpoint.parse().unwrap(),
            load: 0,
            public_key: PublicKey("U1lOVEhFVElDLVBFRVItUFVCTElDLUtFWS0wMDAwMDE=".into()),
            country: "ZA".parse().unwrap(),
        };
        let key = PrivateKey::new("U1lOVEhFVElDLVBSSVZBVEUtS0VZLU5PVC1SRUFMMDE=");
        render_wg_conf(&TunnelConfig::nordlynx(key, &server)).to_string()
    }

    fn argv(call: &[&str]) -> Vec<String> {
        call.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn refresh_endpoint_repoints_the_host_route() {
        let f = Fixture::new();
        f.engine.up(&f.conf).unwrap();
        let before = f.calls().len();
        f.engine
            .refresh_endpoint("203.0.113.1".parse().unwrap())
            .unwrap();
        assert_eq!(
            f.calls()[before..],
            [
                argv(&["route", "-q", "-n", "delete", "-inet", EP]),
                argv(&[
                    "route",
                    "-q",
                    "-n",
                    "add",
                    "-inet",
                    EP,
                    "-gateway",
                    "203.0.113.1"
                ]),
            ]
        );
    }

    #[test]
    fn up_runs_exactly_this_sequence() {
        let f = Fixture::new();
        f.engine.up(&f.conf).unwrap();
        assert_eq!(
            f.calls(),
            [
                argv(&["netstat", "-rn", "-f", "inet"]),
                argv(&["wireguard-go", "utun"]),
                argv(&["wg", "setconf", "utun7", "/dev/stdin"]),
                argv(&[
                    "ifconfig",
                    "utun7",
                    "inet",
                    "10.5.0.2/32",
                    "10.5.0.2",
                    "alias"
                ]),
                argv(&["ifconfig", "utun7", "up"]),
                argv(&["route", "-q", "-n", "add", "-inet", EP, "-gateway", GW]),
                argv(&[
                    "route",
                    "-q",
                    "-n",
                    "add",
                    "-inet",
                    "0.0.0.0/1",
                    "-interface",
                    "utun7"
                ]),
                argv(&[
                    "route",
                    "-q",
                    "-n",
                    "add",
                    "-inet",
                    "128.0.0.0/1",
                    "-interface",
                    "utun7"
                ]),
            ]
        );
        let m = fs::metadata(f.engine.run_dir.join("utun7.sock")).unwrap();
        assert_eq!(
            f.state_file(),
            format!("za utun7 {EP} {}:{}:{BOOT}\n", m.dev(), m.ino())
        );
        assert_eq!(f.engine.active().unwrap(), ["za"]);
        // A recorded tunnel whose wireguard-go is gone is not active.
        fs::remove_file(f.engine.run_dir.join("utun7.sock")).unwrap();
        assert_eq!(f.engine.active().unwrap().len(), 0);
        assert!(
            !f.engine.run_dir.join(NAME_FILE).exists(),
            "name file is consumed"
        );
    }

    #[test]
    fn setconf_is_the_rendered_config_without_address_and_never_on_argv() {
        let f = Fixture::new();
        f.engine.up(&f.conf).unwrap();
        let stdins = f.engine.runner.inner.stdins();
        assert_eq!(stdins.len(), 1);
        let sent = String::from_utf8(stdins[0].clone()).unwrap();
        let full = conf_text(EP);
        assert_eq!(
            sent,
            full.lines()
                .filter(|l| !l.starts_with("Address"))
                .flat_map(|l| [l, "\n"])
                .collect::<String>()
        );
        assert!(sent.contains("PrivateKey = ") && sent.contains("AllowedIPs = 0.0.0.0/0"));
        assert!(!sent.contains("Address") && !sent.contains("DNS"));
        let argv = f.calls().concat().join(" ");
        assert!(!argv.contains("PrivateKey") && !argv.contains("U1lOVEhFVElDLVBSSVZBVEUt"));
    }

    #[test]
    fn down_runs_exactly_this_sequence_and_removes_ours_only() {
        let f = Fixture::new();
        f.engine.up(&f.conf).unwrap();
        let foreign = f.engine.run_dir.join("utun9.sock");
        fs::write(&foreign, "").unwrap();
        let before = f.calls().len();
        f.engine.down(&f.conf).unwrap();
        assert_eq!(
            f.calls()[before..],
            [
                argv(&[
                    "route",
                    "-q",
                    "-n",
                    "delete",
                    "-inet",
                    "0.0.0.0/1",
                    "-interface",
                    "utun7"
                ]),
                argv(&[
                    "route",
                    "-q",
                    "-n",
                    "delete",
                    "-inet",
                    "128.0.0.0/1",
                    "-interface",
                    "utun7"
                ]),
                argv(&["route", "-q", "-n", "delete", "-inet", EP]),
            ]
        );
        assert!(
            !f.engine.run_dir.join("utun7.sock").exists(),
            "daemon told to exit"
        );
        assert!(foreign.exists(), "someone else's tunnel untouched");
        assert_eq!(f.state_file(), "");
        assert_eq!(f.engine.active().unwrap().len(), 0);
    }

    #[test]
    fn down_without_a_record_runs_nothing() {
        let f = Fixture::new();
        // Another tunnel on the machine, not ours.
        fs::write(f.engine.run_dir.join("utun3.sock"), "").unwrap();
        f.engine.down(&f.conf).unwrap();
        assert_eq!(f.calls().len(), 0);
        assert!(f.engine.run_dir.join("utun3.sock").exists());
        assert!(
            f.engine.active().unwrap().is_empty(),
            "foreign sockets are not ours"
        );
    }

    /// Writes a record for `za` on utun7 with the given identity.
    fn record(f: &Fixture, id: &str) {
        let line = format!("za utun7 {EP} {id}\n");
        fs::write(f.engine.state_dir.join(STATE_FILE), line).unwrap();
    }

    fn assert_stale_is_dropped_untouched(f: &Fixture) {
        let sock = f.engine.run_dir.join("utun7.sock");
        assert_eq!(f.engine.active().unwrap().len(), 0);
        assert_eq!(f.state_file(), "", "record dropped");
        record_again(f);
        f.engine.down(&f.conf).unwrap();
        assert_eq!(f.calls().len(), 0, "ran {:?}", f.calls());
        assert!(sock.exists(), "foreign socket removed");
        assert_eq!(f.state_file(), "");
    }

    fn record_again(f: &Fixture) {
        let id = fs::read_to_string(f.engine.state_dir.join("last-id")).unwrap();
        record(f, &id);
    }

    #[test]
    fn a_record_whose_socket_is_another_file_is_stale_and_runs_nothing() {
        // utun7.sock now belongs to some other tunnel (the reviewer's repro).
        let f = Fixture::new();
        fs::write(f.engine.run_dir.join("utun7.sock"), "").unwrap();
        let id = format!("1:1:{BOOT}");
        fs::write(f.engine.state_dir.join("last-id"), &id).unwrap();
        record(&f, &id);
        assert_stale_is_dropped_untouched(&f);
    }

    #[test]
    fn a_record_from_an_earlier_boot_is_stale() {
        let f = Fixture::new();
        let sock = f.engine.run_dir.join("utun7.sock");
        fs::write(&sock, "").unwrap();
        let m = fs::metadata(&sock).unwrap();
        let id = format!("{}:{}:{}", m.dev(), m.ino(), BOOT - 1);
        fs::write(f.engine.state_dir.join("last-id"), &id).unwrap();
        record(&f, &id);
        assert_stale_is_dropped_untouched(&f);
    }

    #[test]
    fn a_record_without_identity_or_socket_is_stale() {
        let f = Fixture::new();
        fs::write(f.engine.run_dir.join("utun7.sock"), "").unwrap();
        fs::write(f.engine.state_dir.join("last-id"), "").unwrap();
        fs::write(
            f.engine.state_dir.join(STATE_FILE),
            format!("za utun7 {EP}\n"),
        )
        .unwrap();
        f.engine.down(&f.conf).unwrap();
        assert_eq!(f.calls().len(), 0);
        assert!(f.engine.run_dir.join("utun7.sock").exists());
        assert_eq!(f.state_file(), "");
        // Socket missing entirely.
        fs::remove_file(f.engine.run_dir.join("utun7.sock")).unwrap();
        record(&f, &format!("1:1:{BOOT}"));
        f.engine.down(&f.conf).unwrap();
        assert_eq!(f.calls().len(), 0);
        assert_eq!(f.state_file(), "");
    }

    #[test]
    fn boottime_parser() {
        assert_eq!(
            parse_boottime("{ sec = 1761800000, usec = 123456 } Thu Oct 30 09:00:00 2026\n"),
            Some(1_761_800_000)
        );
        assert_eq!(parse_boottime("garbage"), None);
        assert_eq!(parse_boottime("{ sec = x, usec = 1 }"), None);
    }

    #[test]
    fn up_again_replaces_the_old_tunnel_first() {
        let f = Fixture::new();
        f.engine.up(&f.conf).unwrap();
        f.engine.runner.inner.push_stdout(""); // route delete 0/1
        f.engine.runner.inner.push_stdout(""); // route delete 128/1
        f.engine.runner.inner.push_stdout(""); // route delete endpoint
        f.engine.runner.inner.push_stdout(NETSTAT);
        let before = f.calls().len();
        f.engine.up(&f.conf).unwrap();
        let calls = f.calls();
        assert_eq!(calls[before][..2], ["route", "-q"]);
        assert_eq!(calls[before][3], "delete");
        assert_eq!(calls[before + 3], argv(&["netstat", "-rn", "-f", "inet"]));
        assert_eq!(calls[before + 4], argv(&["wireguard-go", "utun"]));
        assert_eq!(f.state_file().lines().count(), 1);
    }

    #[test]
    fn failure_midway_tears_down_only_what_was_added() {
        // Fails at ifconfig: no endpoint route yet, so none is deleted.
        let f = Fixture::new();
        f.engine.runner.failing.borrow_mut().push("ifconfig");
        let err = f.engine.up(&f.conf).unwrap_err().to_string();
        assert!(err.contains("ifconfig"), "{err}");
        let calls = f.calls();
        assert_eq!(
            calls.last().unwrap(),
            &argv(&[
                "route",
                "-q",
                "-n",
                "delete",
                "-inet",
                "128.0.0.0/1",
                "-interface",
                "utun7"
            ])
        );
        assert!(
            !calls
                .iter()
                .any(|c| c.contains(&"delete".to_owned()) && c.contains(&EP.to_owned())),
            "{calls:?}"
        );
        assert_eq!(f.state_file(), "");
        assert!(!f.engine.run_dir.join("utun7.sock").exists());

        // Fails at the endpoint route add: nothing of the endpoint is deleted either.
        let g = Fixture::new();
        g.engine.runner.failing.borrow_mut().push("route");
        assert!(g.engine.up(&g.conf).is_err());
        assert!(
            !g.calls()
                .iter()
                .any(|c| c.last().map(String::as_str) == Some(EP) && c[3] == "delete"),
            "{:?}",
            g.calls()
        );
        assert_eq!(g.state_file(), "");
    }

    #[test]
    fn wireguard_go_failure_leaves_no_record() {
        let f = Fixture::new();
        f.engine.runner.failing.borrow_mut().push("wireguard-go");
        assert!(f.engine.up(&f.conf).is_err());
        assert_eq!(f.calls().len(), 2, "netstat, wireguard-go");
        assert_eq!(f.state_file(), "");
    }

    #[test]
    fn missing_gateway_starts_nothing() {
        let f = Fixture::with_netstat("Internet:\n");
        assert!(f.engine.up(&f.conf).is_err());
        assert_eq!(f.calls().len(), 1, "only netstat");
        assert_eq!(f.state_file(), "");
    }

    #[test]
    fn refuses_configs_it_cannot_drive_without_running_anything() {
        let good = conf_text(EP);
        let cases = [
            good.replace("Address = 10.5.0.2/32", "Address = 10.5.0.2/24"),
            good.replace("Address = 10.5.0.2/32", "Address = fd00::2/128"),
            good.replace("0.0.0.0/0", "0.0.0.0/0, ::/0"),
            good.replace("0.0.0.0/0", "10.0.0.0/8"),
            good.replace(EP, "[2001:db8::7]"),
            good.replace("Endpoint = ", "Endpoint = host.example:"),
            good.replace("Address = 10.5.0.2/32\n", ""),
            String::new(),
        ];
        for text in cases {
            let f = Fixture::new();
            fs::write(&f.conf, &text).unwrap();
            assert!(f.engine.up(&f.conf).is_err(), "accepted {text:?}");
            assert!(f.calls().is_empty(), "ran something for {text:?}");
        }
    }

    #[test]
    fn rejects_unsafe_conf_paths_without_running_anything() {
        let f = Fixture::new();
        for bad in [
            "za.conf",
            "/c/za",
            "/c/za.conf.bak",
            "/c/.conf",
            "/c/ZA.conf",
            "/c/zaa.conf",
            "/c/z.conf",
            "/c/z;.conf",
            "/c/-x.conf/../u s.conf",
            "/c/ü.conf",
        ] {
            assert!(f.engine.up(Path::new(bad)).is_err(), "{bad}");
            assert!(f.engine.down(Path::new(bad)).is_err(), "{bad}");
        }
        assert_eq!(f.calls().len(), 0);
    }

    #[test]
    fn utun_name_validator() {
        for ok in ["utun0", "utun7", "utun12345"] {
            assert!(valid_utun(ok), "{ok}");
        }
        for bad in [
            "",
            "utun",
            "utun123456",
            "en0",
            "utun7 ",
            "utun-1",
            "../utun7",
            "utun7/x",
            "UTUN7",
            "utun७",
        ] {
            assert!(!valid_utun(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_hostile_interface_name_from_wireguard_go_is_never_used() {
        let f = Fixture::new();
        // The simulator reports a fine name; swap in a path-like one before it is read.
        let name = f.engine.run_dir.join(NAME_FILE);
        fs::write(&name, "../../etc/x\n").unwrap();
        // take_name reads whatever is there after wireguard-go ran, so test it directly.
        let err = f.engine.take_name().unwrap_err().to_string();
        assert!(err.contains("reported interface"), "{err}");
        assert!(!name.exists());
    }

    #[test]
    fn state_file_round_trips_and_rejects_corruption() {
        let f = Fixture::new();
        let path = f.engine.state_dir.join(STATE_FILE);
        fs::write(&path, "de utun3 -\nza utun7 198.51.100.7 1:2:3\n").unwrap();
        let s = f.engine.load().unwrap();
        assert_eq!(s.len(), 2);
        f.engine.save(&s).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "de utun3 -\nza utun7 198.51.100.7 1:2:3\n"
        );
        for bad in [
            "za utun7\n",
            "za utun7 - extra\n",
            "ZA utun7 -\n",
            "za en0 -\n",
            "za utun7 host\n",
            "za utun7 ../x\n",
            "za  utun7 -\n",
            "za utun7 - 1:2\n",
            "za utun7 - 1:2:3:4\n",
            "za utun7 - a:2:3\n",
            "za utun7 - 1:2:3 x\n",
        ] {
            fs::write(&path, bad).unwrap();
            assert!(f.engine.load().is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn production_engine_checks_the_run_dir_chain_from_root() {
        let e = WgGo::new(FakeRunner::new(), "/s");
        assert_eq!(e.run_dir_trust, Some(("/".into(), 0)));
    }

    #[test]
    fn up_refuses_a_run_dir_others_can_write_before_starting_wireguard_go() {
        let mut f = Fixture::new();
        let top = fs::canonicalize(f.engine.run_dir.parent().unwrap()).unwrap();
        let uid = fs::metadata(&top).unwrap().uid();
        f.engine.run_dir_trust = Some((top, uid));
        fs::set_permissions(&f.engine.run_dir, fs::Permissions::from_mode(0o777)).unwrap();
        let err = f.engine.up(&f.conf).unwrap_err().to_string();
        assert!(err.contains("not writable by others"), "{err}");
        assert_eq!(f.calls().len(), 1, "only netstat, no wireguard-go");
        fs::set_permissions(&f.engine.run_dir, fs::Permissions::from_mode(0o755)).unwrap();
        f.engine.runner.inner.push_stdout(NETSTAT);
        f.engine.up(&f.conf).unwrap();
    }
}
