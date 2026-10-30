//! Dashboard state, the pure reducer and the screen dispatch. Home lives in
//! `hud.rs`, the jump palette in `palette.rs`, presets in `presets.rs`, place
//! samples and bars in `places.rs`; this file keeps the browse tree.

use std::collections::{BTreeSet, HashMap};

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph};
use wglane_core::{
    City, Country, CountryCode, DEFAULT_GRAPH_SAMPLES, Effective, LastRequest, PresetUse, Server,
    TailnetHealth, TuiSettings, TunnelStatus, fold,
};

use super::actions::{self, ACTIONS, Action};
use super::continent::{Continent, continent};
use super::hud;
use super::palette::{self, Palette};
use super::places::{self, PlaceKey, Sample, Scale};
use super::presets;
use super::probe::{Probe, ServerKey, cursor_after};

/// Below this nothing is usable: a size notice is drawn instead.
pub(crate) const MIN_SIZE: (u16, u16) = (30, 6);
/// Countries sampled around the anchor at start, for "fastest right now".
const NEARBY: usize = 24;

/// Keys of the palette, for the help overlay.
const PALETTE_HELP: [(&str, &str); 5] = [
    ("type", "filter places, presets, actions"),
    ("@  >", "only presets / only actions"),
    ("tab", "cycle kind: all, places, presets, actions"),
    ("↑↓ ctrl-j/k", "move (j, k are letters here)"),
    ("enter  esc", "go, close"),
];

/// Keys of the browse tree, for the help overlay.
const BROWSE_HELP: [(&str, &str); 5] = [
    ("enter", "connect a country or city; open/close a group"),
    ("h l  ←→  space", "close / open a group or country"),
    ("j k  ↑↓  g G", "move, first / last"),
    ("/", "filter by code, country or city"),
    ("esc", "clear the filter, then home"),
];

/// One line of the server tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row<'a> {
    /// Continent header: country count, whether it is open.
    Group(Continent, usize, bool),
    /// A country: whether its cities are listed below it, and whether it
    /// matches the filter itself (a country shown only for its matching
    /// cities is not selectable).
    Country(&'a Country, bool, bool),
    /// A city row under its country.
    City(&'a Country, &'a City),
}

impl Row<'_> {
    /// The place a row connects to.
    fn place(&self) -> Option<PlaceKey> {
        match self {
            Row::Group(..) => None,
            Row::Country(c, ..) => Some(PlaceKey::country(c.code)),
            Row::City(c, ci) => Some(PlaceKey::city(c.code, &ci.name)),
        }
    }
}

/// What the cursor is on, owned so keys can then change the state.
enum Cursor {
    Nothing,
    Group(Continent),
    /// Country code, its continent, whether it has a city level, whether that is open.
    Country(CountryCode, Continent, bool, bool),
    City(CountryCode),
}

/// Which screen is up (the palette and help are overlays on either).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Screen {
    #[default]
    Home,
    /// The continent > country (> city) tree.
    Browse,
}

/// A tunnel change in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    /// Where to, as shown: `Germany · Frankfurt`, `work (DE/Berlin · fastest)`.
    Connecting(String),
    Disconnecting,
}

/// What the UDP 51820 check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Udp {
    /// The datagram could not leave this machine.
    Blocked,
    /// It left; delivery cannot be tested without a tunnel.
    Sent,
    /// No generated config to take an endpoint from.
    NoEndpoint,
}

/// Display settings from `[tui]` in config.toml.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ui {
    /// Latency samples kept for the graph.
    graph_samples: usize,
    /// A sample above this multiple of the window median (in thousandths) is
    /// drawn as a warning.
    spike_permille: u64,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            graph_samples: DEFAULT_GRAPH_SAMPLES,
            spike_permille: 1500, // DEFAULT_WARN_RATIO
        }
    }
}

impl From<&TuiSettings> for Ui {
    fn from(t: &TuiSettings) -> Self {
        Self {
            graph_samples: t.graph_samples.value,
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "warn_ratio is validated to 1.0..=10.0"
            )]
            spike_permille: (t.warn_ratio.value * 1000.0).round() as u64,
        }
    }
}

impl Ui {
    /// The spike threshold for a window with this median.
    pub(crate) fn spike_limit(self, median: u64) -> u64 {
        median * self.spike_permille / 1000
    }
}

/// One status refresh: the helper's view plus what `state.json` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) status: TunnelStatus,
    /// False when the helper could not say which tunnel is up.
    pub(crate) known: bool,
    /// City of the connected server, when `up` recorded it.
    pub(crate) city: Option<String>,
    /// The last request `up` applied (slot 0).
    pub(crate) last: Option<LastRequest>,
    /// `state.json` says that request is live.
    pub(crate) connected: bool,
    /// The preset the live connection came from.
    pub(crate) preset: Option<PresetUse>,
    /// Seconds since the Unix epoch, for the uptime.
    pub(crate) now: u64,
}

