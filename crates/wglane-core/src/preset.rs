//! Presets: named bundles of settings from `[presets.<name>]` in `config.toml`.
//!
//! Pure. A preset only says what differs from the base settings; [`Preset::resolve`]
//! turns it into the request `up` runs, and [`describe_change`] renders the
//! preview the TUI shows before applying it.

use std::collections::BTreeSet;
use std::fmt;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::{CountryCode, Effective, Pick, fold};

/// Longest preset name.
pub const MAX_PRESET_NAME: usize = 32;
/// Longest `label`.
pub const MAX_PRESET_LABEL: usize = 24;
/// Presets reachable with the number keys 1 to 9; later ones are unkeyed.
pub const KEYED_PRESETS: usize = 9;

/// One `[presets.<name>]` table: every key optional.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Country to connect to; falls back to the base `default_country`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<CountryCode>,
    /// City in `country`; needs `country`. Checked against the network at apply time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// Server selection rule for this preset (`up --preset` honors it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<Pick>,
    /// CIDRs that skip the tunnel, added to the base `bypass_extra`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bypass_extra: Option<Vec<IpNet>>,
    /// Short display text, at most 24 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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

/// What `up` is asked to do once a preset and the base settings are combined.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectiveRequest {
    /// Country to connect to; `None` when neither preset nor base names one.
    pub country: Option<CountryCode>,
    /// City within it.
    pub city: Option<String>,
    /// Server selection rule.
    pub pick: Pick,
    /// Extra bypass CIDRs: base plus preset, without duplicates.
    pub bypass_extra: Vec<IpNet>,
}

/// The connection as it is now; same shape as a request.
pub type ActiveState = EffectiveRequest;

impl EffectiveRequest {
    /// True when both ask for the same connection: country, city (ignoring
    /// case and accents) and bypass set. `pick` is left out: it chooses a server, not a destination.
    pub fn same_connection(&self, other: &Self) -> bool {
        let city = |r: &Self| r.city.as_deref().map(fold);
        let nets = |r: &Self| r.bypass_extra.iter().copied().collect::<BTreeSet<_>>();
        self.country == other.country && city(self) == city(other) && nets(self) == nets(other)
    }
}

impl Preset {
    /// Combines this preset with `base` (flags are applied by the caller afterwards).
    ///
    /// A preset that names a country owns the city too (none means the whole
    /// country); one that does not inherits the base country and `default_city`.
    pub fn resolve(&self, base: &Effective) -> EffectiveRequest {
        let (country, city) = match self.country {
            Some(c) => (Some(c), self.city.clone()),
            None => (
                base.default_country.value,
                base.default_city_for(base.default_country.value),
            ),
        };
        let mut bypass_extra = base.bypass_extra.value.clone();
        for net in self.bypass_extra.iter().flatten() {
            if !bypass_extra.contains(net) {
                bypass_extra.push(*net);
            }
        }
        EffectiveRequest {
            country,
            city,
            pick: self.pick.unwrap_or(base.pick.value),
            bypass_extra,
        }
    }

    /// One line for `preset list`, e.g. `DE, Berlin, fastest, +2 routes`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.extend(self.country.map(|c| c.to_string()));
        parts.extend(self.city.clone());
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

