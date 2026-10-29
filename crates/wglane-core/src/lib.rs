//! Domain types, port traits and pure logic for wglane.
//!
//! This crate does no I/O: adapters in `wglane-nord` and `wglane-macos`
//! implement the traits in [`ports`], and the binary wires them together.

mod bypass;
mod country;
mod error;
mod generate;
pub mod ports;
mod rank;
mod render;
mod secret;
mod server;
mod stale;
mod status;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tunnel_config;

pub use bypass::BypassSet;
pub use country::{Country, CountryCode};
pub use error::{Error, Result};
pub use generate::generate_config;
pub use rank::rank_by_load;
pub use render::render_wg_conf;
pub use secret::{PrivateKey, PublicKey, Token};
pub use server::Server;
pub use stale::{MAX_CONF_AGE, is_stale};
pub use status::{TailnetHealth, TunnelStatus};
pub use tunnel_config::TunnelConfig;
