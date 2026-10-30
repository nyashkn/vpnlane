//! ratatui dashboard. `app.rs` holds state, the pure reducer and drawing
//! (testable with `TestBackend`); this file owns the terminal, the event loop
//! and the effects that touch the system.

mod app;
mod continent;

use std::collections::VecDeque;
use std::time::Duration;

use color_eyre::Result;
use ratatui::crossterm::event::{self, Event as Term, KeyEventKind};
use wglane_core::Overrides;
use wglane_core::ports::ServerCatalog;
use wglane_macos::{HelperClient, SystemRunner, TailscalePing};
use wglane_nord::NordApi;

use crate::cmd::{down, status, up};
use crate::context::Context;
use app::{Effect, Event, State};

/// How often the status card refreshes.
const TICK: Duration = Duration::from_secs(2);

/// Runs the dashboard until the user quits.
pub(crate) fn run(ctx: &Context) -> Result<()> {
    let settings = ctx.settings(Overrides::default())?;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        // https://no-color.org: any non-empty value. Handled here, not by
        // crossterm: under NO_COLOR it turns each color change into an empty
        // SGR (a full reset), which drops bold and reverse mid-row.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let mut state = State::with_settings(&settings.tui);
        state.no_color = no_color;
        let exit_ip_check = settings.exit_ip_check.value;
        let mut effects: VecDeque<Effect> = app::init().into();
        loop {
            // Draw before blocking effects so "connecting..." is on screen.
            terminal.draw(|f| app::draw(&state, f))?;
            if state.should_quit {
                return Ok(());
            }
            let event = if let Some(effect) = effects.pop_front() {
                execute(effect, ctx, exit_ip_check)
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
fn execute(effect: Effect, ctx: &Context, exit_ip_check: bool) -> Event {
    match effect {
        Effect::LoadCountries => {
            Event::Countries(NordApi::new().countries().map_err(|e| e.to_string()))
        }
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
            Event::Status(st, known, city)
        }
        Effect::Connect(cc, city) => Event::Finished(
            up::connect_real(ctx, cc, city.as_ref())
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
