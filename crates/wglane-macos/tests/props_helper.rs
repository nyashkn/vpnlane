//! Property tests for what the root helper accepts: the client-side config
//! validation (shared with the helper's `UpRequest::new`), the config
//! renderer, the `route` argv builders and the netstat parser.
//!
//! Only the public API is reachable from here, so the helper's private
//! validators are exercised through `HelperClient::up` and `RouteTable`.

#![allow(clippy::unwrap_used, reason = "tests; a panic is the failure signal")]

use std::cell::RefCell;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use ipnet::IpNet;
use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence, RngAlgorithm, RngSeed};
use wglane_core::ports::{RouteBypass, Tunnel};
use wglane_core::{BypassSet, PrivateKey, PublicKey, Result, Server, TunnelConfig, render_wg_conf};
use wglane_macos::{CommandOutput, CommandRunner, HELPER_PATH, HelperClient, RouteTable};

/// Fixed seed: CI is deterministic; shrinking and failure persistence stay on.
fn cfg() -> Config {
    Config {
        cases: 512,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(0x48_45_4c_50_45_52),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/props_helper.proptest-regressions",
        ))),
        ..Config::default()
    }
}

/// Records every call; answers with a fixed stdout and exit 0.
#[derive(Default)]
struct Recorder {
    stdout: String,
    calls: RefCell<Vec<Vec<String>>>,
    stdins: RefCell<Vec<Vec<u8>>>,
}

impl Recorder {
    fn with_stdout(stdout: &str) -> Self {
        Self {
            stdout: stdout.into(),
            ..Self::default()
        }
    }
}

impl CommandRunner for Recorder {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|a| (*a).to_owned()));
        self.calls.borrow_mut().push(call);
        Ok(CommandOutput {
            code: Some(0),
            stdout: self.stdout.clone(),
            stderr: String::new(),
        })
    }

    fn run_with_stdin(&self, program: &str, args: &[&str], stdin: &[u8]) -> Result<CommandOutput> {
        self.stdins.borrow_mut().push(stdin.to_vec());
        self.run(program, args)
    }
}

const BANNED: [&str; 8] = [
    "PostUp",
    "PreUp",
    "PostDown",
    "PreDown",
    "SaveConfig",
    "Table",
    "DNS",
    "Script",
];

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn wg_key() -> impl Strategy<Value = String> {
    "[A-Za-z0-9+/]{43}=".prop_map(String::from)
}

/// Mostly-valid keys, corrupted in the ways an attacker would try.
fn hostile_key() -> impl Strategy<Value = String> {
    let junk = prop::sample::select(vec![
        "\n", "\r", "\0", "=", "[", "]", " ", "\t", "..", "/", "\\", "\u{2028}", "é", ";", "#",
    ]);
    prop_oneof![
        3 => wg_key(),
        2 => (wg_key(), 0usize..44, junk.clone()).prop_map(|(mut k, i, j)| {
            // Replace char i (all ASCII) by a hostile string.
            k.replace_range(i..=i, j);
            k
        }),
        2 => (wg_key(), prop::sample::select(BANNED.to_vec()), "[ -~]{0,16}")
            .prop_map(|(k, kw, v)| format!("{k}\n{kw} = {v}")),
        1 => (wg_key(), junk).prop_map(|(k, j)| format!("{k}{j}")),
        2 => ".{0,60}",
    ]
}

fn station() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        any::<u32>().prop_map(|n| IpAddr::V4(Ipv4Addr::from(n))),
        any::<u128>().prop_map(|n| IpAddr::V6(n.into())),
        prop::sample::select(vec![
            "0.0.0.0",
            "127.0.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "ff02::1",
        ])
        .prop_map(|s| s.parse().unwrap()),
    ]
}

/// Valid configs, valid configs with one char replaced, and noise.
fn conf_like() -> impl Strategy<Value = String> {
    let valid = || {
        (wg_key(), wg_key(), 0x0100_0000u32..0xdf00_0000)
            .prop_map(|(k, p, n)| conf_text(&k, &p, IpAddr::V4(Ipv4Addr::from(n))))
    };
    prop_oneof![
        2 => valid(),
        1 => (valid(), any::<prop::sample::Index>(), any::<char>()).prop_map(|(t, at, c)| {
            let mut chars: Vec<char> = t.chars().collect();
            let at = at.index(chars.len());
            chars[at] = c;
            chars.into_iter().collect()
        }),
        1 => ".{0,400}",
    ]
}