/// Dashboard state. Changed only by [`update`].
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "flat UI record: each flag is independent and read by draw"
)]
pub(crate) struct State {
    pub(crate) should_quit: bool,
    pub(crate) screen: Screen,
    /// The jump palette, while open.
    pub(crate) palette: Option<Palette>,
    pub(crate) countries: Vec<Country>,
    /// The last server-list load failed (the list is empty for that reason).
    pub(crate) load_failed: bool,
    /// Index into [`State::rows`], not into `countries`.
    pub(crate) selected: usize,
    /// Open continent groups (all closed by default).
    pub(crate) expanded: BTreeSet<Continent>,
    /// Open countries (those with a city level; all closed by default).
    pub(crate) expanded_countries: BTreeSet<CountryCode>,
    /// City of the connected server, when known (recorded by `up`).
    pub(crate) city: Option<String>,
    /// The tree was opened on the connected country (done once, at start).
    pub(crate) placed: bool,
    /// Nearby countries were asked for samples (done once, at start).
    pub(crate) sampled_nearby: bool,
    /// Browse filter typed after `/`.
    pub(crate) query: String,
    pub(crate) searching: bool,
    /// The `?` key overlay is open.
    pub(crate) help: bool,
    pub(crate) status: Option<TunnelStatus>,
    /// False when the helper could not say which tunnel is up.
    pub(crate) tunnel_known: bool,
    /// The last request (`state.json`), slot 0.
    pub(crate) last: Option<LastRequest>,
    /// `state.json` says the last request is live.
    pub(crate) connected: bool,
    /// The preset the live connection came from.
    pub(crate) active_preset: Option<PresetUse>,
    /// Clock of the last status, seconds since the Unix epoch.
    pub(crate) now: u64,
    /// Tailnet round trips in ms, oldest first.
    pub(crate) latency: Vec<u64>,
    pub(crate) busy: Option<Busy>,
    /// The tunnel change that failed last, for `enter` to retry.
    pub(crate) retry: Option<Effect>,
    /// A status refresh is queued or running (ticks do not pile up behind it).
    pub(crate) refreshing: bool,
    /// Recommended server per place, for load and latency.
    pub(crate) samples: HashMap<PlaceKey, Sample>,
    /// Latency results of the current probe round.
    pub(crate) probes: HashMap<ServerKey, Probe>,
    /// Counts probe rounds; results from an older round are dropped.
    pub(crate) probe_epoch: u64,
    /// `exit_ip_check` from the settings: off is shown loudly.
    pub(crate) exit_ip_check: bool,
    /// The last UDP 51820 check, if one ran.
    pub(crate) udp: Option<Udp>,
    pub(crate) udp_checking: bool,
    /// Bottom-line message (errors, results).
    pub(crate) message: String,
    /// `message` reports a failure.
    pub(crate) failed: bool,
    /// Draw without color (`NO_COLOR`); symbols and modifiers still carry state.
    pub(crate) no_color: bool,
    /// Resolved settings: presets and their previews.
    pub(crate) settings: Option<Effective>,
    pub(crate) ui: Ui,
}

/// Something that happened: a key, a clock tick, or an effect's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Key(KeyCode),
    Tick,
    Countries(Result<Vec<Country>, String>),
    Status(Box<Snapshot>),
    /// A connect, preset apply or disconnect finished.
    Finished(Result<(), String>),
    /// The recommended server of `place` (`None`: lookup failed), asked in round `epoch`.
    Sampled {
        epoch: u64,
        place: PlaceKey,
        server: Option<Server>,
    },
    /// One server answered (`Some` ms) or did not (`None`), in round `epoch`.
    ProbeResult {
        epoch: u64,
        key: ServerKey,
        latency_ms: Option<u64>,
    },
    /// The UDP 51820 check: `None` without an endpoint, `Some(true)` when blocked.
    Udp(Option<bool>),
}

/// I/O the reducer asks the event loop to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    LoadCountries,
    Refresh,
    /// Connect to the least-loaded server in the country, or in the city.
    Connect(CountryCode, Option<City>),
    /// `up --preset NAME`, or `up -` (the last request) for `None`.
    Apply(Option<String>),
    Disconnect,
    /// Fetch each place's recommended server, send one `Sampled` each, then
    /// probe the servers found (one `ProbeResult` each), all tagged `epoch`.
    Sample {
        epoch: u64,
        places: Vec<(PlaceKey, Option<City>)>,
    },
    /// Ping the servers concurrently and send back one `ProbeResult` each, all
    /// tagged with `epoch`.
    Probe {
        epoch: u64,
        servers: Vec<Server>,
    },
    /// The doctor's UDP 51820 check.
    CheckUdp,
}

impl State {
    /// A fresh state with the settings applied.
    pub(crate) fn with_settings(settings: &Effective) -> Self {
        let t = &settings.tui;
        Self {
            ui: Ui::from(t),
            expanded: if t.start_expanded.value {
                Continent::ALL.into()
            } else {
                BTreeSet::new()
            },
            exit_ip_check: settings.exit_ip_check.value,
            settings: Some(settings.clone()),
            ..Self::default()
        }
    }

    /// A country with at most one city has no city level: its row connects.
    fn is_leaf(c: &Country) -> bool {
        c.cities.len() <= 1
    }

