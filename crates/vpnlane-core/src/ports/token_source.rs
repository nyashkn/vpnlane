use crate::{Result, Token};

/// Somewhere an access token may live (env var, config file).
///
/// Tokens are never accepted on argv: they would leak via `ps` and shell history.
pub trait TokenSource {
    /// `Ok(None)` when this source is simply not configured; `Err` when it is
    /// configured but unusable (bad permissions, malformed). Callers chain
    /// sources and stop at the first `Some`.
    fn token(&self) -> Result<Option<Token>>;

    /// Human hint for `TokenMissing`, e.g. "set `WGLANE_TOKEN`".
    fn describe(&self) -> String;
}
