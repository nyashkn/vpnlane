use std::collections::{BTreeSet, HashMap};

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Sparkline, SparklineBar,
};
use wglane_core::{
    City, Country, CountryCode, DEFAULT_GRAPH_SAMPLES, Server, TailnetHealth, TuiSettings,
    TunnelStatus, fold,
};

use super::continent::{Continent, continent};
use super::probe::{Probe, ServerKey, key as server_key};

/// Height of the latency graph in rows.
const SPARK_ROWS: u16 = 2;
/// Bar levels the graph can draw (8 per row).
const SPARK_LEVELS: u64 = 8 * SPARK_ROWS as u64;
/// Below this the panels garble, so a size notice is drawn instead.
const MIN_SIZE: (u16, u16) = (40, 8);

/// Rows of the `?` overlay: key, what it does.
const HELP: [(&str, &str); 11] = [
    ("enter", "connect to a country or city; open/close a group"),
    ("space", "open/close a group or country"),
    ("h l", "close / open a group or country"),
    ("d", "disconnect"),
    ("/", "search by code, country or city"),
    ("esc", "clear search, then quit"),
    ("j k  up down", "move"),
    ("g G  home end", "first / last"),
    ("r", "reload the server list"),
    ("?", "this help (any key closes)"),
    ("q", "quit"),
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

/// What the cursor is on, owned so keys can then change the state.
enum Cursor {
    Nothing,
    Group(Continent),
    /// Country code, its continent, whether it has a city level, whether that is open.
    Country(CountryCode, Continent, bool, bool),
    City(CountryCode),
}

/// A tunnel change in flight (the UI is blocked on it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    /// Country, and the city when one was picked.
    Connecting(CountryCode, Option<String>),
    Disconnecting,
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
            // Validated to 1.0..=10.0, so the product fits and is not negative.
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
    fn spike_limit(self, median: u64) -> u64 {
        median * self.spike_permille / 1000
    }
}

/// Dashboard state. Changed only by [`update`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "flat UI record: each flag is independent and read by draw"
)]
pub(crate) struct State {
    pub(crate) should_quit: bool,
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
    /// Country filter typed after `/`.
    pub(crate) query: String,
    pub(crate) searching: bool,
    /// The `?` key overlay is open.
    pub(crate) help: bool,
    pub(crate) status: Option<TunnelStatus>,
    /// False when the helper could not say which tunnel is up.
    pub(crate) tunnel_known: bool,
    /// Tailnet round trips in ms, oldest first.
    pub(crate) latency: Vec<u64>,
    pub(crate) busy: Option<Busy>,
    /// A status refresh is queued or running (ticks do not pile up behind it).
    pub(crate) refreshing: bool,
    /// Latency results of the current probe round.
    pub(crate) probes: HashMap<ServerKey, Probe>,
    /// Counts probe rounds; results from an older round are dropped.
    pub(crate) probe_epoch: u64,
    /// Bottom-line message (errors, results).
    pub(crate) message: String,
    /// `message` reports a failure.
    pub(crate) failed: bool,
    /// Draw without color (`NO_COLOR`); symbols and modifiers still carry state.
    pub(crate) no_color: bool,
    ui: Ui,
}

/// Something that happened: a key, a clock tick, or an effect's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Key(KeyCode),
    Tick,
    Countries(Result<Vec<Country>, String>),
    /// A probe result; the flag is false when the helper could not answer.
    /// The string is the connected server's city, when recorded.
    Status(TunnelStatus, bool, Option<String>),
    /// Connect/disconnect finished.
    Finished(Result<(), String>),
    /// Start a new probe round over these servers (replaces the last round).
    /// An empty list just ends the current one.
    #[allow(dead_code, reason = "sent by the server-list redesign; reducer tested")]
    StartProbe(Vec<Server>),
    /// One server answered (`Some` ms) or did not (`None`), in round `epoch`.
    ProbeResult {
        epoch: u64,
        key: ServerKey,
        latency_ms: Option<u64>,
    },
}

/// I/O the reducer asks the event loop to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    LoadCountries,
    Refresh,
    /// Connect to the least-loaded server in the country, or in the city.
    Connect(CountryCode, Option<City>),
    Disconnect,
    /// Ping the servers concurrently and send back one `ProbeResult` each, all
    /// tagged with `epoch`.
    Probe {
        epoch: u64,
        servers: Vec<Server>,
    },
}

