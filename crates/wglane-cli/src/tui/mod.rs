//! ratatui dashboard. `app.rs` holds state and drawing (testable with
//! `TestBackend`); this file owns the terminal and the event loop.

mod app;

use color_eyre::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use app::App;

/// Runs the dashboard until the user quits.
pub(crate) fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::default();
    let result = (|| -> Result<()> {
        while !app.should_quit {
            terminal.draw(|f| app.draw(f))?;
            // TODO(bead): tui-events: poll with timeout and refresh status on tick.
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                app.on_key(key.code);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
