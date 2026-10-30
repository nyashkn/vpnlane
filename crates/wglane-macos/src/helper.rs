use std::ffi::OsString;
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::str::FromStr;

use ipnet::IpNet;
use wglane_core::ports::{RouteBypass, Tunnel};
use wglane_core::{
    BypassSet, CountryCode, Error, PrivateKey, PublicKey, Result, Server, TunnelConfig,
    render_wg_conf,
};
use zeroize::Zeroizing;

use crate::privilege::trusted_tool;
use crate::routes::{check_dest, is_bad_bypass};
use crate::{CommandOutput, CommandRunner, RouteTable, WgQuick};

/// Root-owned install location of `wglane-helper` (never the user-writable
/// Homebrew copy: a sudoers rule on a user-writable binary is a root exploit).
pub const HELPER_PATH: &str = "/usr/local/libexec/wglane-helper";

/// Root-owned (0700) directory where the helper keeps the configs it renders
/// and its bypass-route record. Nothing in it is ever taken from the caller.
pub const STATE_DIR: &str = "/var/db/wglane";

/// Root-owned copies of `bash`, `wg-quick`, `wg` and `wireguard-go`, put there
/// by `install-helper`. Homebrew's own copies are user-writable, so the helper
/// never runs, sources or searches them.
pub const TOOL_DIR: &str = "/usr/local/libexec/wglane/bin";
/// What `install-helper` copies into [`TOOL_DIR`]: wg-quick needs bash 4+.
pub(crate) const TOOLS: [&str; 4] = ["bash", "wg-quick", "wg", "wireguard-go"];

const SUDO: &str = "/usr/bin/sudo";
/// The only `PATH` programs run by the helper see; the caller's is discarded.
/// [`TOOL_DIR`] first, then system dirs only.
const ROOT_PATH: &str = "/usr/local/libexec/wglane/bin:/usr/bin:/bin:/usr/sbin:/sbin";
const BYPASS_STATE: &str = "bypass.json";
/// A rendered config is ~300 bytes; anything near this is not one.
const MAX_STDIN: usize = 1024;
const MAX_CONF_BYTES: u64 = 4096;
const MAX_CIDRS: usize = 1024;

/// The complete helper protocol. This verb set becomes the socket protocol
/// if wglane later grows a launchd daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperVerb {
    /// `up <CC>` + stdin: render a config from the stdin fields into the
    /// root-owned dir and wg-quick up that copy.
    Up,
    /// `down`: tear down every wglane tunnel and its bypass routes.
    Down,
    /// `bypass <gateway> <cidr>...`: (re)install bypass routes.
    Bypass,
}

impl HelperVerb {
    /// Wire name of the verb.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Bypass => "bypass",
        }
    }

    fn from_arg(arg: &str) -> Option<Self> {
        [Self::Up, Self::Down, Self::Bypass]
            .into_iter()
            .find(|v| v.as_str() == arg)
    }
}

// ---------------------------------------------------------------------------
// Validated request fields (shared by client and helper)
// ---------------------------------------------------------------------------

/// The fields the helper needs to render one NordLynx config. Everything
/// else in the config (address, allowed IPs, keepalive) comes from
/// `TunnelConfig::nordlynx`, so a caller cannot inject a `PostUp` line.
struct UpRequest {
    private_key: PrivateKey,
    public_key: PublicKey,
    endpoint: SocketAddr,
}

impl UpRequest {
    /// Validates and wraps the three fields. Errors never contain a value.
    fn new(private_key: &str, public_key: &str, endpoint: &str) -> Result<Self, String> {
        if !is_wg_key(private_key) {
            return Err("private_key is not a base64 WireGuard key".into());
        }
        if !is_wg_key(public_key) {
            return Err("public_key is not a base64 WireGuard key".into());
        }
        let endpoint: SocketAddr = endpoint
            .parse()
            .map_err(|_| "endpoint is not ip:port".to_owned())?;
        let ip = endpoint.ip().to_canonical();
        if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
            return Err("endpoint address is not a routable unicast address".into());
        }
        let req = Self {
            private_key: PrivateKey::new(private_key),
            public_key: PublicKey(public_key.to_owned()),
            endpoint,
        };
        // NordLynx endpoints use one fixed port; `nordlynx` is the authority.
        if req.config().endpoint != endpoint {
            return Err("endpoint port is not the NordLynx port".into());
        }
        Ok(req)
    }

    fn config(&self) -> TunnelConfig {
        let server = Server {
            hostname: "helper.invalid".into(),
            station: self.endpoint.ip(),
            load: 0,
            public_key: self.public_key.clone(),
            // Unused by rendering.
            country: CountryCode::from_str("ZZ").unwrap_or_else(|_| unreachable!()),
        };
        TunnelConfig::nordlynx(self.private_key.clone(), &server)
    }

    fn render(&self) -> Zeroizing<String> {
        render_wg_conf(&self.config())
    }

    /// stdin payload: exactly three `name=value` lines.
    fn to_wire(&self) -> Zeroizing<String> {
        Zeroizing::new(format!(
            "private_key={}\npublic_key={}\nendpoint={}\n",
            self.private_key.expose(),
            self.public_key.0,
            self.endpoint
        ))
    }

    fn from_wire(text: &str) -> Result<Self, String> {
        let body = text
            .strip_suffix('\n')
            .ok_or("stdin must end with a newline")?;
        let mut lines = body.split('\n');
        let mut field = |name: &str| {
            lines
                .next()
                .and_then(|l| l.strip_prefix(name))
                .and_then(|l| l.strip_prefix('='))
                .ok_or_else(|| format!("stdin line {name}=... missing or out of order"))
        };
        let (private_key, public_key, endpoint) = (
            field("private_key")?,
            field("public_key")?,
            field("endpoint")?,
        );
        if lines.next().is_some() {
            return Err("stdin has extra lines".into());
        }
        Self::new(private_key, public_key, endpoint)
    }

    /// Recovers the fields from a config written by `render_wg_conf`, and
    /// accepts it only if re-rendering reproduces `text` byte for byte.
    fn from_rendered(text: &str) -> Result<Self, String> {
        let find = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key))
                .ok_or_else(|| format!("config has no {key:?} line"))
        };
        let req = Self::new(
            find("PrivateKey = ")?,
            find("PublicKey = ")?,
            find("Endpoint = ")?,
        )?;
        if *req.render() == text {
            Ok(req)
        } else {
            Err("config has lines wglane did not generate (PostUp?); regenerate it".into())
        }
    }
}

