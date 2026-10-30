//! The home screen: a status HUD (state, safety, exit, tailnet, bypass, the
//! latency graph), the presets strip, the fastest places right now and the
//! jump box. Label/value rows, so it narrows without a second layout; below
//! 60 columns (or a short terminal) it folds into the tiny view.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Sparkline, SparklineBar};
use wglane_core::{CountryCode, TailnetHealth};

use super::app::{Busy, State, Udp, median};
use super::places::{self, Scale};
use super::presets;

/// Narrowest full home layout; below it, the tiny view.
pub(crate) const NARROW: u16 = 60;
/// Width from which info rows sit in two columns.
const WIDE: u16 = 80;

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// Is the connection safe, as far as `status` can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Safety {
    /// Nothing to say (down, or the checks passed).
    Fine,
    /// `exit_ip_check = false`: nothing can tell.
    Off,
    /// Up, but the exit lookup failed.
    Unchecked,
    /// Up and the exit looks wrong: the reasons.
    Leak(Vec<String>),
}

/// The safety verdict from the last status.
pub(crate) fn safety(s: &State) -> Safety {
    if !s.exit_ip_check {
        return Safety::Off;
    }
    let (Some(st), Some(cc)) = (&s.status, s.connected_cc()) else {
        return Safety::Fine;
    };
    if s.busy.is_some() {
        return Safety::Fine;
    }
    let mut why = Vec::new();
    if let (Some(ip), Some(exit)) = (st.exit_ip, st.exit_country)
        && exit != cc
    {
        why.push(format!("exit IP {ip} is in {exit}, not {cc}"));
    }
    if st.protected == Some(false) {
        why.push("Nord says not protected".to_owned());
    }
    if !why.is_empty() {
        Safety::Leak(why)
    } else if st.exit_ip.is_none() {
        Safety::Unchecked
    } else {
        Safety::Fine
    }
}

/// `Germany · Frankfurt` for a code (and the city when known).
fn place_name(s: &State, cc: CountryCode, city: Option<&str>) -> String {
    let name = s
        .countries
        .iter()
        .find(|c| c.code == cc)
        .map_or_else(|| cc.to_string(), |c| c.name.clone());
    match city {
        Some(c) => format!("{name} · {c}"),
        None => name,
    }
}

/// The city of the live tunnel: what `up` recorded, else the last request's.
fn live_city(s: &State, cc: CountryCode) -> Option<String> {
    s.city.clone().or_else(|| {
        s.last
            .as_ref()
            .and_then(|l| l.location.as_ref())
            .filter(|l| l.country() == cc)
            .and_then(|l| l.city().map(str::to_owned))
    })
}

fn uptime(secs: u64) -> String {
    match secs {
        0..60 => format!("up {secs}s"),
        60..3600 => format!("up {}m", secs / 60),
        _ => format!("up {}h {}m", secs / 3600, secs % 3600 / 60),
    }
}