    /// Cities of `c` matching the filter (all of them without one).
    fn city_hits<'a>(&self, c: &'a Country) -> Vec<&'a City> {
        let q = fold(&self.query);
        c.cities
            .iter()
            .filter(|ci| fold(&ci.name).contains(&q))
            .collect()
    }

    /// The country row itself matches: code or name, or (for a country with
    /// no city level) its only city.
    fn country_hit(&self, c: &Country) -> bool {
        let q = fold(&self.query);
        fold(c.code.as_str()).contains(&q)
            || fold(&c.name).contains(&q)
            || (Self::is_leaf(c) && !self.city_hits(c).is_empty())
    }

    /// Countries matching the query by code, name or city: code-prefix hits
    /// first, then the rest, each group alphabetical. An empty query keeps the
    /// catalog order.
    pub(crate) fn visible(&self) -> Vec<&Country> {
        let q = fold(&self.query);
        if q.is_empty() {
            return self.countries.iter().collect();
        }
        let mut hits: Vec<(bool, String, &Country)> = self
            .countries
            .iter()
            .filter(|c| self.country_hit(c) || !self.city_hits(c).is_empty())
            .map(|c| (!fold(c.code.as_str()).starts_with(&q), fold(&c.name), c))
            .collect();
        hits.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        hits.into_iter().map(|(_, _, c)| c).collect()
    }

    /// The tree: continent headers, each followed by its countries when open,
    /// and a country's cities when it is open. A filter opens every group and
    /// country that has a match, shows only matching cities, and orders groups
    /// by their best match, so the first selectable row is the best match.
    pub(crate) fn rows(&self) -> Vec<Row<'_>> {
        let filtering = !self.query.is_empty();
        let mut groups: Vec<(Continent, Vec<&Country>)> = Vec::new();
        for c in self.visible() {
            let k = continent(c.code);
            match groups.iter_mut().find(|g| g.0 == k) {
                Some(g) => g.1.push(c),
                None => groups.push((k, vec![c])),
            }
        }
        if !filtering {
            groups.sort_by_key(|g| g.0);
        }
        let mut rows = Vec::new();
        for (k, list) in groups {
            let open = filtering || self.expanded.contains(&k);
            rows.push(Row::Group(k, list.len(), open));
            if !open {
                continue;
            }
            for c in list {
                let cities = self.city_hits(c);
                let show = !Self::is_leaf(c)
                    && if filtering {
                        !cities.is_empty()
                    } else {
                        self.expanded_countries.contains(&c.code)
                    };
                rows.push(Row::Country(c, show, !filtering || self.country_hit(c)));
                if show {
                    rows.extend(cities.into_iter().map(|ci| Row::City(c, ci)));
                }
            }
        }
        rows
    }

    /// Headers are skipped while a filter is active: only matches are picked.
    fn selectable(&self, row: &Row<'_>) -> bool {
        self.query.is_empty() || matches!(row, Row::City(..) | Row::Country(_, _, true))
    }

    /// Nearest selectable row from `from` (inclusive) in one direction.
    fn seek(&self, from: usize, down: bool) -> Option<usize> {
        let rows = self.rows();
        let ok = |i: &usize| self.selectable(&rows[*i]);
        if down {
            (from..rows.len()).find(ok)
        } else {
            (0..rows.len().min(from.saturating_add(1))).rev().find(ok)
        }
    }

    fn step(&mut self, down: bool) {
        let next = if down {
            self.seek(self.selected + 1, true)
        } else {
            self.selected
                .checked_sub(1)
                .and_then(|i| self.seek(i, false))
        };
        if let Some(i) = next {
            self.selected = i;
        }
    }

    fn first(&self) -> usize {
        self.seek(0, true).unwrap_or(0)
    }

    fn last(&self) -> usize {
        self.seek(usize::MAX, false).unwrap_or(0)
    }

    /// Rows changed underneath the selection: keep it on a selectable row.
    fn clamp_selection(&mut self) {
        let n = self.rows().len();
        self.selected = self.selected.min(n.saturating_sub(1));
        self.selected = self
            .seek(self.selected, true)
            .or_else(|| self.seek(self.selected, false))
            .unwrap_or(0);
    }

    /// The query changed: select the best match.
    fn refilter(&mut self) {
        self.selected = self.first();
    }

    fn selected_row(&self) -> Option<Row<'_>> {
        self.rows().get(self.selected).copied()
    }

    /// The header under the cursor (never while filtering: groups are forced open).
    fn selected_group(&self) -> Option<Continent> {
        match self.selected_row() {
            Some(Row::Group(k, ..)) if self.query.is_empty() => Some(k),
            _ => None,
        }
    }

    fn cursor(&self) -> Cursor {
        let filtering = !self.query.is_empty();
        match self.selected_row() {
            None => Cursor::Nothing,
            Some(Row::Group(k, ..)) => Cursor::Group(k),
            Some(Row::Country(c, open, _)) => Cursor::Country(
                c.code,
                continent(c.code),
                !Self::is_leaf(c) && !filtering,
                open,
            ),
            Some(Row::City(c, _)) => Cursor::City(c.code),
        }
    }

    /// Opens or closes a country's city level (not while filtering).
    fn set_open(&mut self, cc: CountryCode, open: bool) {
        if open {
            self.expanded_countries.insert(cc);
        } else {
            self.expanded_countries.remove(&cc);
        }
    }

    fn select_country(&mut self, cc: CountryCode) {
        if let Some(i) = self
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Country(c, ..) if c.code == cc))
        {
            self.selected = i;
        }
    }

    fn select_group(&mut self, k: Continent) {
        if let Some(i) = self
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Group(g, ..) if *g == k))
        {
            self.selected = i;
        }
    }

    /// Once both the catalog and a status are in: open the connected
    /// country's group and put the cursor on it.
    fn place(&mut self) {
        if self.placed || self.countries.is_empty() || self.status.is_none() {
            return;
        }
        self.placed = true;
        let Some(code) = self.active().first().map(|a| a.to_uppercase()) else {
            return;
        };
        let Some(cc) = self
            .countries
            .iter()
            .map(|c| c.code)
            .find(|c| c.as_str() == code)
        else {
            return;
        };
        self.expanded.insert(continent(cc));
        // With the city known, open the country and put the cursor on it.
        let city = self.city.clone().filter(|name| {
            self.countries.iter().any(|c| {
                c.code == cc && !Self::is_leaf(c) && c.cities.iter().any(|ci| ci.name == *name)
            })
        });
        if city.is_some() {
            self.expanded_countries.insert(cc);
        }
        if let Some(i) = self.rows().iter().position(|r| match (r, &city) {
            (Row::City(c, ci), Some(name)) => c.code == cc && ci.name == *name,
            (Row::Country(c, ..), None) => c.code == cc,
            _ => false,
        }) {
            self.selected = i;
        }
    }

    /// Codes of the tunnels the last probe saw up.
    pub(crate) fn active(&self) -> &[String] {
        self.status.as_ref().map_or(&[], |st| st.active.as_slice())
    }

    /// The country of the live tunnel.
    pub(crate) fn connected_cc(&self) -> Option<CountryCode> {
        self.active().first()?.parse().ok()
    }

    pub(crate) fn is_up(&self) -> bool {
        self.busy.is_none() && self.connected_cc().is_some()
    }

    /// The exit looks wrong while up.
    pub(crate) fn leaking(&self) -> bool {
        matches!(hud::safety(self), hud::Safety::Leak(_))
    }

    /// The catalog entry of a city.
    pub(crate) fn city_of(&self, cc: CountryCode, name: &str) -> Option<&City> {
        let want = fold(name);
        self.countries
            .iter()
            .find(|c| c.code == cc)?
            .cities
            .iter()
            .find(|ci| fold(&ci.name) == want)
    }

    /// A non-error message for the bottom line.
    pub(crate) fn note(&mut self, text: &str) {
        text.clone_into(&mut self.message);
        self.failed = false;
    }

    /// Starts a tunnel change.
    pub(crate) fn start(&mut self, busy: Busy, effect: Effect, fx: &mut Vec<Effect>) {
        self.busy = Some(busy);
        self.message.clear();
        self.failed = false;
        self.retry = Some(effect.clone());
        fx.push(effect);
    }

    /// Connects `k` (least-loaded server there).
    pub(crate) fn connect_place(&mut self, k: &PlaceKey, fx: &mut Vec<Effect>) {
        let city = match &k.city {
            Some(name) => match self.city_of(k.cc, name) {
                Some(c) => Some(c.clone()),
                None => return self.note("that city is not in the server list"),
            },
            None => None,
        };
        let name = self
            .countries
            .iter()
            .find(|c| c.code == k.cc)
            .map_or_else(|| k.cc.to_string(), |c| c.name.clone());
        let to = match &city {
            Some(c) => format!("{name} · {}", c.name),
            None => name,
        };
        self.start(Busy::Connecting(to), Effect::Connect(k.cc, city), fx);
    }

    /// Samples the countries (and open countries' cities) of every open group.
    pub(crate) fn request_open_groups(&mut self, fx: &mut Vec<Effect>) {
        let keys: Vec<PlaceKey> = self.rows().iter().filter_map(Row::place).collect();
        places::request(self, keys, fx);
    }

    /// Once the catalog and a status are in: sample the countries around the
    /// connected (or last) one, plus every preset's place and the last one,
    /// so "fastest right now" has candidates.
    fn sample_nearby(&mut self, fx: &mut Vec<Effect>) {
        if self.sampled_nearby || self.countries.is_empty() || self.status.is_none() {
            return;
        }
        self.sampled_nearby = true;
        let last = self.last.as_ref().and_then(|l| l.location.clone());
        let anchor = self
            .connected_cc()
            .or(last.as_ref().map(wglane_core::Location::country));
        let mut keys: Vec<PlaceKey> = Vec::new();
        if let Some(cc) = anchor {
            let k = continent(cc);
            keys.extend(
                self.countries
                    .iter()
                    .filter(|c| continent(c.code) == k)
                    .map(|c| PlaceKey::country(c.code))
                    .take(NEARBY),
            );
        }
        let wanted = self.settings.as_ref().map(|s| {
            s.resolved_presets()
                .into_iter()
                .filter_map(|p| p.request.location)
                .collect::<Vec<_>>()
        });
        for l in wanted.unwrap_or_default().into_iter().chain(last) {
            keys.push(PlaceKey::country(l.country()));
        }
        places::request(self, keys, fx);
    }

    /// A foreground color, or none under `NO_COLOR`.
    pub(crate) fn fg(&self, color: Color) -> Style {
        if self.no_color {
            Style::new()
        } else {
            Style::new().fg(color)
        }
    }

    /// What a list's cursor is on, by identity, for [`cursor_after`].
    fn list_keys(&self) -> Vec<Option<PlaceKey>> {
        self.rows().iter().map(Row::place).collect()
    }
}

