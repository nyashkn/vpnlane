//! The jump palette (`space` or `:`): one fuzzy list over places (countries
//! and cities), presets and actions. Cities are only reachable here and in
//! the browse tree's country detail. Pure: state, ranking, keys, drawing.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use wglane_core::fold;

use super::actions::{self, ACTIONS, Action};
use super::app::{Effect, State};
use super::continent::continent;
use super::places::{self, PlaceKey, Scale};
use super::presets::{self, Slot};

/// Places listed for one query, at most.
const MAX_PLACES: usize = 40;
/// Places sampled (load + latency) per query: the ones on screen first.
const SAMPLE_TOP: usize = 8;

/// Which kinds of item are listed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    All,
    Places,
    Presets,
    Actions,
}

impl Kind {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Places,
            Self::Places => Self::Presets,
            Self::Presets => Self::Actions,
            Self::Actions => Self::All,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::All => " jump ",
            Self::Places => " jump · places ",
            Self::Presets => " jump · presets ",
            Self::Actions => " jump · actions ",
        }
    }
}

/// Palette state; open while `State::palette` is `Some`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Palette {
    /// As typed; a leading `@` means presets only, `>` actions only.
    pub(crate) query: String,
    /// Chosen with `tab`; a prefix in the query wins.
    pub(crate) kind: Kind,
    /// Index into [`items`].
    pub(crate) cursor: usize,
}

impl Palette {
    pub(crate) fn with_query(q: &str) -> Self {
        Self {
            query: q.to_owned(),
            ..Self::default()
        }
    }

    /// The kind in force and the folded text to match.
    fn parse(&self) -> (Kind, String) {
        let (kind, text) = match self.query.chars().next() {
            Some('@') => (Kind::Presets, &self.query[1..]),
            Some('>') => (Kind::Actions, &self.query[1..]),
            _ => (self.kind, self.query.as_str()),
        };
        (kind, fold(text.trim()))
    }
}

/// One palette row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Place(PlaceKey),
    Preset(Slot),
    Action(Action),
}

/// How well `q` (folded, non-empty) matches `text`; lower is better.
/// Exact, then prefix, then word prefix, then substring, then letters in order.
pub(crate) fn score(q: &str, text: &str) -> Option<(u8, usize)> {
    let t = fold(text);
    if t == q {
        return Some((0, 0));
    }
    if t.starts_with(q) {
        return Some((1, t.len() - q.len()));
    }
    if t.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(q))
    {
        return Some((2, t.len()));
    }
    if let Some(i) = t.find(q) {
        return Some((3, i));
    }
    // Subsequence from the start of a word, every query char in order;
    // fewer skipped chars is better.
    let first = q.chars().next()?;
    let start = t
        .char_indices()
        .find(|&(i, c)| c == first && t[..i].chars().last().is_none_or(|p| !p.is_alphanumeric()))?
        .0;
    let mut gaps = 0;
    let mut it = t[start..].chars();
    for qc in q.chars() {
        loop {
            match it.next() {
                Some(c) if c == qc => break,
                Some(_) => gaps += 1,
                None => return None,
            }
        }
    }
    // Scattered letters are noise, not a match.
    (gaps <= q.chars().count()).then_some((4, gaps))
}

fn best(q: &str, texts: &[&str]) -> Option<(u8, usize)> {
    texts.iter().filter_map(|t| score(q, t)).min()
}

/// Place hits for `q`: countries by code or name, cities by name (alone or
/// with their country). Match quality first; within one quality, measured
/// latency (so the list re-sorts as probes land), then load, then name.
fn place_hits(s: &State, q: &str) -> Vec<PlaceKey> {
    let mut hits: Vec<((u8, usize), String, PlaceKey)> = Vec::new();
    for c in &s.countries {
        if let Some(sc) = best(q, &[c.code.as_str(), &c.name]) {
            hits.push((sc, c.name.clone(), PlaceKey::country(c.code)));
        }
        {
            for city in &c.cities {
                let with = format!("{} {}", city.name, c.name);
                if let Some(sc) = best(q, &[&city.name, &with]) {
                    hits.push((sc, city.name.clone(), PlaceKey::city(c.code, &city.name)));
                }
            }
        }
    }
    hits.sort_by(|a, b| (a.0.0, &a.1).cmp(&(b.0.0, &b.1)));
    let mut out = Vec::new();
    let mut i = 0;
    while i < hits.len() && out.len() < MAX_PLACES {
        let tier = hits[i].0.0;
        let j = hits[i..]
            .iter()
            .position(|h| h.0.0 != tier)
            .map_or(hits.len(), |n| i + n);
        out.extend(places::rank(
            s,
            hits[i..j].iter().map(|h| h.2.clone()).collect(),
        ));
        i = j;
    }
    out.truncate(MAX_PLACES);
    out
}