fn conf_text(private: &str, public: &str, ip: IpAddr) -> String {
    let server = Server {
        hostname: "h.invalid".into(),
        station: ip,
        load: 0,
        public_key: PublicKey(public.to_owned()),
        country: "ZZ".parse().expect("valid country code"),
    };
    let conf = TunnelConfig::nordlynx(PrivateKey::new(private), &server);
    render_wg_conf(&conf).as_str().to_owned()
}

/// Writes `text` to `<tmp>/<stem>.conf` and runs `HelperClient::up` on it.
fn run_up(stem: &str, text: &str) -> (Result<()>, Recorder) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{stem}.conf"));
    std::fs::write(&path, text).expect("stem is a legal file name");
    let rec = Recorder::default();
    let res = HelperClient::new(&rec).up(&path);
    (res, rec)
}

fn is_wg_key(s: &str) -> bool {
    s.len() == 44
        && s.ends_with('=')
        && s[..43]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

fn routable(ip: IpAddr) -> bool {
    !(ip.is_unspecified() || ip.is_loopback() || ip.is_multicast())
}

fn check_conf(text: &str, private: &str, public: &str, ip: IpAddr) -> Result<(), TestCaseError> {
    let (res, rec) = run_up("us", text);
    let should_pass = is_wg_key(private) && is_wg_key(public) && routable(ip);
    prop_assert_eq!(res.is_ok(), should_pass, "conf:\n{}", text);
    if !should_pass {
        prop_assert!(rec.calls.borrow().is_empty() && rec.stdins.borrow().is_empty());
        return Ok(());
    }
    let calls = rec.calls.borrow();
    let stdins = rec.stdins.borrow();
    prop_assert_eq!(calls.len(), 1);
    prop_assert_eq!(stdins.len(), 1);
    let wire = std::str::from_utf8(&stdins[0]).expect("utf-8 wire");
    let body = wire.strip_suffix('\n').expect("trailing newline");
    let lines: Vec<&str> = body.split('\n').collect();
    prop_assert_eq!(lines.len(), 3, "wire:\n{}", wire);
    let (k, p, e) = (
        lines[0]
            .strip_prefix("private_key=")
            .expect("private_key line"),
        lines[1]
            .strip_prefix("public_key=")
            .expect("public_key line"),
        lines[2].strip_prefix("endpoint=").expect("endpoint line"),
    );
    // (7) Round trip: the fields we rendered are the fields the helper gets.
    prop_assert_eq!((k, p), (private, public));
    let endpoint: SocketAddr = e.parse().expect("endpoint parses");
    prop_assert_eq!(endpoint.ip(), ip);
    prop_assert_eq!(endpoint.port(), 51820);
    // Nothing but the template: re-rendering the wire fields gives the file.
    prop_assert_eq!(conf_text(k, p, ip), text);
    prop_assert!(!BANNED.iter().any(|b| text.contains(b)));
    Ok(())
}

/// Mostly arbitrary nets, with extra weight on the edges that matter.
fn any_net() -> impl Strategy<Value = IpNet> {
    let v4_addr = prop_oneof![
        3 => any::<u32>(),
        2 => 0u32..256,                       // 0.0.0.x
        1 => (0u32..256).prop_map(|n| 0x7f00_0000 | n),
        1 => (0u32..256).prop_map(|n| 0xe000_0000 | n),
        1 => Just(u32::MAX),
    ];
    let v6_addr = prop_oneof![
        3 => any::<u128>(),
        2 => 0u128..256,                      // ::x
        1 => Just(1u128),
        1 => (0u128..256).prop_map(|n| 0xff00u128 << 112 | n),
    ];
    prop_oneof![
        (v4_addr, 0u8..=32).prop_map(|(a, p)| IpNet::new(Ipv4Addr::from(a).into(), p).unwrap()),
        (v6_addr, 0u8..=128).prop_map(|(a, p)| IpNet::new(Ipv6Addr::from(a).into(), p).unwrap()),
    ]
}

fn gateway() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        any::<u32>().prop_map(|n| IpAddr::V4(Ipv4Addr::from(n))),
        any::<u128>().prop_map(|n| IpAddr::V6(n.into())),
        prop::sample::select(vec![
            "0.0.0.0",
            "127.0.0.1",
            "224.0.0.9",
            "::",
            "::1",
            "ff02::1"
        ])
        .prop_map(|s| s.parse().unwrap()),
        Just("192.0.2.1".parse().unwrap()),
    ]
}

