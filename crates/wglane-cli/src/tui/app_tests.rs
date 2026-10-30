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

/// Three countries in the browse tree, all groups closed: rows are three headers.
fn loaded() -> State {
    State {
        screen: Screen::Browse,
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

fn with_latency(samples: impl IntoIterator<Item = u64>) -> State {
    samples
        .into_iter()
        .fold(loaded(), |s, ms| up(s, &["ke"], ms))
}

fn snap(status: TunnelStatus, known: bool, city: Option<String>) -> Event {
    Event::Status(Box::new(Snapshot {
        status,
        known,
        city,
        last: None,
        connected: false,
        preset: None,
        now: 0,
    }))
}

fn up(s: State, active: &[&str], ms: u64) -> State {
    update(s, snap(status(active, reachable(ms)), true, None)).0
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
    assert_eq!(s.busy, Some(Busy::Connecting("United States".into())));
    let (s, fx) = key(s, KeyCode::Char('d'));
    assert_eq!(fx, Vec::new());
    assert_eq!(s.busy, Some(Busy::Connecting("United States".into())));
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
    assert_eq!(
        (&s.busy, s.message.as_str()),
        (&None, "connected: South Africa")
    );
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

#[test]
fn tui_settings_apply() {
    let with = |expanded: bool, samples: usize, ratio: f64| {
        State::with_settings(&settings_from(&format!(
            "[tui]\nstart_expanded = {expanded}\ngraph_samples = {samples}\nwarn_ratio = {ratio:?}\n"
        )))
    };
    let s = with(true, 12, 2.0);
    assert_eq!(s.expanded.len(), Continent::ALL.len());
    assert_eq!(s.ui.spike_limit(100), 200);
    let mut s = s;
    for i in 0..30 {
        s = up(s, &[], i);
    }
    assert_eq!(s.latency.len(), 12);
    assert!(with(false, 60, 1.5).expanded.is_empty());
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
    let s = update(s, snap(status(&[], down), true, None)).0;
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
    // Opening a group only asks for its countries' samples.
    assert!(fx.iter().all(|e| matches!(e, Effect::Sample { .. })) && s.busy.is_none());
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
    assert_eq!(fx, [Effect::Refresh, Effect::LoadCountries]);
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
    let s = key(s, KeyCode::Esc).0;
    assert_eq!(
        (s.screen, s.should_quit),
        (Screen::Home, false),
        "then home"
    );
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
        // This walk stays in the tree; the all-modes walk is below.
        s.screen = Screen::Browse;
        s.palette = None;
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

fn render_sized(s: &State, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(s, f)).unwrap();
    t
}

fn render(s: &State) -> Terminal<TestBackend> {
    render_sized(s, 70, 10)
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
    assert_eq!(s.busy, Some(Busy::Connecting("United States".into())));
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
        Some(Busy::Connecting("United States · Los Angeles".into()))
    );
    // Input is blocked while connecting, but the result names the city.
    let (s, _) = update(s, Event::Finished(Ok(())));
    assert_eq!(s.message, "connected: United States · Los Angeles");
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
        snap(status(&["us"], reachable(30)), true, Some("Chicago".into())),
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
        snap(
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
fn ticks_do_not_pile_up_refreshes() {
    let (s, fx) = update(State::default(), Event::Tick);
    assert_eq!(fx, [Effect::Refresh]);
    // The first refresh has not answered yet: no second one.
    let (s, fx) = update(s, Event::Tick);
    assert_eq!(fx, Vec::new());
    let (s, _) = update(s, snap(status(&[], reachable(1)), true, None));
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
    assert_eq!(s.busy, Some(Busy::Connecting("United States".into())));
    // The connect finishes behind the user's back; the screen follows.
    let (s, fx) = update(s, Event::Finished(Ok(())));
    assert_eq!(
        (s.busy.clone(), s.message.as_str()),
        (None, "connected: United States")
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
    assert_eq!(s.busy, Some(Busy::Connecting("United States".into())));
    assert_eq!(fx, Vec::new(), "quit starts no new work");
}

// ---- palette-first redesign -------------------------------------------------

use wglane_core::{FileConfig, Location, Overrides, Pick, PublicKey};

use super::super::hud::{Safety, safety};
use super::super::palette::{Item, Kind};
use super::super::presets::{self as pre, Slot};

const CONFIG: &str = r#"
location = "DE/Frankfurt"
preset_order = ["work", "stream", "travel"]

[presets.work]
label = "Office exit"
location = "DE/Frankfurt"
bypass_extra = ["198.51.100.0/24", "198.51.100.77/32"]

[presets.stream]
location = "US/New York"
pick = "fastest"

[presets.travel]
location = "ZA"
"#;

fn settings_from(toml: &str) -> Effective {
    FileConfig::parse(toml)
        .unwrap()
        .resolve(&Overrides::default(), Overrides::default())
}

/// The fake catalog the palette searches.
fn catalog() -> Vec<Country> {
    vec![
        with_cities("CH", "Switzerland", &[("Zurich", 6), ("Lausanne", 2)]),
        with_cities("CL", "Chile", &[("Santiago", 3)]),
        with_cities(
            "DE",
            "Germany",
            &[("Frankfurt", 12), ("Berlin", 5), ("Hamburg", 4)],
        ),
        with_cities("FR", "France", &[("Paris", 9)]),
        with_cities("GB", "United Kingdom", &[("London", 11), ("Manchester", 3)]),
        with_cities("NL", "Netherlands", &[("Amsterdam", 14)]),
        with_cities(
            "US",
            "United States",
            &[("New York", 9), ("San Francisco", 6), ("San Jose", 4)],
        ),
        with_cities("ZA", "South Africa", &[("Johannesburg", 4)]),
    ]
}

fn last(loc: &str, preset: Option<&str>) -> LastRequest {
    LastRequest {
        location: Some(loc.parse::<Location>().unwrap()),
        pick: Pick::LeastLoad,
        bypass_extra: if preset == Some("work") {
            vec![
                "198.51.100.0/24".parse().unwrap(),
                "198.51.100.77/32".parse().unwrap(),
            ]
        } else {
            Vec::new()
        },
        preset: preset.map(|p| PresetUse {
            name: p.into(),
            overridden: Vec::new(),
        }),
        at: 1_000,
    }
}

fn tunnel(active: &[&str], exit_cc: &str, protected: Option<bool>) -> TunnelStatus {
    TunnelStatus {
        active: active.iter().map(|s| (*s).to_owned()).collect(),
        exit_ip: Some("203.0.113.7".parse().unwrap()),
        exit_country: Some(cc(exit_cc)),
        protected,
        tailnet: reachable(41),
        bypass_routes: 14,
    }
}

fn status_event(st: TunnelStatus, last: Option<LastRequest>, connected: bool) -> Event {
    Event::Status(Box::new(Snapshot {
        preset: last
            .as_ref()
            .filter(|_| connected && !st.active.is_empty())
            .and_then(|l| l.preset.clone()),
        status: st,
        known: true,
        city: Some("Frankfurt".into()),
        last,
        connected,
        now: 1_000 + 4_320,
    }))
}

/// Connected to DE/Frankfurt with preset `work`, nothing sampled yet.
fn home_bare() -> State {
    let mut s = State::with_settings(&settings_from(CONFIG));
    s = update(s, Event::Countries(Ok(catalog()))).0;
    update(
        s,
        status_event(
            tunnel(&["de"], "DE", Some(true)),
            Some(last("DE/Frankfurt", Some("work"))),
            true,
        ),
    )
    .0
}

fn server_at(host: &str, country: &str, load: u8) -> Server {
    Server {
        hostname: host.into(),
        station: "203.0.113.20".parse().unwrap(),
        load,
        public_key: PublicKey(String::new()),
        country: cc(country),
        city: None,
    }
}

/// Feeds a sample (and optionally its latency) as the worker would.
fn sampled(s: State, place: PlaceKey, host: &str, load: u8, ms: Option<u64>) -> State {
    let epoch = s.probe_epoch;
    let sv = server_at(host, place.cc.as_str(), load);
    let s = update(
        s,
        Event::Sampled {
            epoch,
            place,
            server: Some(sv),
        },
    )
    .0;
    match ms {
        Some(ms) => {
            update(
                s,
                Event::ProbeResult {
                    epoch,
                    key: host.into(),
                    latency_ms: Some(ms),
                },
            )
            .0
        }
        None => s,
    }
}

fn country_key(c: &str) -> PlaceKey {
    PlaceKey::country(cc(c))
}

fn city_key(c: &str, name: &str) -> PlaceKey {
    PlaceKey::city(cc(c), name)
}

/// `home_bare` with nearby countries and a few cities measured, and a graph.
fn home() -> State {
    let mut s = home_bare();
    for (k, host, load, ms) in [
        (country_key("NL"), "nl0001", 18, Some(27)),
        (country_key("GB"), "gb0002", 45, Some(31)),
        (country_key("FR"), "fr0003", 52, Some(33)),
        (country_key("DE"), "de1042", 22, Some(38)),
        (country_key("CH"), "ch0005", 9, Some(41)),
        (city_key("US", "San Francisco"), "us0006", 58, Some(151)),
        (city_key("US", "San Jose"), "us0007", 72, Some(149)),
        (city_key("CL", "Santiago"), "cl0008", 15, Some(233)),
        (city_key("CH", "Lausanne"), "ch0009", 11, Some(43)),
    ] {
        s = sampled(s, k, host, load, ms);
    }
    for i in 0..40u64 {
        let ms = 35 + (i * 7) % 13 + if i == 28 { 30 } else { 0 };
        s = update(
            s,
            status_event(
                tunnel(&["de"], "DE", Some(true)),
                Some(last("DE/Frankfurt", Some("work"))),
                true,
            ),
        )
        .0;
        if let Some(l) = s.latency.last_mut() {
            *l = ms;
        }
    }
    s.message.clear();
    s
}

fn typing(s: State, text: &str) -> State {
    typed(s, text)
}

fn palette(s: State, q: &str) -> State {
    typing(key(s, KeyCode::Char(' ')).0, q)
}

fn item_names(s: &State) -> Vec<String> {
    palette::items(s)
        .iter()
        .map(|i| match i {
            Item::Place(k) => k.city.clone().unwrap_or_else(|| k.cc.to_string()),
            Item::Preset(Slot::Last) => "@last".into(),
            Item::Preset(Slot::Preset(i)) => format!("@{i}"),
            Item::Action(a) => format!(">{a:?}"),
        })
        .collect()
}

fn cursor_item(s: &State) -> Option<Item> {
    palette::items(s).get(s.palette.as_ref()?.cursor).cloned()
}

fn screen(s: &State, w: u16, h: u16) -> String {
    format!("{}", render_sized(s, w, h).backend())
}

#[test]
fn space_and_colon_open_the_palette_where_letters_type() {
    for open_key in [' ', ':'] {
        let (s, _) = key(home(), KeyCode::Char(open_key));
        assert_eq!(s.palette, Some(Palette::default()));
        let s = typing(s, "qdjk");
        assert!(!s.should_quit && s.busy.is_none());
        assert_eq!(s.palette.as_ref().unwrap().query, "qdjk");
        let (s, _) = key(s, KeyCode::Esc);
        assert!(
            s.palette.is_none() && !s.should_quit,
            "esc closes, never quits"
        );
    }
}

#[test]
fn palette_search_finds_cities_across_countries_and_samples_them() {
    let s = home_bare();
    let (s, _) = key(s, KeyCode::Char(' '));
    let s = typing(s, "sa");
    let (s, _) = key(s, KeyCode::Char('n'));
    let names = item_names(&s);
    // Prefix hits first (none measured: alphabetical), then the substring hit.
    assert_eq!(
        names[..4],
        ["San Francisco", "San Jose", "Santiago", "Lausanne"]
    );
    // Places shown get a server lookup, each once.
    assert_eq!(
        s.samples.get(&city_key("US", "San Jose")),
        Some(&Sample::Requested)
    );
    let (_, again) = key(key(s, KeyCode::Backspace).0, KeyCode::Char('n'));
    assert!(
        again.iter().all(|e| !matches!(e, Effect::Sample { places, .. } if places.iter().any(|p| p.0 == city_key("US", "San Jose")))),
        "never asked twice"
    );
}

#[test]
fn enter_in_the_palette_connects_the_city_under_the_cursor() {
    let s = palette(home_bare(), "san j");
    assert_eq!(
        cursor_item(&s),
        Some(Item::Place(city_key("US", "San Jose")))
    );
    let (s, fx) = key(s, KeyCode::Enter);
    let Some(Effect::Connect(code, Some(city))) = fx.first() else {
        panic!("{fx:?}");
    };
    assert_eq!((code.as_str(), city.name.as_str()), ("US", "San Jose"));
    assert_eq!(
        s.busy,
        Some(Busy::Connecting("United States · San Jose".into()))
    );
    assert!(s.palette.is_none());
}

#[test]
fn a_query_without_match_lists_nothing_and_enter_does_nothing() {
    let s = palette(home(), "xqzv");
    assert_eq!(palette::items(&s), Vec::new());
    let s = keys(s, &[KeyCode::Down, KeyCode::Up, KeyCode::End]);
    let (s, fx) = key(s, KeyCode::Enter);
    assert!(fx.is_empty() && s.busy.is_none() && s.palette.is_none());
}

#[test]
fn prefixes_and_tab_choose_the_kind() {
    let s = palette(home(), "@");
    assert!(item_names(&s).iter().all(|n| n.starts_with('@')));
    assert_eq!(item_names(&s)[..4], ["@last", "@0", "@1", "@2"]);
    let s = palette(home(), ">brow");
    assert_eq!(item_names(&s), [">Browse"]);
    let (s, _) = key(s, KeyCode::Enter);
    assert_eq!(s.screen, Screen::Browse);
    // tab: all -> places -> presets -> actions -> all
    let mut s = palette(home(), "");
    let mut kinds = Vec::new();
    for _ in 0..4 {
        s = key(s, KeyCode::Tab).0;
        kinds.push(s.palette.as_ref().unwrap().kind);
    }
    assert_eq!(
        kinds,
        [Kind::Places, Kind::Presets, Kind::Actions, Kind::All]
    );
    // tab after a prefix drops it and moves on from its kind.
    let s = key(palette(home(), "@"), KeyCode::Tab).0;
    assert_eq!(
        (
            s.palette.as_ref().unwrap().query.as_str(),
            s.palette.as_ref().unwrap().kind
        ),
        ("", Kind::Actions)
    );
}

#[test]
fn the_empty_palette_teaches_presets_fastest_places_and_every_action() {
    let s = palette(home(), "");
    let names = item_names(&s);
    assert_eq!(names[..4], ["@last", "@0", "@1", "@2"]);
    assert_eq!(names[4..7], ["NL", "GB", "FR"], "fastest first");
    assert_eq!(
        names.iter().filter(|n| n.starts_with('>')).count(),
        ACTIONS.len()
    );
}

#[test]
fn number_keys_apply_a_preset_at_once_and_0_repeats_the_last_request() {
    let (s, fx) = key(home(), KeyCode::Char('2'));
    assert_eq!(fx, [Effect::Apply(Some("stream".into()))]);
    assert!(matches!(&s.busy, Some(Busy::Connecting(t)) if t.starts_with("stream (US/New York")));
    // One change at a time.
    let (_, fx) = key(s, KeyCode::Char('1'));
    assert_eq!(fx, Vec::new());
    let (_, fx) = key(home(), KeyCode::Char('0'));
    assert_eq!(fx, [Effect::Apply(None)]);
    let (s, fx) = key(home(), KeyCode::Char('7'));
    assert!(fx.is_empty() && s.busy.is_none());
    assert!(s.message.contains("no preset on 7"));
    // No state file yet: 0 explains itself.
    let (s, fx) = key(
        State::with_settings(&settings_from(CONFIG)),
        KeyCode::Char('0'),
    );
    assert!(fx.is_empty() && s.message.contains("no last request"));
}

#[test]
fn p_previews_presets_with_the_resolved_change() {
    let (s, fx) = key(home(), KeyCode::Char('p'));
    assert_eq!(fx, Vec::new());
    assert_eq!(s.palette.as_ref().unwrap().query, "@");
    let active = pre::slots(&s);
    assert!(
        active.iter().any(|v| v.name == "work" && v.active),
        "the active preset is marked"
    );
    assert_eq!(pre::preview(&s, Slot::Preset(0)), ["already active"]);
    assert_eq!(
        pre::preview(&s, Slot::Preset(1)),
        [
            "location  DE/Frankfurt -> US/New York",
            "pick  least-load -> fastest",
            "bypass_extra  -2 routes"
        ]
    );
    assert_eq!(pre::preview(&s, Slot::Last), ["already active"]);
    // Down to `stream`, enter applies it.
    let s = keys(s, &[KeyCode::Down, KeyCode::Down]);
    assert_eq!(cursor_item(&s), Some(Item::Preset(Slot::Preset(1))));
    let (_, fx) = key(s, KeyCode::Enter);
    assert_eq!(fx, [Effect::Apply(Some("stream".into()))]);
}

#[test]
fn the_last_slot_previews_a_plain_diff_while_down() {
    let s = update(
        home(),
        status_event(tunnel(&[], "KE", None), Some(last("ZA", None)), false),
    )
    .0;
    assert_eq!(pre::preview(&s, Slot::Last), ["location  - -> ZA"]);
    assert!(s.active_preset.is_none(), "down: no preset is active");
}

#[test]
fn the_list_re_sorts_as_probes_land_and_the_cursor_stays_put() {
    let s = palette(home_bare(), "san");
    let s = keys(s, &[KeyCode::Down, KeyCode::Down]);
    assert_eq!(
        cursor_item(&s),
        Some(Item::Place(city_key("CL", "Santiago")))
    );
    let order = |s: &State| item_names(s)[..3].to_vec();
    assert_eq!(order(&s), ["San Francisco", "San Jose", "Santiago"]);
    // Santiago answers first and fastest: it moves to the top, the cursor with it.
    let s = sampled(s, city_key("CL", "Santiago"), "cl0008", 15, Some(90));
    assert_eq!(order(&s)[0], "Santiago");
    assert_eq!(
        cursor_item(&s),
        Some(Item::Place(city_key("CL", "Santiago")))
    );
    let s = sampled(s, city_key("US", "San Jose"), "us0007", 72, Some(30));
    assert_eq!(order(&s), ["San Jose", "Santiago", "San Francisco"]);
    assert_eq!(
        cursor_item(&s),
        Some(Item::Place(city_key("CL", "Santiago")))
    );
}

#[test]
fn probe_results_from_before_a_tunnel_change_are_dropped() {
    let s = sampled(home_bare(), country_key("NL"), "nl0001", 18, None);
    let old = s.probe_epoch;
    assert_eq!(s.probes["nl0001"], Probe::Pending);
    // A connect finishes: new round over every sampled server.
    let (s, fx) = update(s, Event::Finished(Ok(())));
    assert_eq!(s.probe_epoch, old + 1);
    assert!(fx.contains(&Effect::Probe {
        epoch: old + 1,
        servers: vec![server_at("nl0001", "NL", 18)]
    }));
    let before = s.clone();
    let (s, _) = update(
        s,
        Event::ProbeResult {
            epoch: old,
            key: "nl0001".into(),
            latency_ms: Some(5),
        },
    );
    assert_eq!(s, before, "a stale result changes nothing");
    // A sample asked in the old round is kept, and probed again in the new one.
    let (s, fx) = update(
        s,
        Event::Sampled {
            epoch: old,
            place: country_key("FR"),
            server: Some(server_at("fr0003", "FR", 52)),
        },
    );
    assert_eq!(
        fx,
        [Effect::Probe {
            epoch: old + 1,
            servers: vec![server_at("fr0003", "FR", 52)]
        }]
    );
    assert_eq!(s.probes["fr0003"], Probe::Pending);
}

#[test]
fn f_connects_the_fastest_measured_place_and_explains_when_none() {
    let (s, fx) = key(home(), KeyCode::Char('f'));
    assert_eq!(fx, [Effect::Connect(cc("NL"), None)]);
    assert!(s.busy.is_some());
    // Everything failed while connected: say so, connect nothing.
    let mut s = home_bare();
    s = sampled(s, country_key("NL"), "nl0001", 18, None);
    let epoch = s.probe_epoch;
    s = update(
        s,
        Event::ProbeResult {
            epoch,
            key: "nl0001".into(),
            latency_ms: None,
        },
    )
    .0;
    let (s, fx) = key(s, KeyCode::Char('f'));
    assert_eq!(fx, Vec::new());
    assert!(s.message.contains("latency unavailable while connected"));
    assert!(screen(&s, 96, 24).contains("latency unavailable while connected"));
}

fn leaking() -> State {
    update(
        home(),
        status_event(
            TunnelStatus {
                exit_ip: Some("203.0.113.99".parse().unwrap()),
                ..tunnel(&["de"], "NL", Some(false))
            },
            Some(last("DE/Frankfurt", Some("work"))),
            true,
        ),
    )
    .0
}

#[test]
fn a_wrong_exit_country_or_unprotected_flag_is_a_leak_and_pins_disconnect() {
    let s = leaking();
    assert_eq!(
        safety(&s),
        Safety::Leak(vec![
            "exit IP 203.0.113.99 is in NL, not DE".into(),
            "Nord says not protected".into()
        ])
    );
    let s = palette(s, "");
    assert_eq!(cursor_item(&s), Some(Item::Action(Action::Disconnect)));
    // Typing then enter goes to the match, not the pinned row.
    let s = typing(s, "amst");
    assert_eq!(
        cursor_item(&s),
        Some(Item::Place(city_key("NL", "Amsterdam")))
    );
    // Down is not leaking.
    let down = update(
        home(),
        status_event(tunnel(&[], "NL", Some(false)), None, false),
    )
    .0;
    assert_eq!(safety(&down), Safety::Fine);
}

#[test]
fn a_disabled_leak_check_is_loud() {
    let s = State::with_settings(&settings_from(&format!("exit_ip_check = false\n{CONFIG}")));
    assert_eq!(safety(&s), Safety::Off);
    assert!(screen(&s, 96, 24).contains("leak check disabled"));
    assert!(screen(&s, 34, 7).contains("leak check disabled"));
}

#[test]
fn a_failed_connect_checks_udp_and_enter_retries_it() {
    let (s, _) = key(home(), KeyCode::Char('3'));
    let (s, fx) = update(
        s,
        Event::Finished(Err("no handshake from za0042 in 10 s".into())),
    );
    assert!(fx.contains(&Effect::CheckUdp) && fx.contains(&Effect::Refresh));
    assert!(s.failed && s.udp_checking);
    let (s, _) = update(s, Event::Udp(Some(true)));
    assert_eq!(s.udp, Some(Udp::Blocked));
    assert!(screen(&s, 96, 24).contains("udp 51820 looks blocked on this network"));
    let (s, fx) = key(s, KeyCode::Enter);
    assert_eq!(fx, [Effect::Apply(Some("travel".into()))]);
    assert!(s.busy.is_some());
    // Without a failure enter on home does nothing.
    assert_eq!(key(home(), KeyCode::Enter).1, Vec::new());
}

#[test]
fn an_unknown_tunnel_says_why_and_r_rechecks() {
    let s = update(home_bare(), snap(status(&[], reachable(30)), false, None)).0;
    let text = screen(&s, 96, 24);
    assert!(text.contains("? unknown") && text.contains("the helper did not answer"));
    assert!(text.contains("r retries"));
    let (_, fx) = key(s, KeyCode::Char('r'));
    assert!(fx.contains(&Effect::Refresh));
}

#[test]
fn b_browses_esc_comes_home_and_slash_searches_the_tree() {
    let (s, _) = key(home(), KeyCode::Char('b'));
    assert_eq!(s.screen, Screen::Browse);
    let (s, _) = key(s, KeyCode::Esc);
    assert_eq!((s.screen, s.should_quit), (Screen::Home, false));
    let s = typing(key(s, KeyCode::Char('/')).0, "ger");
    assert_eq!(
        (s.screen, s.searching, s.query.as_str()),
        (Screen::Browse, true, "ger")
    );
    assert!(key(home(), KeyCode::Esc).0.should_quit, "esc on home quits");
}

#[test]
fn opening_a_group_in_browse_samples_its_countries() {
    let s = key(home(), KeyCode::Char('b')).0;
    let s = keys(s, &[KeyCode::Char('G')]);
    let (s, fx) = key(s, KeyCode::Char('l'));
    assert!(s.expanded.contains(&Continent::SouthAmerica));
    assert!(fx.iter().any(|e| matches!(e, Effect::Sample { places, .. } if places.iter().any(|p| p.0 == country_key("CL")))));
}

#[test]
fn a_random_walk_over_every_mode_keeps_state_valid() {
    const KEYS: [KeyCode; 30] = [
        KeyCode::Char(' '),
        KeyCode::Char(':'),
        KeyCode::Char('@'),
        KeyCode::Char('>'),
        KeyCode::Char('s'),
        KeyCode::Char('a'),
        KeyCode::Char('n'),
        KeyCode::Char('/'),
        KeyCode::Char('b'),
        KeyCode::Char('p'),
        KeyCode::Char('f'),
        KeyCode::Char('d'),
        KeyCode::Char('r'),
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Char('h'),
        KeyCode::Char('l'),
        KeyCode::Char('g'),
        KeyCode::Char('G'),
        KeyCode::Char('?'),
        KeyCode::Char('0'),
        KeyCode::Char('2'),
        KeyCode::Tab,
        KeyCode::Backspace,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Enter,
        KeyCode::Esc,
    ];
    let mut seed = 0x9e37_79b9_u64;
    let mut s = home();
    let mut tick = 0u64;
    for _ in 0..8_000 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let k = KEYS[(seed >> 33) as usize % KEYS.len()];
        let (next, fx) = key(s, k);
        s = next;
        // Results land between keys: a probe for anything pending, sometimes a finish.
        for e in fx {
            if let Effect::Sample { epoch, places } = e {
                for (place, _) in places {
                    tick += 1;
                    let host = format!("h{tick}");
                    s = update(
                        s,
                        Event::Sampled {
                            epoch,
                            place,
                            server: Some(server_at(&host, "DE", u8::try_from(tick % 100).unwrap())),
                        },
                    )
                    .0;
                    s = update(
                        s,
                        Event::ProbeResult {
                            epoch,
                            key: host,
                            latency_ms: (!tick.is_multiple_of(3)).then_some(tick % 250),
                        },
                    )
                    .0;
                }
            }
        }
        if s.busy.is_some() && seed.is_multiple_of(3) {
            let ok = seed.is_multiple_of(2);
            s = update(
                s,
                Event::Finished(if ok { Ok(()) } else { Err("boom".into()) }),
            )
            .0;
        }
        s.should_quit = false;
        let rows = s.rows();
        assert!(s.selected < rows.len().max(1));
        if let Some(p) = &s.palette {
            assert!(
                p.cursor < palette::items(&s).len().max(1),
                "cursor {} of {:?}",
                p.cursor,
                p
            );
        }
        if seed.is_multiple_of(97) {
            // Every mode draws at every size without panicking.
            for (w, h) in [(96, 24), (60, 20), (34, 7), (20, 4)] {
                let _ = render_sized(&s, w, h);
            }
        }
    }
}

#[test]
fn no_color_draws_no_colors_in_any_state() {
    let colored = |s: &State, w, h| {
        render_sized(s, w, h)
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg != Color::Reset || c.bg != Color::Reset)
    };
    let mut states = vec![
        home(),
        leaking(),
        palette(home(), "san"),
        key(home(), KeyCode::Char('?')).0,
    ];
    let mut failed = home();
    failed.message = "boom".into();
    failed.failed = true;
    failed.udp = Some(Udp::Blocked);
    states.push(failed);
    for s in &mut states {
        assert!(colored(s, 96, 24));
        s.no_color = true;
        for (w, h) in [(96, 24), (60, 20), (34, 7)] {
            assert!(!colored(s, w, h), "{w}x{h}");
        }
        // The state still reads without colour: symbols and reverse video.
        assert!(screen(s, 96, 24).contains('●') || screen(s, 96, 24).contains("LEAK"));
    }
}

// Snapshots of every state in the variant page, 96x24 unless named.

#[test]
fn draws_home_connected() {
    insta::assert_snapshot!(render_sized(&home(), 96, 24).backend());
}

#[test]
fn draws_home_connecting() {
    let (s, _) = key(home(), KeyCode::Char('2'));
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}

#[test]
fn draws_home_down_before_anything_is_measured() {
    let s = update(
        home_bare(),
        status_event(
            tunnel(&[], "KE", None),
            Some(last("DE/Frankfurt", None)),
            false,
        ),
    )
    .0;
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}

#[test]
fn draws_palette_city_results_with_bars_and_fastest() {
    insta::assert_snapshot!(render_sized(&palette(home(), "san"), 96, 24).backend());
}

#[test]
fn draws_palette_while_probing() {
    insta::assert_snapshot!(render_sized(&palette(home_bare(), "san"), 96, 24).backend());
}

#[test]
fn draws_palette_empty() {
    insta::assert_snapshot!(render_sized(&palette(home(), ""), 96, 30).backend());
}

#[test]
fn draws_palette_no_match() {
    insta::assert_snapshot!(render_sized(&palette(home(), "xqzv"), 96, 24).backend());
}

#[test]
fn draws_preset_preview() {
    let s = keys(
        key(home(), KeyCode::Char('p')).0,
        &[KeyCode::Down, KeyCode::Down],
    );
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}

#[test]
fn draws_leak_warning() {
    insta::assert_snapshot!(render_sized(&leaking(), 96, 24).backend());
}

#[test]
fn draws_leak_check_disabled() {
    let mut s = home();
    s.exit_ip_check = false;
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}

#[test]
fn draws_error_with_the_udp_hint() {
    let (s, _) = key(home(), KeyCode::Char('3'));
    let (s, _) = update(
        s,
        Event::Finished(Err("no handshake from za0042 in 10 s".into())),
    );
    let (s, _) = update(s, Event::Udp(Some(true)));
    let s = update(
        s,
        status_event(
            TunnelStatus {
                exit_ip: Some("203.0.113.50".parse().unwrap()),
                ..tunnel(&[], "KE", None)
            },
            Some(last("DE/Frankfurt", Some("work"))),
            false,
        ),
    )
    .0;
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}

#[test]
fn draws_unknown_tunnel() {
    let s = update(home_bare(), snap(status(&[], reachable(30)), false, None)).0;
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}

#[test]
fn draws_help() {
    insta::assert_snapshot!(render_sized(&key(home(), KeyCode::Char('?')).0, 96, 30).backend());
}

#[test]
fn draws_home_60x20() {
    insta::assert_snapshot!(render_sized(&home(), 60, 20).backend());
}

#[test]
fn draws_palette_60x20() {
    insta::assert_snapshot!(render_sized(&palette(home(), "san"), 60, 20).backend());
}

#[test]
fn draws_tiny_34x7() {
    insta::assert_snapshot!(render_sized(&home(), 34, 7).backend());
}

#[test]
fn draws_too_small() {
    insta::assert_snapshot!(render_sized(&home(), 29, 5).backend());
}

#[test]
fn draws_browse_with_bars() {
    let s = keys(key(home(), KeyCode::Char('b')).0, &[KeyCode::Char('g')]);
    insta::assert_snapshot!(render_sized(&s, 96, 24).backend());
}
