//! Hand-written fakes for every port. Enabled by the `testing` feature.
//!
//! Fakes hold canned answers in public fields and record calls in `RefCell`s,
//! so a test can arrange, act, then assert on what the code under test did.

mod bypass_provider;
mod health_probe;
mod network_watcher;
mod route_bypass;
mod server_catalog;
mod token_source;
mod tunnel;

pub use bypass_provider::FakeBypassProvider;
pub use health_probe::{FakeExitIp, FakeTailnet};
pub use network_watcher::FakeWatcher;
pub use route_bypass::FakeRoutes;
pub use server_catalog::FakeCatalog;
pub use token_source::FakeToken;
pub use tunnel::FakeTunnel;