/// Effects to run at startup.
pub(crate) fn init() -> Vec<Effect> {
    vec![Effect::LoadCountries, Effect::Refresh, Effect::CheckUdp]
}

/// Pure transition: next state plus the effects to run.
pub(crate) fn update(mut s: State, event: Event) -> (State, Vec<Effect>) {
    let mut fx = Vec::new();
    // Results can re-sort the palette: keep its cursor on the same row.
    let before = palette::items(&s);
    // Keys move the cursor on purpose; only results must not.
    let is_key = matches!(event, Event::Key(_));
    match event {
        Event::Key(k) => on_key(&mut s, k, &mut fx),
        Event::Tick => {
            if s.busy.is_none() && !s.refreshing {
                s.refreshing = true;
                fx.push(Effect::Refresh);
            }
        }
        Event::Countries(Ok(list)) => {
            let keys = s.list_keys();
            let at = s.selected;
            s.countries = list;
            s.load_failed = false;
            s.selected = cursor_after(&keys, at, &s.list_keys());
            s.clamp_selection();
            s.place();
            s.sample_nearby(&mut fx);
        }
        Event::Countries(Err(e)) => {
            s.load_failed = true;
            s.message = format!("servers: {e}");
            s.failed = true;
        }
        Event::Status(snap) => on_status(&mut s, *snap, &mut fx),
        Event::Finished(result) => {
            s.failed = result.is_err();
            s.message = match (s.busy.take(), result) {
                (_, Err(e)) => e,
                (Some(Busy::Connecting(to)), Ok(())) => format!("connected: {to}"),
                (_, Ok(())) => "disconnected".into(),
            };
            if s.failed {
                // A failed connect is often a blocked network: say so.
                s.udp_checking = true;
                fx.push(Effect::CheckUdp);
            } else {
                s.retry = None;
            }
            s.refreshing = true;
            fx.push(Effect::Refresh);
            // The path to every server changed: old latencies no longer hold.
            places::reprobe(&mut s, &mut fx);
        }
        Event::Sampled {
            epoch,
            place,
            server,
        } => on_sampled(&mut s, epoch, place, server, &mut fx),
        Event::ProbeResult {
            epoch,
            key,
            latency_ms,
        } => {
            // A late result from before a tunnel change must not land.
            if epoch == s.probe_epoch && s.probes.contains_key(&key) {
                s.probes
                    .insert(key, latency_ms.map_or(Probe::Failed, Probe::Latency));
            }
        }
        Event::Udp(v) => {
            s.udp_checking = false;
            s.udp = Some(match v {
                None => Udp::NoEndpoint,
                Some(true) => Udp::Blocked,
                Some(false) => Udp::Sent,
            });
        }
    }
    if let Some(cur) = s.palette.as_ref().map(|p| p.cursor).filter(|_| !is_key) {
        let after = palette::items(&s);
        let at = cursor_after(&before, cur, &after).min(after.len().saturating_sub(1));
        if let Some(p) = s.palette.as_mut() {
            p.cursor = at;
        }
    }
    (s, fx)
}

