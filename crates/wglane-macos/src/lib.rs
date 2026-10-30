//! macOS adapters for the [`wglane_core::ports`].
//!
//! Every external program runs through [`CommandRunner`], so tests use
//! [`testing::FakeRunner`] and never touch the real network stack.
//!
//! Privilege split: [`WgQuick`] and [`RouteTable`] run as root inside the
//! `wglane-helper` binary; the unprivileged CLI uses [`HelperClient`], which
//! implements the same ports by calling `sudo -n wglane-helper <verb>`.

mod bypass;
mod command;
mod conf_store;
mod helper;
mod privilege;
mod probe;
mod routes;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tunnel;
mod watcher;

pub use bypass::{StaticBypass, TailscaleBypass};
pub use command::{CommandOutput, CommandRunner, SystemRunner};
pub use conf_store::ConfStore;
pub use helper::{HELPER_PATH, HelperClient, HelperVerb, STATE_DIR, TOOL_DIR, helper_main};
pub use privilege::{SUDOERS_PATH, install_helper, render_sudoers_rule, uninstall_helper};
pub use probe::TailscalePing;
pub use routes::RouteTable;
pub use tunnel::WgQuick;
pub use watcher::RouteMonitor;
