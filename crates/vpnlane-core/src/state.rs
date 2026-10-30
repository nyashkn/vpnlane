//! Runtime state: what vpnlane last applied, kept apart from `config.toml`.
//!
//! The file is machine-written and lives next to the config as `state.json`;
//! the user's `config.toml` is never touched. This module is the schema only;
//! the CLI reads and writes the bytes.

use std::collections::BTreeMap;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::{Location, Pick};

/// Schema version written to `state.json`. A file with another version is
/// not read (and not overwritten blindly) by this release.
pub const STATE_VERSION: u32 = 1;

/// Everything vpnlane remembers between runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Schema version, [`STATE_VERSION`].
    pub version: u32,
    /// A tunnel was brought up and `down` has not run since. `down` clears
    /// the active preset but keeps [`State::last`], so `up -` still works.
    #[serde(default)]
    pub connected: bool,
    /// The last request that was applied.
    #[serde(default)]
    pub last: Option<LastRequest>,
    /// Bypass routes the helper last installed (the helper's own record is root-only).
    #[serde(default)]
    pub bypass_routes: usize,
    /// Per tunnel (lower-case country code): how its cached `.conf` was made.
    #[serde(default)]
    pub confs: BTreeMap<String, ConfNote>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            connected: false,
            last: None,
            bypass_routes: 0,
            confs: BTreeMap::new(),
        }
    }
}

/// A request as it was applied: the resolved values, not the layers they came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRequest {
    /// Where; `None` only for a request migrated from an older release that
    /// never recorded it (the bypass set and preset are still real).
    #[serde(default)]
    pub location: Option<Location>,
    /// How the server was chosen.
    pub pick: Pick,
    /// Extra bypass CIDRs applied (base plus preset), the set `watch` re-applies.
    pub bypass_extra: Vec<IpNet>,
    /// The preset it was applied from, if any.
    #[serde(default)]
    pub preset: Option<PresetUse>,
    /// When, in seconds since the Unix epoch.
    pub at: u64,
}

/// A preset in use, and which of its values were overridden afterwards.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetUse {
    /// The preset's name.
    pub name: String,
    /// Keys whose value differs from the preset's: `"location"`, `"pick"`.
    #[serde(default)]
    pub overridden: Vec<String>,
}

impl PresetUse {
    /// `work`, or `work (location overridden)`.
    pub fn describe(&self) -> String {
        if self.overridden.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({} overridden)", self.name, self.overridden.join(", "))
        }
    }
}

/// How a cached tunnel config was generated.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfNote {
    /// The city the request named; `None` for a whole country.
    #[serde(default)]
    pub requested: Option<String>,
    /// The city of the server the config points at, when the provider said.
    #[serde(default)]
    pub picked: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_round_trips_and_has_the_documented_shape() {
        let mut s = State {
            connected: true,
            bypass_routes: 3,
            last: Some(LastRequest {
                location: Some("DE/Berlin".parse().unwrap()),
                pick: Pick::Fastest,
                bypass_extra: vec!["203.0.113.0/24".parse().unwrap()],
                preset: Some(PresetUse {
                    name: "work".into(),
                    overridden: vec!["location".into()],
                }),
                at: 1_767_225_600,
            }),
            ..State::default()
        };
        s.confs.insert(
            "de".into(),
            ConfNote {
                requested: Some("Berlin".into()),
                picked: Some("Berlin".into()),
            },
        );
        let json = serde_json::to_string_pretty(&s).unwrap();
        insta::assert_snapshot!(json);
        assert_eq!(serde_json::from_str::<State>(&json).unwrap(), s);
        assert_eq!(
            s.last.unwrap().preset.unwrap().describe(),
            "work (location overridden)"
        );
    }

    #[test]
    fn unknown_future_fields_are_ignored() {
        let s: State = serde_json::from_str(r#"{"version":1,"later":true}"#).unwrap();
        assert_eq!(s, State::default());
    }
}