impl State {
    /// A fresh state with the `[tui]` settings applied.
    pub(crate) fn with_settings(t: &TuiSettings) -> Self {
        Self {
            ui: Ui::from(t),
            expanded: if t.start_expanded.value {
                Continent::ALL.into()
            } else {
                BTreeSet::new()
            },
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
    fn active(&self) -> &[String] {
        self.status.as_ref().map_or(&[], |st| st.active.as_slice())
    }

    fn start(&mut self, busy: Busy) {
        self.busy = Some(busy);
        self.message.clear();
        self.failed = false;
    }

    /// A foreground color, or none under `NO_COLOR`.
    fn fg(&self, color: Color) -> Style {
        if self.no_color {
            Style::new()
        } else {
            Style::new().fg(color)
        }
    }
}

/// Effects to run at startup.
pub(crate) fn init() -> Vec<Effect> {
    vec![Effect::LoadCountries, Effect::Refresh]
}

/// Pure transition: next state plus the effects to run.
pub(crate) fn update(mut s: State, event: Event) -> (State, Vec<Effect>) {
    let mut effects = Vec::new();
    match event {
        Event::Key(k) => on_key(&mut s, k, &mut effects),
        Event::Tick => {
            if s.busy.is_none() && !s.refreshing {
                s.refreshing = true;
                effects.push(Effect::Refresh);
            }
        }
        Event::Countries(Ok(list)) => {
            s.countries = list;
            s.load_failed = false;
            s.clamp_selection();
            s.place();
        }
        Event::Countries(Err(e)) => {
            s.load_failed = true;
            s.message = format!("servers: {e}");
            s.failed = true;
        }
        Event::Status(st, known, city) => {
            s.city = city;
            if let TailnetHealth::Reachable { latency_ms, .. } = &st.tailnet {
                s.latency.push(*latency_ms);
                if s.latency.len() > s.ui.graph_samples {
                    s.latency.remove(0);
                }
            }
            s.status = Some(st);
            s.refreshing = false;
            s.tunnel_known = known;
            s.place();
        }
        Event::Finished(result) => {
            s.failed = result.is_err();
            s.message = match (s.busy.take(), result) {
                (_, Err(e)) => e,
                (Some(Busy::Connecting(cc, None)), Ok(())) => format!("connected to {cc}"),
                (Some(Busy::Connecting(cc, Some(city))), Ok(())) => {
                    format!("connected to {cc}, {city}")
                }
                (_, Ok(())) => "disconnected".into(),
            };
            s.refreshing = true;
            effects.push(Effect::Refresh);
        }
        Event::StartProbe(servers) => {
            s.probe_epoch += 1;
            s.probes = servers
                .iter()
                .map(|x| (server_key(x), Probe::Pending))
                .collect();
            if !servers.is_empty() {
                effects.push(Effect::Probe {
                    epoch: s.probe_epoch,
                    servers,
                });
            }
        }
        Event::ProbeResult {
            epoch,
            key,
            latency_ms,
        } => {
            // A late result from before a country change must not land.
            if epoch == s.probe_epoch && s.probes.contains_key(&key) {
                let probe = latency_ms.map_or(Probe::Failed, Probe::Latency);
                s.probes.insert(key, probe);
            }
        }
    }
    (s, effects)
}

fn on_key(s: &mut State, k: KeyCode, fx: &mut Vec<Effect>) {
    match k {
        // The overlay swallows one key: closing it never quits or connects.
        _ if s.help => s.help = false,
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
        KeyCode::Char('q') | KeyCode::Esc => s.should_quit = true,
        KeyCode::Char('?') => s.help = true,
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
        // Keys that do I/O are ignored while a tunnel change is in flight.
        _ if s.busy.is_some() => {}
        KeyCode::Enter => {
            let target = match s.selected_row() {
                Some(Row::Country(c, ..)) => Some((c.code, None)),
                Some(Row::City(c, city)) => Some((c.code, Some(city.clone()))),
                _ => None,
            };
            if let Some((code, city)) = target {
                s.start(Busy::Connecting(
                    code,
                    city.as_ref().map(|c| c.name.clone()),
                ));
                fx.push(Effect::Connect(code, city));
            }
        }
        KeyCode::Char('d') => {
            s.start(Busy::Disconnecting);
            fx.push(Effect::Disconnect);
        }
        KeyCode::Char('r') => {
            s.load_failed = false;
            fx.push(Effect::LoadCountries);
        }
        _ => {}
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

/// Draws the whole dashboard: server list left, stats card right, hints bottom.
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
    let [top, main, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(header_line(s, top.width)), top);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(main);

    draw_list(s, frame, left);

    let card = Block::bordered().title(" stats ");
    let inner = card.inner(right);
    frame.render_widget(card, right);
    let [text, spark, _] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(SPARK_ROWS),
        Constraint::Min(0),
    ])
    .areas(inner);
    let shown = shown_latency(s, spark.width);
    frame.render_widget(Paragraph::new(stats_lines(s, text.width)), text);
    // Scale to the visible window (min..max) so steady and wobbling links both
    // show their shape; a flat series draws a low flat line, not a slab.
    let lo = shown.iter().copied().min().unwrap_or(0);
    let span = shown.iter().copied().max().unwrap_or(0) - lo;
    let limit = s.ui.spike_limit(median(shown));
    let bars: Vec<SparklineBar> = shown
        .iter()
        .map(|&v| {
            let level = 2 + (v - lo) * (SPARK_LEVELS - 2) / span.max(1);
            let bar = SparklineBar::from(level);
            if v > limit {
                bar.style(s.fg(Color::Yellow))
            } else {
                bar
            }
        })
        .collect();
    frame.render_widget(Sparkline::default().data(bars).max(SPARK_LEVELS), spark);

    frame.render_widget(Paragraph::new(bottom_line(s)), bottom);

    if s.help {
        draw_help(frame);
    }
}

fn draw_list(s: &State, frame: &mut Frame, area: ratatui::layout::Rect) {
    let rows = s.rows();
    let title = match (s.searching, s.query.is_empty()) {
        (true, _) => format!(" search: {}_ ", s.query),
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
                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    mark,
                    Span::raw(format!(" {}  {}{chevron}", c.code, c.name)),
                ]))
            }
            Row::City(c, city) => {
                let here = is_up(c) && s.city.as_deref() == Some(city.name.as_str());
                let mark = if here { up_mark() } else { Span::raw(" ") };
                ListItem::new(Line::from(vec![
                    Span::raw("      "),
                    mark,
                    Span::raw(format!(" {} ({})", city.name, city.server_count)),
                ]))
            }
        })
        .collect();
    if items.is_empty() {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let empty = match (s.query.is_empty(), s.load_failed) {
            (false, _) => "no match (esc clears)",
            (true, true) => "load failed (r retry)",
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

fn draw_help(frame: &mut Frame) {
    let key_w = HELP.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, what)| Line::from(format!(" {k:<key_w$}  {what}")))
        .collect();
    let width = lines.iter().map(Line::width).max().unwrap_or(0) + 3;
    let height = lines.len() + 2;
    let area = frame.area().centered(
        Constraint::Length(u16::try_from(width).unwrap_or(u16::MAX)),
        Constraint::Length(u16::try_from(height).unwrap_or(u16::MAX)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" keys ")),
        area,
    );
}

fn bottom_line(s: &State) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    if s.searching {
        return Line::from("type to filter  enter keep  esc clear  up/down select");
    }
    if s.message.is_empty() {
        return Line::from("? help  enter connect  d disconnect  / search  q quit");
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
    Line::from(vec![msg, Span::styled("  ? help", dim)])
}

/// The newest samples that fit `width` columns.
fn shown_latency(s: &State, width: u16) -> &[u64] {
    &s.latency[s.latency.len().saturating_sub(usize::from(width))..]
}

fn median(samples: &[u64]) -> u64 {
    let mut v = samples.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied().unwrap_or(0)
}

/// The tunnel state with its symbol; `name` adds the country name when up.
fn tunnel_span(s: &State, name: bool) -> Span<'static> {
    let bold = Modifier::BOLD;
    match (&s.busy, s.active()) {
        (Some(Busy::Connecting(cc, city)), _) => Span::styled(
            format!(
                "◐ connecting to {cc}{}...",
                city.as_ref().map_or_else(String::new, |c| format!(", {c}"))
            ),
            s.fg(Color::Yellow).add_modifier(bold),
        ),
        (Some(Busy::Disconnecting), _) => {
            Span::styled("◐ disconnecting...", s.fg(Color::Yellow).add_modifier(bold))
        }
        (None, _) if s.status.is_none() => Span::styled("· checking...", Style::new()),
        (None, _) if !s.tunnel_known => Span::styled(
            "? unknown (run wglane doctor)",
            s.fg(Color::Red).add_modifier(bold),
        ),
        (None, [first, ..]) => {
            let code = first.to_uppercase();
            let name = s
                .countries
                .iter()
                .find(|c| name && c.code.as_str() == code)
                .map_or_else(String::new, |c| format!("  {}", c.name));
            let city = s
                .city
                .as_ref()
                .map_or_else(String::new, |c| format!(", {c}"));
            Span::styled(
                format!("● up  {code}{name}{city}"),
                s.fg(Color::Green).add_modifier(bold),
            )
        }
        (None, []) => Span::styled("○ down", Style::new().add_modifier(bold)),
    }
}

/// One-line summary above the panels. Drops the sparkline, then the exit ip,
/// then the latency as the terminal narrows.
fn header_line(s: &State, width: u16) -> Line<'static> {
    let up = s.busy.is_none() && !s.active().is_empty();
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
        let mut spans = vec![tunnel_span(s, false)];
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

fn stats_lines(s: &State, width: u16) -> Vec<Line<'static>> {
    let tunnel = tunnel_span(s, true);
    let exit = s
        .status
        .as_ref()
        .and_then(|st| st.exit_ip)
        .map_or_else(|| "unknown".into(), |ip| ip.to_string());
    let tailnet = match s.status.as_ref().map(|st| &st.tailnet) {
        Some(TailnetHealth::Reachable { peer, latency_ms }) => {
            Span::styled(format!("ok ({peer}, {latency_ms} ms)"), s.fg(Color::Green))
        }
        Some(TailnetHealth::Unreachable { reason }) => {
            Span::styled(format!("unreachable ({reason})"), s.fg(Color::Yellow))
        }
        Some(TailnetHealth::NotInstalled) => "tailscale not installed".into(),
        _ => "unknown".into(),
    };
    vec![
        Line::from(vec![Span::raw("tunnel   "), tunnel]),
        Line::from(format!("exit ip  {exit}")),
        Line::from(vec![Span::raw("tailnet  "), tailnet]),
        Line::from(latency_label(shown_latency(s, width), usize::from(width))),
    ]
}