/// Checks every recorded `route` call. `strict_default` also rejects a
/// destination that was truncated to the unspecified address (`0.0.0.0/N`).
fn check_route_calls(
    rec: &Recorder,
    ok: bool,
    gw: IpAddr,
    is_apply: bool,
    strict_default: bool,
) -> Result<(), TestCaseError> {
    let calls = rec.calls.borrow();
    if !ok {
        prop_assert!(calls.is_empty(), "rejected request still ran {calls:?}");
        return Ok(());
    }
    for call in calls.iter() {
        prop_assert_eq!(&call[..2], ["route", "-n"]);
        let verb = call[2].as_str();
        prop_assert!(["add", "delete"].contains(&verb), "verb {verb}");
        let mut rest = &call[3..];
        if rest[0] == "-inet6" {
            rest = &rest[1..];
        }
        let dest: IpNet = match rest[0].as_str() {
            "-net" => rest[1].parse().expect("net parses"),
            "-host" => IpNet::from(rest[1].parse::<IpAddr>().expect("host parses")),
            other => return Err(TestCaseError::fail(format!("flag {other}"))),
        };
        let ip = dest.addr();
        prop_assert!(dest.prefix_len() > 0, "default route: {call:?}");
        prop_assert!(
            !(ip.is_loopback() || ip.is_multicast()),
            "bad destination: {call:?}"
        );
        prop_assert!(ip != IpAddr::V4(Ipv4Addr::BROADCAST), "broadcast: {call:?}");
        if strict_default {
            prop_assert!(!ip.is_unspecified(), "default-ish route: {call:?}");
        }
        if verb == "add" {
            prop_assert_eq!(rest.len(), 3);
            prop_assert_eq!(rest[2].parse::<IpAddr>().ok(), Some(gw));
            prop_assert!(routable(gw));
        } else {
            prop_assert_eq!(rest.len(), 2);
        }
    }
    prop_assert!(is_apply || calls.iter().all(|c| c[2] == "delete"));
    // `apply` only adds: it must never delete a route it did not install.
    prop_assert!(!is_apply || calls.iter().all(|c| c[2] == "add"));
    Ok(())
}

/// Plausible `netstat -rn` rows mixed with arbitrary text.
fn netstat_like() -> impl Strategy<Value = String> {
    let row = prop_oneof![
        3 => (
            prop_oneof!["default", "0/1", "10.0.0.0/8", "[a-z0-9./]{1,10}"],
            prop_oneof![
                any::<u32>().prop_map(|n| Ipv4Addr::from(n).to_string()),
                "link#[0-9]{1,2}",
                "[0-9a-f:]{2,12}",
                "\\PC{0,8}",
            ],
            "[UGSCHIcg]{1,8}",
            prop_oneof!["utun[0-9]{1,2}", "en[0-9]", "lo0", "\\PC{0,6}"],
            prop::option::of("[0-9]{1,3}"),
        )
            .prop_map(|(d, g, f, n, e)| format!("{d} {g} {f} {n} {}", e.unwrap_or_default())),
        1 => "\\PC{0,40}",
        1 => Just(String::new()),
    ];
    prop::collection::vec(row, 0..10).prop_map(|rows| rows.join("\n"))
}

fn check_routes(nets: &[IpNet], gw: IpAddr, strict: bool) -> Result<(), TestCaseError> {
    let set: BypassSet = nets.iter().copied().collect();
    let rec = Recorder::default();
    let applied = RouteTable::new(&rec).apply(&set, gw);
    check_route_calls(&rec, applied.is_ok(), gw, true, strict)?;
    prop_assert!(applied.is_err() || nets.iter().all(|n| !n.contains(&gw)));
    let rec = Recorder::default();
    let cleared = RouteTable::new(&rec).clear(&set);
    check_route_calls(&rec, cleared.is_ok(), gw, false, strict)
}

