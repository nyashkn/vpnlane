//! Presets: named bundles of settings from `[presets.<name>]` in `config.toml`.
//!
//! Pure. A preset only says what differs from the base settings; [`Preset::resolve`]
//! turns it into the request `up` runs, and [`describe_change`] renders the
//! preview the TUI shows before applying it.

use std::collections::BTreeSet;
use std::fmt;

use ipnet::IpNet;
use serde::Serialize;

use crate::config::{RawPreset, fold_location};
use crate::{Deprecation, Effective, Location, Overrides, Pick};

/// Longest preset name.
pub const MAX_PRESET_NAME: usize = 32;
/// Longest `label`.
pub const MAX_PRESET_LABEL: usize = 24;
/// Presets reachable with the number keys 1 to 9; later ones are unkeyed.
pub const KEYED_PRESETS: usize = 9;

/// One `[presets.<name>]` table: an overlay of the top-level connection
/// keys, plus a display `label`. Every key is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Preset {
    /// Short display text, at most 24 characters. Preset-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where to connect; replaces the base `location`. The city is checked at apply time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Server selection rule; replaces the base `pick`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<Pick>,
    /// CIDRs that skip the tunnel, added to the base `bypass_extra`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass_extra: Option<Vec<IpNet>>,
}

/// A preset with its name, in key order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NamedPreset {
    /// The table name.
    pub name: String,
    /// Its settings.
    #[serde(flatten)]
    pub preset: Preset,
}

/// A preset with its number key and the request it resolves to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolvedPreset {
    /// The table name.
    pub name: String,
    /// Display text.
    pub label: Option<String>,
    /// Number key 1 to 9; `None` past the ninth.
    pub key: Option<usize>,
    /// Base, preset and environment combined, as `up --preset` would run it.
    pub request: EffectiveRequest,
}

/// What `up` is asked to do once the layers are combined.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectiveRequest {
    /// Where to connect; `None` when no layer names a place.
    pub location: Option<Location>,
    /// Server selection rule.
    pub pick: Pick,
    /// Extra bypass CIDRs: base plus preset, without duplicates.
    pub bypass_extra: Vec<IpNet>,
}

/// The connection as it is now; same shape as a request.
pub type ActiveState = EffectiveRequest;

impl EffectiveRequest {
    /// True when both ask for the same connection: place (city ignoring case
    /// and accents) and bypass set. `pick` is left out: it chooses a server, not a destination.
    pub fn same_connection(&self, other: &Self) -> bool {
        let nets = |r: &Self| r.bypass_extra.iter().copied().collect::<BTreeSet<_>>();
        let same_place = match (&self.location, &other.location) {
            (Some(a), Some(b)) => a.same_place(b),
            (a, b) => a.is_none() && b.is_none(),
        };
        same_place && nets(self) == nets(other)
    }
}

impl Preset {
    /// Combines this preset with `base`, the environment included (flags are
    /// applied by the caller afterwards).
    pub fn resolve(&self, base: &Effective) -> EffectiveRequest {
        base.request(Some(self), &Overrides::default())
    }

    /// Folds one raw table into a preset. On failure: the key to locate in the table, and the message.
    pub(crate) fn build(
        name: &str,
        raw: RawPreset,
        deprecated: &mut Vec<Deprecation>,
    ) -> Result<Self, (&'static str, String)> {
        let at = format!("presets.{name}");
        for (key, set) in [
            ("exit_ip_check", raw.exit_ip_check.is_some()),
            ("tui", raw.tui.is_some()),
        ] {
            if set {
                return Err((
                    key,
                    format!(
                        "{at}.{key}: not a connection setting, so it cannot differ per preset; set it at the top level of config.toml"
                    ),
                ));
            }
        }
        let names = (
            format!("{at}.country"),
            format!("{at}.city"),
            format!("{at}.location"),
        );
        let location = fold_location(
            raw.location,
            raw.country,
            raw.city,
            (&names.0, &names.1, &names.2),
            deprecated,
        )?;
        crate::config::check_bypass(&format!("{at}.bypass_extra"), raw.bypass_extra.as_deref())
            .map_err(|m| ("bypass_extra", m))?;
        if let Some(l) = &raw.label {
            let n = l.chars().count();
            if n == 0 || n > MAX_PRESET_LABEL || l.chars().any(char::is_control) {
                return Err((
                    "label",
                    format!(
                        "{at}.label {l:?}: use 1 to {MAX_PRESET_LABEL} characters, no control characters"
                    ),
                ));
            }
        }
        Ok(Self {
            label: raw.label,
            location,
            pick: raw.pick,
            bypass_extra: raw.bypass_extra,
        })
    }

