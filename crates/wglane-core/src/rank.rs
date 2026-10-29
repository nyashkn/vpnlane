use crate::Server;

/// Sorts servers by load, lowest first.
///
/// The sort is stable, so servers with equal load keep the provider's order;
/// Nord's recommendations endpoint already orders by distance from the caller,
/// which gives "load, then distance" without a geo lookup.
pub fn rank_by_load(servers: &mut [Server]) {
    servers.sort_by_key(|s| s.load);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PublicKey;

    fn s(name: &str, load: u8) -> Server {
        Server {
            hostname: name.into(),
            station: "192.0.2.1".parse().unwrap(),
            load,
            public_key: PublicKey(String::new()),
            country: "DE".parse().unwrap(),
        }
    }

    #[test]
    fn lowest_load_first_ties_keep_order() {
        let mut v = vec![s("near-busy", 40), s("near", 10), s("far", 10)];
        rank_by_load(&mut v);
        let names: Vec<_> = v.iter().map(|s| s.hostname.as_str()).collect();
        assert_eq!(names, ["near", "far", "near-busy"]);
    }
}