/// 44-char base64 of 32 bytes. Format check only: alphabet, length, padding.
fn is_wg_key(s: &str) -> bool {
    s.len() == 44
        && s.ends_with('=')
        && s.bytes()
            .take(43)
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// `^[A-Z]{2}$`, stricter than `CountryCode::from_str` (no trim, no lower case).
fn parse_cc(s: &str) -> Result<CountryCode, HelperError> {
    if s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase()) {
        s.parse().map_err(HelperError::Run)
    } else {
        Err(HelperError::usage(format!(
            "country code {s:?} is not two capital letters"
        )))
    }
}

fn parse_gateway(s: &str) -> Result<IpAddr, HelperError> {
    let ip: IpAddr = s
        .parse()
        .map_err(|_| HelperError::usage(format!("gateway {s:?} is not an IP address")))?;
    let ip = ip.to_canonical();
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return Err(HelperError::usage(format!(
            "gateway {s:?} is not a unicast address"
        )));
    }
    Ok(ip)
}

fn parse_cidr(s: &str) -> Result<IpNet, HelperError> {
    let net: IpNet = s
        .parse()
        .map_err(|_| HelperError::usage(format!("{s:?} is not ADDR/PREFIX")))?;
    if is_bad_bypass(&net) {
        return Err(HelperError::usage(format!(
            "bypass destination {s:?} is too broad or not unicast"
        )));
    }
    Ok(net.trunc())
}

fn is_cc_stem(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// Unprivileged side
// ---------------------------------------------------------------------------

/// Unprivileged side: implements [`Tunnel`] and [`RouteBypass`] by running
/// `sudo -n <HELPER_PATH> <verb> <args>`.
#[derive(Debug)]
pub struct HelperClient<R> {
    runner: R,
}

impl<R: CommandRunner> HelperClient<R> {
    /// Uses `runner` to call `sudo`.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    fn call(&self, args: &[&str], stdin: &[u8]) -> Result<CommandOutput> {
        let mut full = vec!["-n", HELPER_PATH];
        full.extend_from_slice(args);
        let out = self.runner.run_with_stdin(SUDO, &full, stdin)?;
        if out.success() {
            return Ok(out);
        }
        let mut stderr = out.stderr.trim().to_owned();
        if stderr.contains("password is required") || stderr.contains("terminal is required") {
            stderr.push_str("; run `sudo wglane install-helper`");
        }
        Err(Error::Command {
            program: "sudo wglane-helper".into(),
            status: out
                .code
                .map_or_else(|| "killed by signal".into(), |c| format!("exit {c}")),
            stderr,
        })
    }
}

impl<R: CommandRunner> Tunnel for HelperClient<R> {
    /// Sends the config's fields (not its path) to the helper. Refuses a file
    /// that `render_wg_conf` did not produce byte for byte.
    fn up(&self, conf: &Path) -> Result<()> {
        let cc = conf
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_uppercase)
            .ok_or_else(|| Error::InvalidCountryCode(conf.display().to_string()))?;
        parse_cc(&cc).map_err(|_| Error::InvalidCountryCode(cc.clone()))?;
        let file = fs::File::open(conf)
            .map_err(|e| Error::io(format!("opening {}", conf.display()), e))?;
        let mut buf = Zeroizing::new(Vec::new());
        file.take(MAX_CONF_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| Error::io(format!("reading {}", conf.display()), e))?;
        let text = std::str::from_utf8(&buf).map_err(|_| bad_conf(conf, "not UTF-8"))?;
        let req = UpRequest::from_rendered(text).map_err(|why| bad_conf(conf, &why))?;
        self.call(&[HelperVerb::Up.as_str(), &cc], req.to_wire().as_bytes())?;
        Ok(())
    }

    /// Tears down every wglane tunnel, not just `conf`'s: the helper knows
    /// only its own configs, never a caller-supplied path.
    fn down(&self, _conf: &Path) -> Result<()> {
        self.call(&[HelperVerb::Down.as_str()], &[])?;
        Ok(())
    }

    fn active(&self) -> Result<Vec<String>> {
        // /var/run/wireguard is world-readable: no sudo.
        WgQuick::new(&self.runner).active()
    }
}

fn bad_conf(conf: &Path, why: &str) -> Error {
    Error::Command {
        program: "wglane".into(),
        status: "refused".into(),
        stderr: format!("{}: {why}", conf.display()),
    }
}

impl<R: CommandRunner> RouteBypass for HelperClient<R> {
    fn physical_gateway(&self) -> Result<IpAddr> {
        RouteTable::new(&self.runner).physical_gateway()
    }

    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize> {
        if set.is_empty() {
            return Ok(0);
        }
        let gateway = gateway.to_string();
        let cidrs: Vec<String> = set.iter().map(ToString::to_string).collect();
        let mut args = vec![HelperVerb::Bypass.as_str(), &gateway];
        args.extend(cidrs.iter().map(String::as_str));
        let out = self.call(&args, &[])?;
        // The count is for display; if the helper's reply is odd, the set size is right.
        Ok(out.stdout.trim().parse().unwrap_or(set.len()))
    }