/// The state word with its symbol (it reads the same without colour), and
/// the place it is about.
pub(crate) fn state_spans(s: &State, tiny: bool) -> Vec<Span<'static>> {
    let pad = |w: &str, n: usize| {
        if tiny {
            format!("{w}  ")
        } else {
            format!("{w:<n$}")
        }
    };
    match (&s.busy, s.connected_cc()) {
        (Some(Busy::Connecting(to)), _) => vec![
            Span::styled(
                pad("◐ connecting", 14),
                s.fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(to.clone(), bold()),
        ],
        (Some(Busy::Disconnecting), cc) => vec![
            Span::styled(
                pad("◐ disconnecting", 17),
                s.fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                cc.map_or_else(String::new, |cc| {
                    format!("from {}", place_name(s, cc, None))
                }),
                dim(),
            ),
        ],
        (None, _) if s.status.is_none() => vec![
            Span::raw(pad("· checking", 12)),
            Span::styled("asking the helper which tunnel is up", dim()),
        ],
        (None, _) if !s.tunnel_known => vec![
            Span::styled(
                pad("? unknown", 12),
                s.fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw("the helper did not answer"),
        ],
        (None, Some(cc)) => {
            let city = live_city(s, cc);
            let place = if tiny {
                format!("{cc}{}", city.map_or_else(String::new, |c| format!(" {c}")))
            } else {
                place_name(s, cc, city.as_deref())
            };
            vec![
                Span::styled(
                    pad("● up", 10),
                    s.fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::styled(place, bold()),
            ]
        }
        (None, None) => vec![
            Span::styled(pad("○ down", 10), bold()),
            Span::raw("not connected"),
        ],
    }
}

/// The dim line under the state.
fn sub_line(s: &State) -> Line<'static> {
    let text = match (&s.busy, s.connected_cc()) {
        (Some(_), _) => "keys that change the tunnel wait; space, b and ? still work".to_owned(),
        (None, _) if s.status.is_some() && !s.tunnel_known => {
            "r retries · run `wglane doctor` in a shell to see why".to_owned()
        }
        (None, Some(cc)) => {
            let mut parts = Vec::new();
            match s
                .last
                .as_ref()
                .filter(|l| l.location.as_ref().is_some_and(|p| p.country() == cc))
            {
                Some(l) => {
                    parts.extend(l.location.as_ref().map(ToString::to_string));
                    parts.push(l.pick.to_string());
                    if s.connected && l.at > 0 && s.now >= l.at {
                        parts.push(uptime(s.now - l.at));
                    }
                }
                None => parts.push(cc.to_string()),
            }
            parts.join(" · ")
        }
        (None, None) => s
            .last
            .as_ref()
            .and_then(|l| l.location.as_ref())
            .map_or_else(String::new, |l| {
                format!("last: {l}{}", if s.failed { ", failed" } else { "" })
            }),
    };
    Line::from(Span::styled(text, dim()))
}

/// `left` with `right` flush right in `width`.
fn spread(mut left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let lw: usize = left.iter().map(Span::width).sum();
    let rw: usize = right.iter().map(Span::width).sum();
    if lw + rw + 2 <= width {
        left.push(Span::raw(" ".repeat(width - lw - rw)));
        left.extend(right);
    }
    Line::from(left)
}

fn preset_tag(s: &State) -> Vec<Span<'static>> {
    match &s.active_preset {
        Some(p) if s.is_up() => vec![
            Span::styled("preset ", dim()),
            Span::styled(p.describe(), bold()),
        ],
        _ => Vec::new(),
    }
}

/// The safety block: three reverse-red lines for a leak, one loud line when
/// the check is off, nothing otherwise.
fn safety_lines(s: &State, width: usize) -> Vec<Line<'static>> {
    let block = |text: String, color: Color| {
        let style = s
            .fg(color)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD);
        Line::from(Span::styled(format!("{text:<width$}"), style))
    };
    match safety(s) {
        Safety::Leak(why) => vec![
            block(format!(" ▲ LEAK RISK  {}", why.join(" · ")), Color::Red),
            block(
                "   traffic may be leaving outside the tunnel, or the exit lookup is stale".into(),
                Color::Red,
            ),
            block(
                "   d disconnect   r recheck now   (rechecks every 2 s)".into(),
                Color::Red,
            ),
        ],
        Safety::Off => vec![block(
            " ▲ leak check disabled  exit_ip_check = false: nothing tells you if traffic leaks"
                .into(),
            Color::Yellow,
        )],
        Safety::Fine | Safety::Unchecked => Vec::new(),
    }
}

fn label(text: &str) -> Span<'static> {
    Span::styled(format!("{text:<10}"), dim())
}

fn exit_spans(s: &State) -> Vec<Span<'static>> {
    let Some(st) = &s.status else {
        return vec![Span::styled("-", dim())];
    };
    if !s.exit_ip_check {
        return vec![Span::styled(
            "not checked (exit_ip_check = false)",
            s.fg(Color::Yellow),
        )];
    }
    let Some(ip) = st.exit_ip else {
        return vec![Span::styled("unknown (lookup failed)", dim())];
    };
    if !s.tunnel_known {
        return vec![Span::raw(ip.to_string())];
    }
    if !s.is_up() {
        return vec![Span::raw(format!("{ip} · not tunnelled"))];
    }
    let mut out = vec![Span::raw(format!(
        "{ip}{}",
        st.exit_country
            .map_or_else(String::new, |c| format!(" · {c}"))
    ))];
    out.push(Span::raw(" · "));
    out.push(match st.protected {
        Some(true) => Span::styled("protected", s.fg(Color::Green)),
        Some(false) => Span::styled(
            "not protected",
            s.fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        None => Span::styled("protection unknown", dim()),
    });
    out
}