/// A status refresh landed.
fn on_status(s: &mut State, snap: Snapshot, fx: &mut Vec<Effect>) {
    let Snapshot {
        status,
        known,
        city,
        last,
        connected,
        preset,
        now,
    } = snap;
    s.city = city;
    if let TailnetHealth::Reachable { latency_ms, .. } = &status.tailnet {
        s.latency.push(*latency_ms);
        if s.latency.len() > s.ui.graph_samples {
            s.latency.remove(0);
        }
    }
    s.status = Some(status);
    s.refreshing = false;
    s.tunnel_known = known;
    s.last = last;
    s.connected = connected;
    s.active_preset = preset;
    s.now = now;
    s.place();
    s.sample_nearby(fx);
}

/// A place's server arrived: keep it, and make sure it gets probed in this round.
fn on_sampled(
    s: &mut State,
    epoch: u64,
    place: PlaceKey,
    server: Option<Server>,
    fx: &mut Vec<Effect>,
) {
    let Some(sv) = server else {
        s.samples.insert(place, Sample::Failed);
        return;
    };
    if epoch == s.probe_epoch {
        s.probes
            .entry(sv.hostname.clone())
            .or_insert(Probe::Pending);
    } else if let std::collections::hash_map::Entry::Vacant(e) = s.probes.entry(sv.hostname.clone())
    {
        // Its probe will carry the old epoch and be dropped: ask again.
        e.insert(Probe::Pending);
        fx.push(Effect::Probe {
            epoch: s.probe_epoch,
            servers: vec![sv.clone()],
        });
    }
    s.samples.insert(place, Sample::Server(sv));
}

