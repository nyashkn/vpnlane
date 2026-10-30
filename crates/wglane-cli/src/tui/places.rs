//! Places (a country, or one city in it) and the sample behind their
//! indicators: the provider's recommended server there, with its load and a
//! probed latency. Ordering and bars are pure; the worker fetches and probes.

use std::collections::HashSet;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use wglane_core::{City, CountryCode, Server};

use super::app::{Effect, State};
use super::probe::{Probe, key as server_key, rank_with_probes};

/// A load at or above this is drawn as a warning.
const BUSY_LOAD: u8 = 60;
/// At most this many places per sample request (Nord API calls).
const SAMPLE_CAP: usize = 40;

/// A country, or a city in it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PlaceKey {
    pub(crate) cc: CountryCode,
    /// City name as the provider spells it.
    pub(crate) city: Option<String>,
}

impl PlaceKey {
    pub(crate) fn country(cc: CountryCode) -> Self {
        Self { cc, city: None }
    }

    pub(crate) fn city(cc: CountryCode, name: &str) -> Self {
        Self {
            cc,
            city: Some(name.to_owned()),
        }
    }
}

/// What we know of a place's servers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Sample {
    /// Asked the worker, no answer yet.
    Requested,
    /// The recommended server there.
    Server(Server),
    /// The lookup failed or found no server.
    Failed,
}

/// The sampled server of `k`, if any.
pub(crate) fn server<'a>(s: &'a State, k: &PlaceKey) -> Option<&'a Server> {
    match s.samples.get(k) {
        Some(Sample::Server(sv)) => Some(sv),
        _ => None,
    }
}

/// Measured latency of `k` in this probe round.
pub(crate) fn latency(s: &State, k: &PlaceKey) -> Option<u64> {
    match s.probes.get(&server(s, k)?.hostname) {
        Some(Probe::Latency(ms)) => Some(*ms),
        _ => None,
    }
}

/// `keys` in display order: sampled places ranked by [`rank_with_probes`]
/// (latency, then failed, then not probed, load breaking ties), then the
/// places without a sample in their input order.
pub(crate) fn rank(s: &State, keys: Vec<PlaceKey>) -> Vec<PlaceKey> {
    let (with, without): (Vec<_>, Vec<_>) = keys.into_iter().partition(|k| server(s, k).is_some());
    let servers: Vec<Server> = with.iter().filter_map(|k| server(s, k).cloned()).collect();
    let ranked = rank_with_probes(&servers, &s.probes);
    let mut out: Vec<PlaceKey> = ranked
        .order
        .iter()
        .filter_map(|r| servers.iter().position(|x| std::ptr::eq(x, *r)))
        .map(|i| with[i].clone())
        .collect();
    out.extend(without);
    out
}

/// The lowest-latency place of `keys`, once one has answered.
pub(crate) fn fastest(s: &State, keys: &[PlaceKey]) -> Option<PlaceKey> {
    keys.iter()
        .filter_map(|k| latency(s, k).map(|ms| (ms, k)))
        .min_by_key(|(ms, _)| *ms)
        .map(|(_, k)| k.clone())
}

/// The `n` best places measured so far (countries only), and how many were measured.
pub(crate) fn best_countries(s: &State, n: usize) -> (Vec<PlaceKey>, usize) {
    let keys: Vec<PlaceKey> = s
        .samples
        .iter()
        .filter(|(k, v)| k.city.is_none() && matches!(v, Sample::Server(_)))
        .map(|(k, _)| k.clone())
        .collect();
    let total = keys.len();
    let mut sorted = keys;
    sorted.sort();
    let mut ranked = rank(s, sorted);
    ranked.truncate(n);
    (ranked, total)
}

/// Every probe of this round failed (and at least one ran).
pub(crate) fn all_failed(s: &State) -> bool {
    !s.probes.is_empty() && s.probes.values().all(|p| *p == Probe::Failed)
}

/// Asks the worker for samples of the `keys` not sampled or asked for yet
/// (at most [`SAMPLE_CAP`]), marking them requested.
pub(crate) fn request(
    s: &mut State,
    keys: impl IntoIterator<Item = PlaceKey>,
    fx: &mut Vec<Effect>,
) {
    let mut seen = HashSet::new();
    let mut places: Vec<(PlaceKey, Option<City>)> = Vec::new();
    for k in keys {
        if places.len() >= SAMPLE_CAP || s.samples.contains_key(&k) || !seen.insert(k.clone()) {
            continue;
        }
        let city = match &k.city {
            None => None,
            Some(name) => {
                if let Some(c) = s.city_of(k.cc, name) {
                    Some(c.clone())
                } else {
                    // Not in the catalog (a preset spelled differently): nothing to ask.
                    s.samples.insert(k, Sample::Failed);
                    continue;
                }
            }
        };
        places.push((k, city));
    }
    if places.is_empty() {
        return;
    }
    for (k, _) in &places {
        s.samples.insert(k.clone(), Sample::Requested);
    }
    fx.push(Effect::Sample {
        epoch: s.probe_epoch,
        places,
    });
}

