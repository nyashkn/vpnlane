//! Shared, resolved settings handed to every subcommand.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use color_eyre::eyre::{Result, eyre};
use wglane_core::{
    ConfNote, Effective, FileConfig, LastRequest, Overrides, PresetUse, STATE_VERSION, State,
    deprecation_line,
};
use wglane_macos::{ConfStore, StaticBypass};
use wglane_nord::{EnvToken, FileToken, TokenChain};

use crate::cli::GlobalArgs;

/// Resolved global flags plus constructors for the adapters most commands need.
#[derive(Clone, Debug)]
pub(crate) struct Context {
    /// `--json` was passed.
    pub(crate) json: bool,
    /// `-v` count.
    #[allow(
        dead_code,
        reason = "logging is set up from the flag in lib.rs, not read from Context"
    )]
    pub(crate) verbose: u8,
    /// Config directory (token, generated configs).
    pub(crate) config_dir: PathBuf,
}

impl Context {
    pub(crate) fn from_global(g: &GlobalArgs) -> Result<Self> {
        let config_dir = if let Some(d) = &g.config_dir {
            d.clone()
        } else {
            let home = std::env::var_os("HOME").ok_or_else(|| eyre!("HOME is not set"))?;
            PathBuf::from(home).join(".config").join("wglane")
        };
        Ok(Self {
            json: g.json,
            verbose: g.verbose,
            config_dir,
        })
    }

    /// Env var first, then `<config_dir>/token`. Use `.require()` to get a token.
    pub(crate) fn token_source(&self) -> TokenChain<EnvToken, FileToken> {
        TokenChain::new(EnvToken::new(), FileToken::in_config_dir(&self.config_dir))
    }

    /// Generated tunnel configs in the config dir.
    pub(crate) fn conf_store(&self) -> ConfStore {
        ConfStore::new(&self.config_dir)
    }

    /// `<config_dir>/config.toml`.
    pub(crate) fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// The config file; a missing file is the defaults, never an error.
    pub(crate) fn file_config(&self) -> Result<FileConfig> {
        let path = self.config_file();
        match fs::read_to_string(&path) {
            Ok(text) => FileConfig::parse(&text).map_err(|e| eyre!("{}: {e}", path.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(FileConfig::default()),
            Err(e) => Err(eyre!("cannot read {}: {e}", path.display())),
        }
    }

    /// Settings after precedence: `flags` > environment > file > default.
    /// Prints the one-line deprecation notice the first time old spellings are seen.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "callers (the TUI included) pass a temporary; a reference would only add a borrow to each"
    )]
    pub(crate) fn settings(&self, flags: Overrides) -> Result<Effective> {
        let env = Overrides::from_env(|k| std::env::var(k).ok())?;
        let eff = self.file_config()?.resolve(&flags, env);
        if let Some(line) = deprecation_line(&eff.deprecated)
            && !DEPRECATION_SHOWN.swap(true, Ordering::Relaxed)
        {
            eprintln!("{line}");
        }
        Ok(eff)
    }

    /// The bypass set to re-apply to a live tunnel: the one `up` last applied
    /// (base plus preset, as resolved then), because the helper replaces the
    /// whole set. With nothing recorded, the top-level `bypass_extra`.
    pub(crate) fn applied_bypass(&self) -> Result<StaticBypass> {
        let extra = match self.last_request() {
            Some(last) => last.bypass_extra,
            None => self.settings(Overrides::default())?.bypass_extra.value,
        };
        Ok(StaticBypass(extra.into_iter().collect()))
    }

    fn state_file(&self) -> PathBuf {
        self.config_dir.join("state.json")
    }

    /// Everything wglane remembers (`state.json`), best effort: an unreadable
    /// file reads as empty, with a warning.
    pub(crate) fn state(&self) -> State {
        self.load_state().unwrap_or_else(|e| {
            tracing::warn!("{e}");
            State::default()
        })
    }