    /// On failure: the key to locate in the table, and the message.
    pub(crate) fn validate(&self, name: &str) -> Result<(), (&'static str, String)> {
        let at = format!("presets.{name}");
        if self.city.is_some() && self.country.is_none() {
            return Err(("city", format!("{at}: city needs a country")));
        }
        if let Some(city) = &self.city {
            crate::config::check_city(&format!("{at}.city"), city).map_err(|m| ("city", m))?;
        }
        crate::config::check_bypass(&format!("{at}.bypass_extra"), self.bypass_extra.as_deref())
            .map_err(|m| ("bypass_extra", m))?;
        if let Some(l) = &self.label {
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
        Ok(())
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
/// judged on what the preset resolves to against `base`, not as written.
/// Unchanged keys are omitted; no difference at all is [`ChangeLine::AlreadyActive`].
pub fn describe_change(
    current: Option<&ActiveState>,
    preset: &Preset,
    base: &Effective,
) -> Vec<ChangeLine> {
    let req = preset.resolve(base);
    let shown = |v: Option<String>| v.unwrap_or_else(|| "-".to_owned());
    let mut out = Vec::new();
    let mut changed = |key, from: Option<String>, to: Option<String>| {
        out.push(ChangeLine::Changed {
            key,
            from: shown(from),
            to: shown(to),
        });
    };
    let cur_country = current.and_then(|c| c.country);
    let cur_city = current.and_then(|c| c.city.clone());
    if req.country.is_some() && cur_country != req.country {
        changed(
            "country",
            cur_country.map(|c| c.to_string()),
            req.country.map(|c| c.to_string()),
        );
    }
    if cur_city.as_deref().map(fold) != req.city.as_deref().map(fold) {
        changed("city", cur_city, req.city.clone());
    }
    // Not connected: only an explicit preset pick is news.
    let cur_pick = current.map(|c| c.pick);
    if cur_pick != Some(req.pick) && (current.is_some() || preset.pick.is_some()) {
        changed(
            "pick",
            cur_pick.map(|p| p.to_string()),
            Some(req.pick.to_string()),
        );
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
    use crate::{FileConfig, Overrides};

    fn cc(s: &str) -> CountryCode {
        s.parse().unwrap()
    }

    fn net(s: &str) -> IpNet {
        s.parse().unwrap()
    }

    fn base(toml: &str) -> Effective {
        FileConfig::parse(toml)
            .unwrap()
            .resolve(Overrides::default(), Overrides::default())
    }

    fn state(country: &str, city: Option<&str>, pick: Pick, nets: &[&str]) -> ActiveState {
        ActiveState {
            country: Some(cc(country)),
            city: city.map(Into::into),
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
    fn resolve_precedence_preset_over_base() {
        let b = base(
            "default_country = \"US\"\ndefault_city = \"Chicago\"\npick = \"fastest\"\n\
             bypass_extra = [\"203.0.113.0/24\"]\n",
        );
        // Empty preset: the base, unchanged.
        let r = Preset::default().resolve(&b);
        assert_eq!(
            (r.country, r.city.as_deref()),
            (Some(cc("US")), Some("Chicago"))
        );
        assert_eq!(r.pick, Pick::Fastest);
        // A country takes the city with it: none means the whole country.
        let de = Preset {
            country: Some(cc("DE")),
            pick: Some(Pick::LeastLoad),
            bypass_extra: Some(vec![net("203.0.113.0/24"), net("203.0.113.9/32")]),
            ..Preset::default()
        };
        let r = de.resolve(&b);
        assert_eq!(
            (r.country, r.city, r.pick),
            (Some(cc("DE")), None, Pick::LeastLoad)
        );
        assert_eq!(
            r.bypass_extra,
            [net("203.0.113.0/24"), net("203.0.113.9/32")]
        );
        // No base country: the preset's own, or none.
        assert_eq!(Preset::default().resolve(&base("")).country, None);
    }

    #[test]
    fn describe_change_cases() {
        let berlin = Preset {
            country: Some(cc("DE")),
            city: Some("Berlin".into()),
            pick: Some(Pick::Fastest),
            bypass_extra: Some(vec![net("203.0.113.0/24"), net("203.0.113.9/32")]),
            label: None,
        };
        let cur = state("US", None, Pick::LeastLoad, &[]);
        insta::assert_snapshot!(lines(Some(&cur), &berlin).join("\n"), @r"
        country  US -> DE
        city  - -> Berlin
        pick  least-load -> fastest
        bypass_extra  +2 routes
        ");
        insta::assert_snapshot!(lines(None, &berlin).join("\n"), @r"
        country  - -> DE
        city  - -> Berlin
        pick  - -> fastest
        bypass_extra  +2 routes
        ");
        // Unchanged keys are omitted; one new route only counts the new one.
        let cur = state("DE", Some("berlin"), Pick::Fastest, &["203.0.113.0/24"]);
        assert_eq!(lines(Some(&cur), &berlin), ["bypass_extra  +1 route"]);
        // Dropping a city is a change.
        let de = Preset {
            country: Some(cc("DE")),
            ..Preset::default()
        };
        assert_eq!(
            lines(Some(&cur), &de),
            [
                "city  berlin -> -",
                "pick  fastest -> least-load",
                "bypass_extra  -1 route"
            ]
        );
        // Identical.
        let cur = state(
            "DE",
            Some("Berlin"),
            Pick::Fastest,
            &["203.0.113.0/24", "203.0.113.9/32"],
        );
        assert_eq!(lines(Some(&cur), &berlin), ["already active"]);
        assert_eq!(lines(None, &Preset::default()), ["already active"]);
    }

    #[test]
    fn describe_change_uses_the_resolved_request() {
        let b = base("default_country = \"ZA\"\nbypass_extra = [\"203.0.113.0/24\"]\n");
        let cur = state("DE", None, Pick::LeastLoad, &["203.0.113.0/24"]);
        // No country in the preset: the switch to default_country still shows.
        let fast = Preset {
            pick: Some(Pick::Fastest),
            ..Preset::default()
        };
        assert_eq!(
            lines_in(&b, Some(&cur), &fast),
            ["country  DE -> ZA", "pick  least-load -> fastest"]
        );
        // An empty preset is active only when it resolves to the connection.
        let empty = Preset::default();
        assert_eq!(lines_in(&b, Some(&cur), &empty), ["country  DE -> ZA"]);
        let za = state("ZA", None, Pick::LeastLoad, &["203.0.113.0/24"]);
        assert_eq!(lines_in(&b, Some(&za), &empty), ["already active"]);
        // Routes that applying would remove are shown.
        let work = state(
            "ZA",
            None,
            Pick::LeastLoad,
            &["203.0.113.0/24", "203.0.113.9/32", "203.0.113.10/32"],
        );
        assert_eq!(
            lines_in(&b, Some(&work), &empty),
            ["bypass_extra  -2 routes"]
        );
    }

    #[test]
    fn default_city_only_for_default_country() {
        let file =
            FileConfig::parse("default_country = \"US\"\ndefault_city = \"Chicago\"\n").unwrap();
        let env = |c: &str| Overrides {
            country: Some(cc(c)),
            ..Overrides::default()
        };
        let r = Preset::default().resolve(&file.resolve(Overrides::default(), env("DE")));
        assert_eq!((r.country, r.city), (Some(cc("DE")), None));
        let r = Preset::default().resolve(&file.resolve(Overrides::default(), env("US")));
        assert_eq!(r.city.as_deref(), Some("Chicago"));
        let r =
            Preset::default().resolve(&file.resolve(Overrides::default(), Overrides::default()));
        assert_eq!(r.city.as_deref(), Some("Chicago"));
    }

    #[test]
    fn same_connection_ignores_case_order_and_pick() {
        let a = state(
            "DE",
            Some("Zürich"),
            Pick::Fastest,
            &["203.0.113.0/24", "203.0.113.9/32"],
        );
        let b = state(
            "DE",
            Some("zurich"),
            Pick::LeastLoad,
            &["203.0.113.9/32", "203.0.113.0/24"],
        );
        assert!(a.same_connection(&b));
        assert!(!a.same_connection(&state("DE", None, Pick::Fastest, &[])));
    }

    #[test]
    fn summary_line() {
        assert_eq!(Preset::default().summary(), "(no changes)");
        let p = Preset {
            country: Some(cc("DE")),
            city: Some("Berlin".into()),
            bypass_extra: Some(vec![net("203.0.113.9/32")]),
            ..Preset::default()
        };
        assert_eq!(p.summary(), "DE, Berlin, +1 route");
    }
}