fn on_key(s: &mut State, k: KeyCode, fx: &mut Vec<Effect>) {
    if s.help {
        // The overlay swallows one key: closing it never quits or connects.
        s.help = false;
    } else if s.palette.is_some() {
        palette::on_key(s, k, fx);
    } else if s.screen == Screen::Browse {
        browse_key(s, k, fx);
    } else {
        home_key(s, k, fx);
    }
}

fn open_palette(s: &mut State, fx: &mut Vec<Effect>) {
    s.palette = Some(Palette::default());
    let list = palette::items(s);
    palette::sample_shown(s, &list, fx);
}

fn home_key(s: &mut State, k: KeyCode, fx: &mut Vec<Effect>) {
    match k {
        KeyCode::Char(' ' | ':') => open_palette(s, fx),
        KeyCode::Char('/') => {
            s.screen = Screen::Browse;
            s.searching = true;
        }
        KeyCode::Char(c @ '0'..='9') => match presets::for_key(s, c) {
            Some(v) => presets::apply(s, v.slot, fx),
            None if c == '0' => s.note("no last request yet: connect once, then 0 repeats it"),
            None => s.note(&format!(
                "no preset on {c}: add [presets.NAME] tables to config.toml"
            )),
        },
        KeyCode::Enter => match s.retry.clone() {
            Some(effect) if s.failed && s.busy.is_none() => {
                let busy = match &effect {
                    Effect::Disconnect => Busy::Disconnecting,
                    _ => Busy::Connecting("retry".into()),
                };
                s.start(busy, effect, fx);
            }
            _ => {}
        },
        KeyCode::Esc => s.should_quit = true,
        KeyCode::Char(c) => {
            if let Some(a) = Action::for_key(c) {
                actions::run(s, a, fx);
            }
        }
        _ => {}
    }
}

fn browse_key(s: &mut State, k: KeyCode, fx: &mut Vec<Effect>) {
    let opened = (s.expanded.clone(), s.expanded_countries.clone());
    match k {
        // Search mode owns the keyboard: `q` and `d` are ordinary characters.
        KeyCode::Char(c) if s.searching => {
            s.query.push(c);
            s.refilter();
        }
        KeyCode::Backspace if s.searching => {
            s.query.pop();
            s.refilter();
        }
        KeyCode::Esc if s.searching || !s.query.is_empty() => {
            s.query.clear();
            s.searching = false;
            s.refilter();
        }
        KeyCode::Enter if s.searching => {
            s.searching = false;
            s.refilter();
        }
        KeyCode::Esc => s.screen = Screen::Home,
        KeyCode::Char('q') => s.should_quit = true,
        KeyCode::Char('?') => s.help = true,
        KeyCode::Char(':') => open_palette(s, fx),
        KeyCode::Up | KeyCode::Char('k') => s.step(false),
        KeyCode::Down | KeyCode::Char('j') => s.step(true),
        KeyCode::Home | KeyCode::Char('g') => s.selected = s.first(),
        KeyCode::End | KeyCode::Char('G') => s.selected = s.last(),
        KeyCode::Char('/') => s.searching = true,
        // Toggles are UI-only, so they work while a change is in flight.
        KeyCode::Enter | KeyCode::Char(' ') if s.selected_group().is_some() => {
            if let Some(k) = s.selected_group()
                && !s.expanded.remove(&k)
            {
                s.expanded.insert(k);
            }
        }
        KeyCode::Char(' ') => {
            if let Cursor::Country(cc, _, true, open) = s.cursor() {
                s.set_open(cc, !open);
            }
        }
        KeyCode::Char('l' | 'h') | KeyCode::Right | KeyCode::Left => move_in_tree(s, k),
        KeyCode::Char('r') => actions::run(s, Action::Recheck, fx),
        // Keys that change the tunnel are ignored while one change runs.
        _ if s.busy.is_some() => {}
        KeyCode::Enter => {
            if let Some(k) = s.selected_row().and_then(|r| r.place()) {
                s.connect_place(&k, fx);
            }
        }
        KeyCode::Char('d') => actions::run(s, Action::Disconnect, fx),
        _ => {}
    }
    if (s.expanded.clone(), s.expanded_countries.clone()) != opened {
        s.request_open_groups(fx);
    } else if !s.query.is_empty() {
        let keys: Vec<PlaceKey> = s.rows().iter().filter_map(Row::place).take(8).collect();
        places::request(s, keys, fx);
    }
}

