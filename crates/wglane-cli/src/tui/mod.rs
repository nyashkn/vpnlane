//! ratatui dashboard. `app.rs` holds state, the pure reducer and drawing
//! (testable with `TestBackend`); this file owns the terminal and the event
//! loop; `worker.rs` runs the effects on their own thread so a connect never
//! freezes the screen; `probe.rs` holds latency results and their ordering.

mod app;
mod continent;
#[allow(
    dead_code,
    reason = "ordering and cursor helpers for the server-list redesign; tested here"
)]
mod probe;
mod worker;

use std::time::{Duration, Instant};

use color_eyre::Result;
use ratatui::crossterm::event::{self, Event as Term, KeyEventKind};
use wglane_core::Overrides;
use wglane_core::ports::ServerCatalog;
use wglane_macos::{HelperClient, SystemRunner, TailscalePing};
use wglane_nord::NordApi;

use crate::cmd::{down, fastest, status, up};
use crate::context::Context;
use app::{Busy, Effect, Event, State};
use worker::Worker;

/// How often the status card refreshes.
const TICK: Duration = Duration::from_secs(2);
/// How long the loop waits for a key before looking at the worker again.
const FRAME: Duration = Duration::from_millis(50);

/// Runs the dashboard until the user quits.
///
/// Quitting while a connect or disconnect is running restores the terminal at
/// once, then waits for that one change to finish (it is never abandoned
/// half-done) and reports its error, if any. Anything else in flight is
/// dropped.
pub(crate) fn run(ctx: &Context) -> Result<()> {
    let settings = ctx.settings(Overrides::default())?;
    let exit_ip_check = settings.exit_ip_check.value;
    let worker = {
        let ctx = ctx.clone();
        Worker::spawn(move |effect, emit| execute(effect, &ctx, exit_ip_check, emit))
    };
    let mut terminal = ratatui::init();
    let result = (|| -> Result<State> {
        // https://no-color.org: any non-empty value. Handled here, not by
        // crossterm: under NO_COLOR it turns each color change into an empty
        // SGR (a full reset), which drops bold and reverse mid-row.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let mut state = State::with_settings(&settings.tui);
        state.no_color = no_color;
        for effect in app::init() {
            worker.send(effect);
        }
        let mut last_tick = Instant::now();
        let mut dirty = true;
        loop {
            while let Some(event) = worker.try_recv() {
                step(&mut state, event, &worker);
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
                        step(&mut state, Event::Key(k.code), &worker);
                    }
                    _ => {}
                }
                dirty = true;
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                step(&mut state, Event::Tick, &worker);
            }
        }
    })();
    ratatui::restore();
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
    for event in worker.shutdown(waiting) {
        if let Event::Finished(Err(e)) = event {
            eprintln!("error: {e}");
        }
    }
    result.map(drop)
}

/// Feeds one event to the reducer and hands its effects to the worker.
fn step(state: &mut State, event: Event, worker: &Worker) {
    let (next, effects) = app::update(std::mem::take(state), event);
    *state = next;
    for effect in effects {
        worker.send(effect);
    }
}

/// Runs one effect to completion on the worker thread, emitting its events.
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
            emit(Event::Status(st, known, city));
        }
        Effect::Connect(cc, city) => emit(Event::Finished(
            up::connect_real(ctx, cc, city.as_ref())
                .map(drop)
                .map_err(|e| e.to_string()),
        )),
        Effect::Disconnect => emit(Event::Finished(
            down::disconnect_real(ctx)
                .map(drop)
                .map_err(|e| e.to_string()),
        )),
        Effect::Probe { epoch, servers } => {
            let helper = HelperClient::new(SystemRunner);
            // Bound to the physical interface while a tunnel is up, so the
            // dashboard never measures the tunnel's own exit.
            match fastest::real_prober(&helper) {
                Ok(prober) => {
                    fastest::probe_all(&prober, &servers, |i, latency| {
                        emit(Event::ProbeResult {
                            epoch,
                            key: probe::key(&servers[i]),
                            latency_ms: latency
                                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                        });
                    });
                }
                // No safe way to probe: report every server as unanswered.
                Err(_) => {
                    for s in &servers {
                        emit(Event::ProbeResult {
                            epoch,
                            key: probe::key(s),
                            latency_ms: None,
                        });
                    }
                }
            }
        }
    }
}
