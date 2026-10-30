use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Sparkline};
use wglane_core::{Country, CountryCode, TailnetHealth, TunnelStatus};

/// Latency samples kept for the sparkline.
const HISTORY: usize = 60;

/// A tunnel change in flight (the UI is blocked on it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    Connecting(CountryCode),
    Disconnecting,
}

/// Dashboard state. Changed only by [`update`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) should_quit: bool,
    pub(crate) countries: Vec<Country>,
    pub(crate) selected: usize,
    pub(crate) status: Option<TunnelStatus>,
    /// Tailnet round trips in ms, oldest first.
    pub(crate) latency: Vec<u64>,
    pub(crate) busy: Option<Busy>,
    /// Bottom-line message (errors, results).
    pub(crate) message: String,
}

/// Something that happened: a key, a clock tick, or an effect's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    Key(KeyCode),
    Tick,
    Countries(Result<Vec<Country>, String>),
    Status(TunnelStatus),
    /// Connect/disconnect finished.
    Finished(Result<(), String>),
}

/// I/O the reducer asks the event loop to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    LoadCountries,
    Refresh,
    Connect(CountryCode),
    Disconnect,
}

/// Effects to run at startup.
pub(crate) fn init() -> Vec<Effect> {
    vec![Effect::LoadCountries, Effect::Refresh]
}

/// Pure transition: next state plus the effects to run.
pub(crate) fn update(mut s: State, event: Event) -> (State, Vec<Effect>) {
    let mut effects = Vec::new();
    match event {
        Event::Key(KeyCode::Char('q') | KeyCode::Esc) => s.should_quit = true,
        // Keys that change the tunnel are ignored while a change is in flight.
        Event::Key(_) if s.busy.is_some() => {}
        Event::Key(KeyCode::Up) => s.selected = s.selected.saturating_sub(1),
        Event::Key(KeyCode::Down) => {
            s.selected = (s.selected + 1).min(s.countries.len().saturating_sub(1));
        }
        Event::Key(KeyCode::Enter) => {
            if let Some(c) = s.countries.get(s.selected) {
                s.busy = Some(Busy::Connecting(c.code));
                effects.push(Effect::Connect(c.code));
            }
        }
        Event::Key(KeyCode::Char('d')) => {
            s.busy = Some(Busy::Disconnecting);
            effects.push(Effect::Disconnect);
        }
        Event::Key(_) => {}
        Event::Tick => {
            if s.busy.is_none() {
                effects.push(Effect::Refresh);
            }
        }
        Event::Countries(Ok(list)) => {
            s.countries = list;
            s.selected = s.selected.min(s.countries.len().saturating_sub(1));
        }
        Event::Countries(Err(e)) => s.message = format!("servers: {e}"),
        Event::Status(st) => {
            if let TailnetHealth::Reachable { latency_ms, .. } = &st.tailnet {
                s.latency.push(*latency_ms);
                if s.latency.len() > HISTORY {
                    s.latency.remove(0);
                }
            }
            s.status = Some(st);
        }
        Event::Finished(result) => {
            s.message = match (s.busy.take(), result) {
                (_, Err(e)) => e,
                (Some(Busy::Connecting(cc)), Ok(())) => format!("connected to {cc}"),
                (_, Ok(())) => "disconnected".into(),
            };
            effects.push(Effect::Refresh);
        }
    }
    (s, effects)
}

/// Draws the whole dashboard: server list left, stats card right, hints bottom.
pub(crate) fn draw(s: &State, frame: &mut Frame) {
    let [main, bottom] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(main);

    let items = s
        .countries
        .iter()
        .map(|c| ListItem::new(format!("{}  {}", c.code, c.name)));
    let list = List::new(items)
        .block(Block::bordered().title(" servers "))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut list_state = ListState::default().with_selected(Some(s.selected));
    frame.render_stateful_widget(list, left, &mut list_state);

    let card = Block::bordered().title(" stats ");
    let inner = card.inner(right);
    frame.render_widget(card, right);
    let [text, spark] = Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).areas(inner);
    frame.render_widget(Paragraph::new(stats_lines(s)), text);
    frame.render_widget(Sparkline::default().data(&s.latency), spark);

    let hints = "up/down select  enter connect  d disconnect  q quit";
    let line = if s.message.is_empty() {
        hints.to_owned()
    } else {
        format!("{}  |  {hints}", s.message)
    };
    frame.render_widget(Paragraph::new(line), bottom);
}

