//! ratatui dashboard, palette-first. `app.rs` holds state, the pure reducer
//! and the browse tree; `hud.rs` draws home, `palette.rs` the jump palette,
//! `presets.rs` the preset slots and previews, `places.rs` place samples and
//! bars, `actions.rs` the action registry. This file owns the terminal and
//! the event loop; `worker.rs` runs effects on their own threads so nothing
//! freezes the screen; `probe.rs` holds latency results and their ordering.

mod actions;
mod app;
mod continent;
mod hud;
mod palette;
mod places;
mod presets;
mod probe;
mod worker;

use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use ratatui::crossterm::event::{self, Event as Term, KeyCode, KeyEventKind, KeyModifiers};
use wglane_core::ports::ServerCatalog;
use wglane_core::{Overrides, Server};
use wglane_macos::{HelperClient, SystemRunner, TailscalePing};
use wglane_nord::NordApi;

use crate::cmd::{doctor, down, fastest, status, up};
use crate::context::{self, Context};
use app::{Busy, Effect, Event, Snapshot, State};
use places::PlaceKey;
use worker::Worker;

/// How often the status card refreshes.
const TICK: Duration = Duration::from_secs(2);
/// How long the loop waits for a key before looking at the workers again.
const FRAME: Duration = Duration::from_millis(50);
/// Nord API lookups running at once while sampling places.
const SAMPLE_THREADS: usize = 6;

/// Runs the dashboard until the user quits.
///
/// Quitting while a connect or disconnect is running restores the terminal at
/// once, then waits for that one change to finish (it is never abandoned
/// half-done) and reports its error, if any. Anything else in flight is
/// dropped.
pub(crate) fn run(ctx: &Context) -> Result<()> {
    let settings = ctx.settings(Overrides::default())?;
    let exit_ip_check = settings.exit_ip_check.value;
    let spawn = || {
        let ctx = ctx.clone();
        Worker::spawn(move |effect, emit| execute(effect, &ctx, exit_ip_check, emit))
    };
    // Tunnel changes and status on one thread, place samples and probes on
    // another, so a slow probe round never delays a connect.
    let (tunnel, probes) = (spawn(), spawn());
    let send = |effect: Effect| match effect {
        Effect::Sample { .. } | Effect::Probe { .. } => probes.send(effect),
        _ => tunnel.send(effect),
    };
    context::set_screen_taken(true);
    let mut terminal = ratatui::init();
    let result = (|| -> Result<State> {
        // https://no-color.org: any non-empty value. Handled here, not by
        // crossterm: under NO_COLOR it turns each color change into an empty
        // SGR (a full reset), which drops bold and reverse mid-row.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let mut state = State::with_settings(&settings);
        state.no_color = no_color;
        for effect in app::init() {
            send(effect);
        }
        let mut last_tick = Instant::now();
        let mut dirty = true;
        loop {
            while let Some(event) = tunnel.try_recv().or_else(|| probes.try_recv()) {
                step(&mut state, event, &send);
                dirty = true;
            }
            if dirty {
                terminal.draw(|f| app::draw(&state, f))?;
                dirty = false;
            }
            if state.should_quit {
                return Ok(state);
            }
            if event::poll(FRAME)? {
                match event::read()? {
                    Term::Key(k) if k.kind == KeyEventKind::Press => {
                        // ctrl-j/k (and n/p) move in the palette, where letters type.
                        let code = match (k.modifiers.contains(KeyModifiers::CONTROL), k.code) {
                            (true, KeyCode::Char('j' | 'n')) => KeyCode::Down,
                            (true, KeyCode::Char('k' | 'p')) => KeyCode::Up,
                            (true, KeyCode::Char('c')) => {
                                state.should_quit = true;
                                continue;
                            }
                            (_, c) => c,
                        };
                        step(&mut state, Event::Key(code), &send);
                    }
                    _ => {}
                }
                dirty = true;
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                step(&mut state, Event::Tick, &send);
            }
        }
    })();
    ratatui::restore();
    context::set_screen_taken(false);
    let waiting = result.as_ref().is_ok_and(|s| s.busy.is_some());
    if let Ok(State {
        busy: Some(busy), ..
    }) = &result
    {
        let what = match busy {
            Busy::Connecting(..) => "connect",
            Busy::Disconnecting => "disconnect",
        };
        eprintln!("finishing the {what} before exit...");
    }
    let _ = probes.shutdown(false);
    for event in tunnel.shutdown(waiting) {
        if let Event::Finished(Err(e)) = event {
            eprintln!("error: {e}");
        }
    }
    result.map(drop)
}