proptest! {
    #![proptest_config(cfg())]

    // (1)(2)(7) Whatever bytes sit in a conf file, if the client accepts it the
    // helper receives exactly three well-formed fields, and the file was
    // byte-for-byte what `render_wg_conf` makes from them (so no PostUp & co).
    #[test]
    fn accepted_conf_is_exactly_the_template(
        private in hostile_key(), public in hostile_key(), ip in station(),
    ) {
        let text = conf_text(&private, &public, ip);
        check_conf(&text, &private, &public, ip)?;
    }

    // (1)(2) Injecting any line into a valid conf makes the client refuse it.
    #[test]
    fn injected_line_is_refused(
        private in wg_key(), public in wg_key(), n in any::<u32>(),
        kw in prop::sample::select(BANNED.to_vec()),
        value in "[ -~]{0,24}", pos in any::<prop::sample::Index>(),
    ) {
        let ip = IpAddr::V4(Ipv4Addr::from(n | 0x0100_0000));
        prop_assume!(ip.is_ipv4() && !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast());
        let valid = conf_text(&private, &public, ip);
        let mut lines: Vec<&str> = valid.lines().collect();
        let evil = format!("{kw} = {value}");
        lines.insert(pos.index(lines.len() + 1), &evil);
        let tampered = lines.join("\n") + "\n";
        let (res, rec) = run_up("us", &tampered);
        prop_assert!(res.is_err(), "accepted tampered conf:\n{tampered}");
        prop_assert!(rec.calls.borrow().is_empty() && rec.stdins.borrow().is_empty());
    }

    // (2) Arbitrary conf-file text: acceptance implies byte-exact template.
    #[test]
    // Re-check by hand: `prop_assert!(false)` in the `is_ok` branch must fail.
    fn arbitrary_text_is_never_accepted_unless_template(text in conf_like()) {
        let (res, rec) = run_up("us", &text);
        if res.is_ok() {
            prop_assert!(!BANNED.iter().any(|b| text.contains(b)));
            // The one accepted shape is the template over the fields sent on.
            let stdins = rec.stdins.borrow();
            let wire = std::str::from_utf8(&stdins[0]).expect("utf-8 wire");
            let f: Vec<&str> = wire.lines().map(|l| l.split_once('=').expect("name=value").1).collect();
            let endpoint: SocketAddr = f[2].parse().expect("endpoint");
            prop_assert_eq!(conf_text(f[0], f[1], endpoint.ip()), text);
        }
    }

    // (2) The country code comes from the file stem; only two ASCII letters
    // may reach the helper's argv, upper-cased, and nothing else is run.
    #[test]
    fn stem_must_be_two_letters(stem in prop_oneof!["[ -.0-~]{0,6}", "[a-zA-Z]{2}", "(é|世|ß|ǅ){1,2}", "[A-Za-z]{1,3}\\.{1,2}"]) {
        let valid = conf_text(KEY_A, KEY_B, "192.0.2.10".parse().unwrap());
        let (res, rec) = run_up(&stem, &valid);
        let calls = rec.calls.borrow();
        if res.is_ok() {
            prop_assert!(stem.len() == 2 && stem.bytes().all(|b| b.is_ascii_alphabetic()));
            let want = vec![
                "/usr/bin/sudo".to_owned(), "-n".to_owned(), HELPER_PATH.to_owned(),
                "up".to_owned(), stem.to_ascii_uppercase(),
            ];
            prop_assert_eq!(&*calls, &vec![want]);
        } else {
            prop_assert!(calls.is_empty());
        }
    }

    // (3) Route argv: nothing emitted can be a default, loopback, multicast or
    // broadcast destination, and a rejected set runs nothing at all.
    #[test]
    fn route_argv_never_hijacks(
        nets in prop::collection::vec(any_net(), 0..8), gw in gateway(),
    ) {
        check_routes(&nets, gw, false)?;
    }

    // (3, strict) Also nothing truncates to an unspecified-address network.
    #[test]
    fn route_argv_never_unspecified_after_trunc(
        nets in prop::collection::vec(any_net(), 0..8), gw in gateway(),
    ) {
        check_routes(&nets, gw, true)?;
    }

    // (4) The netstat parser never panics, and returns exactly the first
    // non-utun `default` row whose gateway is an IPv4 address.
    #[test]
    fn netstat_gateway_parser(text in netstat_like()) {
        let rec = Recorder::with_stdout(&text);
        let table = RouteTable::new(&rec);
        let got = table.physical_gateway();
        let want = text.lines().find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            match f[..] {
                ["default", gw, _, netif, ..] if !netif.starts_with("utun") => gw.parse::<Ipv4Addr>().ok(),
                _ => None,
            }
        });
        match (got, want) {
            (Ok(IpAddr::V4(g)), Some(w)) => prop_assert_eq!(g, w),
            (Err(_), None) => {}
            (g, w) => prop_assert!(false, "got {g:?}, wanted {w:?}"),
        }
    }
}

// Guard: the property above must keep reaching its accepted branch.
#[test]
fn conf_like_reaches_the_accepted_branch() {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let strategy = conf_like();
    let n = (0..256)
        .filter(|_| {
            run_up("us", &strategy.new_tree(&mut runner).unwrap().current())
                .0
                .is_ok()
        })
        .count();
    assert!(n >= 64, "accepted branch reached {n}/256 times");
}

const KEY_A: &str = "U1lOVEhFVElDLVBSSVZBVEUtS0VZLU5PVC1SRUFMMDE=";
const KEY_B: &str = "U1lOVEhFVElDLVBFRVItUFVCTElDLUtFWS0wMDAwMDE=";
