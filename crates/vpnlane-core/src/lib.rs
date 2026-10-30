//! Domain types, port traits and pure logic for vpnlane.
//!
//! This crate does no I/O: adapters in `vpnlane-nord` and `vpnlane-macos`
//! implement the traits in [`ports`], and the binary wires them together.

#[macro_use]
pub mod brand;
mod bypass;
mod config;
mod country;
mod error;
mod fastest;
mod generate;
mod location;
pub mod ports;
mod preset;
mod rank;
mod render;
mod secret;
mod server;
mod stale;
mod state;
mod status;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tunnel_config;

pub use bypass::BypassSet;
pub use config::{
    DEFAULT_GRAPH_SAMPLES, DEFAULT_WARN_RATIO, Deprecation, Effective, FileConfig, MAX_BYPASS,
    Overrides, Pick, SAMPLE_CONFIG, Setting, Source, TuiSettings, deprecation_line,
};
pub use country::{City, Country, CountryCode, fold};
pub use error::{Error, Result};
pub use fastest::{Choice, FASTEST_CANDIDATES, PROBE_TIMEOUT, pick_fastest};
pub use generate::{generate_config, generate_config_by};
pub use location::Location;
pub use preset::{
    ActiveState, ChangeLine, EffectiveRequest, KEYED_PRESETS, MAX_PRESET_LABEL, MAX_PRESET_NAME,
    NamedPreset, Preset, ResolvedPreset, describe_change,
};
pub use rank::rank_by_load;
pub use render::render_wg_conf;
pub use secret::{PrivateKey, PublicKey, Token};
pub use server::Server;
pub use stale::{MAX_CONF_AGE, is_stale};
pub use state::{ConfNote, LastRequest, PresetUse, STATE_VERSION, State};
pub use status::{ExitInsights, TailnetHealth, TunnelStatus};
pub use tunnel_config::TunnelConfig;