/// The rows for the current query. During a leak the first row is pinned to
/// "disconnect".
pub(crate) fn items(s: &State) -> Vec<Item> {
    let Some(p) = &s.palette else {
        return Vec::new();
    };
    let (kind, q) = p.parse();
    let want = |k: Kind| kind == Kind::All || kind == k;
    let mut out = Vec::new();
    let pinned = s.leaking();
    if pinned {
        out.push(Item::Action(Action::Disconnect));
    }
    if q.is_empty() && want(Kind::Presets) {
        out.extend(presets::slots(s).into_iter().map(|v| Item::Preset(v.slot)));
    }
    if want(Kind::Places) {
        let places = if q.is_empty() {
            let n = if kind == Kind::Places { 12 } else { 3 };
            places::best_countries(s, n).0
        } else {
            place_hits(s, &q)
        };
        out.extend(places.into_iter().map(Item::Place));
    }
    if !q.is_empty() && want(Kind::Presets) {
        for v in presets::slots(s) {
            if best(&q, &[&v.name, &v.summary]).is_some() {
                out.push(Item::Preset(v.slot));
            }
        }
    }
    if want(Kind::Actions) {
        for (a, label, _, words) in ACTIONS {
            if (pinned && a == Action::Disconnect) || a == Action::Presets && kind == Kind::Presets
            {
                continue;
            }
            if q.is_empty() || best(&q, &[label, words]).is_some() {
                out.push(Item::Action(a));
            }
        }
    }
    out
}

/// Keys while the palette is open: letters type, arrows move.
pub(crate) fn on_key(s: &mut State, k: KeyCode, fx: &mut Vec<Effect>) {
    let Some(p) = s.palette.as_mut() else {
        return;
    };
    let mut edited = false;
    match k {
        KeyCode::Esc => {
            s.palette = None;
            return;
        }
        KeyCode::Char(c) => {
            p.query.push(c);
            edited = true;
        }
        KeyCode::Backspace => {
            p.query.pop();
            edited = true;
        }
        KeyCode::Tab => {
            let (kind, _) = p.parse();
            p.query = p.query.trim_start_matches(['@', '>']).to_owned();
            p.kind = kind.next();
            edited = true;
        }
        KeyCode::Up => p.cursor = p.cursor.saturating_sub(1),
        KeyCode::Down => p.cursor += 1,
        KeyCode::Home => p.cursor = 0,
        KeyCode::End => p.cursor = usize::MAX,
        KeyCode::Enter => {
            let item = items(s)
                .get(s.palette.as_ref().map_or(0, |p| p.cursor))
                .cloned();
            s.palette = None;
            if let Some(item) = item {
                run(s, item, fx);
            }
            return;
        }
        _ => {}
    }
    let list = items(s);
    let leaking = s.leaking();
    if let Some(p) = s.palette.as_mut() {
        if edited {
            // The best match, never the pinned row: typing then enter must not disconnect.
            p.cursor = usize::from(leaking && list.len() > 1 && !p.query.is_empty());
        }
        p.cursor = p.cursor.min(list.len().saturating_sub(1));
    }
    if edited {
        sample_shown(s, &list, fx);
    }
}

/// Asks for load and latency of the top places listed.
pub(crate) fn sample_shown(s: &mut State, list: &[Item], fx: &mut Vec<Effect>) {
    let keys: Vec<PlaceKey> = list
        .iter()
        .filter_map(|i| match i {
            Item::Place(k) => Some(k.clone()),
            _ => None,
        })
        .take(SAMPLE_TOP)
        .collect();
    places::request(s, keys, fx);
}

fn run(s: &mut State, item: Item, fx: &mut Vec<Effect>) {
    match item {
        Item::Place(k) => {
            if s.busy.is_some() {
                s.note("busy: wait for the current change to finish");
            } else {
                s.connect_place(&k, fx);
            }
        }
        Item::Preset(slot) => presets::apply(s, slot, fx),
        Item::Action(a) => actions::run(s, a, fx),
    }
}

