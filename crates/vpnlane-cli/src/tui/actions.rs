//! The action registry: one table of (action, label, direct key) that the
//! palette, the help overlay and the home keys all read, so they never drift.

use super::app::{Busy, Effect, Screen, State};
use super::palette::Palette;
use super::places;

/// Something the user can do from home or the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Disconnect,
    Recheck,
    Fastest,
    Presets,
    Browse,
    Udp,
    Reload,
    Help,
    Quit,
}

/// Every action: what the palette lists, its direct key, extra match words.
pub(crate) const ACTIONS: [(Action, &str, Option<char>, &str); 9] = [
    (Action::Disconnect, "disconnect", Some('d'), "down off stop"),
    (
        Action::Recheck,
        "recheck leak, tailnet and latency",
        Some('r'),
        "refresh status probe",
    ),
    (
        Action::Fastest,
        "connect to the fastest right now",
        Some('f'),
        "quick best ping",
    ),
    (Action::Presets, "preview presets", Some('p'), "preset diff"),
    (
        Action::Browse,
        "browse all servers",
        Some('b'),
        "tree list country",
    ),
    (
        Action::Udp,
        "check udp 51820 (doctor)",
        None,
        "doctor blocked network",
    ),
    (
        Action::Reload,
        "reload the server list",
        None,
        "countries refresh",
    ),
    (Action::Help, "show all keys", Some('?'), "help keys"),
    (Action::Quit, "quit", Some('q'), "exit close"),
];

impl Action {
    /// Label and direct key.
    pub(crate) fn entry(self) -> (&'static str, Option<char>, &'static str) {
        ACTIONS
            .iter()
            .find(|e| e.0 == self)
            .map_or(("", None, ""), |e| (e.1, e.2, e.3))
    }

    /// The action bound to `c` on the home screen.
    pub(crate) fn for_key(c: char) -> Option<Self> {
        ACTIONS.iter().find(|e| e.2 == Some(c)).map(|e| e.0)
    }

    /// Starts or changes the tunnel (refused while a change is running).
    pub(crate) fn changes_tunnel(self) -> bool {
        matches!(self, Self::Disconnect | Self::Fastest)
    }
}

/// Runs `a`.
pub(crate) fn run(s: &mut State, a: Action, fx: &mut Vec<Effect>) {
    if a.changes_tunnel() && s.busy.is_some() {
        s.note("busy: wait for the current change to finish");
        return;
    }
    match a {
        Action::Disconnect => {
            s.start(Busy::Disconnecting, Effect::Disconnect, fx);
        }
        Action::Recheck => {
            if !s.refreshing {
                s.refreshing = true;
                fx.push(Effect::Refresh);
            }
            if s.load_failed {
                s.load_failed = false;
                fx.push(Effect::LoadCountries);
            }
            // Lookups that failed get another try; measured ones a new probe round.
            let failed: Vec<_> = s
                .samples
                .iter()
                .filter(|(_, v)| **v == places::Sample::Failed)
                .map(|(k, _)| k.clone())
                .collect();
            for k in &failed {
                s.samples.remove(k);
            }
            places::reprobe(s, fx);
            places::request(s, failed, fx);
        }
        Action::Fastest => {
            let (best, _) = places::best_countries(s, 1);
            match best.first().filter(|k| places::latency(s, k).is_some()) {
                Some(k) => {
                    let k = k.clone();
                    s.connect_place(&k, fx);
                }
                None => s.note(if places::all_failed(s) && s.is_up() {
                    "f needs a measured latency: latency unavailable while connected"
                } else {
                    "f needs a measured latency: nothing has answered yet"
                }),
            }
        }
        Action::Presets => s.palette = Some(Palette::with_query("@")),
        Action::Browse => {
            s.screen = Screen::Browse;
            s.request_open_groups(fx);
        }
        Action::Udp => {
            s.udp_checking = true;
            fx.push(Effect::CheckUdp);
        }
        Action::Reload => {
            s.load_failed = false;
            fx.push(Effect::LoadCountries);
        }
        Action::Help => s.help = true,
        Action::Quit => s.should_quit = true,
    }
}