    /// One line for `preset list`, e.g. `DE/Berlin, fastest, +2 routes`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.extend(self.location.as_ref().map(ToString::to_string));
        parts.extend(self.pick.map(|p| p.to_string()));
        let n = self.bypass_extra.as_ref().map_or(0, Vec::len);
        if n > 0 {
            parts.push(routes(n));
        }
        if parts.is_empty() {
            "(no changes)".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

/// Name rule: 1 to 32 characters of `[a-z0-9_-]`.
pub(crate) fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= MAX_PRESET_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "preset name {name:?}: use 1 to {MAX_PRESET_NAME} characters from a-z, 0-9, '_' and '-'"
        ))
    }
}

fn routes(n: usize) -> String {
    format!("+{n} route{}", if n == 1 { "" } else { "s" })
}

/// One line of the preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeLine {
    /// A key that would change; `-` stands for unset.
    Changed {
        /// Setting name.
        key: &'static str,
        /// Current value.
        from: String,
        /// Value after applying.
        to: String,
    },
    /// This many bypass routes would be added.
    AddRoutes(usize),
    /// This many bypass routes would be removed.
    RemoveRoutes(usize),
    /// Nothing would change.
    AlreadyActive,
}

impl fmt::Display for ChangeLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed { key, from, to } => write!(f, "{key}  {from} -> {to}"),
            Self::AddRoutes(n) => write!(f, "bypass_extra  {}", routes(*n)),
            Self::RemoveRoutes(n) => write!(f, "bypass_extra  -{}", &routes(*n)[1..]),
            Self::AlreadyActive => f.write_str("already active"),
        }
    }
}

