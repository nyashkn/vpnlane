use std::path::PathBuf;

use crate::CountryCode;

/// Result alias used by every wglane library crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every failure a port or pure function can report.
///
/// Messages say what to do next, because the CLI shows them verbatim.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Scaffolded code path not written yet; the stub names its work item in a comment.
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),

    /// User input was not an ISO 3166-1 alpha-2 code.
    #[error("invalid country code {0:?}: use two letters, e.g. US, DE or ZA")]
    InvalidCountryCode(String),

    /// The provider does not serve this country.
    #[error("unknown country {}: run `{} servers` to list supported countries", .0, crate::brand::BIN)]
    UnknownCountry(CountryCode),

    /// The country has no city by this name.
    #[error("unknown city {query:?} in {country}; valid cities: {valid}")]
    UnknownCity {
        /// Country searched.
        country: CountryCode,
        /// What the user typed.
        query: String,
        /// Valid city names, comma separated.
        valid: String,
    },

    /// No server for this country speaks WireGuard right now.
    #[error("no WireGuard servers available for {0}")]
    NoServers(CountryCode),

    /// No token source produced a token.
    #[error("no access token found: {hint}")]
    TokenMissing {
        /// Where the user can put a token.
        hint: String,
    },

    /// Token file is readable by group or others.
    #[error("token file {} has mode {mode:o}; run `chmod 600 {}`", path.display(), path.display())]
    TokenInsecure {
        /// The offending file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },

    /// Token text failed validation.
    #[error("access token is malformed: {0}")]
    TokenMalformed(&'static str),

    /// The VPN provider's API failed or changed shape.
    #[error("provider API: {0}")]
    Api(String),

    /// An external program exited unsuccessfully.
    #[error("`{program}` failed ({status}): {stderr}")]
    Command {
        /// Program name, e.g. `wg-quick`.
        program: String,
        /// Exit status as text.
        status: String,
        /// Trimmed stderr.
        stderr: String,
    },

    /// The config file or a config environment variable is invalid.
    #[error("{0}")]
    Config(String),

    /// Filesystem or process I/O failed.
    #[error("{context}")]
    Io {
        /// What wglane was doing.
        context: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// Wraps an I/O error with what wglane was doing at the time.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
