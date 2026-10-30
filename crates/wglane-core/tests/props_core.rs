//! Property tests for the pure core: renderer, ranking, staleness.

#![allow(clippy::unwrap_used, reason = "tests; a panic is the failure signal")]

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence, RngAlgorithm, RngSeed};
use wglane_core::{
    MAX_CONF_AGE, PrivateKey, PublicKey, Server, TunnelConfig, is_stale, rank_by_load,
    render_wg_conf,
};

/// Fixed seed: CI is deterministic; shrinking and failure persistence stay on.
fn cfg() -> Config {
    Config {
        cases: 512,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(0x57_47_4c_41_4e_45),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/props_core.proptest-regressions",
        ))),
        ..Config::default()
    }
}

/// 43 base64 chars + `=`: the shape the helper accepts for a key.
fn wg_key() -> impl Strategy<Value = String> {
    "[A-Za-z0-9+/]{43}=".prop_map(String::from)
}

fn station() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        any::<u32>().prop_map(|n| IpAddr::V4(Ipv4Addr::from(n))),
        any::<u128>().prop_map(|n| IpAddr::V6(n.into())),
    ]
}

fn server(station: IpAddr, public_key: String, load: u8, name: String) -> Server {
    Server {
        hostname: name,
        station,
        load,
        public_key: PublicKey(public_key),
        country: "DE".parse().expect("valid country code"),
    }
}

/// Section headers, blank separator, and the only keys the template emits.
const ALLOWED_KEYS: [&str; 7] = [
    "PrivateKey",
    "Address",
    "PublicKey",
    "AllowedIPs",
    "Endpoint",
    "PersistentKeepalive",
    "", // blank line
];

proptest! {
    #![proptest_config(cfg())]

    // (1) Rendering with well-formed keys emits only the fixed template: no
    // PostUp/PreUp/PostDown/PreDown/SaveConfig/Table/DNS/Script, no extra line.
    #[test]
    fn render_emits_only_the_template(
        private in wg_key(), public in wg_key(), ip in station(),
    ) {
        let conf = TunnelConfig::nordlynx(PrivateKey::new(private), &server(ip, public, 0, "h".into()));
        let text = render_wg_conf(&conf);
        let lines: Vec<&str> = text.lines().collect();
        prop_assert!(text.ends_with('\n'));
        prop_assert_eq!(lines.iter().filter(|l| l.starts_with('[')).count(), 2);
        prop_assert_eq!(lines[0], "[Interface]");
        for line in &lines {
            if line.starts_with('[') {
                prop_assert!(*line == "[Interface]" || *line == "[Peer]");
                continue;
            }
            let key = line.split_once(" = ").map_or(*line, |(k, _)| k);
            prop_assert!(ALLOWED_KEYS.contains(&key), "unexpected line {line:?}");
        }
        for banned in ["PostUp", "PreUp", "PostDown", "PreDown", "SaveConfig", "Table", "DNS", "Script"] {
            prop_assert!(!text.contains(banned), "{banned} in rendered config");
        }
    }

    // (7) Render then parse recovers the key fields.
    #[test]
    fn render_round_trips_fields(
        private in wg_key(), public in wg_key(), ip in station(),
    ) {
        let conf = TunnelConfig::nordlynx(PrivateKey::new(private.clone()), &server(ip, public.clone(), 0, "h".into()));
        let text = render_wg_conf(&conf);
        let find = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)).map(str::to_owned);
        prop_assert_eq!(find("PrivateKey = "), Some(private));
        prop_assert_eq!(find("PublicKey = "), Some(public));
        prop_assert_eq!(find("Endpoint = "), Some(conf.endpoint.to_string()));
        prop_assert_eq!(find("Address = "), Some(conf.address.to_string()));
        prop_assert_eq!(find("AllowedIPs = "), Some("0.0.0.0/0".to_owned()));
        let parsed: std::net::SocketAddr = find("Endpoint = ").unwrap().parse().unwrap();
        prop_assert_eq!(parsed, conf.endpoint);
    }

    // (5) rank_by_load: a permutation, sorted by load, stable for ties.
    #[test]
    fn rank_is_sorted_stable_permutation(loads in prop::collection::vec(0u8..=100, 0..64)) {
        let mut servers: Vec<Server> = loads
            .iter()
            .enumerate()
            .map(|(i, &l)| server(Ipv4Addr::LOCALHOST.into(), String::new(), l, format!("h{i}")))
            .collect();
        rank_by_load(&mut servers);
        let idx = |s: &Server| s.hostname[1..].parse::<usize>().unwrap();
        let mut seen: Vec<usize> = servers.iter().map(idx).collect();
        for w in servers.windows(2) {
            prop_assert!(w[0].load <= w[1].load);
            if w[0].load == w[1].load {
                prop_assert!(idx(&w[0]) < idx(&w[1]), "tie order changed");
            }
        }
        seen.sort_unstable();
        prop_assert_eq!(seen, (0..loads.len()).collect::<Vec<_>>());
    }

    // (6) is_stale is monotonic in age, with the boundary at MAX_CONF_AGE.
    #[test]
    fn is_stale_is_monotonic(a in 0u64..=2_000_000, b in 0u64..=2_000_000) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (lo, hi) = (Duration::from_secs(lo), Duration::from_secs(hi));
        prop_assert!(!is_stale(Some(lo)) || is_stale(Some(hi)));
        prop_assert_eq!(is_stale(Some(lo)), lo > MAX_CONF_AGE);
    }

    #[test]
    fn is_stale_extremes(secs in any::<u64>(), nanos in 0u32..1_000_000_000) {
        let age = Duration::new(secs, nanos);
        prop_assert_eq!(is_stale(Some(age)), age > MAX_CONF_AGE);
        prop_assert!(is_stale(None));
    }
}