fn place_label(s: &State, k: &PlaceKey) -> (String, String) {
    let name = s
        .countries
        .iter()
        .find(|c| c.code == k.cc)
        .map_or_else(|| k.cc.to_string(), |c| c.name.clone());
    match &k.city {
        Some(city) => (city.clone(), format!("{} · {}", k.cc, continent(k.cc))),
        None => (name, format!("{} · {}", k.cc, continent(k.cc))),
    }
}

fn fit(text: &str, w: usize) -> String {
    let n = text.chars().count();
    if n <= w {
        format!("{text:<w$}")
    } else {
        let mut t: String = text.chars().take(w.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// Draws the palette over whatever is on screen (dimmed first).
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "one pass builds headings, rows, preview and scroll window together"
)]
pub(crate) fn draw(s: &State, frame: &mut Frame) {
    let Some(p) = &s.palette else {
        return;
    };
    let full = frame.area();
    frame
        .buffer_mut()
        .set_style(full, Style::new().add_modifier(Modifier::DIM));
    let wide = full.width >= 80;
    let width = if wide { full.width.min(76) } else { full.width };
    let top = if full.height >= 14 { 2 } else { 0 };
    let area = Rect {
        x: full.x + (full.width - width) / 2,
        y: full.y + top,
        width,
        // Down to the footer, which keeps its hints.
        height: full.height.saturating_sub(top + 1),
    };
    let inner_w = usize::from(width.saturating_sub(2));
    let dim = Style::new().add_modifier(Modifier::DIM);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let list = items(s);
    let cursor = p.cursor.min(list.len().saturating_sub(1));
    let (kind, _) = p.parse();

    let mut head = vec![
        Line::from(vec![
            Span::styled(
                " › ",
                s.fg(ratatui::style::Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(p.query.clone(), bold),
            Span::styled("_", bold.add_modifier(Modifier::UNDERLINED)),
        ]),
        Line::default(),
    ];
    if s.countries.is_empty() && kind != Kind::Presets && kind != Kind::Actions {
        head.push(Line::from(Span::styled(
            if s.load_failed {
                "   server list failed to load: r in the list retries"
            } else {
                "   loading the server list..."
            },
            dim,
        )));
    }

    // Body: group headings plus rows; remember which line holds the cursor.
    let shown_places: Vec<PlaceKey> = list
        .iter()
        .filter_map(|i| match i {
            Item::Place(k) => Some(k.clone()),
            _ => None,
        })
        .collect();
    let (lat_bar, load_bar, region) = if wide { (5, 5, true) } else { (4, 0, false) };
    let scale = Scale::new(s, &shown_places, lat_bar, load_bar);
    let mut body: Vec<Line> = Vec::new();
    let mut cursor_line = 0;
    let mut group = None;
    for (i, item) in list.iter().enumerate() {
        let g = match item {
            Item::Action(Action::Disconnect) if i == 0 && s.leaking() => 0,
            Item::Place(_) => 1,
            Item::Preset(_) => 2,
            Item::Action(_) => 3,
        };
        if group != Some(g) {
            group = Some(g);
            let n = list
                .iter()
                .filter(|x| {
                    matches!(
                        (g, x),
                        (1, Item::Place(_)) | (2, Item::Preset(_)) | (3, Item::Action(_))
                    )
                })
                .count();
            let title = match g {
                0 => "leak risk: disconnect first".to_owned(),
                1 if p.parse().1.is_empty() => "fastest right now".to_owned(),
                1 => format!("places · {n}"),
                2 => format!("presets · {n}"),
                _ => format!("actions · {n}"),
            };
            if !body.is_empty() {
                body.push(Line::default());
            }
            let mut spans = vec![Span::styled(format!("   {title}"), dim)];
            if g == 1 {
                let used = 3 + title.chars().count();
                let cols = scale.width();
                let pad = inner_w.saturating_sub(used + cols);
                let lat = format!(
                    "{:<w$}",
                    "LATENCY",
                    w = 7 + if lat_bar > 0 { lat_bar + 1 } else { 0 } + 2
                );
                spans.push(Span::styled(format!("{}{lat}LOAD", " ".repeat(pad)), dim));
            }
            body.push(Line::from(spans));
        }
        if i == cursor {
            cursor_line = body.len();
        }
        let mark = if i == cursor { " > " } else { "   " };
        let mut spans = vec![Span::raw(mark)];
        match item {
            Item::Place(k) => {
                let m = places::metrics(s, k, &scale);
                let mw: usize = m.iter().map(Span::width).sum();
                let (name, reg) = place_label(s, k);
                let reg = if region { reg } else { k.cc.to_string() };
                let room = inner_w.saturating_sub(3 + mw + 1);
                let nw = 16.min(room);
                spans.push(Span::raw(fit(&name, nw)));
                spans.push(Span::styled(
                    format!(" {}", fit(&reg, room.saturating_sub(nw + 1))),
                    dim,
                ));
                spans.extend(m);
            }
            Item::Preset(slot) => {
                let v = presets::slots(s).into_iter().find(|v| v.slot == *slot);
                let v = v.unwrap_or(presets::View {
                    slot: *slot,
                    key: None,
                    name: String::new(),
                    summary: String::new(),
                    active: false,
                });
                let tail = format!(
                    "{}  {}",
                    if v.active { "● active" } else { "        " },
                    v.key.unwrap_or(' ')
                );
                let room = inner_w.saturating_sub(3 + tail.chars().count() + 2);
                spans.push(Span::raw(fit(
                    &format!("{:<8} {}", v.name, v.summary),
                    room,
                )));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    tail,
                    if v.active {
                        s.fg(ratatui::style::Color::Green)
                    } else {
                        Style::new()
                    },
                ));
            }
            Item::Action(a) => {
                let (label, key, _) = a.entry();
                let room = inner_w.saturating_sub(3 + 3);
                spans.push(Span::raw(fit(label, room)));
                spans.push(Span::styled(
                    format!(" {}", key.unwrap_or(' ')),
                    s.fg(ratatui::style::Color::Cyan),
                ));
            }
        }
        let line = Line::from(spans);
        body.push(if i == cursor {
            line.style(Style::new().add_modifier(Modifier::REVERSED))
        } else {
            line
        });
    }
    if list.is_empty() {
        body.push(Line::from(Span::styled(
            "   no match: places match city, country or code; @ presets, > actions",
            dim,
        )));
    }

    // Preview of the preset under the cursor.
    let mut tail: Vec<Line> = Vec::new();
    if let Some(Item::Preset(slot)) = list.get(cursor) {
        let name = presets::slots(s)
            .into_iter()
            .find(|v| v.slot == *slot)
            .map_or_else(String::new, |v| v.name);
        tail.push(Line::default());
        tail.push(Line::from(Span::styled(
            format!("   {name} will change"),
            bold,
        )));
        for l in presets::preview(s, *slot) {
            tail.push(Line::from(Span::styled(format!("   {l}"), dim)));
        }
    }
    let hint = match list.get(cursor) {
        Some(Item::Preset(_)) => "enter apply (reconnects) · ↑↓ move · tab kind · esc",
        _ if wide => "enter go · ↑↓ move · tab kind · @ presets · > actions · esc",
        _ => "enter go · tab kind · esc",
    };
    let foot = Line::from(Span::styled(format!(" {hint}"), dim));

    let inner_h = usize::from(area.height.saturating_sub(2));
    let fixed = head.len() + tail.len() + 2;
    let room = inner_h.saturating_sub(fixed).max(1);
    let offset = (cursor_line + 1).saturating_sub(room);
    let mut lines = head;
    lines.extend(body.into_iter().skip(offset).take(room));
    let pad = inner_h.saturating_sub(lines.len() + tail.len() + 1);
    lines.extend(tail);
    lines.extend(std::iter::repeat_n(Line::default(), pad));
    lines.push(foot);
    let height = u16::try_from(lines.len() + 2)
        .unwrap_or(area.height)
        .min(area.height);
    let area = Rect { height, ..area };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(Span::styled(kind.title(), bold))),
        area.intersection(full),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_orders_exact_prefix_word_substring_fuzzy() {
        assert_eq!(score("de", "DE"), Some((0, 0)));
        assert_eq!(score("san", "San Jose").map(|s| s.0), Some(1));
        assert_eq!(score("york", "New York").map(|s| s.0), Some(2));
        assert_eq!(score("san", "Lausanne").map(|s| s.0), Some(3));
        assert_eq!(score("frk", "Frankfurt").map(|s| s.0), Some(4));
        assert_eq!(score("zz", "Frankfurt"), None);
        assert_eq!(
            score("san", "Astana"),
            None,
            "scattered, not from a word start"
        );
        assert_eq!(score("san", "Botswana"), None);
        assert_eq!(score("zur", "Zürich").map(|s| s.0), Some(1), "accents fold");
    }
}