/// Open/close/step keys (`h` `l` left right) at group, country and city level.
fn move_in_tree(s: &mut State, k: KeyCode) {
    match k {
        KeyCode::Char('l') => match s.cursor() {
            Cursor::Group(k) => {
                s.expanded.insert(k);
            }
            Cursor::Country(cc, _, true, _) => s.set_open(cc, true),
            _ => {}
        },
        // Right opens a closed header or country; on an open one it steps in.
        KeyCode::Right => match s.cursor() {
            Cursor::Group(k) if s.expanded.insert(k) => {}
            Cursor::Country(cc, _, true, false) => s.set_open(cc, true),
            Cursor::Group(_) | Cursor::Country(_, _, true, true) => s.step(true),
            _ => {}
        },
        // Left closes a header or open country; otherwise it only jumps up a level.
        KeyCode::Left if s.query.is_empty() => match s.cursor() {
            Cursor::Group(k) => {
                s.expanded.remove(&k);
            }
            Cursor::Country(cc, _, true, true) => s.set_open(cc, false),
            Cursor::Country(_, k, ..) => s.select_group(k),
            Cursor::City(cc) => s.select_country(cc),
            Cursor::Nothing => {}
        },
        // `h` also closes: an open country, else the group (moving to its header).
        KeyCode::Char('h') if s.query.is_empty() => match s.cursor() {
            Cursor::Group(k) => {
                s.expanded.remove(&k);
            }
            Cursor::Country(cc, _, true, true) => s.set_open(cc, false),
            Cursor::Country(_, k, ..) => {
                s.expanded.remove(&k);
                s.select_group(k);
            }
            Cursor::City(cc) => {
                s.set_open(cc, false);
                s.select_country(cc);
            }
            Cursor::Nothing => {}
        },
        _ => {}
    }
}

/// Draws the screen, then the overlays (palette, help) on top.
pub(crate) fn draw(s: &State, frame: &mut Frame) {
    let area = frame.area();
    if area.width < MIN_SIZE.0 || area.height < MIN_SIZE.1 {
        let text = vec![
            Line::from("terminal too small"),
            Line::from(format!(
                "need {}x{}, have {}x{}",
                MIN_SIZE.0, MIN_SIZE.1, area.width, area.height
            )),
            Line::from("q quit"),
        ];
        let box_ = area.centered_vertically(Constraint::Length(3));
        frame.render_widget(Paragraph::new(text).centered(), box_);
        return;
    }
    match s.screen {
        Screen::Home => hud::draw(s, frame),
        Screen::Browse => draw_browse(s, frame),
    }
    if s.palette.is_some() {
        palette::draw(s, frame);
    }
    if s.help {
        draw_help(s, frame);
    }
}

fn draw_browse(s: &State, frame: &mut Frame) {
    let area = frame.area();
    let [top, title, main, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(header_line(s, top.width)), top);
    let dim = Style::new().add_modifier(Modifier::DIM);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("browse", Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(
                "  the full tree, for when you do not know the name · esc home",
                dim,
            ),
        ])),
        title,
    );
    draw_list(s, frame, main);
    frame.render_widget(Paragraph::new(browse_footer(s, bottom.width)), bottom);
}

fn browse_footer(s: &State, width: u16) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    if s.searching {
        return Line::from(Span::styled(
            "type to filter  enter keep  esc clear  ↑↓ select",
            dim,
        ));
    }
    let hints = "enter connect  h l open/close  / filter  : jump  esc home  ? keys";
    if s.message.is_empty() {
        return Line::from(Span::styled(hints, dim));
    }
    // The message comes first so a narrow terminal cuts the hints, not the error.
    let msg = if s.failed {
        Span::styled(
            format!("error: {}", s.message),
            s.fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw(s.message.clone())
    };
    let line = Line::from(vec![msg.clone(), Span::styled("  ? keys", dim)]);
    if line.width() <= usize::from(width) {
        line
    } else {
        Line::from(msg)
    }
}

fn draw_list(s: &State, frame: &mut Frame, area: Rect) {
    let rows = s.rows();
    let title = match (s.searching, s.query.is_empty()) {
        (true, _) => format!(" filter: {}_ ", s.query),
        (false, false) => format!(" servers (/{}) ", s.query),
        (false, true) => " servers ".into(),
    };
    let active = s.active();
    let is_up = |c: &Country| {
        active
            .iter()
            .any(|a| a.eq_ignore_ascii_case(c.code.as_str()))
    };
    let up_mark = || Span::styled("●", s.fg(Color::Green));
    let wide = area.width >= 80;
    let shown: Vec<PlaceKey> = rows.iter().filter_map(Row::place).collect();
    let scale = Scale::new(
        s,
        &shown,
        if wide { 5 } else { 3 },
        if wide { 5 } else { 0 },
    );
    let name_w = usize::from(area.width)
        .saturating_sub(4 + scale.width() + 2)
        .min(34);
    let pad_to = |spans: &mut Vec<Span<'static>>| {
        let w: usize = spans.iter().map(Span::width).sum();
        if w < name_w + 2 {
            spans.push(Span::raw(" ".repeat(name_w + 2 - w)));
        }
    };
    let mut items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            Row::Group(k, n, open) => {
                let mut spans = vec![Span::styled(
                    format!("{} {k} ({n})", if *open { "▾" } else { "▸" }),
                    Style::new().add_modifier(Modifier::BOLD),
                )];
                // A closed group still shows that the tunnel is in it.
                if !open
                    && s.countries
                        .iter()
                        .any(|c| continent(c.code) == *k && is_up(c))
                {
                    spans.extend([Span::raw(" "), up_mark()]);
                }
                ListItem::new(Line::from(spans))
            }
            Row::Country(c, open, _) => {
                let mark = if is_up(c) { up_mark() } else { Span::raw(" ") };
                // Only countries with a city level get a chevron.
                let chevron = match (State::is_leaf(c), open) {
                    (true, _) => "",
                    (false, true) => " ▾",
                    (false, false) => " ▸",
                };
                let mut spans = vec![
                    Span::raw("  "),
                    mark,
                    Span::raw(format!(" {}  {}{chevron}", c.code, c.name)),
                ];
                pad_to(&mut spans);
                spans.extend(places::metrics(s, &PlaceKey::country(c.code), &scale));
                ListItem::new(Line::from(spans))
            }
            Row::City(c, city) => {
                let here = is_up(c) && s.city.as_deref() == Some(city.name.as_str());
                let mark = if here { up_mark() } else { Span::raw(" ") };
                let mut spans = vec![
                    Span::raw("      "),
                    mark,
                    Span::raw(format!(" {} ({})", city.name, city.server_count)),
                ];
                pad_to(&mut spans);
                spans.extend(places::metrics(
                    s,
                    &PlaceKey::city(c.code, &city.name),
                    &scale,
                ));
                ListItem::new(Line::from(spans))
            }
        })
        .collect();
    if items.is_empty() {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let empty = match (s.query.is_empty(), s.load_failed) {
            (false, _) => "no match (esc clears)",
            (true, true) => "load failed (r retries)",
            (true, false) => "loading servers...",
        };
        items.push(ListItem::new(empty).style(dim));
    }
    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_symbol("> ")
        .highlight_spacing(HighlightSpacing::Always)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut list_state =
        ListState::default().with_selected((!rows.is_empty()).then_some(s.selected));
    frame.render_stateful_widget(list, area, &mut list_state);
}