/// Feeds one event to the reducer and hands its effects to the workers.
fn step(state: &mut State, event: Event, send: &impl Fn(Effect)) {
    let (next, effects) = app::update(std::mem::take(state), event);
    *state = next;
    for effect in effects {
        send(effect);
    }
}

/// Runs one effect to completion on a worker thread, emitting its events.
fn execute(effect: Effect, ctx: &Context, exit_ip_check: bool, emit: &mut dyn FnMut(Event)) {
    match effect {
        Effect::LoadCountries => emit(Event::Countries(
            NordApi::new().countries().map_err(|e| e.to_string()),
        )),
        Effect::Refresh => {
            let runner = SystemRunner;
            let (st, known) = status::gather(
                &HelperClient::new(runner),
                &NordApi::new(),
                &TailscalePing::new(runner),
                ctx.recorded_routes(),
                exit_ip_check,
            );
            let city = st.active.first().and_then(|a| ctx.recorded_city(a));
            let state = ctx.state();
            let up = !st.active.is_empty();
            emit(Event::Status(Box::new(Snapshot {
                city,
                connected: state.connected,
                preset: state
                    .last
                    .as_ref()
                    .filter(|_| state.connected && up)
                    .and_then(|l| l.preset.clone()),
                last: state.last,
                known,
                status: st,
                now: context::unix_now(),
            })));
        }
        Effect::Connect(cc, city) => emit(Event::Finished(
            up::connect_real(ctx, cc, city.as_ref())
                .map(drop)
                .map_err(|e| e.to_string()),
        )),
        Effect::Apply(preset) => {
            let args = up::Args {
                target: preset.is_none().then_some(up::Target::Last),
                city: None,
                preset,
                pick: None,
                fastest: false,
            };
            emit(Event::Finished(
                up::apply_real(ctx, &args)
                    .map(drop)
                    .map_err(|e| e.to_string()),
            ));
        }
        Effect::Disconnect => emit(Event::Finished(
            down::disconnect_real(ctx)
                .map(drop)
                .map_err(|e| e.to_string()),
        )),
        Effect::CheckUdp => emit(Event::Udp(doctor::udp_blocked(ctx))),
        Effect::Sample { epoch, places } => {
            let mut found = Vec::new();
            for chunk in places.chunks(SAMPLE_THREADS) {
                let got: Vec<(PlaceKey, Option<Server>)> = thread::scope(|scope| {
                    let handles: Vec<_> = chunk
                        .iter()
                        .map(|(key, city)| {
                            scope.spawn(move || {
                                let server = NordApi::new()
                                    .recommended(key.cc, city.as_ref(), 1)
                                    .ok()
                                    .and_then(|v| v.into_iter().next());
                                (key.clone(), server)
                            })
                        })
                        .collect();
                    handles.into_iter().filter_map(|h| h.join().ok()).collect()
                });
                for (place, server) in got {
                    found.extend(server.clone());
                    emit(Event::Sampled {
                        epoch,
                        place,
                        server,
                    });
                }
            }
            found.sort_by(|a, b| a.hostname.cmp(&b.hostname));
            found.dedup_by(|a, b| a.hostname == b.hostname);
            probe(&found, epoch, emit);
        }
        Effect::Probe { epoch, servers } => probe(&servers, epoch, emit),
    }
}

/// The only place the dashboard measures latency.
///
/// While a tunnel is up, `real_prober` binds pings to the physical interface
/// (so they never measure the tunnel's own exit) and they often all fail; the
/// screen then says `latency unavailable while connected`. The planned helper
/// host routes replace only this function.
fn probe(servers: &[Server], epoch: u64, emit: &mut dyn FnMut(Event)) {
    let helper = HelperClient::new(SystemRunner);
    match fastest::real_prober(&helper) {
        Ok(prober) => {
            fastest::probe_all(&prober, servers, |i, latency| {
                emit(Event::ProbeResult {
                    epoch,
                    key: probe::key(&servers[i]),
                    latency_ms: latency.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                });
            });
        }
        // No safe way to probe: report every server as unanswered.
        Err(_) => {
            for s in servers {
                emit(Event::ProbeResult {
                    epoch,
                    key: probe::key(s),
                    latency_ms: None,
                });
            }
        }
    }
}
