use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::widgets::{Block, Paragraph};

/// Dashboard state.
#[derive(Debug)]
pub(crate) struct App {
    pub(crate) should_quit: bool,
    /// Bottom-line message (errors, key hints).
    pub(crate) message: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_quit: false,
            message: "dashboard not built yet. Press q to quit.".into(),
        }
    }
}

impl App {
    pub(crate) fn on_key(&mut self, key: KeyCode) {
        // TODO(bead): tui-actions: arrows select server, Enter connects, d disconnects.
        if matches!(key, KeyCode::Char('q') | KeyCode::Esc) {
            self.should_quit = true;
        }
    }

    pub(crate) fn draw(&self, frame: &mut Frame) {
        // TODO(bead): tui-layout: server list (left), stats card with latency
        // Sparkline and exit IP (right), status line with key hints (bottom).
        let body = Paragraph::new(self.message.as_str()).block(Block::bordered().title(" wglane "));
        frame.render_widget(body, frame.area());
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn q_quits() {
        let mut app = App::default();
        app.on_key(KeyCode::Char('x'));
        assert!(!app.should_quit);
        app.on_key(KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn draws_title() {
        let mut t = Terminal::new(TestBackend::new(60, 5)).unwrap();
        t.draw(|f| App::default().draw(f)).unwrap();
        insta::assert_snapshot!(t.backend());
    }
}