/// The `?` overlay: home keys from the action registry, then the palette's
/// and the tree's.
fn draw_help(s: &State, frame: &mut Frame) {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut rows: Vec<(String, String)> = vec![
        ("space  :".into(), "open the jump palette".into()),
        (
            "0-9".into(),
            "apply a preset now (0 = last); reconnects".into(),
        ),
    ];
    for (a, label, key, _) in ACTIONS {
        if let Some(k) = key
            && a != Action::Quit
        {
            rows.push((k.to_string(), label.to_owned()));
        }
    }
    rows.push(("enter".into(), "retry after an error".into()));
    rows.push(("q  esc".into(), "quit".into()));
    let mut lines = vec![Line::from(Span::styled(" home", bold))];
    let key_w = 16;
    let row = |k: &str, what: &str| {
        Line::from(vec![
            Span::styled(format!(" {k:<key_w$}"), s.fg(Color::Cyan)),
            Span::raw(what.to_owned()),
        ])
    };
    lines.extend(rows.iter().map(|(k, w)| row(k, w)));
    // Groups that do not fit the terminal are left out, last first.
    let room = usize::from(frame.area().height).saturating_sub(2);
    for (title, keys) in [
        (" in the palette", &PALETTE_HELP),
        (" browse (b)", &BROWSE_HELP),
    ] {
        if lines.len() + keys.len() + 2 > room {
            break;
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(title, bold)));
        lines.extend(keys.iter().map(|(k, w)| row(k, w)));
    }
    let width = lines.iter().map(Line::width).max().unwrap_or(0) + 3;
    let height = lines.len() + 2;
    let full = frame.area();
    let area = full
        .centered(
            Constraint::Length(u16::try_from(width).unwrap_or(u16::MAX)),
            Constraint::Length(u16::try_from(height).unwrap_or(u16::MAX)),
        )
        .intersection(full);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(Span::styled(" keys · any key closes ", bold))),
        area,
    );
}

pub(crate) fn median(samples: &[u64]) -> u64 {
    let mut v = samples.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

/// One-line summary above the browse tree (the HUD in one row). Drops the
/// mini graph, then the exit ip, then the latency as the terminal narrows.
fn header_line(s: &State, width: u16) -> Line<'static> {
    let up = s.is_up();
    let shown = &s.latency[s.latency.len().saturating_sub(12)..];
    let (lo, hi) = (
        shown.iter().copied().min().unwrap_or(0),
        shown.iter().copied().max().unwrap_or(0),
    );
    let limit = s.ui.spike_limit(median(shown));
    let spark: Vec<Span> = shown
        .iter()
        .map(|&v| {
            let i = usize::try_from((v - lo) * 7 / (hi - lo).max(1)).unwrap_or(7);
            let bar = Span::raw(["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"][i]);
            if v > limit {
                bar.style(s.fg(Color::Yellow))
            } else {
                bar
            }
        })
        .collect();
    let ms = shown.last().filter(|_| up).map(|v| format!("  {v} ms"));
    let exit = s.status.as_ref().and_then(|st| st.exit_ip).filter(|_| up);
    let build = |with_spark: bool, exit_ip: bool| {
        let mut spans = hud::state_spans(s, true);
        spans.extend(ms.clone().map(Span::raw));
        if with_spark && ms.is_some() {
            spans.push(Span::raw(" "));
            spans.extend(spark.clone());
        }
        if let (true, Some(ip)) = (exit_ip, exit) {
            spans.push(Span::raw(format!("  exit {ip}")));
        }
        Line::from(spans)
    };
    [(true, true), (false, true), (false, false)]
        .into_iter()
        .map(|(a, b)| build(a, b))
        .find(|l| l.width() <= usize::from(width))
        .unwrap_or_else(|| build(false, false))
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
