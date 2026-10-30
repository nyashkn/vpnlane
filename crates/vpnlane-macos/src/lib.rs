//! macOS adapters for the [`vpnlane_core::ports`].
//!
//! Every external program runs through [`CommandRunner`], so tests use
//! [`testing::FakeRunner`] and never touch the real network stack.
//!
//! Privilege split: [`WgGo`] and [`RouteTable`] run as root inside the
//! `vpnlane-helper` binary; the unprivileged CLI uses [`HelperClient`], which
//! implements the same ports by calling `sudo -n vpnlane-helper <verb>`.

#[macro_use]
extern crate vpnlane_core;

mod bypass;
mod command;
mod conf_store;
mod helper;
mod ping_probe;
mod privilege;
mod probe;
mod routes;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tunnel;
mod udp_probe;
mod watcher;

pub use bypass::{StaticBypass, TailscaleBypass};
pub use command::{CommandOutput, CommandRunner, SystemRunner};
pub use conf_store::ConfStore;
pub use helper::{HELPER_PATH, HelperClient, HelperVerb, STATE_DIR, TOOL_DIR, helper_main};
pub use ping_probe::{PingProbe, physical_interface};
pub use privilege::{
    SUDOERS_PATH, check_tool_dir, install_helper, render_sudoers_rule, uninstall_helper,
};
pub use probe::TailscalePing;
pub use routes::RouteTable;
pub use tunnel::WgGo;
pub use udp_probe::UdpSend;
pub use watcher::RouteMonitor;