fn stats_lines(s: &State) -> Vec<Line<'static>> {
    let tunnel = match (s.busy, s.status.as_ref().map(|st| st.active.as_slice())) {
        (Some(Busy::Connecting(cc)), _) => format!("connecting to {cc}..."),
        (Some(Busy::Disconnecting), _) => "disconnecting...".into(),
        (None, Some([first, ..])) => format!("up ({})", first.to_uppercase()),
        (None, Some([])) => "down".into(),
        (None, None) => "unknown".into(),
    };
    let exit = s
        .status
        .as_ref()
        .and_then(|st| st.exit_ip)
        .map_or_else(|| "unknown".into(), |ip| ip.to_string());
    let tailnet = match s.status.as_ref().map(|st| &st.tailnet) {
        Some(TailnetHealth::Reachable { peer, latency_ms }) => {
            format!("ok ({peer}, {latency_ms} ms)")
        }
        Some(TailnetHealth::Unreachable { reason }) => format!("unreachable ({reason})"),
        Some(TailnetHealth::NotInstalled) => "tailscale not installed".into(),
        _ => "unknown".into(),
    };
    vec![
        Line::from(format!("tunnel   {tunnel}")),
        Line::from(format!("exit ip  {exit}")),
        Line::from(format!("tailnet  {tailnet}")),
        Line::from("latency"),
    ]
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
        }
    }

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

    fn status(active: &[&str], tailnet: TailnetHealth) -> TunnelStatus {
        TunnelStatus {
            active: active.iter().map(|s| (*s).to_owned()).collect(),
            exit_ip: Some("203.0.113.7".parse().unwrap()),
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

    fn key(s: State, k: KeyCode) -> (State, Vec<Effect>) {
        update(s, Event::Key(k))
    }

    #[test]
    fn q_and_esc_quit() {
        assert!(key(State::default(), KeyCode::Char('q')).0.should_quit);
        assert!(key(State::default(), KeyCode::Esc).0.should_quit);
        assert!(!key(State::default(), KeyCode::Char('x')).0.should_quit);
    }

    #[test]
    fn arrows_clamp_to_the_list() {
        let (s, _) = key(loaded(), KeyCode::Up);
        assert_eq!(s.selected, 0);
        let (s, _) = key(s, KeyCode::Down);
        let (s, _) = key(s, KeyCode::Down);
        let (s, _) = key(s, KeyCode::Down);
        assert_eq!(s.selected, 2);
        // An empty list must not underflow.
        assert_eq!(key(State::default(), KeyCode::Down).0.selected, 0);
    }

    #[test]
    fn enter_connects_the_selection_and_blocks_input() {
        let (s, _) = key(loaded(), KeyCode::Down);
        let (s, fx) = key(s, KeyCode::Enter);
        assert_eq!(fx, [Effect::Connect(cc("US"))]);
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"))));
        let (s, fx) = key(s, KeyCode::Char('d'));
        assert_eq!(fx, Vec::new());
        assert_eq!(s.busy, Some(Busy::Connecting(cc("US"))));
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
        let (s, _) = key(loaded(), KeyCode::Enter);
        let (s, fx) = update(s, Event::Finished(Ok(())));
        assert_eq!((s.busy, s.message.as_str()), (None, "connected to DE"));
        assert_eq!(fx, [Effect::Refresh]);
        let (s, _) = key(s, KeyCode::Char('d'));
        let (s, _) = update(s, Event::Finished(Err("helper missing".into())));
        assert_eq!(s.message, "helper missing");
    }

    #[test]
    fn tick_refreshes_unless_busy() {
        assert_eq!(update(State::default(), Event::Tick).1, [Effect::Refresh]);
        let (s, _) = key(State::default(), KeyCode::Char('d'));
        assert_eq!(update(s, Event::Tick).1, Vec::new());
    }

    #[test]
    fn latency_history_is_capped_and_skips_unreachable() {
        let mut s = State::default();
        for i in 0..70 {
            s = update(s, Event::Status(status(&[], reachable(i)))).0;
        }
        assert_eq!(s.latency.len(), HISTORY);
        assert_eq!(s.latency.last(), Some(&69));
        let down = TailnetHealth::NotInstalled;
        let s = update(s, Event::Status(status(&[], down))).0;
        assert_eq!(s.latency.len(), HISTORY);
    }

    #[test]
    fn country_reload_keeps_selection_in_range() {
        let (s, _) = key(loaded(), KeyCode::Down);
        let (s, _) = key(s, KeyCode::Down);
        let s = update(s, Event::Countries(Ok(vec![country("DE", "Germany")]))).0;
        assert_eq!(s.selected, 0);
    }

    fn render(s: &State) -> Terminal<TestBackend> {
        let mut t = Terminal::new(TestBackend::new(70, 10)).unwrap();
        t.draw(|f| draw(s, f)).unwrap();
        t
    }

    #[test]
    fn draws_connected() {
        let mut s = loaded();
        s.selected = 1;
        for ms in [12, 18, 15, 40, 22] {
            s = update(s, Event::Status(status(&["us"], reachable(ms)))).0;
        }
        insta::assert_snapshot!(render(&s).backend());
    }

    #[test]
    fn draws_empty_and_connecting() {
        insta::assert_snapshot!("draws_empty", render(&State::default()).backend());
        let (s, _) = key(loaded(), KeyCode::Enter);
        insta::assert_snapshot!("draws_connecting", render(&s).backend());
    }
}