fn tailnet_spans(s: &State) -> Vec<Span<'static>> {
    match s.status.as_ref().map(|st| &st.tailnet) {
        Some(TailnetHealth::Reachable { peer, latency_ms }) => vec![
            Span::styled("ok", s.fg(Color::Green)),
            Span::raw(format!(" · {peer} {latency_ms} ms")),
        ],
        Some(TailnetHealth::Unreachable { reason }) => {
            vec![Span::styled(
                format!("unreachable ({reason})"),
                s.fg(Color::Yellow),
            )]
        }
        Some(TailnetHealth::NotInstalled) => vec![Span::raw("tailscale not installed")],
        _ => vec![Span::styled("-", dim())],
    }
}

fn bypass_spans(s: &State) -> Vec<Span<'static>> {
    match &s.status {
        Some(st) if s.is_up() => vec![Span::raw(format!(
            "{} route{}",
            st.bypass_routes,
            if st.bypass_routes == 1 { "" } else { "s" }
        ))],
        Some(_) => vec![Span::styled("none (no tunnel)", dim())],
        None => vec![Span::styled("-", dim())],
    }
}

fn udp_span(s: &State) -> Span<'static> {
    match (s.udp_checking, s.udp) {
        (true, _) => Span::styled("udp checking...", dim()),
        (_, Some(Udp::Blocked)) => Span::styled(
            "udp 51820 blocked",
            s.fg(Color::Yellow)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD),
        ),
        (_, Some(Udp::Sent)) => Span::raw("udp 51820 sent"),
        (_, Some(Udp::NoEndpoint) | None) => Span::styled("udp not checked", dim()),
    }
}

fn checks_spans(s: &State) -> Vec<Span<'static>> {
    let leak = match safety(s) {
        Safety::Leak(_) => Span::styled("leak RISK", s.fg(Color::Red).add_modifier(Modifier::BOLD)),
        Safety::Off => Span::styled(
            "leak check off",
            s.fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Safety::Unchecked => Span::styled("leak ?", dim()),
        Safety::Fine if s.is_up() => Span::styled("leak ok", s.fg(Color::Green)),
        Safety::Fine => Span::styled("leak n/a (not up)", dim()),
    };
    vec![leak, Span::raw(" · "), udp_span(s)]
}

/// Info rows: two columns from [`WIDE`], one below.
fn info_lines(s: &State, width: u16) -> Vec<Line<'static>> {
    let row = |l: &str, v: Vec<Span<'static>>| {
        let mut spans = vec![label(l)];
        spans.extend(v);
        spans
    };
    let pairs = [
        (row("exit", exit_spans(s)), row("tailnet", tailnet_spans(s))),
        (
            row("bypass", bypass_spans(s)),
            row("checks", checks_spans(s)),
        ),
    ];
    if width >= WIDE {
        let half = usize::from(width) / 2;
        pairs
            .into_iter()
            .map(|(mut a, b)| {
                let w: usize = a.iter().map(Span::width).sum();
                a.push(Span::raw(" ".repeat(half.saturating_sub(w).max(2))));
                a.extend(b);
                Line::from(a)
            })
            .collect()
    } else {
        pairs
            .into_iter()
            .flat_map(|(a, b)| [Line::from(a), Line::from(b)])
            .collect()
    }
}

/// After a failed change: the error, the UDP verdict, the next step.
fn error_lines(s: &State) -> Vec<Line<'static>> {
    if !s.failed || s.message.is_empty() {
        return Vec::new();
    }
    let doctor = match (s.udp_checking, s.udp) {
        (true, _) => vec![Span::styled("checking udp 51820...", dim())],
        (_, Some(Udp::Blocked)) => vec![Span::styled(
            "udp 51820 looks blocked on this network",
            s.fg(Color::Yellow)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )],
        (_, Some(Udp::Sent)) => vec![Span::raw(
            "udp 51820 leaves this machine; delivery cannot be tested",
        )],
        (_, Some(Udp::NoEndpoint) | None) => {
            vec![Span::styled(
                "udp 51820 not checked (no generated config yet)",
                dim(),
            )]
        }
    };
    let next = if s.udp == Some(Udp::Blocked) {
        "switch network (a phone hotspot often works) · enter retries · space jump"
    } else {
        "enter retries · space jump · wglane doctor in a shell for the full check"
    };
    let mut err = vec![label("error")];
    err.push(Span::styled(
        s.message.clone(),
        s.fg(Color::Red).add_modifier(Modifier::BOLD),
    ));
    let mut doc = vec![label("doctor")];
    doc.extend(doctor);
    vec![
        Line::from(err),
        Line::from(doc),
        Line::from(vec![label("next"), Span::raw(next)]),
    ]
}