    /// No-op: the helper records what it installed and removes it on `down`
    /// (and removes stale entries on the next `bypass`). There is no
    /// standalone clear verb; see the open question in `docs/ARCHITECTURE.md`.
    fn clear(&self, _set: &BypassSet) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Root side
// ---------------------------------------------------------------------------

/// Why the helper stopped.
#[derive(Debug)]
enum HelperError {
    /// Bad argv or stdin: exit 2.
    Usage(String),
    /// A valid request that failed: exit 1.
    Run(Error),
}

impl HelperError {
    fn usage(msg: impl Into<String>) -> Self {
        Self::Usage(msg.into())
    }
}

impl From<Error> for HelperError {
    fn from(e: Error) -> Self {
        Self::Run(e)
    }
}

impl fmt::Display for HelperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(m) => write!(
                f,
                "{m}\nusage: wglane-helper up <CC> < fields | down | bypass <gateway> <cidr>..."
            ),
            Self::Run(e) => write!(f, "{e}"),
        }
    }
}

/// Root-side verb dispatcher. Generic over the ports so tests use fakes.
struct Helper<T, B> {
    tunnel: T,
    routes: B,
    state_dir: PathBuf,
    /// Required owner of `state_dir` (0 in production).
    owner: u32,
}

impl<T: Tunnel, B: RouteBypass> Helper<T, B> {
    /// Runs one invocation. `args` excludes argv[0]; returns text for stdout.
    fn run(&self, args: &[OsString], stdin: &mut impl Read) -> Result<String, HelperError> {
        let args: Vec<&str> = args
            .iter()
            .map(|a| {
                a.to_str()
                    .ok_or_else(|| HelperError::usage("argument is not UTF-8"))
            })
            .collect::<Result<_, _>>()?;
        let (verb, rest) = args
            .split_first()
            .ok_or_else(|| HelperError::usage("no verb"))?;
        match HelperVerb::from_arg(verb) {
            Some(HelperVerb::Up) => match rest {
                [cc] => self.up(parse_cc(cc)?, stdin).map(|()| String::new()),
                _ => Err(HelperError::usage("up takes exactly one argument")),
            },
            Some(HelperVerb::Down) if rest.is_empty() => self.down().map(|()| String::new()),
            Some(HelperVerb::Down) => Err(HelperError::usage("down takes no arguments")),
            Some(HelperVerb::Bypass) => match rest {
                [gateway, cidrs @ ..] if !cidrs.is_empty() && cidrs.len() <= MAX_CIDRS => {
                    let cidrs: BypassSet = cidrs
                        .iter()
                        .map(|c| parse_cidr(c))
                        .collect::<Result<_, _>>()?;
                    self.bypass(parse_gateway(gateway)?, &cidrs)
                        .map(|n| format!("{n}\n"))
                }
                _ => Err(HelperError::usage(
                    "bypass takes a gateway and 1..=1024 CIDRs",
                )),
            },
            None => Err(HelperError::usage(format!("unknown verb {verb:?}"))),
        }
    }

    fn up(&self, cc: CountryCode, stdin: &mut impl Read) -> Result<(), HelperError> {
        let mut buf = Zeroizing::new(Vec::with_capacity(MAX_STDIN + 1));
        stdin
            .take(MAX_STDIN as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| Error::io("reading stdin", e))?;
        let text = std::str::from_utf8(&buf)
            .ok()
            .filter(|_| buf.len() <= MAX_STDIN)
            .ok_or_else(|| HelperError::usage("stdin is not a short UTF-8 field list"))?;
        let req = UpRequest::from_wire(text).map_err(HelperError::Usage)?;

        self.ensure_dir()?;
        let path = self.conf_path(cc);
        write_private(&path, req.render().as_bytes())?;
        if let Err(e) = self.tunnel.up(&path) {
            // Do not leave the key at rest for a tunnel that never came up.
            let _ = fs::remove_file(&path);
            return Err(e.into());
        }
        Ok(())
    }