/// A new probe round over every sampled server (after a tunnel change, or `r`):
/// old results are dropped, late ones by their epoch.
pub(crate) fn reprobe(s: &mut State, fx: &mut Vec<Effect>) {
    s.probe_epoch += 1;
    let mut servers: Vec<Server> = Vec::new();
    for v in s.samples.values() {
        if let Sample::Server(sv) = v
            && !servers.iter().any(|x| x.hostname == sv.hostname)
        {
            servers.push(sv.clone());
        }
    }
    servers.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    s.probes = servers
        .iter()
        .map(|x| (server_key(x), Probe::Pending))
        .collect();
    if !servers.is_empty() {
        fx.push(Effect::Probe {
            epoch: s.probe_epoch,
            servers,
        });
    }
}

/// `width` cells of a bar for `value` out of `max`, in eighths, padded with `·`.
pub(crate) fn bar(value: u64, max: u64, width: usize) -> String {
    const PART: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    if width == 0 {
        return String::new();
    }
    let cells = u64::try_from(width).unwrap_or(u64::MAX);
    let eighths = if max == 0 || value == 0 {
        0
    } else {
        // Round up, so the smallest non-zero value still shows a sliver.
        (value * cells * 8).div_ceil(max).clamp(1, cells * 8)
    };
    let full = usize::try_from(eighths / 8).unwrap_or(width);
    let part = PART[usize::try_from(eighths % 8).unwrap_or(0)];
    let used = full + usize::from(!part.is_empty());
    format!("{}{part}{}", "█".repeat(full), "·".repeat(width - used))
}

/// Scale and widths for one list of metric columns: bars are relative to the
/// rows shown, so the number is always printed next to them.
#[derive(Clone, Debug)]
pub(crate) struct Scale {
    max_ms: u64,
    max_load: u64,
    lat_bar: usize,
    load_bar: usize,
    fastest: Option<PlaceKey>,
}

impl Scale {
    pub(crate) fn new(s: &State, shown: &[PlaceKey], lat_bar: usize, load_bar: usize) -> Self {
        Self {
            max_ms: shown
                .iter()
                .filter_map(|k| latency(s, k))
                .max()
                .unwrap_or(0),
            max_load: shown
                .iter()
                .filter_map(|k| server(s, k).map(|x| u64::from(x.load)))
                .max()
                .unwrap_or(0),
            lat_bar,
            load_bar,
            fastest: fastest(s, shown),
        }
    }

    /// Cells the metric columns take.
    pub(crate) fn width(&self) -> usize {
        let bar = |w: usize| if w == 0 { 0 } else { w + 1 };
        7 + bar(self.lat_bar) + 2 + 4 + bar(self.load_bar) + 8
    }
}

/// `151 ms ███▎·   58% ██▉··  fastest`. Unknown values are `-`, a place
/// being measured reads `probing`; nothing is ever estimated.
pub(crate) fn metrics(s: &State, k: &PlaceKey, sc: &Scale) -> Vec<Span<'static>> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let pad = |w: usize| {
        if w == 0 {
            String::new()
        } else {
            " ".repeat(w + 1)
        }
    };
    let sample = s.samples.get(k);
    let probe = server(s, k).and_then(|x| s.probes.get(&x.hostname));
    let mut out = Vec::new();
    match (sample, probe) {
        (Some(Sample::Server(_)), Some(Probe::Latency(ms))) => {
            out.push(Span::raw(format!("{:>7}", format!("{ms} ms"))));
            if sc.lat_bar > 0 {
                out.push(Span::raw(format!(" {}", bar(*ms, sc.max_ms, sc.lat_bar))));
            }
        }
        (Some(Sample::Requested), _) | (Some(Sample::Server(_)), Some(Probe::Pending)) => {
            out.push(Span::styled(
                format!("{:>7}{}", "probing", pad(sc.lat_bar)),
                dim,
            ));
        }
        _ => out.push(Span::styled(format!("{:>7}{}", "-", pad(sc.lat_bar)), dim)),
    }
    out.push(Span::raw("  "));
    match server(s, k) {
        Some(x) => {
            let style = if x.load >= BUSY_LOAD {
                s.fg(Color::Yellow)
            } else {
                Style::new()
            };
            let mut text = format!("{:>3}%", x.load);
            if sc.load_bar > 0 {
                text.push(' ');
                text.push_str(&bar(u64::from(x.load), sc.max_load, sc.load_bar));
            }
            out.push(Span::styled(text, style));
        }
        None => out.push(Span::styled(format!("{:>4}{}", "-", pad(sc.load_bar)), dim)),
    }
    if sc.fastest.as_ref() == Some(k) {
        out.push(Span::styled(
            "  fastest",
            s.fg(Color::Green).add_modifier(Modifier::BOLD),
        ));
    } else {
        out.push(Span::raw("         "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_are_relative_round_up_and_pad() {
        assert_eq!(bar(10, 10, 4), "████");
        assert_eq!(bar(5, 10, 4), "██··");
        assert_eq!(bar(1, 100, 4), "▏···", "a sliver, never empty");
        assert_eq!(bar(0, 100, 4), "····");
        assert_eq!(bar(3, 0, 4), "····");
        assert_eq!(bar(3, 7, 0), "");
        for v in 0..=50 {
            assert_eq!(bar(v, 50, 5).chars().count(), 5);
        }
    }
}