/// `latency   38 ms  min 35  max 61   to tailnet peer, last 92`.
fn latency_line(shown: &[u64], width: usize) -> Line<'static> {
    let (Some(now), Some(lo), Some(hi)) = (shown.last(), shown.iter().min(), shown.iter().max())
    else {
        return Line::from(vec![
            label("latency"),
            Span::styled("- (no tailnet peer answered yet)", dim()),
        ]);
    };
    let base = vec![
        label("latency"),
        Span::raw(format!("{now} ms  min {lo}  max {hi}")),
    ];
    let mut full = base.clone();
    full.push(Span::styled(
        format!("   to tailnet peer, last {}", shown.len()),
        dim(),
    ));
    if Line::from(full.clone()).width() <= width {
        Line::from(full)
    } else {
        Line::from(base)
    }
}

/// The tailnet latency graph, scaled to the visible window (min..max) so
/// steady and wobbling links both show their shape. Spikes in yellow.
fn draw_graph(s: &State, frame: &mut Frame, area: Rect, shown: &[u64]) {
    let levels = 8 * u64::from(area.height);
    let lo = shown.iter().copied().min().unwrap_or(0);
    let span = shown.iter().copied().max().unwrap_or(0) - lo;
    let limit = s.ui.spike_limit(median(shown));
    let bars: Vec<SparklineBar> = shown
        .iter()
        .map(|&v| {
            let bar = SparklineBar::from(2 + (v - lo) * (levels - 2) / span.max(1));
            if v > limit {
                bar.style(s.fg(Color::Yellow))
            } else {
                bar
            }
        })
        .collect();
    frame.render_widget(Sparkline::default().data(bars).max(levels), area);
}

/// `fastest right now` and its rows (country level), honest when nothing
/// answered.
fn fastest_lines(s: &State, width: u16) -> Vec<Line<'static>> {
    let (best, measured) = places::best_countries(s, 3);
    let any = best.iter().any(|k| places::latency(s, k).is_some());
    let mut out = Vec::new();
    let title = if !any && places::all_failed(s) {
        "nearby, by load"
    } else {
        "fastest right now"
    };
    let mut head = vec![Span::styled(title, bold())];
    if measured > 0 {
        head.push(Span::styled(format!("  of {measured} measured"), dim()));
    }
    out.push(Line::from(head));
    if measured == 0 {
        let why = if s.countries.is_empty() {
            "waiting for the server list"
        } else if s.samples.is_empty() {
            "nothing measured yet: space and type a place to measure it"
        } else if s.samples.values().any(|v| *v == places::Sample::Requested) {
            "probing..."
        } else {
            "nothing measured: the server lookups failed (r retries)"
        };
        out.push(Line::from(Span::styled(why, dim())));
        return out;
    }
    let wide = width >= WIDE;
    let scale = Scale::new(s, &best, if wide { 4 } else { 3 }, if wide { 4 } else { 0 });
    for k in &best {
        let name = s
            .countries
            .iter()
            .find(|c| c.code == k.cc)
            .map_or_else(String::new, |c| c.name.clone());
        let mut spans = vec![Span::raw(format!(
            "{:<4}{:<16}",
            k.cc.as_str(),
            name.chars().take(15).collect::<String>()
        ))];
        spans.extend(places::metrics(s, k, &scale));
        out.push(Line::from(spans));
    }
    if places::all_failed(s) {
        out.push(Line::from(Span::styled(
            places::unavailable_hint(s),
            s.fg(Color::Yellow),
        )));
    }
    out
}