    fn down(&self) -> Result<(), HelperError> {
        let mut first_err: Option<Error> = None;
        let mut note = |r: Result<()>| {
            if let Err(e) = r {
                first_err.get_or_insert(e);
            }
        };
        if self.dir_exists()? {
            let active = self.tunnel.active()?;
            let entries = fs::read_dir(&self.state_dir)
                .map_err(|e| Error::io(format!("reading {}", self.state_dir.display()), e))?;
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                let path = entry.path();
                if name.ends_with(".conf.tmp") {
                    note(rm(&path));
                } else if let Some(stem) = name.strip_suffix(".conf").filter(|s| is_cc_stem(s)) {
                    // Keep the file if teardown failed, so a retry can still find it.
                    if active.iter().any(|a| a == stem)
                        && let Err(e) = self.tunnel.down(&path)
                    {
                        note(Err(e));
                        continue;
                    }
                    note(rm(&path));
                }
            }
            let old = self.load_bypass()?;
            note(self.routes.clear(&old));
            note(rm(&self.state_dir.join(BYPASS_STATE)));
        }
        first_err.map_or(Ok(()), |e| Err(e.into()))
    }

    fn bypass(&self, gateway: IpAddr, new: &BypassSet) -> Result<usize, HelperError> {
        // Routes may only go via the real physical gateway, never an arbitrary host.
        let real = self.routes.physical_gateway()?;
        if real != gateway {
            return Err(HelperError::Run(Error::Command {
                program: "wglane-helper".into(),
                status: "refused".into(),
                stderr: format!("{gateway} is not the physical gateway ({real})"),
            }));
        }
        // Refuse before recording or deleting anything.
        for net in new.iter() {
            check_dest(net, gateway)?;
        }
        self.ensure_dir()?;
        let old = self.load_bypass()?;
        // Record before installing, so a partial failure is still cleaned up by `down`.
        let json = serde_json::to_vec(new).map_err(|e| Error::Api(e.to_string()))?;
        write_private(&self.state_dir.join(BYPASS_STATE), &json)?;
        // Only routes recorded by an earlier bypass are ours to delete; `apply` never deletes.
        self.routes.clear(&old)?;
        Ok(self.routes.apply(new, gateway)?)
    }

    fn conf_path(&self, cc: CountryCode) -> PathBuf {
        self.state_dir.join(format!("{}.conf", cc.to_lowercase()))
    }

    fn load_bypass(&self) -> Result<BypassSet> {
        let path = self.state_dir.join(BYPASS_STATE);
        let raw = match fs::read(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BypassSet::new()),
            Err(e) => return Err(Error::io(format!("reading {}", path.display()), e)),
        };
        let nets: Vec<IpNet> = serde_json::from_slice(&raw)
            .map_err(|e| Error::Api(format!("{} is corrupt: {e}", path.display())))?;
        Ok(nets.into_iter().collect())
    }

    /// True if the state dir exists and is safe; error if it exists and is not.
    fn dir_exists(&self) -> Result<bool> {
        let meta = match fs::symlink_metadata(&self.state_dir) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                return Err(Error::io(
                    format!("checking {}", self.state_dir.display()),
                    e,
                ));
            }
        };
        if meta.is_dir() && meta.uid() == self.owner && meta.mode() & 0o777 == 0o700 {
            Ok(true)
        } else {
            Err(Error::Command {
                program: "wglane-helper".into(),
                status: "refused".into(),
                stderr: format!(
                    "{} must be a real directory owned by uid {}, mode 0700",
                    self.state_dir.display(),
                    self.owner
                ),
            })
        }
    }

    fn ensure_dir(&self) -> Result<()> {
        if !self.dir_exists()? {
            DirBuilder::new()
                .mode(0o700)
                .create(&self.state_dir)
                .map_err(|e| Error::io(format!("creating {}", self.state_dir.display()), e))?;
            self.dir_exists()?;
        }
        Ok(())
    }
}

fn rm(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(Error::io(format!("removing {}", path.display()), e))
        }
        _ => Ok(()),
    }
}

/// Writes `body` to `path` via `<path>.tmp` (`O_EXCL`, so a planted symlink is
/// refused), mode 0600, then renames over `path`.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let ctx = |what: &str| format!("{what} {}", path.display());
    rm(&tmp)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| Error::io(ctx("creating temp file for"), e))?;
    f.write_all(body)
        .and_then(|()| f.sync_all())
        .map_err(|e| Error::io(ctx("writing"), e))?;
    fs::rename(&tmp, path).map_err(|e| Error::io(ctx("installing"), e))
}

/// Runs only the few programs the helper needs, with a fixed `PATH` and
/// nothing else in the environment. `wg-quick` and its `bash` come only from
/// [`TOOL_DIR`], and only if the file and every ancestor is root-owned and not
/// group/world-writable at the moment of the call.
struct RootRunner;

impl RootRunner {
    /// The argv prefix (program, then fixed leading args) for `program`.
    /// `trust` turns a candidate path into a verified one or refuses it.
    fn resolve(
        program: &str,
        tool_dir: &Path,
        trust: impl Fn(&Path) -> Result<PathBuf>,
    ) -> Result<Vec<PathBuf>> {
        match program {
            // Explicit `bash wg-quick` so the script's `#!/usr/bin/env bash` is never consulted.
            "wg-quick" => ["bash", "wg-quick"]
                .iter()
                .map(|t| trust(&tool_dir.join(t)))
                .collect(),
            "route" => Ok(vec!["/sbin/route".into()]),
            "netstat" => Ok(vec!["/usr/sbin/netstat".into()]),
            other => Err(Error::Command {
                program: other.to_owned(),
                status: "refused".into(),
                stderr: "not on the helper's program allowlist".into(),
            }),
        }
    }
}

