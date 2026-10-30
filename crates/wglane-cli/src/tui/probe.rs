//! Latency probe results kept in the dashboard state, and the pure ordering
//! built on them. The reducer stores results; nothing here measures anything.

use std::collections::HashMap;

use wglane_core::Server;

/// Identifies a server across re-sorts: its hostname.
pub(crate) type ServerKey = String;

/// The key of `s`.
pub(crate) fn key(s: &Server) -> ServerKey {
    s.hostname.clone()
}

/// Where a server stands in the current probe round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Probe {
    /// Asked, no answer yet.
    Pending,
    /// Round trip in milliseconds.
    Latency(u64),
    /// Timed out or could not run.
    Failed,
}

/// Servers in display order, plus the best measured one.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ranked<'a> {
    pub(crate) order: Vec<&'a Server>,
    /// The lowest-latency server, once any probe has succeeded. Always the
    /// first of `order`.
    pub(crate) fastest: Option<&'a Server>,
}

/// Orders `servers` for display: answered servers by latency (load breaks
/// ties), then failed ones, then those not probed yet; within each, by load,
/// then the input order. Stable, so the list only moves as results arrive.
pub(crate) fn rank_with_probes<'a>(
    servers: &'a [Server],
    probes: &HashMap<ServerKey, Probe>,
) -> Ranked<'a> {
    // (class, latency): 0 answered, 1 failed, 2 not probed or pending.
    let rank = |s: &Server| match probes.get(&s.hostname) {
        Some(Probe::Latency(ms)) => (0u8, *ms),
        Some(Probe::Failed) => (1, 0),
        Some(Probe::Pending) | None => (2, 0),
    };
    let mut order: Vec<&Server> = servers.iter().collect();
    order.sort_by_key(|s| (rank(s), s.load));
    let fastest = order
        .first()
        .copied()
        .filter(|s| matches!(probes.get(&s.hostname), Some(Probe::Latency(_))));
    Ranked { order, fastest }
}

/// The cursor index in `next` that sits on the same server as index `cursor`
/// of `prev`. Falls back to the nearest valid index when that server is gone.
pub(crate) fn cursor_after(prev: &[&Server], cursor: usize, next: &[&Server]) -> usize {
    prev.get(cursor)
        .and_then(|on| next.iter().position(|s| s.hostname == on.hostname))
        .unwrap_or_else(|| cursor.min(next.len().saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use wglane_core::PublicKey;

    use super::*;

    fn s(name: &str, load: u8) -> Server {
        Server {
            hostname: name.into(),
            station: "203.0.113.1".parse().unwrap(),
            load,
            public_key: PublicKey(String::new()),
            country: "DE".parse().unwrap(),
            city: None,
        }
    }

    fn names<'a>(r: &Ranked<'a>) -> Vec<&'a str> {
        r.order.iter().map(|s| s.hostname.as_str()).collect()
    }

    fn list() -> Vec<Server> {
        vec![s("a", 30), s("b", 10), s("c", 20), s("d", 5)]
    }

    fn probes(p: &[(&str, Probe)]) -> HashMap<ServerKey, Probe> {
        p.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn unprobed_servers_keep_load_order_and_nothing_is_fastest() {
        let l = list();
        let r = rank_with_probes(&l, &HashMap::new());
        assert_eq!(names(&r), ["d", "b", "c", "a"]);
        assert_eq!(r.fastest, None);
        // Pending counts as unprobed.
        let p = probes(&[("a", Probe::Pending)]);
        assert_eq!(rank_with_probes(&l, &p).fastest, None);
    }

    #[test]
    fn answered_then_failed_then_unprobed_with_load_breaking_ties() {
        let l = list();
        let p = probes(&[
            ("a", Probe::Latency(40)),
            ("c", Probe::Latency(40)),
            ("b", Probe::Failed),
        ]);
        let r = rank_with_probes(&l, &p);
        // a and c tie on 40 ms: c has the lower load. d was not probed.
        assert_eq!(names(&r), ["c", "a", "b", "d"]);
        assert_eq!(r.fastest.map(|s| s.hostname.as_str()), Some("c"));
    }

    #[test]
    fn a_failure_alone_does_not_make_a_fastest() {
        let l = list();
        let r = rank_with_probes(&l, &probes(&[("d", Probe::Failed)]));
        assert_eq!(r.fastest, None);
        assert_eq!(names(&r)[0], "d", "failed sorts ahead of unprobed");
    }

    #[test]
    fn cursor_stays_on_the_same_server_as_results_re_sort_the_list() {
        let l = list();
        let mut p = HashMap::new();
        let before = rank_with_probes(&l, &p);
        // Cursor on "a", last row.
        let cursor = before.order.iter().position(|s| s.hostname == "a").unwrap();
        assert_eq!(cursor, 3);
        // "a" turns out to be the fastest: the list re-sorts and "a" moves to the top.
        p.insert("a".into(), Probe::Latency(12));
        let after = rank_with_probes(&l, &p);
        assert_eq!(names(&after)[0], "a");
        let moved = cursor_after(&before.order, cursor, &after.order);
        assert_eq!(after.order[moved].hostname, "a");
        // And again when others arrive around it.
        p.insert("d".into(), Probe::Latency(5));
        p.insert("c".into(), Probe::Failed);
        let later = rank_with_probes(&l, &p);
        let moved2 = cursor_after(&after.order, moved, &later.order);
        assert_eq!(later.order[moved2].hostname, "a");
    }

    #[test]
    fn cursor_clamps_when_its_server_leaves_the_list() {
        let l = list();
        let all = rank_with_probes(&l, &HashMap::new());
        let fewer = rank_with_probes(&l[..2], &HashMap::new());
        assert_eq!(cursor_after(&all.order, 3, &fewer.order), 1);
        assert_eq!(cursor_after(&all.order, 0, &[]), 0);
    }
}