fn jump_box(s: &State, frame: &mut Frame, area: Rect, narrow: bool) {
    let text = if narrow {
        "jump to a place, preset or action"
    } else {
        "jump to a city, country, preset or action"
    };
    let keys = if narrow { "space" } else { "space  or  :" };
    let inner = usize::from(area.width.saturating_sub(4));
    let line = spread(
        vec![
            Span::styled("› ", s.fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(text, dim()),
        ],
        vec![Span::styled(keys, dim())],
        inner,
    );
    let mut spans = vec![Span::raw(" ")];
    spans.extend(line.spans);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::bordered()),
        area,
    );
}

/// The context footer: an error or a leak first, then the keys that matter now.
pub(crate) fn footer(s: &State, width: u16) -> Line<'static> {
    let mut keys: Vec<&str> = if s.busy.is_some() {
        vec!["space jump", "b browse", "? keys", "q quit"]
    } else {
        vec![
            "space jump",
            "0-9 preset",
            "p preview",
            "f fastest",
            "b browse",
            "d disconnect",
            "? keys",
            "q quit",
        ]
    };
    let mut first: Vec<Span<'static>> = Vec::new();
    if let Safety::Leak(_) = safety(s) {
        first.push(Span::styled(
            "leak check failed",
            s.fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        keys = vec!["d disconnect", "r recheck", "space jump", "? keys"];
    } else if s.failed && !s.message.is_empty() {
        first.push(Span::styled(
            format!("error: {}", s.message),
            s.fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        keys = vec!["enter retry", "space jump", "? keys", "q quit"];
    } else if s.busy.is_some() {
        first.push(Span::styled(
            "busy: connect, presets and disconnect wait",
            dim(),
        ));
    } else if !s.message.is_empty() {
        first.push(Span::raw(s.message.clone()));
    }
    // Drop keys from the right until it fits; "? keys" stays last.
    let w = usize::from(width);
    loop {
        let mut spans = first.clone();
        let hint = keys.join("  ");
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(hint, dim()));
        let line = Line::from(spans);
        if line.width() <= w || keys.len() <= 1 {
            return line;
        }
        let drop = keys.len().saturating_sub(2);
        if keys[drop] == "? keys" {
            keys.remove(keys.len() - 1);
        } else {
            keys.remove(drop);
        }
    }
}

/// Section of the home screen, for the fit-by-priority layout.
enum Part {
    Gap,
    State,
    Sub,
    Strip,
    Lines(Vec<Line<'static>>),
    Latency,
    Graph(u16),
    Jump,
}

/// Draws home into the whole frame.
#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "one layout pass over the sections; splitting it scatters the priorities"
)]
pub(crate) fn draw(s: &State, frame: &mut Frame) {
    let area = frame.area();
    if area.width < NARROW || area.height < 14 {
        return draw_tiny(s, frame);
    }
    let w = area.width;
    let narrow = w < WIDE;
    let margin = if narrow { 0 } else { 2 };
    let inner = Rect {
        x: area.x + margin,
        width: area.width - 2 * margin,
        ..area
    };
    let shown_lat = &s.latency[s.latency.len().saturating_sub(usize::from(inner.width))..];
    // (part, priority: lower survives a short terminal first)
    let mut parts: Vec<(Part, u8)> = vec![
        (Part::Gap, 9),
        (Part::State, 0),
        (Part::Sub, 3),
        (Part::Strip, 1),
        (Part::Gap, 8),
    ];
    let warn = safety_lines(s, usize::from(inner.width));
    if !warn.is_empty() {
        parts.push((Part::Lines(warn), 0));
        parts.push((Part::Gap, 8));
    }
    let err = error_lines(s);
    if !err.is_empty() {
        parts.push((Part::Lines(err), 1));
        parts.push((Part::Gap, 8));
    }
    parts.push((Part::Lines(info_lines(s, inner.width)), 2));
    parts.push((Part::Gap, 7));
    parts.push((Part::Latency, 4));
    parts.push((Part::Graph(if narrow { 2 } else { 3 }), 6));
    parts.push((Part::Gap, 7));
    parts.push((Part::Lines(fastest_lines(s, inner.width)), 5));
    parts.push((Part::Jump, 4));
    let height = |p: &Part| match p {
        Part::Lines(l) => u16::try_from(l.len()).unwrap_or(u16::MAX),
        Part::Graph(n) => *n,
        Part::Jump => 3,
        _ => 1,
    };
    // Keep parts by priority until the footer row is all that is left.
    let budget = area.height - 1;
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|&i| parts[i].1);
    let mut keep = vec![false; parts.len()];
    let mut used = 0;
    for i in order {
        let h = height(&parts[i].0);
        if used + h <= budget {
            used += h;
            keep[i] = true;
        }
    }
    let mut y = area.y;
    let right = preset_tag(s);
    for (i, (part, _)) in parts.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        let h = height(part);
        // The jump box sits just above the footer.
        if matches!(part, Part::Jump) {
            y = y.max(area.y + budget - 3);
        }
        let r = Rect {
            y,
            height: h,
            ..inner
        };
        match part {
            Part::Gap => {}
            Part::State => frame.render_widget(
                Paragraph::new(spread(
                    state_spans(s, false),
                    right.clone(),
                    usize::from(inner.width),
                )),
                r,
            ),
            Part::Sub => frame.render_widget(
                Paragraph::new(Line::from(
                    [Span::raw(if narrow { "" } else { "          " })]
                        .into_iter()
                        .chain(sub_line(s).spans)
                        .collect::<Vec<_>>(),
                )),
                r,
            ),
            Part::Strip => {
                let line = presets::strip(s, usize::from(inner.width).saturating_sub(12));
                frame.render_widget(
                    Paragraph::new(spread(
                        line.spans,
                        vec![Span::styled("p preview", dim())],
                        usize::from(inner.width),
                    )),
                    r,
                );
            }
            Part::Lines(l) => frame.render_widget(Paragraph::new(l.clone()), r),
            Part::Latency => frame.render_widget(
                Paragraph::new(latency_line(shown_lat, usize::from(inner.width))),
                r,
            ),
            Part::Graph(_) => draw_graph(s, frame, r, shown_lat),
            Part::Jump => jump_box(s, frame, r, narrow),
        }
        y += h;
    }
    let bottom = Rect {
        y: area.y + area.height - 1,
        height: 1,
        ..area
    };
    frame.render_widget(Paragraph::new(footer(s, area.width)), bottom);
}

