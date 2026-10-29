//! Port traits. Adapters implement these; nothing here does I/O.
//!
//! All ports are blocking and take `&self` (or `&mut self` for streams), so
//! callers stay generic (`fn f<C: ServerCatalog>(c: &C)`) and tests pass fakes
//! from [`crate::testing`].

mod bypass_provider;
mod health_probe;
mod network_watcher;
mod route_bypass;
mod server_catalog;
mod token_source;
mod tunnel;

pub use bypass_provider::BypassProvider;
pub use health_probe::{ExitIpProbe, TailnetProbe};
pub use network_watcher::NetworkWatcher;
pub use route_bypass::RouteBypass;
pub use server_catalog::ServerCatalog;
pub use token_source::TokenSource;
pub use tunnel::Tunnel;
