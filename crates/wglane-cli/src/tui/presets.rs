//! Presets in the TUI: slot 0 is the last request from `state.json`, slots
//! 1-9 the presets of config.toml in key order. A number key applies at once
//! (reconnects); the preview shows what would change first.

use std::collections::BTreeSet;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use wglane_core::{
    ActiveState, ChangeLine, CountryCode, EffectiveRequest, Location, Pick, describe_change,
};

use super::app::{Busy, Effect, State};

/// A preset slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// `0`: the last request (`wglane up -`).
    Last,
    /// Index into the presets of config.toml.
    Preset(usize),
}

/// One slot as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct View {
    pub(crate) slot: Slot,
    /// `0`-`9`; `None` past the ninth preset.
    pub(crate) key: Option<char>,
    pub(crate) name: String,
    /// `DE/Berlin · least-load · +2 routes`.
    pub(crate) summary: String,
    /// The live connection was applied from it.
    pub(crate) active: bool,
}

fn summary(req: &EffectiveRequest) -> String {
    let mut parts = vec![
        req.location
            .as_ref()
            .map_or_else(|| "no location".to_owned(), ToString::to_string),
        req.pick.to_string(),
    ];
    if !req.bypass_extra.is_empty() {
        parts.push(format!("+{} bypass", req.bypass_extra.len()));
    }
    parts.join(" · ")
}

/// Every slot: `last` first (when a request was recorded), then the presets.
pub(crate) fn slots(s: &State) -> Vec<View> {
    let mut out = Vec::new();
    if let Some(last) = s.last.as_ref().filter(|l| l.location.is_some()) {
        out.push(View {
            slot: Slot::Last,
            key: Some('0'),
            name: "last".into(),
            summary: summary(&EffectiveRequest {
                location: last.location.clone(),
                pick: last.pick,
                bypass_extra: last.bypass_extra.clone(),
            }),
            active: false,
        });
    }
    let active = s.active_preset.as_ref().map(|p| p.name.as_str());
    if let Some(settings) = &s.settings {
        for (i, p) in settings.resolved_presets().into_iter().enumerate() {
            out.push(View {
                slot: Slot::Preset(i),
                key: p
                    .key
                    .and_then(|k| char::from_digit(u32::try_from(k).ok()?, 10)),
                active: active == Some(p.name.as_str()),
                summary: summary(&p.request),
                name: p.name,
            });
        }
    }
    out
}

/// The slot behind number key `c`.
pub(crate) fn for_key(s: &State, c: char) -> Option<View> {
    slots(s).into_iter().find(|v| v.key == Some(c))
}

/// The connection as it is now, from the last status and the last request
/// (the TUI twin of `preset::current_state`, without asking the helper again).
pub(crate) fn current(s: &State) -> Option<ActiveState> {
    let cc: CountryCode = s.active().first()?.parse().ok()?;
    Some(
        match s
            .last
            .as_ref()
            .filter(|l| l.location.as_ref().is_none_or(|p| p.country() == cc))
        {
            Some(l) => ActiveState {
                location: l.location.clone().or(Some(Location::country_only(cc))),
                pick: l.pick,
                bypass_extra: l.bypass_extra.clone(),
            },
            None => ActiveState {
                location: Some(Location::country_only(cc)),
                pick: Pick::default(),
                bypass_extra: Vec::new(),
            },
        },
    )
}

/// What applying `slot` would change now. Presets go through core
/// `describe_change` (the resolved overlay, environment included); `last` is
/// a plain diff of the recorded request.
pub(crate) fn preview(s: &State, slot: Slot) -> Vec<String> {
    let now = current(s);
    let lines = match slot {
        Slot::Preset(i) => match &s.settings {
            Some(settings) => match settings.presets.value.get(i) {
                Some(p) => describe_change(now.as_ref(), &p.preset, settings),
                None => Vec::new(),
            },
            None => Vec::new(),
        },
        Slot::Last => match s.last.as_ref() {
            Some(l) => diff(
                now.as_ref(),
                &EffectiveRequest {
                    location: l.location.clone(),
                    pick: l.pick,
                    bypass_extra: l.bypass_extra.clone(),
                },
            ),
            None => Vec::new(),
        },
    };
    lines.iter().map(ToString::to_string).collect()
}