    /// Reads `state.json`. A missing file is the old per-file records (migrated
    /// once, then deleted) or empty; a corrupt one is empty; a file from a
    /// newer schema is an error, so it is never overwritten.
    fn load_state(&self) -> Result<State> {
        let path = self.state_file();
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<State>(&text) {
                Ok(s) if s.version == STATE_VERSION => Ok(s),
                Ok(s) => Err(eyre!(
                    "{} is version {}, this wglane reads version {STATE_VERSION}: upgrade wglane or delete the file",
                    path.display(),
                    s.version
                )),
                Err(e) => {
                    tracing::warn!("ignoring unreadable {}: {e}", path.display());
                    Ok(State::default())
                }
            },
            Err(e) if e.kind() == ErrorKind::NotFound => self.migrate_legacy(),
            Err(e) => Err(eyre!("cannot read {}: {e}", path.display())),
        }
    }

    fn save_state(&self, state: &State) -> Result<()> {
        let tmp = self.config_dir.join("state.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(state)? + "\n")?;
        Ok(fs::rename(&tmp, self.state_file())?)
    }

    fn update_state(&self, change: impl FnOnce(&mut State)) -> Result<()> {
        let mut state = self.load_state()?;
        change(&mut state);
        self.save_state(&state)
    }

    /// One-time import of the per-file records older releases kept
    /// (`active-preset`, `bypass-routes`, `<cc>.city`) into `state.json`;
    /// the old files are deleted once the new one is written.
    fn migrate_legacy(&self) -> Result<State> {
        let read = |name: &str| fs::read_to_string(self.config_dir.join(name)).ok();
        let mut state = State::default();
        let mut old: Vec<PathBuf> = Vec::new();
        let preset = read("active-preset")
            .map(|t| t.trim().to_owned())
            .filter(|n| !n.is_empty());
        if read("active-preset").is_some() {
            old.push(self.config_dir.join("active-preset"));
        }
        if let Some(n) = read("bypass-routes") {
            state.bypass_routes = n.trim().parse().unwrap_or(0);
            old.push(self.config_dir.join("bypass-routes"));
        }
        for entry in fs::read_dir(&self.config_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            let tunnel = path.extension().is_some_and(|e| e == "city");
            if let (true, Some(stem)) = (tunnel, path.file_stem().and_then(|s| s.to_str())) {
                let text = fs::read_to_string(&path).unwrap_or_default();
                let mut lines = text
                    .lines()
                    .map(|l| Some(l.trim().to_owned()).filter(|l| !l.is_empty()));
                let (requested, picked) = (lines.next().flatten(), lines.next().flatten());
                state
                    .confs
                    .insert(stem.to_owned(), ConfNote { requested, picked });
                old.push(path);
            }
        }
        if old.is_empty() {
            return Ok(state);
        }
        // The old active preset becomes the last request when its place is known.
        state.connected = preset.is_some() || state.bypass_routes > 0;
        if let (Some(name), Ok(file)) = (preset, self.file_config()) {
            let eff = file.resolve(&Overrides::default(), Overrides::default());
            let req = eff.overlay(eff.preset(&name));
            if let Some(location) = req.location.filter(|_| eff.preset(&name).is_some()) {
                state.last = Some(LastRequest {
                    location,
                    pick: req.pick,
                    bypass_extra: req.bypass_extra,
                    preset: Some(PresetUse {
                        name,
                        overridden: Vec::new(),
                    }),
                    at: unix_now(),
                });
            }
        }
        self.save_state(&state)?;
        for path in old {
            let _ = fs::remove_file(path);
        }
        Ok(state)
    }

    /// The last request `up` applied; stays after `down` (it is what `up -` repeats).
    pub(crate) fn last_request(&self) -> Option<LastRequest> {
        self.state().last
    }

    /// The preset the live connection was applied from, with what was
    /// overridden. `None` when no preset was used or `down` has run.
    pub(crate) fn active_preset(&self) -> Option<PresetUse> {
        let s = self.state();
        s.last.filter(|_| s.connected)?.preset
    }

    /// Notes a successful connect.
    pub(crate) fn record_applied(&self, request: LastRequest) -> Result<()> {
        self.update_state(|s| {
            s.connected = true;
            s.last = Some(request);
        })
    }

    /// Notes `down`: the active preset and the route count go, the last request stays.
    pub(crate) fn record_down(&self) -> Result<()> {
        self.update_state(|s| {
            s.connected = false;
            s.bypass_routes = 0;
        })
    }

    /// Bypass routes the helper last installed, as recorded by `up`. The
    /// helper's own `bypass.json` sits in a root-only dir, so status keeps
    /// this copy of the count instead. 0 when nothing is recorded.
    pub(crate) fn recorded_routes(&self) -> usize {
        self.state().bypass_routes
    }

    pub(crate) fn record_routes(&self, n: usize) -> Result<()> {
        self.update_state(|s| s.bypass_routes = n)
    }

    /// City of the server the `<tunnel>.conf` (e.g. `us`) points at, as noted
    /// when `up` generated it. `None` when unknown (config made by `gen`, an
    /// older wglane, or the provider gave no city).
    #[cfg(any(feature = "tui", test))]
    pub(crate) fn recorded_city(&self, tunnel: &str) -> Option<String> {
        self.state().confs.remove(tunnel)?.picked
    }

    /// The city `up` was asked for when it generated `<tunnel>.conf`; `None`
    /// for a whole-country request, or when nothing is recorded.
    pub(crate) fn recorded_request(&self, tunnel: &str) -> Option<String> {
        self.state().confs.remove(tunnel)?.requested
    }

    /// Notes the requested and the picked city of a freshly generated config
    /// (both `None` removes the note).
    pub(crate) fn record_city(
        &self,
        tunnel: &str,
        requested: Option<&str>,
        picked: Option<&str>,
    ) -> Result<()> {
        self.update_state(|s| {
            if requested.is_none() && picked.is_none() {
                s.confs.remove(tunnel);
            } else {
                s.confs.insert(
                    tunnel.to_owned(),
                    ConfNote {
                        requested: requested.map(Into::into),
                        picked: picked.map(Into::into),
                    },
                );
            }
        })
    }
}

