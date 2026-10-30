use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use wglane_core::ports::TokenSource;
use wglane_core::{Error, Result, Token};

/// Environment variable checked first for the access token.
pub const TOKEN_ENV: &str = brand_env!("TOKEN");

/// Reads the token from an environment variable.
#[derive(Clone, Debug)]
pub struct EnvToken {
    var: String,
    // Injected so tests never mutate the process environment.
    lookup: fn(&str) -> Option<String>,
}

impl EnvToken {
    /// Reads [`TOKEN_ENV`] from the process environment.
    pub fn new() -> Self {
        Self::with_lookup(TOKEN_ENV, |k| std::env::var(k).ok())
    }

    /// Reads `var` via `lookup` (tests pass a fake environment).
    pub fn with_lookup(var: impl Into<String>, lookup: fn(&str) -> Option<String>) -> Self {
        Self {
            var: var.into(),
            lookup,
        }
    }
}

impl Default for EnvToken {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenSource for EnvToken {
    fn token(&self) -> Result<Option<Token>> {
        match (self.lookup)(&self.var) {
            Some(v) if !v.trim().is_empty() => Token::new(&v).map(Some),
            _ => Ok(None),
        }
    }

    fn describe(&self) -> String {
        format!("set {}", self.var)
    }
}

/// Reads the token from a file that only the owner can read (mode 0600).
#[derive(Clone, Debug)]
pub struct FileToken {
    path: PathBuf,
}

impl FileToken {
    /// Token file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<config_dir>/token`, e.g. `~/.config/wglane/token`.
    pub fn in_config_dir(config_dir: &std::path::Path) -> Self {
        Self::new(config_dir.join("token"))
    }
}

impl TokenSource for FileToken {
    fn token(&self) -> Result<Option<Token>> {
        let meta = match fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(format!("reading {}", self.path.display()), e)),
        };
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(Error::TokenInsecure {
                path: self.path.clone(),
                mode,
            });
        }
        // shortcut: does not check file owner == current user (needs getuid, i.e. libc); add with the helper's root checks.
        let raw = zeroize::Zeroizing::new(
            fs::read_to_string(&self.path)
                .map_err(|e| Error::io(format!("reading {}", self.path.display()), e))?,
        );
        Token::new(&raw).map(Some)
    }

    fn describe(&self) -> String {
        format!("write it to {} with mode 600", self.path.display())
    }
}

/// Tries `first`, then `second`. Errors stop the chain: a broken token
/// file is reported, not skipped.
#[derive(Clone, Debug)]
pub struct TokenChain<A, B> {
    first: A,
    second: B,
}

impl<A: TokenSource, B: TokenSource> TokenChain<A, B> {
    /// Chain of two sources.
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }

    /// Like [`TokenSource::token`] but `None` becomes `TokenMissing`.
    pub fn require(&self) -> Result<Token> {
        self.token()?.ok_or_else(|| Error::TokenMissing {
            hint: self.describe(),
        })
    }
}

impl<A: TokenSource, B: TokenSource> TokenSource for TokenChain<A, B> {
    fn token(&self) -> Result<Option<Token>> {
        match self.first.token()? {
            Some(t) => Ok(Some(t)),
            None => self.second.token(),
        }
    }

    fn describe(&self) -> String {
        format!("{} or {}", self.first.describe(), self.second.describe())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::Permissions;

    use wglane_core::testing::FakeToken;

    use super::*;

    fn write(dir: &tempfile::TempDir, body: &str, mode: u32) -> FileToken {
        let path = dir.path().join("token");
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, Permissions::from_mode(mode)).unwrap();
        FileToken::new(path)
    }

    #[test]
    fn env_set_unset_blank() {
        let set = EnvToken::with_lookup("X", |_| Some("abc123\n".into()));
        assert_eq!(set.token().unwrap().unwrap().expose(), "abc123");
        assert!(
            EnvToken::with_lookup("X", |_| None)
                .token()
                .unwrap()
                .is_none()
        );
        assert!(
            EnvToken::with_lookup("X", |_| Some("  ".into()))
                .token()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn env_malformed_is_error() {
        let bad = EnvToken::with_lookup("X", |_| Some("abc def".into()));
        assert!(matches!(bad.token(), Err(Error::TokenMalformed(_))));
    }

    #[test]
    fn file_0600_is_read_and_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let t = write(&dir, "deadbeef\n", 0o600).token().unwrap().unwrap();
        assert_eq!(t.expose(), "deadbeef");
    }

    #[test]
    fn file_0400_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write(&dir, "deadbeef", 0o400).token().unwrap().is_some());
    }

    #[test]
    fn file_group_or_world_readable_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for mode in [0o640, 0o604, 0o644] {
            let err = write(&dir, "deadbeef", mode).token().unwrap_err();
            assert!(
                matches!(err, Error::TokenInsecure { mode: m, .. } if m == mode),
                "mode {mode:o}: {err}"
            );
            assert!(err.to_string().contains("chmod 600"));
        }
    }

    #[test]
    fn file_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            FileToken::in_config_dir(dir.path())
                .token()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn chain_prefers_first_and_falls_back() {
        let a = FakeToken(Some("aaa".into()));
        let b = FakeToken(Some("bbb".into()));
        assert_eq!(
            TokenChain::new(a, b.clone()).require().unwrap().expose(),
            "aaa"
        );
        assert_eq!(
            TokenChain::new(FakeToken(None), b)
                .require()
                .unwrap()
                .expose(),
            "bbb"
        );
    }

    #[test]
    fn chain_missing_names_every_source() {
        let dir = tempfile::tempdir().unwrap();
        let chain = TokenChain::new(
            EnvToken::with_lookup(TOKEN_ENV, |_| None),
            FileToken::in_config_dir(dir.path()),
        );
        let msg = chain.require().unwrap_err().to_string();
        assert!(
            msg.contains("WGLANE_TOKEN") && msg.contains("token with mode 600"),
            "{msg}"
        );
    }

    #[test]
    fn chain_stops_on_broken_first_source() {
        let dir = tempfile::tempdir().unwrap();
        let chain = TokenChain::new(write(&dir, "x", 0o644), FakeToken(Some("bbb".into())));
        assert!(chain.require().is_err());
    }
}
