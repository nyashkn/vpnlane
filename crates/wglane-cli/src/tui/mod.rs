//! ratatui dashboard. `app.rs` holds state, the pure reducer and drawing
//! (testable with `TestBackend`); this file owns the terminal, the event loop
//! and the effects that touch the system.

mod app;

use std::collections::VecDeque;
use std::time::Duration;

use color_eyre::Result;
use ratatui::crossterm::event::{self, Event as Term, KeyEventKind};
use wglane_core::ports::{ExitIpProbe, ServerCatalog, TailnetProbe, Tunnel};
use wglane_core::{TailnetHealth, TunnelStatus};
use wglane_macos::{HelperClient, SystemRunner, TailscalePing};
use wglane_nord::NordApi;

use crate::cmd::{down, up};
use crate::context::Context;
use app::{Effect, Event, State};

/// How often the status card refreshes.
const TICK: Duration = Duration::from_secs(2);

/// Runs the dashboard until the user quits.
pub(crate) fn run(ctx: &Context) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        let mut state = State::default();
        let mut effects: VecDeque<Effect> = app::init().into();
        loop {
            // Draw before blocking effects so "connecting..." is on screen.
            terminal.draw(|f| app::draw(&state, f))?;
            if state.should_quit {
                return Ok(());
            }
            let event = if let Some(effect) = effects.pop_front() {
                execute(effect, ctx)
            } else if event::poll(TICK)? {
                match event::read()? {
                    Term::Key(k) if k.kind == KeyEventKind::Press => Event::Key(k.code),
                    _ => continue,
                }
            } else {
                Event::Tick
            };
            let (next, more) = app::update(state, event);
            state = next;
            effects.extend(more);
        }
    })();
    ratatui::restore();
    result
}

/// Runs one effect to completion and reports what happened.
fn execute(effect: Effect, ctx: &Context) -> Event {
    match effect {
        Effect::LoadCountries => {
            Event::Countries(NordApi::new().countries().map_err(|e| e.to_string()))
        }
        Effect::Refresh => Event::Status(probe(ctx)),
        Effect::Connect(cc) => Event::Finished(
            up::connect_real(ctx, cc)
                .map(drop)
                .map_err(|e| e.to_string()),
        ),
        Effect::Disconnect => Event::Finished(
            down::disconnect_real(ctx)
                .map(drop)
                .map_err(|e| e.to_string()),
        ),
    }
}

/// Reads tunnel state without sudo. A failing probe is a field value, not an error.
fn probe(ctx: &Context) -> TunnelStatus {
    let runner = SystemRunner;
    TunnelStatus {
        active: HelperClient::new(runner).active().unwrap_or_default(),
        exit_ip: NordApi::new().exit_ip().ok(),
        tailnet: TailscalePing::new(runner).tailnet().unwrap_or_else(|e| {
            TailnetHealth::Unreachable {
                reason: e.to_string(),
            }
        }),
        bypass_routes: ctx.recorded_routes(),
    }
}