impl CommandRunner for RootRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let tool_dir = Path::new(TOOL_DIR);
        let prog = Self::resolve(program, tool_dir, |p| {
            trusted_tool(p, tool_dir, Path::new("/"), 0)
        })?;
        let out = Command::new(&prog[0])
            .args(&prog[1..])
            .args(args)
            .env_clear()
            .env("PATH", ROOT_PATH)
            .output()
            .map_err(|e| Error::io(format!("running `{}`", prog[0].display()), e))?;
        Ok(CommandOutput {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// Entry point of the `wglane-helper` binary. Exit 0 ok, 1 request failed,
/// 2 bad usage or not root.
pub fn helper_main() -> ExitCode {
    if !rustix::process::geteuid().is_root() {
        eprintln!("wglane-helper: must run as root (via `sudo -n`)");
        return ExitCode::from(2);
    }
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let helper = Helper {
        tunnel: WgQuick::new(RootRunner),
        routes: RouteTable::new(RootRunner),
        state_dir: PathBuf::from(STATE_DIR),
        owner: 0,
    };
    match helper.run(&args, &mut io::stdin().lock()) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e @ HelperError::Usage(_)) => {
            eprintln!("wglane-helper: {e}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("wglane-helper: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use wglane_core::testing::{FakeRoutes, FakeTunnel};

    use super::*;
    use crate::testing::FakeRunner;

    const PRIV: &str = "U1lOVEhFVElDLVBSSVZBVEUtS0VZLU5PVC1SRUFMMDE=";
    const PUB: &str = "U1lOVEhFVElDLVBFRVItUFVCTElDLUtFWS0wMDAwMDE=";
    const GW: &str = "192.168.1.1";

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn wire(endpoint: &str) -> String {
        format!("private_key={PRIV}\npublic_key={PUB}\nendpoint={endpoint}\n")
    }

    fn helper(dir: &Path) -> Helper<FakeTunnel, FakeRoutes> {
        Helper {
            tunnel: FakeTunnel::default(),
            routes: FakeRoutes::new(GW.parse().unwrap()),
            state_dir: dir.join("state"),
            owner: rustix::process::geteuid().as_raw(),
        }
    }

    fn run(h: &Helper<FakeTunnel, FakeRoutes>, a: &[&str], stdin: &str) -> Result<String, String> {
        h.run(&args(a), &mut Cursor::new(stdin.as_bytes().to_vec()))
            .map_err(|e| e.to_string())
    }

    fn conf_text() -> String {
        UpRequest::new(PRIV, PUB, "192.0.2.10:51820")
            .unwrap()
            .render()
            .to_string()
    }

    #[test]
    fn wire_round_trips() {
        let req = UpRequest::from_wire(&wire("192.0.2.10:51820")).unwrap();
        assert_eq!(*req.to_wire(), wire("192.0.2.10:51820"));
        assert!(UpRequest::from_wire(&wire("[2001:db8::10]:51820")).is_ok());
    }

    #[test]
    fn wire_rejects_injection_and_malformed_input() {
        let good = wire("192.0.2.10:51820");
        let mut bad: Vec<String> = vec![
            String::new(),
            "\n".into(),
            good.trim_end().to_owned(), // no trailing newline
            format!("{good}PostUp = /bin/sh\n"),
            format!("{good}\n"),
            good.replace("private_key", "PrivateKey"),
            good.replace('\n', "\r\n"),
            // fields out of order
            format!("public_key={PUB}\nprivate_key={PRIV}\nendpoint=192.0.2.10:51820\n"),
            // newline smuggled into a value
            wire("192.0.2.10:51820\nPostUp = id"),
            wire("192.0.2.10:51820 # x"),
            wire(" 192.0.2.10:51820"),
            // endpoint policy
            wire("0.0.0.0:51820"),
            wire("127.0.0.1:51820"),
            wire("224.0.0.1:51820"),
            wire("[::]:51820"),
            wire("[::1]:51820"),
            wire("192.0.2.10:51821"),
            wire("192.0.2.10:0"),
            wire("example.com:51820"),
            wire("192.0.2.10"),
        ];
        for key in [
            "",
            "short=",
            &format!("{PRIV}A"),
            &PRIV.replace('U', "\n"),
            &PRIV.replace('U', "$"),
            &PRIV.replace('=', "A"),
            &format!("{}\nPostUp = x=", &PRIV[..PRIV.len() - 1]),
        ] {
            bad.push(format!(
                "private_key={key}\npublic_key={PUB}\nendpoint=192.0.2.10:51820\n"
            ));
            bad.push(format!(
                "private_key={PRIV}\npublic_key={key}\nendpoint=192.0.2.10:51820\n"
            ));
        }
        for text in bad {
            assert!(UpRequest::from_wire(&text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn errors_never_echo_secret_values() {
        let bad = format!(
            "private_key={}\npublic_key={PUB}\nendpoint=192.0.2.10:51820\n",
            "!".repeat(44)
        );
        let msg = UpRequest::from_wire(&bad).err().unwrap();
        assert!(!msg.contains("!!!"));
    }

    #[test]
    fn cc_validator_table() {
        for ok in ["US", "ZA", "DE"] {
            assert!(parse_cc(ok).is_ok(), "rejected {ok}");
        }
        for bad in [
            "", "U", "usa", "us", "Us", "USA", "U1", " US", "US ", "US\n", "../", "..", "U/", "ÜS",
            "-h", "U\0", "US\nZA",
        ] {
            assert!(parse_cc(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn gateway_validator_table() {
        for ok in ["192.168.1.1", "10.0.0.1", "fe80::1", "2001:db8::1"] {
            assert!(parse_gateway(ok).is_ok(), "rejected {ok}");
        }
        for bad in [
            "",
            "0.0.0.0",
            "127.0.0.1",
            "224.0.0.1",
            "::",
            "::1",
            "ff02::1",
            "1.2.3",
            "1.2.3.4.5",
            "1.2.3.4/24",
            " 1.2.3.4",
            "1.2.3.4\n",
            "1.2.3.4 -host",
            "-net",
            "256.1.1.1",
            "fe80::1%en0",
            "localhost",
            "0x7f.1",
        ] {
            assert!(parse_gateway(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn cidr_validator_table() {
        for ok in [
            "1.2.3.4/32",
            "100.64.0.0/10",
            "10.0.0.0/8",
            "2001:db8::/32",
            "2001:db8::1/128",
        ] {
            assert!(parse_cidr(ok).is_ok(), "rejected {ok}");
        }
        assert_eq!(parse_cidr("10.1.2.3/8").unwrap().to_string(), "10.0.0.0/8");
        for bad in [
            "",
            "0.0.0.0/0",
            "0.0.0.0/32",
            "128.0.0.0/1",
            "10.0.0.0/7",
            "::/0",
            "2000::/3",
            "127.0.0.1/32",
            "224.0.0.0/24",
            "::1/128",
            "ff00::/16",
            "1.2.3.4",
            "1.2.3.4/33",
            "1.2.3.4/-1",
            "1.2.3.4/32 ",
            " 1.2.3.4/32",
            "1.2.3.4/32\n",
            "1.2.3.4/32;id",
            "-net/8",
            "1.2.3.4/255.255.255.255",
        ] {
            assert!(parse_cidr(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn argv_shape_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        for bad in [
            &[][..],
            &[""],
            &["UP"],
            &["up "],
            &["--help"],
            &["-h"],
            &["sh"],
            &["up"],
            &["up", "US", "ZA"],
            &["down", "x"],
            &["bypass"],
            &["bypass", GW],
            &["bypass", "x", "1.2.3.4/32"],
            &["bypass", GW, "0.0.0.0/0"],
            &["up", "../../etc"],
            &["up", "/tmp/x.conf"],
        ] {
            let err = run(&h, bad, &wire("192.0.2.10:51820")).unwrap_err();
            assert!(err.contains("usage:"), "{bad:?} gave {err}");
        }
        assert!(h.tunnel.up.borrow().is_empty());
        assert!(!h.state_dir.exists(), "rejected input must not touch disk");
    }

    #[test]
    fn non_utf8_argument_is_usage_error() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        let r = h.run(
            &[OsString::from("up"), OsString::from_vec(vec![0xff, 0xfe])],
            &mut io::empty(),
        );
        assert!(matches!(r, Err(HelperError::Usage(_))));
    }

    #[test]
    fn mapped_loopback_and_unspecified_endpoints_are_refused() {
        for ep in [
            "[::ffff:127.0.0.1]:51820",
            "[::ffff:0.0.0.0]:51820",
            "[::ffff:224.0.0.1]:51820",
        ] {
            assert!(UpRequest::from_wire(&wire(ep)).is_err(), "{ep}");
        }
        assert!(parse_gateway("::ffff:127.0.0.1").is_err());
        assert_eq!(
            parse_gateway("::ffff:192.0.2.1").unwrap().to_string(),
            "192.0.2.1"
        );
    }

    #[test]
    fn bypass_refuses_gateway_covering_cidr_before_touching_state() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        let err = run(&h, &["bypass", GW, "1.2.3.4/32", "0.0.0.0/0"], "").unwrap_err();
        assert!(err.contains("usage:"), "{err}");
        let net = format!("{GW}/32");
        let err = run(&h, &["bypass", GW, "1.2.3.4/32", &net], "").unwrap_err();
        assert!(err.contains("contains the gateway"), "{err}");
        assert!(!h.state_dir.exists());
        assert!(h.routes.installed.borrow().is_empty());
    }

    #[test]
    fn oversized_stdin_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        // Valid fields plus padding: the size is the only thing wrong.
        let big = format!("{}{}", wire("192.0.2.10:51820"), "\n".repeat(MAX_STDIN));
        let err = run(&h, &["up", "ZA"], &big).unwrap_err();
        assert!(err.contains("not a short UTF-8 field list"), "{err}");
        assert!(h.tunnel.up.borrow().is_empty());
    }

    #[test]
    fn up_renders_root_copy_with_0600_and_brings_it_up() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).unwrap();
        let path = h.state_dir.join("za.conf");
        assert_eq!(fs::read_to_string(&path).unwrap(), conf_text());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&h.state_dir).unwrap().mode() & 0o777, 0o700);
        assert_eq!(*h.tunnel.up.borrow(), ["za"]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("PostUp") && !text.contains("DNS"));
    }

    #[test]
    fn up_failure_removes_the_rendered_key() {
        struct Failing;
        impl Tunnel for Failing {
            fn up(&self, _: &Path) -> Result<()> {
                Err(Error::NotImplemented("x"))
            }
            fn down(&self, _: &Path) -> Result<()> {
                Ok(())
            }
            fn active(&self) -> Result<Vec<String>> {
                Ok(vec![])
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let h = Helper {
            tunnel: Failing,
            routes: FakeRoutes::new(GW.parse().unwrap()),
            state_dir: dir.path().join("state"),
            owner: rustix::process::geteuid().as_raw(),
        };
        let r = h.run(
            &args(&["up", "ZA"]),
            &mut Cursor::new(wire("192.0.2.10:51820").into_bytes()),
        );
        assert!(matches!(r, Err(HelperError::Run(_))));
        assert!(!h.state_dir.join("za.conf").exists());
    }

    #[test]
    fn insecure_or_foreign_state_dir_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = helper(dir.path());
        fs::create_dir(&h.state_dir).unwrap();
        fs::set_permissions(
            &h.state_dir,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        assert!(run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).is_err());
        fs::remove_dir(&h.state_dir).unwrap();
        // A symlink to a fine directory is still refused.
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &h.state_dir).unwrap();
        assert!(run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).is_err());
        fs::remove_file(&h.state_dir).unwrap();
        // Wrong owner.
        h.owner = rustix::process::geteuid().as_raw() + 1;
        assert!(run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).is_err());
        assert!(h.tunnel.up.borrow().is_empty());
    }

    #[test]
    fn planted_tmp_symlink_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, h.state_dir.join("za.conf.tmp")).unwrap();
        run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    }

    #[test]
    fn down_tears_down_only_wglane_tunnels_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        run(&h, &["up", "ZA"], &wire("192.0.2.10:51820")).unwrap();
        h.tunnel.up.borrow_mut().push("home".into()); // someone else's tunnel
        run(&h, &["bypass", GW, "100.64.0.0/10"], "").unwrap();
        run(&h, &["down"], "").unwrap();
        assert_eq!(*h.tunnel.up.borrow(), ["home"]);
        assert!(h.routes.installed.borrow().is_empty());
        let left: Vec<_> = fs::read_dir(&h.state_dir).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn down_without_state_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        run(&h, &["down"], "").unwrap();
        assert!(!h.state_dir.exists());
    }

    #[test]
    fn bypass_requires_the_physical_gateway_and_replaces_stale_routes() {
        let dir = tempfile::tempdir().unwrap();
        let h = helper(dir.path());
        let err = run(&h, &["bypass", "192.168.1.99", "1.2.3.4/32"], "").unwrap_err();
        assert!(err.contains("not the physical gateway"), "{err}");
        assert!(h.routes.installed.borrow().is_empty());

        assert_eq!(
            run(&h, &["bypass", GW, "1.2.3.4/32", "5.6.7.8/32"], "").unwrap(),
            "2\n"
        );
        assert_eq!(
            run(&h, &["bypass", GW, "5.6.7.8/32", "9.9.9.9/32"], "").unwrap(),
            "2\n"
        );
        let nets: Vec<String> = h
            .routes
            .installed
            .borrow()
            .iter()
            .map(|(n, _)| n.to_string())
            .collect();
        assert_eq!(nets, ["5.6.7.8/32", "9.9.9.9/32"]);
    }

    #[test]
    fn root_runner_resolves_wg_quick_only_inside_the_tool_dir() {
        let dir = Path::new(TOOL_DIR);
        let trust = |p: &Path| Ok(p.to_path_buf());
        assert_eq!(
            RootRunner::resolve("route", dir, trust).unwrap(),
            [Path::new("/sbin/route")]
        );
        assert_eq!(
            RootRunner::resolve("wg-quick", dir, trust).unwrap(),
            [
                Path::new("/usr/local/libexec/wglane/bin/bash"),
                Path::new("/usr/local/libexec/wglane/bin/wg-quick")
            ]
        );
        // A refusal by the trust check is final: no fallback to another location.
        let refuse = |p: &Path| {
            Err(Error::io(
                format!("untrusted {}", p.display()),
                io::ErrorKind::PermissionDenied.into(),
            ))
        };
        assert!(RootRunner::resolve("wg-quick", dir, refuse).is_err());
        for bad in ["sh", "/bin/sh", "./wg-quick", "wg-quick ", "rm", "bash"] {
            assert!(
                RootRunner::resolve(bad, dir, trust).is_err(),
                "resolved {bad:?}"
            );
        }
    }

    #[test]
    fn root_path_has_no_user_writable_entries() {
        assert!(ROOT_PATH.starts_with(TOOL_DIR));
        for entry in ROOT_PATH.split(':').skip(1) {
            assert!(
                ["/usr/bin", "/bin", "/usr/sbin", "/sbin"].contains(&entry),
                "{entry}"
            );
        }
        assert!(!ROOT_PATH.contains("homebrew") && !ROOT_PATH.contains("/usr/local/bin"));
    }

    // ---- client ----

    fn write_conf(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn client_up_sends_fields_on_stdin_and_only_the_cc_on_argv() {
        let dir = tempfile::tempdir().unwrap();
        let conf = write_conf(dir.path(), "za.conf", &conf_text());
        let client = HelperClient::new(FakeRunner::new());
        client.up(&conf).unwrap();
        assert_eq!(
            client.runner.calls(),
            [vec![SUDO, "-n", HELPER_PATH, "up", "ZA"]]
        );
        assert_eq!(
            client.runner.stdins(),
            [wire("192.0.2.10:51820").into_bytes()]
        );
        let argv = client.runner.calls().concat().join(" ");
        assert!(!argv.contains(PRIV) && !argv.contains("za.conf"));
    }

    #[test]
    fn client_up_refuses_configs_wglane_did_not_generate() {
        let dir = tempfile::tempdir().unwrap();
        let good = conf_text();
        let cases = [
            ("us.conf", format!("{good}PostUp = /bin/sh -c id\n")),
            (
                "us.conf",
                good.replace("[Interface]\n", "[Interface]\nPreUp = id\n"),
            ),
            ("us.conf", good.replace("0.0.0.0/0", "0.0.0.0/0, ::/0")),
            ("us.conf", good.replace("51820", "1")),
            (
                "us.conf",
                good.replace("PrivateKey = ", "PostDown = id\nPrivateKey = "),
            ),
            ("us.conf", String::new()),
            ("us.conf", "\0\u{ff}".into()),
            ("usa.conf", good.clone()),
            ("u.conf", good.clone()),
            ("..conf", good.clone()),
        ];
        for (name, text) in cases {
            let client = HelperClient::new(FakeRunner::new());
            let conf = write_conf(dir.path(), name, &text);
            assert!(client.up(&conf).is_err(), "accepted {name}: {text:?}");
            assert_eq!(client.runner.calls(), Vec::<Vec<String>>::new());
        }
        let client = HelperClient::new(FakeRunner::new());
        assert!(client.up(&dir.path().join("missing.conf")).is_err());
    }

    #[test]
    fn client_maps_sudo_password_failure_to_install_hint() {
        let dir = tempfile::tempdir().unwrap();
        let conf = write_conf(dir.path(), "za.conf", &conf_text());
        let runner = FakeRunner::new();
        runner.push(CommandOutput {
            code: Some(1),
            stderr: "sudo: a password is required\n".into(),
            ..Default::default()
        });
        let err = HelperClient::new(runner).up(&conf).unwrap_err().to_string();
        assert!(err.contains("run `sudo wglane install-helper`"), "{err}");
    }

    #[test]
    fn client_down_calls_down_verb() {
        let client = HelperClient::new(FakeRunner::new());
        client.down(Path::new("/anything/za.conf")).unwrap();
        assert_eq!(
            client.runner.calls(),
            [vec![SUDO, "-n", HELPER_PATH, "down"]]
        );
    }

    #[test]
    fn client_apply_passes_gateway_and_cidrs_and_reads_count() {
        let client = HelperClient::new(FakeRunner::new());
        client.runner.push_stdout("2\n");
        let set: BypassSet = ["100.64.0.0/10", "1.2.3.4/32"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(client.apply(&set, GW.parse().unwrap()).unwrap(), 2);
        let call = &client.runner.calls()[0];
        assert_eq!(call[..5], [SUDO, "-n", HELPER_PATH, "bypass", GW]);
        assert_eq!(call[5..].len(), 2);
        assert!(call.iter().any(|a| a == "100.64.0.0/10"));
        // Empty set never calls sudo.
        let empty = HelperClient::new(FakeRunner::new());
        assert_eq!(
            empty.apply(&BypassSet::new(), GW.parse().unwrap()).unwrap(),
            0
        );
        assert_eq!(empty.runner.calls(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn client_clear_is_a_noop() {
        let client = HelperClient::new(FakeRunner::new());
        client.clear(&BypassSet::new()).unwrap();
        assert_eq!(client.runner.calls(), Vec::<Vec<String>>::new());
    }
}

/// Properties of the private validators that run as root.
#[cfg(test)]
mod props {
    use proptest::prelude::*;

    use super::*;

    const PRIV: &str = "U1lOVEhFVElDLVBSSVZBVEUtS0VZLU5PVC1SRUFMMDE=";
    const PUB: &str = "U1lOVEhFVElDLVBFRVItUFVCTElDLUtFWS0wMDAwMDE=";

    fn cfg() -> ProptestConfig {
        ProptestConfig::with_cases(512)
    }

    fn key() -> impl Strategy<Value = String> {
        "[A-Za-z0-9+/]{43}=".prop_map(String::from)
    }

    /// Text with at least one char the helper must never let through.
    fn hostile() -> impl Strategy<Value = String> {
        (
            "[ -~]{0,12}",
            prop::sample::select(vec!["\n", "\r", "\0", " ", "\t", "\u{a0}", "\u{2028}"]),
            "[ -~]{0,12}",
        )
            .prop_map(|(a, c, b)| format!("{a}{c}{b}"))
    }

    proptest! {
        #![proptest_config(cfg())]

        // Accepted fields only ever fill the template's three value slots.
        #[test]
        fn accepted_request_renders_only_the_template(
            private in key(), public in key(), ip in any::<u32>(), port in any::<u16>(),
        ) {
            let endpoint = format!("{}:{port}", std::net::Ipv4Addr::from(ip));
            let Ok(req) = UpRequest::new(&private, &public, &endpoint) else { return Ok(()) };
            let base = UpRequest::new(PRIV, PUB, "192.0.2.10:51820").unwrap().render();
            let want = base
                .replace(PRIV, &private)
                .replace(PUB, &public)
                .replace("192.0.2.10:51820", &req.endpoint.to_string());
            prop_assert_eq!(&**req.render(), want.as_str());
        }

        // Arbitrary stdin: acceptance means the text is exactly our own wire format.
        #[test]
        fn from_wire_accepts_only_canonical_text(text in ".{0,200}") {
            if let Ok(req) = UpRequest::from_wire(&text) {
                prop_assert_eq!(&**req.to_wire(), text.as_str());
            }
        }

        // Structured near-misses of the wire format are refused or round-trip.
        #[test]
        fn from_wire_mutated_fields(
            private in "[ -~]{0,50}", public in "[ -~]{0,50}", endpoint in "[ -~]{0,30}",
        ) {
            let text = format!("private_key={private}\npublic_key={public}\nendpoint={endpoint}\n");
            if let Ok(req) = UpRequest::from_wire(&text) {
                prop_assert_eq!(&**req.to_wire(), text.as_str());
            }
        }

        #[test]
        fn hostile_fields_are_rejected(bad in hostile()) {
            prop_assert!(UpRequest::new(&bad, PUB, "192.0.2.10:51820").is_err());
            prop_assert!(UpRequest::new(PRIV, &bad, "192.0.2.10:51820").is_err());
            prop_assert!(UpRequest::new(PRIV, PUB, &bad).is_err());
            prop_assert!(parse_cc(&bad).is_err());
            prop_assert!(parse_gateway(&bad).is_err());
            prop_assert!(parse_cidr(&bad).is_err());
        }

        #[test]
        fn path_like_tokens_are_rejected(a in "[ -~]{0,8}", b in "[ -~]{0,8}") {
            for sep in ["..", "/"] {
                let s = format!("{a}{sep}{b}");
                prop_assert!(parse_cc(&s).is_err(), "cc {s:?}");
                prop_assert!(parse_gateway(&s).is_err(), "gateway {s:?}");
            }
            let s = format!("{a}..{b}");
            prop_assert!(parse_cidr(&s).is_err(), "cidr {s:?}");
        }

        #[test]
        fn cc_accepts_only_two_capitals(s in "\\PC{0,4}") {
            prop_assert_eq!(
                parse_cc(&s).is_ok(),
                s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
            );
        }

        #[test]
        fn gateway_is_canonical_unicast(s in "[0-9a-f:.]{0,20}") {
            if let Ok(ip) = parse_gateway(&s) {
                prop_assert!(!ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast());
                prop_assert_eq!(s.parse::<IpAddr>().unwrap().to_canonical(), ip);
            }
        }

        // Whatever the input, an accepted CIDR is a canonical, specific unicast network.
        #[test]
        fn cidr_result_is_truncated_and_specific(s in "[0-9a-f:./]{0,45}") {
            if let Ok(net) = parse_cidr(&s) {
                prop_assert_eq!(net, net.trunc());
                prop_assert!(!is_bad_bypass(&net));
            }
        }

        #[test]
        fn cidr_never_yields_an_unspecified_network(a in any::<u32>(), p in 0u8..=32) {
            let s = format!("{}/{p}", std::net::Ipv4Addr::from(a));
            if let Ok(net) = parse_cidr(&s) {
                prop_assert!(!net.addr().is_unspecified() && net.prefix_len() >= 8);
            }
        }
    }
}