/// `latency  283 ms  min 282  max 353  (to tailnet peer, last N)`; the
/// parenthetical is dropped first when the panel is too narrow.
fn latency_label(samples: &[u64], width: usize) -> String {
    match (samples.last(), samples.iter().min(), samples.iter().max()) {
        (Some(now), Some(lo), Some(hi)) => {
            let base = format!("latency  {now} ms  min {lo}  max {hi}");
            let full = format!("{base}  (to tailnet peer, last {})", samples.len());
            if full.chars().count() <= width {
                full
            } else {
                base
            }
        }
        _ => "latency  -".into(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn cc(s: &str) -> CountryCode {
        s.parse().unwrap()
    }

    fn country(code: &str, name: &str) -> Country {
        Country {
            code: cc(code),
            name: name.into(),
            cities: vec![],
        }
    }

    /// Three countries, all groups closed: rows are three headers.
    fn loaded() -> State {
        State {
            countries: vec![
                country("DE", "Germany"),
                country("US", "United States"),
                country("ZA", "South Africa"),
            ],
            ..State::default()
        }
    }

    /// `loaded` with every group open. Rows:
    /// 0 Africa, 1 ZA, 2 Europe, 3 DE, 4 North America, 5 US.
    fn open() -> State {
        State {
            expanded: [
                Continent::Africa,
                Continent::Europe,
                Continent::NorthAmerica,
            ]
            .into(),
            ..loaded()
        }
    }

    fn with_cities(code: &str, name: &str, cities: &[(&str, u32)]) -> Country {
        Country {
            cities: cities
                .iter()
                .zip(1u32..)
                .map(|((n, count), id)| City {
                    id,
                    name: (*n).into(),
                    identifier: format!("{}_{}", code.to_lowercase(), n.to_lowercase()),
                    server_count: *count,
                })
                .collect(),
            ..country(code, name)
        }
    }

    /// Fake data with a city level: DE and US have several cities, ZA one (so
    /// no city level). All continents open, all countries closed. Rows:
    /// 0 Africa, 1 ZA, 2 Europe, 3 DE, 4 North America, 5 US.
    fn tree() -> State {
        State {
            countries: vec![
                with_cities("DE", "Germany", &[("Frankfurt", 12), ("Berlin", 5)]),
                with_cities(
                    "US",
                    "United States",
                    &[("New York", 9), ("Los Angeles", 7), ("Chicago", 4)],
                ),
                with_cities("ZA", "South Africa", &[("Johannesburg", 4)]),
            ],
            ..open()
        }
    }

    /// The tree as text: continents bare, countries with `+`/`-` when they
    /// have a city level (open/closed), cities indented.
    fn shape(s: &State) -> Vec<String> {
        s.rows()
            .iter()
            .map(|r| match r {
                Row::Group(k, ..) => k.to_string(),
                Row::Country(c, _, _) if State::is_leaf(c) => {
                    format!(" {}", c.code)
                }
                Row::Country(c, open, _) => {
                    format!(" {}{}", c.code, if *open { "+" } else { "-" })
                }
                Row::City(_, ci) => format!("   {}", ci.name),
            })
            .collect()
    }

    fn status(active: &[&str], tailnet: TailnetHealth) -> TunnelStatus {
        TunnelStatus {
            active: active.iter().map(|s| (*s).to_owned()).collect(),
            exit_ip: Some("203.0.113.7".parse().unwrap()),
            exit_country: None,
            protected: None,
            tailnet,
            bypass_routes: 0,
        }
    }

    fn reachable(ms: u64) -> TailnetHealth {
        TailnetHealth::Reachable {
            peer: "nas".into(),
            latency_ms: ms,
        }
    }

    fn up(s: State, active: &[&str], ms: u64) -> State {
        update(s, Event::Status(status(active, reachable(ms)), true, None)).0
    }

    fn key(s: State, k: KeyCode) -> (State, Vec<Effect>) {
        update(s, Event::Key(k))
    }

    fn keys(s: State, ks: &[KeyCode]) -> State {
        ks.iter().fold(s, |s, k| key(s, *k).0)
    }

    /// The code under the cursor, if it is on a country.
    fn on(s: &State) -> Option<String> {
        match s.selected_row() {
            Some(Row::Country(c, ..)) => Some(c.code.to_string()),
            _ => None,
        }
    }

    #[test]
    fn q_and_esc_quit() {
        assert!(key(State::default(), KeyCode::Char('q')).0.should_quit);
        assert!(key(State::default(), KeyCode::Esc).0.should_quit);
        assert!(!key(State::default(), KeyCode::Char('x')).0.should_quit);
    }

    #[test]
    fn arrows_clamp_to_the_list() {
        let (s, _) = key(open(), KeyCode::Up);
        assert_eq!(s.selected, 0);
        let s = keys(s, &[KeyCode::Down; 10]);
        assert_eq!(s.selected, 5);
        // An empty list must not underflow.
        assert_eq!(key(State::default(), KeyCode::Down).0.selected, 0);
        assert_eq!(key(State::default(), KeyCode::Up).0.selected, 0);
    }

    #[test]
    fn vim_keys_and_home_end_move_the_selection() {
        let (s, _) = key(open(), KeyCode::Char('j'));
        assert_eq!(on(&s).as_deref(), Some("ZA"));
        let (s, _) = key(s, KeyCode::Char('G'));
        assert_eq!(on(&s).as_deref(), Some("US"));
        let (s, _) = key(s, KeyCode::Char('k'));
        assert_eq!(s.selected, 4);
        let (s, _) = key(s, KeyCode::Char('g'));
        assert_eq!(s.selected, 0);
        assert_eq!(key(s, KeyCode::End).0.selected, 5);
        assert_eq!(key(State::default(), KeyCode::Char('G')).0.selected, 0);
        // While searching they are characters.
        assert_eq!(searching("jkgGhl ").query, "jkgGhl ");
    }

    #[test]
    fn enter_connects_the_selection_and_blocks_input() {
        let (s, _) = key(open(), KeyCode::Char('G'));
        let (s, fx) = key(s, KeyCode::Enter);
        assert_eq!(fx, [Effect::Connect(cc("US"), None)]);
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"), None)));
        let (s, fx) = key(s, KeyCode::Char('d'));
        assert_eq!(fx, Vec::new());
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"), None)));
    }

    #[test]
    fn enter_on_empty_list_does_nothing() {
        let (s, fx) = key(State::default(), KeyCode::Enter);
        assert!(fx.is_empty() && s.busy.is_none());
    }

    #[test]
    fn d_disconnects() {
        let (s, fx) = key(State::default(), KeyCode::Char('d'));
        assert_eq!(fx, [Effect::Disconnect]);
        assert_eq!(s.busy, Some(Busy::Disconnecting));
    }

    #[test]
    fn finished_reports_and_refreshes() {
        let s = keys(open(), &[KeyCode::Down, KeyCode::Enter]);
        let (s, fx) = update(s, Event::Finished(Ok(())));
        assert_eq!((&s.busy, s.message.as_str()), (&None, "connected to ZA"));
        assert_eq!(fx, [Effect::Refresh]);
        let (s, _) = key(s, KeyCode::Char('d'));
        let (s, _) = update(s, Event::Finished(Err("helper missing".into())));
        assert_eq!(s.message, "helper missing");
        assert!(s.failed);
    }

    #[test]
    fn tick_refreshes_unless_busy() {
        assert_eq!(update(State::default(), Event::Tick).1, [Effect::Refresh]);
        let (s, _) = key(State::default(), KeyCode::Char('d'));
        assert_eq!(update(s, Event::Tick).1, Vec::new());
    }

    #[allow(
        clippy::default_trait_access,
        reason = "Overrides is not imported here"
    )]
    fn tui_settings(expanded: bool, samples: usize, ratio: f64) -> TuiSettings {
        let file = format!(
            "[tui]\nstart_expanded = {expanded}\ngraph_samples = {samples}\nwarn_ratio = {ratio:?}\n"
        );
        wglane_core::FileConfig::parse(&file)
            .unwrap()
            .resolve(&Default::default(), Default::default())
            .tui
    }

    #[test]
    fn tui_settings_apply() {
        let s = State::with_settings(&tui_settings(true, 12, 2.0));
        assert_eq!(s.expanded.len(), Continent::ALL.len());
        assert_eq!(s.ui.spike_limit(100), 200);
        let mut s = s;
        for i in 0..30 {
            s = up(s, &[], i);
        }
        assert_eq!(s.latency.len(), 12);
        assert!(
            State::with_settings(&tui_settings(false, 60, 1.5))
                .expanded
                .is_empty()
        );
        assert_eq!(
            State::default().ui.spike_limit(100),
            150,
            "defaults match the old constants"
        );
    }

    #[test]
    fn latency_history_is_capped_and_skips_unreachable() {
        let mut s = State::default();
        for i in 0..70 {
            s = up(s, &[], i);
        }
        assert_eq!(s.latency.len(), DEFAULT_GRAPH_SAMPLES);
        assert_eq!(s.latency.last(), Some(&69));
        let down = TailnetHealth::NotInstalled;
        let s = update(s, Event::Status(status(&[], down), true, None)).0;
        assert_eq!(s.latency.len(), DEFAULT_GRAPH_SAMPLES);
    }

    #[test]
    fn country_reload_keeps_selection_in_range() {
        let (s, _) = key(open(), KeyCode::Char('G'));
        let s = update(s, Event::Countries(Ok(vec![country("DE", "Germany")]))).0;
        assert_eq!(on(&s).as_deref(), Some("DE"));
    }

    #[test]
    fn groups_start_closed_in_continent_order() {
        let s = loaded();
        let rows = s.rows();
        assert_eq!(
            rows,
            [
                Row::Group(Continent::Africa, 1, false),
                Row::Group(Continent::Europe, 1, false),
                Row::Group(Continent::NorthAmerica, 1, false),
            ]
        );
    }

    #[test]
    fn unknown_codes_are_listed_under_other() {
        let mut s = open();
        s.countries.push(country("XK", "Kosovo"));
        s.expanded.insert(Continent::Other);
        let rows = s.rows();
        assert_eq!(rows[rows.len() - 2], Row::Group(Continent::Other, 1, true));
        assert!(matches!(rows[rows.len() - 1], Row::Country(c, ..) if c.name == "Kosovo"));
    }

    #[test]
    fn enter_space_l_and_h_toggle_a_header() {
        let (s, fx) = key(loaded(), KeyCode::Enter);
        assert!(fx.is_empty() && s.busy.is_none());
        assert_eq!(s.rows()[0], Row::Group(Continent::Africa, 1, true));
        let (s, _) = key(s, KeyCode::Char(' '));
        assert_eq!(s.rows()[0], Row::Group(Continent::Africa, 1, false));
        let s = keys(s, &[KeyCode::Char('l'), KeyCode::Char('l')]);
        assert!(s.expanded.contains(&Continent::Africa));
        let (s, _) = key(s, KeyCode::Char('h'));
        assert!(s.expanded.is_empty());
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn h_on_a_country_closes_its_group_and_selects_the_header() {
        let s = keys(open(), &[KeyCode::Char('G'), KeyCode::Char('h')]);
        assert!(!s.expanded.contains(&Continent::NorthAmerica));
        assert_eq!(
            s.selected_row(),
            Some(Row::Group(Continent::NorthAmerica, 1, false))
        );
    }

    #[test]
    fn start_opens_the_connected_group_once() {
        let s = up(loaded(), &["us"], 20);
        assert_eq!(s.expanded, [Continent::NorthAmerica].into());
        assert_eq!(on(&s).as_deref(), Some("US"));
        // Later probes never move the cursor or reopen groups.
        let s = keys(s, &[KeyCode::Char('h'), KeyCode::Char('g')]);
        let s = up(s, &["de"], 20);
        assert!(s.expanded.is_empty() && s.selected == 0);
    }

    #[test]
    fn start_waits_for_the_catalog_and_a_down_tunnel_opens_nothing() {
        let s = up(State::default(), &["za"], 20);
        assert!(!s.placed);
        let s = update(s, Event::Countries(Ok(loaded().countries))).0;
        assert_eq!(on(&s).as_deref(), Some("ZA"));
        let s = up(loaded(), &[], 20);
        assert!(s.placed && s.expanded.is_empty() && s.selected == 0);
    }

    #[test]
    fn help_opens_and_the_next_key_only_closes_it() {
        let (s, fx) = key(open(), KeyCode::Char('?'));
        assert!(s.help && fx.is_empty());
        for k in [
            KeyCode::Char('q'),
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Char('d'),
        ] {
            let (closed, fx) = key(s.clone(), k);
            assert!(!closed.help && !closed.should_quit && closed.busy.is_none());
            assert_eq!(fx, []);
        }
    }

    #[test]
    fn failed_load_is_reported_and_r_retries() {
        let (s, _) = update(loaded(), Event::Countries(Err("offline".into())));
        assert!(s.load_failed && s.failed);
        assert_eq!(s.message, "servers: offline");
        let (s, fx) = key(s, KeyCode::Char('r'));
        assert_eq!(fx, [Effect::LoadCountries]);
        assert!(!s.load_failed);
    }

    #[test]
    fn starting_a_change_clears_the_old_message() {
        let (s, _) = key(open(), KeyCode::Char('d'));
        let (s, _) = update(s, Event::Finished(Err("helper missing".into())));
        assert!(s.failed);
        let s = keys(s, &[KeyCode::Char('G'), KeyCode::Enter]);
        assert!(s.busy.is_some() && s.message.is_empty() && !s.failed);
    }

    fn typed(s: State, text: &str) -> State {
        text.chars().fold(s, |s, c| key(s, KeyCode::Char(c)).0)
    }

    fn searching(text: &str) -> State {
        typed(key(loaded(), KeyCode::Char('/')).0, text)
    }

    fn codes(s: &State) -> Vec<String> {
        s.visible().iter().map(|c| c.code.to_string()).collect()
    }

    #[test]
    fn slash_enters_search_mode() {
        let (s, fx) = key(loaded(), KeyCode::Char('/'));
        assert!(s.searching && fx.is_empty() && !s.should_quit);
    }

    #[test]
    fn typing_filters_by_code_or_name_case_insensitively() {
        assert_eq!(codes(&searching("za")), ["ZA"]);
        assert_eq!(codes(&searching("SOUTH")), ["ZA"]);
        assert_eq!(codes(&searching("")), ["DE", "US", "ZA"]);
    }

    #[test]
    fn code_prefix_ranks_before_name_matches() {
        let s = State {
            countries: vec![country("AT", "Austria"), country("US", "United States")],
            ..State::default()
        };
        let s = typed(key(s, KeyCode::Char('/')).0, "us");
        assert_eq!(codes(&s), ["US", "AT"]);
        // The tree keeps that order: the best match's group comes first.
        assert_eq!(on(&s).as_deref(), Some("US"));
        assert_eq!(s.rows()[2], Row::Group(Continent::Europe, 1, true));
    }

    #[test]
    fn a_filter_opens_matching_groups_and_skips_headers() {
        // Groups are closed, yet every match is shown and selectable.
        let s = searching("a");
        let rows = s.rows();
        assert!(rows.iter().all(|r| !matches!(r, Row::Group(_, _, false))));
        assert_eq!(rows.len(), 6);
        let s = keys(s, &[KeyCode::Enter, KeyCode::Up, KeyCode::Char('g')]);
        assert!(on(&s).is_some());
        let s = keys(s, &[KeyCode::Down; 5]);
        assert!(on(&s).is_some());
        // Toggles are ignored while groups are forced open.
        let (t, _) = key(s.clone(), KeyCode::Char('h'));
        assert_eq!(t.rows(), s.rows());
        // Clearing the filter restores the closed tree.
        let (s, _) = key(s, KeyCode::Esc);
        assert_eq!(s.rows().len(), 3);
    }

    #[test]
    fn backspace_edits_the_query() {
        let (s, _) = key(searching("zax"), KeyCode::Backspace);
        assert_eq!(s.query, "za");
        assert_eq!(codes(&s), ["ZA"]);
        // Backspace on an empty query is harmless.
        assert_eq!(key(searching(""), KeyCode::Backspace).0.query, "");
    }

    #[test]
    fn esc_clears_the_query_and_leaves_search_mode() {
        let (s, _) = key(searching("za"), KeyCode::Esc);
        assert!(!s.searching && s.query.is_empty() && !s.should_quit);
        assert_eq!(codes(&s).len(), 3);
    }

    #[test]
    fn esc_clears_a_kept_filter_before_quitting() {
        let (s, _) = key(searching("za"), KeyCode::Enter);
        let (s, _) = key(s, KeyCode::Esc);
        assert!(s.query.is_empty() && !s.should_quit);
        assert!(key(s, KeyCode::Esc).0.should_quit);
    }

    #[test]
    fn enter_while_searching_keeps_filter_and_does_not_connect() {
        // "u": US by code, then ZA by name ("south").
        let (s, fx) = key(searching("u"), KeyCode::Down);
        assert_eq!(fx, Vec::new());
        assert_eq!(on(&s).as_deref(), Some("ZA"));
        let (s, fx) = key(s, KeyCode::Enter);
        assert!(fx.is_empty() && s.busy.is_none());
        assert!(!s.searching);
        assert_eq!(s.query, "u");
        assert_eq!(on(&s).as_deref(), Some("US"));
        // Outside search mode Enter connects the first match.
        let (s, fx) = key(s, KeyCode::Enter);
        assert_eq!(fx, [Effect::Connect(cc("US"), None)]);
        assert!(s.busy.is_some());
    }

    #[test]
    fn q_and_d_are_characters_while_searching() {
        let s = searching("qd");
        assert!(!s.should_quit && s.busy.is_none());
        assert_eq!(s.query, "qd");
        // ...and a '/' typed in search mode is a character too.
        assert_eq!(typed(searching(""), "/").query, "/");
    }

    #[test]
    fn selection_follows_the_filtered_list() {
        let (s, _) = key(open(), KeyCode::Char('G'));
        let s = typed(key(s, KeyCode::Char('/')).0, "unit");
        assert_eq!(codes(&s), ["US"]);
        assert_eq!(on(&s).as_deref(), Some("US"));
        let s = keys(s, &[KeyCode::Down, KeyCode::Up]);
        assert_eq!(on(&s).as_deref(), Some("US"));
    }

    #[test]
    fn empty_result_is_safe_and_enter_does_nothing() {
        let s = searching("zzz");
        assert!(s.rows().is_empty() && s.selected == 0);
        let (s, _) = key(s, KeyCode::Down);
        let (s, _) = key(s, KeyCode::Up);
        let (s, _) = key(s, KeyCode::Enter);
        let (s, fx) = key(s, KeyCode::Enter);
        assert!(fx.is_empty() && s.busy.is_none());
        let (s, fx) = key(typed(s, "x"), KeyCode::Enter);
        assert!(fx.is_empty() && s.busy.is_none());
    }

    #[test]
    fn selection_stays_on_a_selectable_row_for_any_keys() {
        const KEYS: [KeyCode; 24] = [
            KeyCode::Char('/'),
            KeyCode::Char('z'),
            KeyCode::Char('u'),
            KeyCode::Char('x'),
            KeyCode::Char('n'),
            KeyCode::Char('e'),
            KeyCode::Char('w'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('g'),
            KeyCode::Char('G'),
            KeyCode::Char('h'),
            KeyCode::Char('l'),
            KeyCode::Char(' '),
            KeyCode::Char('?'),
            KeyCode::Backspace,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('d'),
        ];
        // Deterministic LCG walk; Finished clears `busy` so input keeps flowing.
        let mut seed = 0x2545_f491_u64;
        let mut s = tree();
        for _ in 0..20_000 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let k = KEYS[(seed >> 33) as usize % KEYS.len()];
            let (next, fx) = key(s, k);
            s = next;
            // Whatever Enter connects must be the row under the cursor.
            for e in &fx {
                if let Effect::Connect(code, city) = e {
                    match s.selected_row() {
                        Some(Row::Country(c, ..)) => assert!(city.is_none() && c.code == *code),
                        Some(Row::City(c, ci)) => {
                            assert!(c.code == *code && city.as_ref() == Some(ci));
                        }
                        r => panic!("connected from {r:?}"),
                    }
                }
            }
            s.busy = None;
            s.should_quit = false;
            let rows = s.rows();
            assert!(
                s.selected < rows.len().max(1),
                "selected {} of {}, query {:?}",
                s.selected,
                rows.len(),
                s.query
            );
            assert!(
                rows.get(s.selected).is_none_or(|r| s.selectable(r)),
                "header selected under filter {:?}",
                s.query
            );
        }
    }

    #[test]
    fn no_color_draws_no_colors() {
        let mut s = up(open(), &["us"], 9);
        s.message = "boom".into();
        s.failed = true;
        let colored = |s: &State| {
            render(s)
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|c| c.fg != Color::Reset || c.bg != Color::Reset)
        };
        assert!(colored(&s));
        s.no_color = true;
        assert!(!colored(&s));
    }

    fn render_sized(s: &State, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(s, f)).unwrap();
        t
    }

    fn render(s: &State) -> Terminal<TestBackend> {
        render_sized(s, 70, 10)
    }

    fn with_latency(samples: impl IntoIterator<Item = u64>) -> State {
        samples
            .into_iter()
            .fold(loaded(), |s, ms| up(s, &["ke"], ms))
    }

    #[test]
    fn draws_search_with_a_match() {
        let s = searching("za");
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_search_with_no_match() {
        let s = searching("zzz");
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_search_in_tree() {
        let s = State {
            countries: vec![
                country("FR", "France"),
                country("DE", "Germany"),
                country("JP", "Japan"),
                country("KE", "Kenya"),
            ],
            ..State::default()
        };
        // Name matches only, alphabetical: France, Germany, Japan.
        let s = typed(key(s, KeyCode::Char('/')).0, "an");
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_tree_collapsed() {
        insta::assert_snapshot!(render(&up(loaded(), &[], 30)).backend());
    }

    #[test]
    fn draws_tree_expanded() {
        let s = keys(up(open(), &[], 30), &[KeyCode::Char('j')]);
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_latency_without_samples() {
        insta::assert_snapshot!(render_sized(&loaded(), 70, 12).backend());
    }

    #[test]
    fn draws_latency_with_one_sample() {
        insta::assert_snapshot!(render_sized(&with_latency([286]), 70, 12).backend());
    }

    #[test]
    fn draws_latency_with_variance() {
        let s = with_latency((0..30).map(|i| 280 + (i * 7) % 23));
        insta::assert_snapshot!(render_sized(&s, 70, 12).backend());
    }

    fn collapsed_us() -> State {
        // Cursor on the closed North America header.
        keys(up(loaded(), &["us"], 20), &[KeyCode::Char('h')])
    }

    #[test]
    fn right_opens_a_closed_header_then_steps_in() {
        let s = keys(loaded(), &[KeyCode::Right]);
        assert_eq!(s.expanded, [Continent::Africa].into());
        assert!(on(&s).is_none());
        let s = keys(s, &[KeyCode::Right]);
        assert!(on(&s).is_some());
        // On a country, Right does nothing.
        let t = keys(s.clone(), &[KeyCode::Right]);
        assert_eq!(t.selected, s.selected);
    }

    #[test]
    fn left_closes_a_header_or_jumps_from_a_country_to_it() {
        let s = up(loaded(), &["us"], 20);
        assert_eq!(on(&s).as_deref(), Some("US"));
        let s = keys(s, &[KeyCode::Left]);
        assert!(on(&s).is_none());
        assert!(
            s.expanded.contains(&Continent::NorthAmerica),
            "jump keeps the group open"
        );
        let s = keys(s, &[KeyCode::Left]);
        assert!(s.expanded.is_empty());
        assert_eq!(collapsed_us().expanded, s.expanded);
    }

    #[test]
    fn arrows_draw_the_tree_open_and_closed() {
        let s = keys(loaded(), &[KeyCode::Right]);
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_header_up() {
        let s = with_latency((0..8).map(|i| 280 + i * 5 % 17));
        insta::assert_snapshot!(render_sized(&s, 70, 12).backend());
    }

    #[test]
    fn draws_header_down() {
        insta::assert_snapshot!(render_sized(&up(loaded(), &[], 20), 70, 12).backend());
    }

    #[test]
    fn draws_header_connecting() {
        let s = keys(open(), &[KeyCode::Char('G'), KeyCode::Enter]);
        insta::assert_snapshot!(render_sized(&s, 70, 12).backend());
    }

    #[test]
    fn header_degrades_on_narrow_widths() {
        let s = with_latency((0..8).map(|i| 280 + i * 5 % 17));
        for w in [44, 40, 30, 18] {
            let line = header_line(&s, w);
            assert!(line.width() <= usize::from(w), "{w}: {line}");
        }
        assert!(header_line(&s, 44).to_string().contains("203.0.113.7"));
        assert!(!header_line(&s, 30).to_string().contains("exit"));
        assert!(!header_line(&s, 40).to_string().contains('▁'));
        insta::assert_snapshot!(render_sized(&s, 40, 12).backend());
    }

    #[test]
    fn draws_latency_steady() {
        let s = with_latency((0..30).map(|i| 282 + (i * 5) % 8 * 9));
        insta::assert_snapshot!(render_sized(&s, 120, 12).backend());
    }

    #[test]
    fn draws_latency_with_one_spike() {
        let s = with_latency((0..30).map(|i| if i == 20 { 900 } else { 300 + i % 4 }));
        insta::assert_snapshot!(render_sized(&s, 70, 12).backend());
    }

    #[test]
    fn spike_is_drawn_in_the_warning_color_unless_no_color() {
        let mut s = with_latency((0..30).map(|i| if i == 20 { 900 } else { 300 }));
        let yellow = |s: &State| {
            render_sized(s, 70, 12)
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|c| c.fg == Color::Yellow)
                .count()
        };
        assert!(yellow(&s) > 0);
        s.no_color = true;
        assert_eq!(yellow(&s), 0);
    }

    #[test]
    fn draws_latency_in_a_narrow_panel() {
        let s = with_latency((0..30).map(|i| 20 + (i * 13) % 90));
        insta::assert_snapshot!(render_sized(&s, 40, 12).backend());
    }

    #[test]
    fn draws_connected() {
        let s = [12, 18, 15, 40, 22]
            .into_iter()
            .fold(loaded(), |s, ms| up(s, &["us"], ms));
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_empty_and_connecting() {
        insta::assert_snapshot!("draws_empty", render(&State::default()).backend());
        let s = keys(open(), &[KeyCode::Down, KeyCode::Enter]);
        insta::assert_snapshot!("draws_connecting", render(&s).backend());
    }

    #[test]
    fn draws_down() {
        let s = update(
            loaded(),
            Event::Status(status(&[], TailnetHealth::NotInstalled), true, None),
        )
        .0;
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_unknown_when_the_helper_cannot_answer() {
        let s = update(
            loaded(),
            Event::Status(status(&[], reachable(30)), false, None),
        )
        .0;
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_an_error_before_the_hints() {
        let s = keys(open(), &[KeyCode::Down, KeyCode::Enter]);
        let (s, _) = update(
            s,
            Event::Finished(Err("wg-quick failed: run wglane doctor".into())),
        );
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_a_failed_server_load() {
        let s = update(State::default(), Event::Countries(Err("offline".into()))).0;
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_help() {
        let (s, _) = key(loaded(), KeyCode::Char('?'));
        insta::assert_snapshot!(render_sized(&s, 70, 16).backend());
    }

    #[test]
    fn draws_a_too_small_terminal() {
        insta::assert_snapshot!(render_sized(&loaded(), 30, 6).backend());
    }

    #[test]
    fn draws_the_minimum_size() {
        insta::assert_snapshot!(
            render_sized(&up(loaded(), &["de"], 41), MIN_SIZE.0, MIN_SIZE.1).backend()
        );
    }

    fn at(s: &State) -> String {
        match s.selected_row() {
            Some(Row::Group(k, ..)) => k.to_string(),
            Some(Row::Country(c, ..)) => c.code.to_string(),
            Some(Row::City(_, ci)) => ci.name.clone(),
            None => "-".into(),
        }
    }

    /// `tree()` with US open and the cursor on it.
    fn us_open() -> State {
        let s = keys(tree(), &[KeyCode::Char('G'), KeyCode::Char(' ')]);
        assert_eq!(at(&s), "US");
        s
    }

    #[test]
    fn tree_starts_with_countries_closed_and_single_city_countries_flat() {
        assert_eq!(
            shape(&tree()),
            ["Africa", " ZA", "Europe", " DE-", "North America", " US-"]
        );
    }

    #[test]
    fn space_opens_and_closes_a_country_but_not_a_flat_one_or_a_city() {
        let s = us_open();
        assert_eq!(
            shape(&s)[5..],
            [" US+", "   New York", "   Los Angeles", "   Chicago"]
        );
        // On a city and on a flat country, space does nothing.
        let s = keys(s, &[KeyCode::Down, KeyCode::Char(' ')]);
        assert_eq!((at(&s), shape(&s).len()), ("New York".into(), 9));
        let s = key(
            keys(s, &[KeyCode::Char('g'), KeyCode::Down]),
            KeyCode::Char(' '),
        )
        .0;
        assert_eq!((at(&s), shape(&s).len()), ("ZA".into(), 9));
        let s = keys(
            s,
            &[
                KeyCode::Char('G'),
                KeyCode::Char('k'),
                KeyCode::Char('k'),
                KeyCode::Char('k'),
                KeyCode::Char(' '),
            ],
        );
        assert_eq!(at(&s), "US");
        assert_eq!(shape(&s)[5..], [" US-"]);
    }

    #[test]
    fn enter_on_a_country_connects_the_country_even_when_it_has_cities() {
        let (s, fx) = key(us_open(), KeyCode::Enter);
        assert_eq!(fx, [Effect::Connect(cc("US"), None)]);
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"), None)));
    }

    #[test]
    fn enter_on_a_city_connects_that_city() {
        let s = keys(us_open(), &[KeyCode::Down, KeyCode::Down]);
        assert_eq!(at(&s), "Los Angeles");
        let (s, fx) = key(s, KeyCode::Enter);
        let Some(Effect::Connect(code, Some(city))) = fx.first() else {
            panic!("{fx:?}");
        };
        assert_eq!(
            (*code, city.name.as_str(), city.id),
            (cc("US"), "Los Angeles", 2)
        );
        assert_eq!(
            s.busy,
            Some(Busy::Connecting(cc("US"), Some("Los Angeles".into())))
        );
        // Input is blocked while connecting, but the result names the city.
        let (s, _) = update(s, Event::Finished(Ok(())));
        assert_eq!(s.message, "connected to US, Los Angeles");
    }

    #[test]
    fn l_and_right_open_and_h_and_left_close_at_every_level() {
        // Country: l opens, h closes it (cursor stays), h again closes the group.
        let s = keys(tree(), &[KeyCode::Char('G'), KeyCode::Char('l')]);
        assert_eq!(shape(&s).len(), 9);
        let s = key(s, KeyCode::Char('h')).0;
        assert_eq!((at(&s), shape(&s).len()), ("US".into(), 6));
        let s = key(s, KeyCode::Char('h')).0;
        assert_eq!((at(&s), shape(&s).len()), ("North America".into(), 5));
        // Right opens a closed country, then steps into it; right on a city is a no-op.
        let s = keys(tree(), &[KeyCode::Char('G'), KeyCode::Right]);
        assert_eq!((at(&s), shape(&s).len()), ("US".into(), 9));
        let s = key(s, KeyCode::Right).0;
        assert_eq!(at(&s), "New York");
        let s = keys(s, &[KeyCode::Right, KeyCode::Char('l')]);
        assert_eq!((at(&s), shape(&s).len()), ("New York".into(), 9));
        // Left on a city jumps to its country; on an open country it closes it;
        // on a closed country it jumps to the continent.
        let s = key(s, KeyCode::Left).0;
        assert_eq!(at(&s), "US");
        let s = key(s, KeyCode::Left).0;
        assert_eq!((at(&s), shape(&s).len()), ("US".into(), 6));
        let s = key(s, KeyCode::Left).0;
        assert_eq!(at(&s), "North America");
        assert_eq!(shape(&s).len(), 6);
        // h on a city closes its country and lands on it.
        let s = keys(
            tree(),
            &[
                KeyCode::Char('G'),
                KeyCode::Right,
                KeyCode::Right,
                KeyCode::Char('h'),
            ],
        );
        assert_eq!((at(&s), shape(&s).len()), ("US".into(), 6));
        // Flat country: l/right do nothing, left/h go up to the group.
        let s = keys(tree(), &[KeyCode::Down, KeyCode::Char('l'), KeyCode::Right]);
        assert_eq!((at(&s), shape(&s).len()), ("ZA".into(), 6));
        let s = key(s, KeyCode::Left).0;
        assert_eq!(at(&s), "Africa");
        assert_eq!(shape(&s).len(), 6);
    }

    #[test]
    fn up_down_home_end_walk_through_cities() {
        let s = us_open();
        let s = key(s, KeyCode::Char('G')).0;
        assert_eq!(at(&s), "Chicago");
        let s = keys(s, &[KeyCode::Up, KeyCode::Char('k'), KeyCode::Up]);
        assert_eq!(at(&s), "US");
        let s = keys(
            s,
            &[KeyCode::Down, KeyCode::Down, KeyCode::Down, KeyCode::Down],
        );
        assert_eq!(at(&s), "Chicago");
        assert_eq!(key(s, KeyCode::Home).0.selected, 0);
    }

    #[test]
    fn search_by_city_opens_the_country_and_shows_only_matching_cities() {
        let s = searching_in(tree(), "new y");
        assert_eq!(shape(&s), ["North America", " US+", "   New York"]);
        // The parent country does not match itself, so the cursor skips it.
        assert_eq!(at(&s), "New York");
        let (_, fx) = key(key(s, KeyCode::Enter).0, KeyCode::Enter);
        assert!(
            matches!(fx.first(), Some(Effect::Connect(c, Some(ci))) if *c == cc("US") && ci.name == "New York")
        );
    }

    #[test]
    fn search_matches_city_names_case_and_accent_insensitively() {
        let s = State {
            countries: vec![with_cities(
                "CA",
                "Canada",
                &[("Montréal", 3), ("Toronto", 8)],
            )],
            ..State::default()
        };
        let s = searching_in(s, "MONTREAL");
        assert_eq!(shape(&s), ["North America", " CA+", "   Montréal"]);
    }

    #[test]
    fn search_ranks_code_prefix_first_and_country_matches_show_no_cities() {
        // "u" hits the US by code and name; no city name contains it.
        let s = searching_in(tree(), "us");
        assert_eq!(shape(&s), ["North America", " US-"]);
        assert_eq!(at(&s), "US");
        // A name match keeps the country selectable and closed.
        let s = searching_in(tree(), "germ");
        assert_eq!(
            (shape(&s), at(&s)),
            (vec!["Europe".to_owned(), " DE-".to_owned()], "DE".into())
        );
    }

    #[test]
    fn search_on_the_only_city_of_a_flat_country_selects_the_country() {
        let s = searching_in(tree(), "johan");
        assert_eq!(shape(&s), ["Africa", " ZA"]);
        let (_, fx) = key(key(s, KeyCode::Enter).0, KeyCode::Enter);
        assert_eq!(fx, [Effect::Connect(cc("ZA"), None)]);
    }

    #[test]
    fn search_mixes_country_and_city_hits_in_one_tree() {
        // By name: Germany. By city: Frankfurt, Los Angeles, and Johannesburg,
        // which is ZA's only city.
        let s = searching_in(tree(), "an");
        assert_eq!(
            shape(&s),
            [
                "Europe",
                " DE+",
                "   Frankfurt",
                "Africa",
                " ZA",
                "North America",
                " US+",
                "   Los Angeles"
            ]
        );
        // Germany matches by name, so it is selectable and first; US is not.
        assert_eq!(at(&s), "DE");
        let s = keys(s, &[KeyCode::Down, KeyCode::Down, KeyCode::Down]);
        assert_eq!(at(&s), "Los Angeles");
    }

    #[test]
    fn filtering_ignores_open_state_and_clearing_restores_it() {
        let s = key(searching_in(us_open(), "chi"), KeyCode::Enter).0;
        assert_eq!(shape(&s), ["North America", " US+", "   Chicago"]);
        // Toggle keys cannot change the filtered view.
        let s = keys(s, &[KeyCode::Char('h'), KeyCode::Left, KeyCode::Right]);
        assert_eq!(shape(&s), ["North America", " US+", "   Chicago"]);
        let s = key(s, KeyCode::Esc).0;
        assert_eq!(shape(&s).len(), 9, "US was open before the search");
    }

    #[test]
    fn the_connected_city_is_marked_and_opened_at_start() {
        let s = update(
            tree(),
            Event::Status(status(&["us"], reachable(30)), true, Some("Chicago".into())),
        )
        .0;
        assert_eq!(at(&s), "Chicago");
        assert_eq!(
            shape(&s)[5..],
            [" US+", "   New York", "   Los Angeles", "   Chicago"]
        );
    }

    fn searching_in(s: State, text: &str) -> State {
        typed(key(s, KeyCode::Char('/')).0, text)
    }

    fn connected_city_state() -> State {
        update(
            tree(),
            Event::Status(
                status(&["us"], reachable(283)),
                true,
                Some("New York".into()),
            ),
        )
        .0
    }

    #[test]
    fn draws_countries_closed_with_a_city_level() {
        insta::assert_snapshot!(render(&tree()).backend());
    }

    #[test]
    fn draws_country_expanded_with_cities() {
        insta::assert_snapshot!(render_sized(&us_open(), 70, 12).backend());
    }

    #[test]
    fn draws_country_expanded_with_cities_and_the_connected_city() {
        let s = connected_city_state();
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_search_by_city() {
        insta::assert_snapshot!(render(&searching_in(tree(), "new y")).backend());
    }

    #[test]
    fn draws_the_connected_city_in_header_and_card() {
        let s = connected_city_state();
        insta::assert_snapshot!(render_sized(&s, 70, 12).backend());
        let (s, _) = key(s, KeyCode::Enter);
        insta::assert_snapshot!(render_sized(&s, 70, 12).backend());
    }

    fn srv(host: &str, load: u8) -> Server {
        Server {
            hostname: host.into(),
            station: "203.0.113.1".parse().unwrap(),
            load,
            public_key: wglane_core::PublicKey(String::new()),
            country: cc("DE"),
            city: None,
        }
    }

    fn result(epoch: u64, host: &str, ms: Option<u64>) -> Event {
        Event::ProbeResult {
            epoch,
            key: host.into(),
            latency_ms: ms,
        }
    }

    #[test]
    fn start_probe_opens_a_round_and_asks_the_worker() {
        let servers = vec![srv("a", 1), srv("b", 2)];
        let (s, fx) = update(State::default(), Event::StartProbe(servers.clone()));
        assert_eq!(s.probe_epoch, 1);
        assert_eq!(s.probes.len(), 2);
        assert!(s.probes.values().all(|p| *p == Probe::Pending));
        assert_eq!(fx, [Effect::Probe { epoch: 1, servers }]);
    }

    #[test]
    fn results_stream_into_state() {
        let (s, _) = update(
            State::default(),
            Event::StartProbe(vec![srv("a", 1), srv("b", 2)]),
        );
        let (s, fx) = update(s, result(1, "b", Some(42)));
        assert_eq!(fx, Vec::new());
        let (s, _) = update(s, result(1, "a", None));
        assert_eq!(s.probes["b"], Probe::Latency(42));
        assert_eq!(s.probes["a"], Probe::Failed);
    }

    #[test]
    fn results_from_an_older_round_are_dropped() {
        // Round 1 for one country, then the user moves on: round 2.
        let (s, _) = update(State::default(), Event::StartProbe(vec![srv("de1", 1)]));
        let (s, _) = update(s, Event::StartProbe(vec![srv("us1", 1), srv("us2", 2)]));
        assert_eq!(s.probe_epoch, 2);
        assert!(
            !s.probes.contains_key("de1"),
            "the old round's servers are gone"
        );
        // A late answer from round 1 changes nothing, even for a key shared by both rounds.
        let before = s.clone();
        let (s, _) = update(s, result(1, "de1", Some(5)));
        let (s, _) = update(s, result(1, "us1", Some(5)));
        assert_eq!(s, before);
        let (s, _) = update(s, result(2, "us1", Some(70)));
        assert_eq!(s.probes["us1"], Probe::Latency(70));
        // An answer for a server that is not in this round is ignored too.
        let (s, _) = update(s, result(2, "zz9", Some(1)));
        assert!(!s.probes.contains_key("zz9"));
    }

    #[test]
    fn an_empty_round_ends_the_last_one_without_work() {
        let (s, _) = update(State::default(), Event::StartProbe(vec![srv("a", 1)]));
        let (s, fx) = update(s, Event::StartProbe(vec![]));
        assert_eq!((s.probe_epoch, s.probes.len(), fx), (2, 0, Vec::new()));
        assert_eq!(update(s.clone(), result(1, "a", Some(1))).0, s);
    }

    #[test]
    fn ticks_do_not_pile_up_refreshes() {
        let (s, fx) = update(State::default(), Event::Tick);
        assert_eq!(fx, [Effect::Refresh]);
        // The first refresh has not answered yet: no second one.
        let (s, fx) = update(s, Event::Tick);
        assert_eq!(fx, Vec::new());
        let (s, _) = update(s, Event::Status(status(&[], reachable(1)), true, None));
        assert_eq!(update(s, Event::Tick).1, [Effect::Refresh]);
    }

    /// Connecting no longer blocks input: move, search and toggle all work
    /// while the connect is in flight, and a second connect is still refused.
    fn connecting_and_poking() -> State {
        let (s, fx) = key(key(tree(), KeyCode::Char('G')).0, KeyCode::Enter);
        assert_eq!(fx, [Effect::Connect(cc("US"), None)]);
        assert!(s.busy.is_some());
        let s = keys(s, &[KeyCode::Char('k'), KeyCode::Char('k')]);
        let (s, fx) = key(s, KeyCode::Enter);
        assert_eq!(fx, Vec::new(), "no second change while one is running");
        searching_in(s, "fra")
    }

    #[test]
    fn keys_still_work_while_connecting() {
        let s = connecting_and_poking();
        assert_eq!(s.query, "fra");
        assert!(s.searching);
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"), None)));
        // The connect finishes behind the user's back; the screen follows.
        let (s, fx) = update(s, Event::Finished(Ok(())));
        assert_eq!(
            (s.busy.clone(), s.message.as_str()),
            (None, "connected to US")
        );
        assert_eq!(fx, [Effect::Refresh]);
        assert_eq!(s.query, "fra", "the search survived");
    }

    #[test]
    fn quitting_during_a_connect_keeps_the_change_pending_for_the_shell() {
        // The reducer only records the intent. The shell sees `busy` still set,
        // lets that one change finish, then exits (see `worker::shutdown`).
        let (s, _) = key(key(tree(), KeyCode::Char('G')).0, KeyCode::Enter);
        let (s, fx) = key(s, KeyCode::Char('q'));
        assert!(s.should_quit);
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"), None)));
        assert_eq!(fx, Vec::new(), "quit starts no new work");
    }

    #[test]
    fn draws_connecting_but_responsive() {
        insta::assert_snapshot!(render_sized(&connecting_and_poking(), 70, 10).backend());
    }
}