/// What applying `preset` would change from `current` (`None`: not connected),
/// judged on what the overlay resolves to against `base` (environment
/// included), not on the table as written. Unchanged keys are omitted; no
/// difference at all is [`ChangeLine::AlreadyActive`].
pub fn describe_change(
    current: Option<&ActiveState>,
    preset: &Preset,
    base: &Effective,
) -> Vec<ChangeLine> {
    let req = preset.resolve(base);
    let shown = |v: Option<String>| v.unwrap_or_else(|| "-".to_owned());
    let mut out = Vec::new();
    let cur_loc = current.and_then(|c| c.location.as_ref());
    let same = match (cur_loc, req.location.as_ref()) {
        (Some(a), Some(b)) => a.same_place(b),
        (a, b) => a.is_none() && b.is_none(),
    };
    // No place to connect to is an error at apply time, not a change.
    if !same && req.location.is_some() {
        out.push(ChangeLine::Changed {
            key: "location",
            from: shown(cur_loc.map(ToString::to_string)),
            to: shown(req.location.as_ref().map(ToString::to_string)),
        });
    }
    // Not connected: only an explicit preset pick is news.
    let cur_pick = current.map(|c| c.pick);
    if cur_pick != Some(req.pick) && (current.is_some() || preset.pick.is_some()) {
        out.push(ChangeLine::Changed {
            key: "pick",
            from: shown(cur_pick.map(|p| p.to_string())),
            to: req.pick.to_string(),
        });
    }
    let have: BTreeSet<_> = current.iter().flat_map(|c| &c.bypass_extra).collect();
    let want: BTreeSet<_> = req.bypass_extra.iter().collect();
    let added = want.difference(&have).count();
    let removed = have.difference(&want).count();
    if out.is_empty() && added == 0 && removed == 0 {
        return vec![ChangeLine::AlreadyActive];
    }
    if added > 0 {
        out.push(ChangeLine::AddRoutes(added));
    }
    if removed > 0 {
        out.push(ChangeLine::RemoveRoutes(removed));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CountryCode, FileConfig};

    fn cc(s: &str) -> CountryCode {
        s.parse().unwrap()
    }

    fn loc(s: &str) -> Location {
        s.parse().unwrap()
    }

    fn net(s: &str) -> IpNet {
        s.parse().unwrap()
    }

    fn base(toml: &str) -> Effective {
        base_env(toml, Overrides::default())
    }

    fn base_env(toml: &str, env: Overrides) -> Effective {
        FileConfig::parse(toml)
            .unwrap()
            .resolve(&Overrides::default(), env)
    }

    fn state(location: &str, pick: Pick, nets: &[&str]) -> ActiveState {
        ActiveState {
            location: Some(loc(location)),
            pick,
            bypass_extra: nets.iter().map(|n| net(n)).collect(),
        }
    }

    fn lines(cur: Option<&ActiveState>, p: &Preset) -> Vec<String> {
        lines_in(&base(""), cur, p)
    }

    fn lines_in(b: &Effective, cur: Option<&ActiveState>, p: &Preset) -> Vec<String> {
        describe_change(cur, p, b)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn overlay_scalars_override_and_lists_add() {
        let b = base(
            "location = \"US/Chicago\"\npick = \"fastest\"\n\
             bypass_extra = [\"203.0.113.0/24\"]\n",
        );
        // Empty preset: the base, unchanged.
        let r = Preset::default().resolve(&b);
        assert_eq!(r.location, Some(loc("US/Chicago")));
        assert_eq!(r.pick, Pick::Fastest);
        // A location replaces the base location whole: no city is inherited.
        let de = Preset {
            location: Some(loc("DE")),
            pick: Some(Pick::LeastLoad),
            bypass_extra: Some(vec![net("203.0.113.0/24"), net("203.0.113.9/32")]),
            ..Preset::default()
        };
        let r = de.resolve(&b);
        assert_eq!((r.location, r.pick), (Some(loc("DE")), Pick::LeastLoad));
        // The preset's list is added to the base list; a repeat is not doubled.
        assert_eq!(
            r.bypass_extra,
            [net("203.0.113.0/24"), net("203.0.113.9/32")]
        );
        // No base place: the preset's own, or none.
        assert_eq!(Preset::default().resolve(&base("")).location, None);
    }

    #[test]
    fn describe_change_cases() {
        let berlin = Preset {
            location: Some(loc("DE/Berlin")),
            pick: Some(Pick::Fastest),
            bypass_extra: Some(vec![net("203.0.113.0/24"), net("203.0.113.9/32")]),
            label: None,
        };
        let cur = state("US", Pick::LeastLoad, &[]);
        insta::assert_snapshot!(lines(Some(&cur), &berlin).join("\n"), @r"
        location  US -> DE/Berlin
        pick  least-load -> fastest
        bypass_extra  +2 routes
        ");
        insta::assert_snapshot!(lines(None, &berlin).join("\n"), @r"
        location  - -> DE/Berlin
        pick  - -> fastest
        bypass_extra  +2 routes
        ");
        // Unchanged keys are omitted; one new route only counts the new one.
        let cur = state("DE/berlin", Pick::Fastest, &["203.0.113.0/24"]);
        assert_eq!(lines(Some(&cur), &berlin), ["bypass_extra  +1 route"]);
        // Dropping a city is a change.
        let de = Preset {
            location: Some(loc("DE")),
            ..Preset::default()
        };
        assert_eq!(
            lines(Some(&cur), &de),
            [
                "location  DE/berlin -> DE",
                "pick  fastest -> least-load",
                "bypass_extra  -1 route"
            ]
        );
        // Identical.
        let cur = state(
            "DE/Berlin",
            Pick::Fastest,
            &["203.0.113.0/24", "203.0.113.9/32"],
        );
        assert_eq!(lines(Some(&cur), &berlin), ["already active"]);
        assert_eq!(lines(None, &Preset::default()), ["already active"]);
    }

    /// Regression (#29): the preview is of the resolved request.
    #[test]
    fn describe_change_uses_the_resolved_request() {
        let b = base("location = \"ZA\"\nbypass_extra = [\"203.0.113.0/24\"]\n");
        let cur = state("DE", Pick::LeastLoad, &["203.0.113.0/24"]);
        // No location in the preset: the switch to the base location still shows.
        let fast = Preset {
            pick: Some(Pick::Fastest),
            ..Preset::default()
        };
        assert_eq!(
            lines_in(&b, Some(&cur), &fast),
            ["location  DE -> ZA", "pick  least-load -> fastest"]
        );
        // An empty preset is active only when it resolves to the connection.
        let empty = Preset::default();
        assert_eq!(lines_in(&b, Some(&cur), &empty), ["location  DE -> ZA"]);
        let za = state("ZA", Pick::LeastLoad, &["203.0.113.0/24"]);
        assert_eq!(lines_in(&b, Some(&za), &empty), ["already active"]);
        // Routes that applying would remove are shown.
        let work = state(
            "ZA",
            Pick::LeastLoad,
            &["203.0.113.0/24", "203.0.113.9/32", "203.0.113.10/32"],
        );
        assert_eq!(
            lines_in(&b, Some(&work), &empty),
            ["bypass_extra  -2 routes"]
        );
    }

    /// Regression (#29): a city never leaks from the file into another place.
    /// It used to be `default_city` surviving `VPNLANE_COUNTRY=DE`.
    #[test]
    fn a_file_city_never_follows_another_place() {
        let file = "location = \"US/Chicago\"\n";
        let env = |l: &str| Overrides {
            location: Some(loc(l)),
            ..Overrides::default()
        };
        let r = Preset::default().resolve(&base_env(file, env("DE")));
        assert_eq!(r.location, Some(loc("DE")));
        let r = Preset::default().resolve(&base_env(file, env("US")));
        assert_eq!(r.location, Some(loc("US")), "env replaces the whole value");
        let r = Preset::default().resolve(&base_env(file, Overrides::default()));
        assert_eq!(r.location, Some(loc("US/Chicago")));
        // The old spellings behave the same: country alone, city alone is an error.
        let old = "default_country = \"US\"\ndefault_city = \"Chicago\"\n";
        let r = Preset::default().resolve(&base_env(old, env("DE")));
        assert_eq!(r.location, Some(loc("DE")));
    }

    #[test]
    fn resolved_presets_carry_keys_and_requests() {
        let b = base(
            "location = \"ZA\"\npreset_order = [\"work\"]\n\
             [presets.home]\n[presets.work]\nlocation = \"DE/Berlin\"\nlabel = \"Office\"\n",
        );
        let r = b.resolved_presets();
        assert_eq!(
            r.iter()
                .map(|p| (
                    p.name.as_str(),
                    p.key,
                    p.request.location.clone().unwrap().to_string()
                ))
                .collect::<Vec<_>>(),
            [
                ("work", Some(1), "DE/Berlin".to_owned()),
                ("home", Some(2), "ZA".to_owned())
            ]
        );
        assert_eq!(r[0].label.as_deref(), Some("Office"));
    }

    #[test]
    fn same_connection_ignores_case_order_and_pick() {
        let a = state(
            "DE/Zürich",
            Pick::Fastest,
            &["203.0.113.0/24", "203.0.113.9/32"],
        );
        let b = state(
            "DE/zurich",
            Pick::LeastLoad,
            &["203.0.113.9/32", "203.0.113.0/24"],
        );
        assert!(a.same_connection(&b));
        assert!(!a.same_connection(&state("DE", Pick::Fastest, &[])));
    }

    #[test]
    fn summary_line() {
        assert_eq!(Preset::default().summary(), "(no changes)");
        let p = Preset {
            location: Some(loc("DE/Berlin")),
            bypass_extra: Some(vec![net("203.0.113.9/32")]),
            ..Preset::default()
        };
        assert_eq!(p.summary(), "DE/Berlin, +1 route");
        let _ = cc("DE");
    }
}
