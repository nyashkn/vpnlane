//! Shared, resolved settings handed to every subcommand.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};
use wglane_macos::ConfStore;
use wglane_nord::{EnvToken, FileToken, TokenChain};

use crate::cli::GlobalArgs;

/// Resolved global flags plus constructors for the adapters most commands need.
#[derive(Debug)]
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
}