/// Seconds since the Unix epoch (0 if the clock is before it).
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The deprecation notice is printed once per run, however often settings are read.
static DEPRECATION_SHOWN: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &std::path::Path) -> Context {
        Context {
            json: false,
            verbose: 0,
            config_dir: dir.to_owned(),
        }
    }

    #[test]
    fn missing_config_file_is_defaults_and_bad_one_names_path() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        assert_eq!(c.file_config().unwrap(), FileConfig::default());
        fs::write(c.config_file(), "nope = 1\n").unwrap();
        let e = c.file_config().unwrap_err().to_string();
        assert!(
            e.contains("config.toml") && e.contains("unknown field `nope`"),
            "{e}"
        );
    }

    #[test]
    fn routes_record_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        assert_eq!(c.recorded_routes(), 0);
        c.record_routes(7).unwrap();
        assert_eq!(c.recorded_routes(), 7);
        c.record_down().unwrap();
        c.record_down().unwrap();
        assert_eq!(c.recorded_routes(), 0);
    }

    fn last(location: &str, preset: Option<&str>) -> LastRequest {
        LastRequest {
            location: location.parse().unwrap(),
            pick: wglane_core::Pick::LeastLoad,
            bypass_extra: vec!["203.0.113.0/24".parse().unwrap()],
            preset: preset.map(|n| PresetUse {
                name: n.into(),
                overridden: vec![],
            }),
            at: 1,
        }
    }

    #[test]
    fn state_round_trips_and_down_keeps_the_last_request() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        assert_eq!((c.last_request(), c.active_preset()), (None, None));
        c.record_applied(last("DE/Berlin", Some("work"))).unwrap();
        assert_eq!(c.active_preset().unwrap().name, "work");
        // A fresh Context reads the same file.
        assert_eq!(
            ctx(dir.path()).last_request().unwrap().location.to_string(),
            "DE/Berlin"
        );
        c.record_down().unwrap();
        c.record_down().unwrap();
        assert_eq!(c.active_preset(), None, "down clears the active preset");
        assert!(c.last_request().is_some(), "but `up -` still has a target");
        assert!(dir.path().join("state.json").exists());
    }

    #[test]
    fn a_newer_or_corrupt_state_file_is_never_clobbered_or_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        fs::write(c.state_file(), "not json").unwrap();
        assert_eq!(c.state(), State::default());
        c.record_routes(2).unwrap();
        assert_eq!(c.recorded_routes(), 2);
        let future = "{\"version\": 99}";
        fs::write(c.state_file(), future).unwrap();
        assert!(c.record_routes(3).is_err());
        assert_eq!(fs::read_to_string(c.state_file()).unwrap(), future);
    }

    #[test]
    fn conf_notes_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        assert_eq!(c.recorded_city("us"), None);
        c.record_city("us", None, Some("Boston")).unwrap();
        assert_eq!(c.recorded_city("us").as_deref(), Some("Boston"));
        assert_eq!(c.recorded_request("us"), None);
        c.record_city("us", Some("New York"), Some("New York"))
            .unwrap();
        assert_eq!(c.recorded_request("us").as_deref(), Some("New York"));
        c.record_city("us", None, None).unwrap();
        c.record_city("us", None, None).unwrap();
        assert_eq!(
            (c.recorded_city("us"), c.recorded_request("us")),
            (None, None)
        );
    }

    #[test]
    fn old_record_files_are_read_once_then_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        let p = |n: &str| dir.path().join(n);
        fs::write(
            c.config_file(),
            "bypass_extra = [\"203.0.113.0/24\"]\n[presets.work]\nlocation = \"DE/Berlin\"\n",
        )
        .unwrap();
        fs::write(p("active-preset"), "work\n").unwrap();
        fs::write(p("bypass-routes"), "4\n").unwrap();
        fs::write(p("de.city"), "Berlin\nBerlin\n").unwrap();
        fs::write(p("us.city"), "\nBoston\n").unwrap();
        fs::write(p("de.conf"), "keep me").unwrap();

        // Any read triggers the import.
        assert_eq!(c.recorded_routes(), 4);
        assert_eq!(c.recorded_request("de").as_deref(), Some("Berlin"));
        assert_eq!(
            (
                c.recorded_request("us"),
                c.recorded_city("us").as_deref().map(str::to_owned)
            ),
            (None, Some("Boston".into()))
        );
        let active = c.active_preset().unwrap();
        assert_eq!(active.name, "work");
        let last = c.last_request().unwrap();
        assert_eq!(last.location.to_string(), "DE/Berlin");
        assert_eq!(last.bypass_extra.len(), 1);

        for gone in ["active-preset", "bypass-routes", "de.city", "us.city"] {
            assert!(!p(gone).exists(), "{gone} should be deleted");
        }
        assert!(p("de.conf").exists(), "generated configs are not state");
        assert!(p("state.json").exists());
        // Second read comes from state.json, not the (now missing) old files.
        assert_eq!(ctx(dir.path()).recorded_routes(), 4);
    }

    #[test]
    fn applied_bypass_is_the_last_requests_set() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        fs::write(c.config_file(), "bypass_extra = [\"198.51.100.0/24\"]\n").unwrap();
        // Nothing applied yet: the top-level set.
        assert_eq!(c.applied_bypass().unwrap().0.len(), 1);
        let mut l = last("DE", None);
        l.bypass_extra.push("203.0.113.9/32".parse().unwrap());
        c.record_applied(l).unwrap();
        let set = c.applied_bypass().unwrap().0;
        assert_eq!(set.len(), 2);
        assert!(set.iter().all(|n| n.to_string().starts_with("203.0.113")));
    }
}
