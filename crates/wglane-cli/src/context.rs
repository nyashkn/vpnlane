//! Shared, resolved settings handed to every subcommand.

use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};
use wglane_macos::ConfStore;
use wglane_nord::{EnvToken, FileToken, TokenChain};

use crate::cli::GlobalArgs;

/// Resolved global flags plus constructors for the adapters most commands need.
#[derive(Debug)]
#[allow(dead_code, reason = "fields and helpers are read by the cmd beads")]
pub(crate) struct Context {
    /// `--json` was passed.
    pub(crate) json: bool,
    /// `-v` count.
    pub(crate) verbose: u8,
    /// Config directory (token, generated configs).
    pub(crate) config_dir: PathBuf,
}

#[allow(dead_code, reason = "helpers are called by the cmd beads")]
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
}