/// `want` against `now`, in [`ChangeLine`]s.
fn diff(now: Option<&ActiveState>, want: &EffectiveRequest) -> Vec<ChangeLine> {
    let shown = |l: Option<&Location>| l.map_or_else(|| "-".to_owned(), ToString::to_string);
    let mut out = Vec::new();
    let here = now.and_then(|c| c.location.as_ref());
    let same = match (here, want.location.as_ref()) {
        (Some(a), Some(b)) => a.same_place(b),
        (a, b) => a.is_none() && b.is_none(),
    };
    if !same {
        out.push(ChangeLine::Changed {
            key: "location",
            from: shown(here),
            to: shown(want.location.as_ref()),
        });
    }
    if let Some(c) = now
        && c.pick != want.pick
    {
        out.push(ChangeLine::Changed {
            key: "pick",
            from: c.pick.to_string(),
            to: want.pick.to_string(),
        });
    }
    let have: BTreeSet<_> = now.iter().flat_map(|c| &c.bypass_extra).collect();
    let need: BTreeSet<_> = want.bypass_extra.iter().collect();
    let (added, removed) = (
        need.difference(&have).count(),
        have.difference(&need).count(),
    );
    if added > 0 {
        out.push(ChangeLine::AddRoutes(added));
    }
    if removed > 0 {
        out.push(ChangeLine::RemoveRoutes(removed));
    }
    if out.is_empty() {
        out.push(ChangeLine::AlreadyActive);
    }
    out
}

/// Applies `slot`: reconnects at once.
pub(crate) fn apply(s: &mut State, slot: Slot, fx: &mut Vec<Effect>) {
    if s.busy.is_some() {
        s.note("busy: wait for the current change to finish");
        return;
    }
    let Some(v) = slots(s).into_iter().find(|v| v.slot == slot) else {
        return;
    };
    let name = match slot {
        Slot::Last => None,
        Slot::Preset(_) => Some(v.name.clone()),
    };
    s.start(
        Busy::Connecting(format!("{} ({})", v.name, v.summary)),
        Effect::Apply(name),
        fx,
    );
}

/// The header strip: `0 last DE/Berlin   1 work ●   2 stream   3 travel`.
/// Drops summaries, then later slots, to fit `width`.
pub(crate) fn strip(s: &State, width: usize) -> Line<'static> {
    let views = slots(s);
    let dim = Style::new().add_modifier(Modifier::DIM);
    if views.is_empty() {
        return Line::from(Span::styled(
            "presets  none: add [presets.NAME] to config.toml",
            dim,
        ));
    }
    let build = |with_last_summary: bool, n: usize| {
        let mut spans = vec![Span::styled("presets  ", dim)];
        for v in views.iter().take(n) {
            let key = v.key.map_or_else(|| "-".to_owned(), String::from);
            spans.push(Span::styled(key, s.fg(Color::Cyan)));
            let style = if v.active {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            spans.push(Span::styled(format!(" {}", v.name), style));
            if v.slot == Slot::Last && with_last_summary {
                let place = s
                    .last
                    .as_ref()
                    .and_then(|l| l.location.as_ref())
                    .map_or_else(String::new, |l| format!(" {l}"));
                spans.push(Span::styled(place, dim));
            }
            if v.active {
                spans.push(Span::styled(" ● active", s.fg(Color::Green)));
            }
            spans.push(Span::raw("   "));
        }
        if n < views.len() {
            spans.push(Span::styled(format!("+{} ", views.len() - n), dim));
        }
        Line::from(spans)
    };
    (1..=views.len())
        .rev()
        .flat_map(|n| [build(true, n), build(false, n)])
        .find(|l| l.width() <= width)
        .unwrap_or_else(|| build(false, 1))
}