/// Below 60 columns or 14 rows: state, safety, preset, the jump affordance.
fn draw_tiny(s: &State, frame: &mut Frame) {
    let area = frame.area();
    let w = usize::from(area.width);
    let mut state = state_spans(s, true);
    if let Some(ms) = s.latency.last().filter(|_| s.is_up()) {
        state.push(Span::raw(format!("  {ms} ms")));
    }
    let second = match safety(s) {
        Safety::Leak(_) => Line::from(Span::styled(
            format!("{:<w$}", "▲ LEAK RISK  d disconnect"),
            s.fg(Color::Red)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )),
        Safety::Off => Line::from(Span::styled(
            format!("{:<w$}", "▲ leak check disabled"),
            s.fg(Color::Yellow)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD),
        )),
        _ if s.failed && !s.message.is_empty() => Line::from(Span::styled(
            format!("error: {}", s.message),
            s.fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        _ => {
            let mut spans = vec![Span::raw("exit ")];
            spans.extend(exit_spans(s));
            Line::from(spans)
        }
    };
    let mut third = Vec::new();
    if let Some(p) = s.active_preset.as_ref().filter(|_| s.is_up()) {
        third.push(Span::styled("preset ", dim()));
        third.push(Span::raw(format!("{} · ", p.name)));
    }
    third.push(Span::raw("tailnet "));
    third.extend(tailnet_spans(s));
    let jump = Line::from(vec![
        Span::styled("› ", s.fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled("space to jump", dim()),
    ]);
    let mut lines = vec![
        Line::from(state),
        second,
        Line::from(third),
        Line::default(),
        jump,
    ];
    let room = usize::from(area.height).saturating_sub(1);
    if lines.len() > room {
        lines.remove(3);
    }
    lines.truncate(room);
    frame.render_widget(Paragraph::new(lines), area);
    let keys = if s.failed && !s.message.is_empty() {
        "enter retry  space jump  q quit"
    } else {
        "space jump  0-9 preset  q quit"
    };
    let bottom = Rect {
        y: area.y + area.height - 1,
        height: 1,
        ..area
    };
    frame.render_widget(Paragraph::new(Span::styled(keys, dim())), bottom);
}
