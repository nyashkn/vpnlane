//! Shared, resolved settings handed to every subcommand.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};
use wglane_core::{Effective, FileConfig, Overrides};
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
    pub(crate) fn settings(&self, flags: Overrides) -> Result<Effective> {
        let env = Overrides::from_env(|k| std::env::var(k).ok())?;
        Ok(self.file_config()?.resolve(flags, env))
    }

    /// `bypass_extra` from the config file, as a bypass provider.
    pub(crate) fn static_bypass(&self) -> Result<StaticBypass> {
        let extra = self.settings(Overrides::default())?.bypass_extra.value;
        Ok(StaticBypass(extra.into_iter().collect()))
    }

    fn routes_record(&self) -> PathBuf {
        self.config_dir.join("bypass-routes")
    }

    /// Bypass routes the helper last installed, as recorded by `up`. The
    /// helper's own `bypass.json` sits in a root-only dir, so status keeps
    /// this copy of the count instead. 0 when nothing is recorded.
    pub(crate) fn recorded_routes(&self) -> usize {
        fs::read_to_string(self.routes_record())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    pub(crate) fn record_routes(&self, n: usize) -> Result<()> {
        Ok(fs::write(self.routes_record(), format!("{n}\n"))?)
    }

    fn city_record(&self, tunnel: &str) -> PathBuf {
        self.config_dir.join(format!("{tunnel}.city"))
    }

    fn city_lines(&self, tunnel: &str) -> (Option<String>, Option<String>) {
        let text = fs::read_to_string(self.city_record(tunnel)).unwrap_or_default();
        let mut lines = text
            .lines()
            .map(|l| Some(l.trim().to_owned()).filter(|l| !l.is_empty()));
        (lines.next().flatten(), lines.next().flatten())
    }

    /// City of the server the `<tunnel>.conf` (e.g. `us`) points at, as noted
    /// when `up` generated it. `None` when unknown (config made by `gen`, an
    /// older wglane, or the provider gave no city).
    #[cfg(any(feature = "tui", test))]
    pub(crate) fn recorded_city(&self, tunnel: &str) -> Option<String> {
        self.city_lines(tunnel).1
    }

    /// The city `up` was asked for when it generated `<tunnel>.conf`; `None`
    /// for a whole-country request, or when nothing is recorded.
    pub(crate) fn recorded_request(&self, tunnel: &str) -> Option<String> {
        self.city_lines(tunnel).0
    }

    /// Notes the requested and the picked city of a freshly generated config
    /// (both `None` removes the note).
    pub(crate) fn record_city(
        &self,
        tunnel: &str,
        requested: Option<&str>,
        picked: Option<&str>,
    ) -> Result<()> {
        if requested.is_none() && picked.is_none() {
            return match fs::remove_file(self.city_record(tunnel)) {
                Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            };
        }
        let body = format!("{}\n{}\n", requested.unwrap_or(""), picked.unwrap_or(""));
        Ok(fs::write(self.city_record(tunnel), body)?)
    }

    fn preset_record(&self) -> PathBuf {
        self.config_dir.join("active-preset")
    }

    /// Name of the preset the current connection was last applied from, as
    /// noted by `up`/`preset apply` (user-level state beside `<cc>.city`).
    /// `None` when none. It does not check that the tunnel is still up or the
    /// preset still exists; see `cmd::preset::active_name` for that.
    pub(crate) fn recorded_preset(&self) -> Option<String> {
        let text = fs::read_to_string(self.preset_record()).ok()?;
        Some(text.trim().to_owned()).filter(|n| !n.is_empty())
    }

    /// Notes the active preset; `None` clears the note.
    pub(crate) fn record_preset(&self, name: Option<&str>) -> Result<()> {
        match name {
            Some(n) => Ok(fs::write(self.preset_record(), format!("{n}\n"))?),
            None => match fs::remove_file(self.preset_record()) {
                Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
        }
    }

    pub(crate) fn clear_routes_record(&self) -> Result<()> {
        match fs::remove_file(self.routes_record()) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

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
        c.clear_routes_record().unwrap();
        c.clear_routes_record().unwrap();
        assert_eq!(c.recorded_routes(), 0);
    }

    #[test]
    fn preset_record_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        assert_eq!(c.recorded_preset(), None);
        c.record_preset(Some("work")).unwrap();
        assert_eq!(c.recorded_preset().as_deref(), Some("work"));
        c.record_preset(None).unwrap();
        c.record_preset(None).unwrap();
        assert_eq!(c.recorded_preset(), None);
    }

    #[test]
    fn city_record_round_trips() {
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
}
