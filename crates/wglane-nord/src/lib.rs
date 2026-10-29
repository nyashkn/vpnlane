//! Nord adapter: implements [`wglane_core::ports::ServerCatalog`] and
//! [`wglane_core::ports::ExitIpProbe`] over Nord's undocumented public API,
//! plus the token sources.

mod api;
mod token;
#[allow(
    dead_code,
    reason = "parsers are called by api.rs once the nord-api beads land"
)]
mod wire;

pub use api::NordApi;
pub use token::{EnvToken, FileToken, TOKEN_ENV, TokenChain};
