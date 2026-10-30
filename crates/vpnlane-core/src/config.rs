//! Saved settings: the `config.toml` schema, validation and precedence.
//!
//! Pure: callers read the file and the environment and hand in strings.
//! Precedence is CLI flag > environment > file > built-in default.

use std::collections::BTreeMap;
use std::fmt;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::preset::check_name;
use crate::{CountryCode, Error, Location, NamedPreset, Preset, Result};
use crate::{EffectiveRequest, KEYED_PRESETS, ResolvedPreset};

/// Most bypass routes the helper accepts, auto ones plus `bypass_extra`.
pub const MAX_BYPASS: usize = 1024;
/// Default number of latency samples the TUI graph keeps.
pub const DEFAULT_GRAPH_SAMPLES: usize = 60;
/// Default: a sample above this multiple of the window median is a warning.
pub const DEFAULT_WARN_RATIO: f64 = 1.5;

const GRAPH_SAMPLES: std::ops::RangeInclusive<usize> = 10..=600;
const WARN_RATIO: std::ops::RangeInclusive<f64> = 1.0..=10.0;
// Same floor as the root helper (`is_bad_bypass`), so a bad entry fails here, not at `up`.
const MIN_PREFIX_V4: u8 = 8;
const MIN_PREFIX_V6: u8 = 16;

/// How to choose a server within a country.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pick {
    /// Lowest reported load (today's behaviour).
    #[default]
    LeastLoad,
    /// Lowest measured latency.
    Fastest,
}

impl std::str::FromStr for Pick {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "least-load" => Ok(Self::LeastLoad),
            "fastest" => Ok(Self::Fastest),
            _ => Err(Error::Config(format!(
                "pick {s:?}: use least-load or fastest"
            ))),
        }
    }
}

impl fmt::Display for Pick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LeastLoad => "least-load",
            Self::Fastest => "fastest",
        })
    }
}

/// The `config.toml` file after aliases are folded in: the top level is the
/// unnamed base preset, `[presets.<name>]` tables overlay it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileConfig {
    /// Where `vpnlane up` connects with no argument.
    pub location: Option<Location>,
    /// Server selection rule.
    pub pick: Option<Pick>,
    /// Extra CIDRs that skip the tunnel.
    pub bypass_extra: Option<Vec<IpNet>>,
    /// Look up the exit IP in `status` and the TUI.
    pub exit_ip_check: Option<bool>,
    /// The `[tui]` table.
    pub tui: Option<TuiFile>,
    /// Number-key order of presets; the rest follow alphabetically.
    pub preset_order: Option<Vec<String>>,
    /// The `[presets.<name>]` tables.
    pub presets: Option<BTreeMap<String, Preset>>,
    /// Old spellings found in the file (already mapped into the fields above).
    pub deprecated: Vec<Deprecation>,
}

/// An old spelling that still works for one release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deprecation {
    /// What the user wrote, e.g. `default_country` or `VPNLANE_COUNTRY`.
    pub old: String,
    /// What replaces it, e.g. `location`.
    pub new: String,
}

/// One line for stderr naming every replacement, or `None` when nothing is deprecated.
pub fn deprecation_line(list: &[Deprecation]) -> Option<String> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for d in list {
        match groups.iter_mut().find(|(new, _)| *new == d.new) {
            Some((_, olds)) => olds.push(&d.old),
            None => groups.push((&d.new, vec![&d.old])),
        }
    }
    (!groups.is_empty()).then(|| {
        let parts: Vec<_> = groups
            .iter()
            .map(|(new, olds)| format!("{} -> {new}", olds.join(", ")))
            .collect();
        format!(
            "warning: deprecated spelling, still works for this release: {} (see docs/CONFIG.md)",
            parts.join("; ")
        )
    })
}

/// The file exactly as TOML spells it, old keys included.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    location: Option<Location>,
    default_country: Option<CountryCode>,
    default_city: Option<String>,
    pick: Option<Pick>,
    bypass_extra: Option<Vec<IpNet>>,
    exit_ip_check: Option<bool>,
    tui: Option<TuiFile>,
    preset_order: Option<Vec<String>>,
    presets: Option<BTreeMap<String, RawPreset>>,
}

/// A `[presets.<name>]` table as TOML spells it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPreset {
    pub(crate) label: Option<String>,
    pub(crate) location: Option<Location>,
    pub(crate) country: Option<CountryCode>,
    pub(crate) city: Option<String>,
    pub(crate) pick: Option<Pick>,
    pub(crate) bypass_extra: Option<Vec<IpNet>>,
    // Accepted only to say they do not belong here.
    pub(crate) exit_ip_check: Option<toml::Value>,
    pub(crate) tui: Option<toml::Value>,
}

/// The `[tui]` table as written.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuiFile {
    /// Open every continent group at start.
    pub start_expanded: Option<bool>,
    /// Latency samples kept for the graph (10 to 600).
    pub graph_samples: Option<usize>,
    /// Spike warning ratio over the window median (1.0 to 10.0).
    pub warn_ratio: Option<f64>,
}

/// Folds `country`/`city` (old) into `location` (new). `old_*`/`new` name the
/// keys for messages; on error: the field to locate and the message.
pub(crate) fn fold_location(
    location: Option<Location>,
    country: Option<CountryCode>,
    city: Option<String>,
    (old_country, old_city, new): (&str, &str, &str),
    deprecated: &mut Vec<Deprecation>,
) -> Result<Option<Location>, (&'static str, String)> {
    let dep = |old: &str| Deprecation {
        old: old.to_owned(),
        new: new.to_owned(),
    };
    let Some(cc) = country else {
        return match (city, location) {
            (Some(_), _) => Err((
                "city",
                format!("{old_city} needs {old_country}: write {new} = \"CC/City\" instead"),
            )),
            (None, l) => Ok(l),
        };
    };
    if location.is_some() {
        return Err((
            "country",
            format!("{old_country} is the old spelling of {new}: set only {new}"),
        ));
    }
    deprecated.push(dep(old_country));
    if city.is_some() {
        deprecated.push(dep(old_city));
    }
    Location::new(cc, city)
        .map(Some)
        .map_err(|e| ("city", format!("{old_city}: {e}")))
}

impl FileConfig {
    /// Parses and validates `text`. A parse error names the offending key and line.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawFile = toml::from_str(text).map_err(|e| Error::Config(e.to_string()))?;
        let cfg = Self::build(raw).map_err(|(key, msg)| {
            // shortcut: first line starting with the key, not a TOML span; right unless a table repeats the name.
            // A key "header\nfield" is searched for the field after the header.
            let (head, field) = key.split_once('\n').unwrap_or((&key, ""));
            let lines: Vec<_> = text.lines().map(str::trim_start).collect();
            let at = lines.iter().position(|l| l.starts_with(head));
            let at = match (at, field) {
                (Some(h), f) if !f.is_empty() => lines[h..]
                    .iter()
                    .position(|l| l.starts_with(f))
                    .map(|i| h + i)
                    .or(Some(h)),
                (h, _) => h,
            };
            let line = at.map_or(String::new(), |i| format!("line {}: ", i + 1));
            Error::Config(format!("{line}{msg}"))
        })?;
        Ok(cfg)
    }

    /// Folds old spellings in and validates. On failure: the key to locate in the file, and the message.
    fn build(raw: RawFile) -> Result<Self, (String, String)> {
        let at = |key: &str| key.to_owned();
        let mut deprecated = Vec::new();
        let location = fold_location(
            raw.location,
            raw.default_country,
            raw.default_city,
            ("default_country", "default_city", "location"),
            &mut deprecated,
        )
        .map_err(|(f, m)| {
            (
                at(if f == "city" {
                    "default_city"
                } else {
                    "default_country"
                }),
                m,
            )
        })?;
        check_bypass("bypass_extra", raw.bypass_extra.as_deref())
            .map_err(|m| (at("bypass_extra"), m))?;
        let had_presets = raw.presets.is_some();
        let mut presets = BTreeMap::new();
        for (name, p) in raw.presets.into_iter().flatten() {
            // Locate the table header; a name that needs quoting is found by its opening quote.
            let bare = name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            let key = format!("[presets.{}{name}", if bare { "" } else { "\"" });
            let preset = check_name(&name)
                .map_err(|m| ("", m))
                .and_then(|()| Preset::build(&name, p, &mut deprecated))
                .map_err(|(field, m)| (format!("{key}\n{field}"), m))?;
            presets.insert(name, preset);
        }
        let cfg = Self {
            location,
            pick: raw.pick,
            bypass_extra: raw.bypass_extra,
            exit_ip_check: raw.exit_ip_check,
            tui: raw.tui,
            preset_order: raw.preset_order,
            presets: had_presets.then_some(presets),
            deprecated,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// On failure: the key to locate in the file, and the message.
    fn validate(&self) -> Result<(), (String, String)> {
        let at = |key: &str| key.to_owned();
        let known = |n: &String| self.presets.as_ref().is_some_and(|p| p.contains_key(n));
        for (i, n) in self.preset_order.iter().flatten().enumerate() {
            let dup = self.preset_order.iter().flatten().take(i).any(|o| o == n);
            if !known(n) || dup {
                let why = if dup {
                    "listed twice"
                } else {
                    "is not a [presets] table"
                };
                return Err((at("preset_order"), format!("preset_order {n:?}: {why}")));
            }
        }
        if let Some(t) = &self.tui {
            if let Some(n) = t.graph_samples.filter(|n| !GRAPH_SAMPLES.contains(n)) {
                return Err((
                    at("graph_samples"),
                    format!(
                        "tui.graph_samples {n}: use {} to {}",
                        GRAPH_SAMPLES.start(),
                        GRAPH_SAMPLES.end()
                    ),
                ));
            }
            if let Some(r) = t.warn_ratio.filter(|r| !WARN_RATIO.contains(r)) {
                return Err((
                    at("warn_ratio"),
                    format!(
                        "tui.warn_ratio {r}: use {} to {}",
                        WARN_RATIO.start(),
                        WARN_RATIO.end()
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Applies precedence: `flags` > `env` > this file > built-in default.
    pub fn resolve(&self, flags: &Overrides, env: Overrides) -> Effective {
        let tui = self.tui.clone().unwrap_or_default();
        let mut deprecated = self.deprecated.clone();
        deprecated.extend(env.deprecated.iter().cloned());
        Effective {
            location: layer(
                flags.location.clone().map(Some),
                env.location.clone().map(Some),
                self.location.clone().map(Some),
                None,
            ),
            pick: layer(flags.pick, env.pick, self.pick, Pick::default()),
            bypass_extra: layer(None, None, self.bypass_extra.clone(), Vec::new()),
            preset: layer(
                flags.preset.clone().map(Some),
                env.preset.clone().map(Some),
                None,
                None,
            ),
            exit_ip_check: layer(
                flags.exit_ip_check,
                env.exit_ip_check,
                self.exit_ip_check,
                true,
            ),
            tui: TuiSettings {
                start_expanded: layer(None, None, tui.start_expanded, false),
                graph_samples: layer(None, None, tui.graph_samples, DEFAULT_GRAPH_SAMPLES),
                warn_ratio: layer(None, None, tui.warn_ratio, DEFAULT_WARN_RATIO),
            },
            presets: layer(
                None,
                None,
                self.presets.is_some().then(|| self.ordered_presets()),
                Vec::new(),
            ),
            deprecated,
            base: Base {
                location: self.location.clone(),
                pick: self.pick.unwrap_or_default(),
                bypass_extra: self.bypass_extra.clone().unwrap_or_default(),
            },
            env,
        }
    }

    /// Presets in number-key order: `preset_order` first, then the rest alphabetically.
    pub fn ordered_presets(&self) -> Vec<NamedPreset> {
        let all = self.presets.clone().unwrap_or_default();
        let first = self.preset_order.iter().flatten();
        let named = |n: &String| {
            all.get(n).map(|p| NamedPreset {
                name: n.clone(),
                preset: p.clone(),
            })
        };
        let mut out: Vec<_> = first.filter_map(named).collect();
        out.extend(
            all.iter()
                .filter(|(n, _)| !out.iter().any(|o| &o.name == *n))
                .map(|(n, p)| NamedPreset {
                    name: n.clone(),
                    preset: p.clone(),
                })
                .collect::<Vec<_>>(),
        );
        out
    }
}

/// Bypass rule shared by the top-level key and preset keys.
pub(crate) fn check_bypass(label: &str, nets: Option<&[IpNet]>) -> Result<(), String> {
    for net in nets.into_iter().flatten() {
        let min = if net.addr().is_ipv4() {
            MIN_PREFIX_V4
        } else {
            MIN_PREFIX_V6
        };
        if net.prefix_len() < min || net.addr().is_unspecified() {
            return Err(format!(
                "{label} {net}: too broad, the prefix must be /{min} or longer"
            ));
        }
    }
    let n = nets.map_or(0, <[IpNet]>::len);
    if n > MAX_BYPASS {
        return Err(format!(
            "{label} has {n} entries; the limit is {MAX_BYPASS}"
        ));
    }
    Ok(())
}

fn layer<T>(flag: Option<T>, env: Option<T>, file: Option<T>, default: T) -> Setting<T> {
    let (value, source) = match (flag, env, file) {
        (Some(v), _, _) => (v, Source::Flag),
        (None, Some(v), _) => (v, Source::Env),
        (None, None, Some(v)) => (v, Source::File),
        (None, None, None) => (default, Source::Default),
    };
    Setting { value, source }
}

/// Where a setting's value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A command-line flag or argument.
    Flag,
    /// An environment variable.
    Env,
    /// `config.toml`.
    File,
    /// The built-in default.
    Default,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::File => "file",
            Self::Default => "default",
        })
    }
}

/// A resolved value and where it came from.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Setting<T> {
    /// The effective value.
    pub value: T,
    /// Which layer supplied it.
    pub source: Source,
}

/// Settings that a flag or an environment variable can override.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `location`.
    pub location: Option<Location>,
    /// `pick`.
    pub pick: Option<Pick>,
    /// Which preset to apply (`--preset`, `VPNLANE_PRESET`).
    pub preset: Option<String>,
    /// `exit_ip_check`.
    pub exit_ip_check: Option<bool>,
    /// Old spellings that were read (`VPNLANE_COUNTRY`).
    pub deprecated: Vec<Deprecation>,
}

impl Overrides {
    /// Reads `VPNLANE_LOCATION`, `VPNLANE_PICK`, `VPNLANE_PRESET`,
    /// `VPNLANE_EXIT_IP_CHECK` and the deprecated `VPNLANE_COUNTRY` through `lookup`.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());
        let named = |name: &str, e: Error| Error::Config(format!("{name}: {e}"));
        let mut deprecated = Vec::new();
        let mut location = get(brand_env!("LOCATION"))
            .map(|v| {
                v.parse::<Location>()
                    .map_err(|e| named(brand_env!("LOCATION"), e))
            })
            .transpose()?;
        if let Some(v) = get(brand_env!("COUNTRY")) {
            let cc = v
                .parse::<CountryCode>()
                .map_err(|e| named(brand_env!("COUNTRY"), e))?;
            deprecated.push(Deprecation {
                old: brand_env!("COUNTRY").to_owned(),
                new: brand_env!("LOCATION").to_owned(),
            });
            location = location.or(Some(Location::country_only(cc)));
        }
        let pick = get(brand_env!("PICK"))
            .map(|v| v.parse::<Pick>().map_err(|e| named(brand_env!("PICK"), e)))
            .transpose()?;
        let exit_ip_check = get(brand_env!("EXIT_IP_CHECK"))
            .map(|v| match v.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" => Ok(false),
                _ => Err(Error::Config(format!(
                    "{} {v:?}: use true or false",
                    brand_env!("EXIT_IP_CHECK")
                ))),
            })
            .transpose()?;
        Ok(Self {
            location,
            pick,
            preset: get(brand_env!("PRESET")).map(|v| v.trim().to_owned()),
            exit_ip_check,
            deprecated,
        })
    }
}

/// The file's connection values: the unnamed base preset.
#[derive(Clone, Debug, PartialEq)]
struct Base {
    location: Option<Location>,
    pick: Pick,
    bypass_extra: Vec<IpNet>,
}

/// Every setting after precedence is applied. A connection is built from it
/// with [`Effective::request`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Effective {
    /// Where `up` connects with no argument; `None` when unset. Flag, env or file.
    pub location: Setting<Option<Location>>,
    /// Server selection rule.
    pub pick: Setting<Pick>,
    /// Extra bypass CIDRs (not the auto ones).
    pub bypass_extra: Setting<Vec<IpNet>>,
    /// The preset chosen by flag or `VPNLANE_PRESET`, if any.
    pub preset: Setting<Option<String>>,
    /// Look up the exit IP.
    pub exit_ip_check: Setting<bool>,
    /// TUI settings.
    pub tui: TuiSettings,
    /// Presets in number-key order (`file` when any table exists).
    pub presets: Setting<Vec<NamedPreset>>,
    /// Old spellings in use, from the file and the environment.
    pub deprecated: Vec<Deprecation>,
    #[serde(skip)]
    base: Base,
    #[serde(skip)]
    env: Overrides,
}

impl Effective {
    /// The unnamed base preset with `preset` laid over it: scalars of the
    /// preset win, `bypass_extra` is added. No environment, no flags.
    pub fn overlay(&self, preset: Option<&Preset>) -> EffectiveRequest {
        let mut req = EffectiveRequest {
            location: self.base.location.clone(),
            pick: self.base.pick,
            bypass_extra: self.base.bypass_extra.clone(),
        };
        if let Some(p) = preset {
            req.location = p.location.clone().or(req.location);
            req.pick = p.pick.unwrap_or(req.pick);
            for net in p.bypass_extra.iter().flatten() {
                if !req.bypass_extra.contains(net) {
                    req.bypass_extra.push(*net);
                }
            }
        }
        req
    }

    /// What `up` is asked to do: base, then `preset`, then the environment,
    /// then `flags` (only `location` and `pick` are read from `flags`).
    pub fn request(&self, preset: Option<&Preset>, flags: &Overrides) -> EffectiveRequest {
        let mut req = self.overlay(preset);
        for layer in [&self.env, flags] {
            req.location = layer.location.clone().or(req.location);
            req.pick = layer.pick.unwrap_or(req.pick);
        }
        req
    }

    /// The preset called `name`.
    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets
            .value
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.preset)
    }

    /// Every preset with its key and the request it resolves to, in key order.
    pub fn resolved_presets(&self) -> Vec<ResolvedPreset> {
        self.presets
            .value
            .iter()
            .enumerate()
            .map(|(i, p)| ResolvedPreset {
                name: p.name.clone(),
                label: p.preset.label.clone(),
                key: (i < KEYED_PRESETS).then_some(i + 1),
                request: self.request(Some(&p.preset), &Overrides::default()),
            })
            .collect()
    }
}

/// The `[tui]` settings after precedence.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TuiSettings {
    /// Open every continent group at start.
    pub start_expanded: Setting<bool>,
    /// Latency samples kept.
    pub graph_samples: Setting<usize>,
    /// Spike warning ratio.
    pub warn_ratio: Setting<f64>,
}

/// What `vpnlane config init` writes. Every key is commented out, so a fresh
/// file changes nothing.
pub const SAMPLE_CONFIG: &str = concat!(
    r#"# "#,
    brand_name!(),
    r#" settings. Every key is optional; uncomment what you want.
# Precedence: CLI flag > environment > this file > built-in default.
# Reference: docs/CONFIG.md. The Nord token does NOT go here: it stays in
# <config dir>/token (mode 600) or "#,
    brand_env!("TOKEN"),
    r#".
# This file is yours: "#,
    brand_name!(),
    r#" never writes to it. What it last applied is kept
# in state.json beside it.

# Where `"#,
    brand_name!(),
    r#" up` connects with no argument: a country ("US") or a city in
# one ("US/Chicago"). `"#,
    brand_name!(),
    r#" up DE/Berlin` overrides it for one run.
# Env: "#,
    brand_env!("LOCATION"),
    r#".
# location = "US/Chicago"

# Server choice: "least-load" or "fastest". `up --pick fastest` for one run
# (`--fastest` is the short form). Env: "#,
    brand_env!("PICK"),
    r#".
# pick = "least-load"

# Extra CIDRs that skip the tunnel, added to the automatic Tailscale ones.
# At most 1024 in total; IPv4 prefixes /8 or longer.
# bypass_extra = ["198.51.100.0/24"]

# Look up the exit IP (the leak check) in `status` and the TUI. Turning it off
# is allowed but never silent: `status` and `doctor` say so every time.
# Env: "#,
    brand_env!("EXIT_IP_CHECK"),
    r#". Top level only.
# exit_ip_check = true

# Presets: named overlays of the settings above. A preset may set location,
# pick and bypass_extra (the last one adds to the top-level list) plus a label.
# Apply with `"#,
    brand_name!(),
    r#" preset apply NAME` or `"#,
    brand_name!(),
    r#" up --preset NAME`
# (env: "#,
    brand_env!("PRESET"),
    r#"). Names: a-z, 0-9, '_' and '-', up to 32. The first 9
# (preset_order, then alphabetical) get number keys.
# preset_order = ["work", "home"]

# [tui]
# Open every continent group at start.
# start_expanded = false
# Latency samples kept for the graph (10 to 600).
# graph_samples = 60
# A sample above this multiple of the window median is drawn as a spike (1.0 to 10.0).
# warn_ratio = 1.5

# [presets.work]
# label = "Office exit"
# location = "DE/Berlin"
# bypass_extra = ["203.0.113.0/24"]
# [presets.home]
# location = "ZA"
# pick = "fastest"
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 9] = [
        "preset_order",
        "label",
        "location",
        "pick",
        "bypass_extra",
        "exit_ip_check",
        "start_expanded",
        "graph_samples",
        "warn_ratio",
    ];

    /// Turns the commented-out key lines of the sample into real ones.
    fn uncommented(sample: &str) -> String {
        let is_key = |l: &str| {
            l == "[tui]"
                || l.starts_with("[presets.")
                || KEYS.iter().any(|k| l.starts_with(&format!("{k} =")))
        };
        sample
            .lines()
            .map(|l| l.strip_prefix("# ").filter(|r| is_key(r)).unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn err(text: &str) -> String {
        FileConfig::parse(text).unwrap_err().to_string()
    }

    fn loc(s: &str) -> Location {
        s.parse().unwrap()
    }

    fn resolve(toml: &str) -> Effective {
        FileConfig::parse(toml)
            .unwrap()
            .resolve(&Overrides::default(), Overrides::default())
    }

    #[test]
    fn empty_and_commented_sample_are_defaults() {
        assert_eq!(FileConfig::parse("").unwrap(), FileConfig::default());
        assert_eq!(
            FileConfig::parse(SAMPLE_CONFIG).unwrap(),
            FileConfig::default()
        );
    }

    #[test]
    fn sample_round_trips_when_uncommented() {
        let cfg = FileConfig::parse(&uncommented(SAMPLE_CONFIG)).unwrap();
        assert!(cfg.deprecated.is_empty(), "the sample uses only new keys");
        let eff = cfg.resolve(&Overrides::default(), Overrides::default());
        assert_eq!(eff.location.value, Some(loc("US/Chicago")));
        assert_eq!(eff.bypass_extra.value.len(), 1);
        assert_eq!(eff.pick.value, Pick::LeastLoad);
        assert!(eff.exit_ip_check.value);
        assert_eq!(eff.tui.graph_samples.value, DEFAULT_GRAPH_SAMPLES);
        assert!((eff.tui.warn_ratio.value - DEFAULT_WARN_RATIO).abs() < f64::EPSILON);
        assert_eq!(eff.exit_ip_check.source, Source::File);
        assert_eq!(eff.presets.value.len(), 2);
    }

    #[test]
    fn parses_every_key() {
        let e = resolve(
            "location = \"za\"\npick = \"fastest\"\nexit_ip_check = false\n\
             bypass_extra = [\"203.0.113.0/24\", \"203.0.113.9/32\"]\n\
             [tui]\nstart_expanded = true\ngraph_samples = 120\nwarn_ratio = 2.0\n",
        );
        assert_eq!(e.location.value.unwrap().to_string(), "ZA");
        assert_eq!(e.pick.value, Pick::Fastest);
        assert!(!e.exit_ip_check.value);
        assert_eq!(e.bypass_extra.value.len(), 2);
        assert!(e.tui.start_expanded.value);
        assert_eq!(e.tui.graph_samples.value, 120);
    }

    #[test]
    fn unknown_key_error_names_key_and_line() {
        insta::assert_snapshot!(err("exit_ip_check = true\ncolour = \"red\"\n"));
        insta::assert_snapshot!(err("[tui]\nstart_expanded = true\nwarn = 2\n"));
    }

    #[test]
    fn rejects_bad_values() {
        for bad in [
            "location = \"USA\"",
            "location = \"DE/\"",
            "location = \"DE/Berlin/de1\"",
            "location = \"de1234\"",
            "default_country = \"USA\"",
            "pick = \"slowest\"",
            "default_country = \"US\"\ndefault_city = \"\"",
            "default_country = \"US\"\ndefault_city = \"a/b\"",
            "bypass_extra = [\"nonsense\"]",
            "bypass_extra = [\"10.0.0.0/4\"]",
            "bypass_extra = [\"0.0.0.0/0\"]",
            "[tui]\ngraph_samples = 5",
            "[tui]\ngraph_samples = 601",
            "[tui]\nwarn_ratio = 0.5",
            "[tui]\nwarn_ratio = 11.0",
            "token = \"x\"",
        ] {
            assert!(FileConfig::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn value_errors_name_the_line() {
        let e = err("exit_ip_check = true\n\n[tui]\nstart_expanded = true\ngraph_samples = 5\n");
        assert!(e.starts_with("line 5: tui.graph_samples 5"), "{e}");
        assert!(err("bypass_extra = [\"10.0.0.0/4\"]").starts_with("line 1: "));
    }

    #[test]
    fn bypass_extra_limit() {
        let many = (0..=MAX_BYPASS)
            .map(|i| format!("\"10.{}.{}.1/32\"", i / 256, i % 256))
            .collect::<Vec<_>>()
            .join(",");
        assert!(err(&format!("bypass_extra = [{many}]")).contains("limit is 1024"));
    }

    #[test]
    fn old_keys_map_into_location_and_are_reported() {
        let cfg = FileConfig::parse(
            "default_country = \"US\"\ndefault_city = \"Chicago\"\n\
             [presets.work]\ncountry = \"DE\"\ncity = \"Berlin\"\n\
             [presets.home]\ncountry = \"ZA\"\n\
             [presets.fast]\npick = \"fastest\"\n",
        )
        .unwrap();
        assert_eq!(cfg.location, Some(loc("US/Chicago")));
        let p = |n: &str| cfg.presets.as_ref().unwrap()[n].location.clone();
        assert_eq!(p("work"), Some(loc("DE/Berlin")));
        assert_eq!(p("home"), Some(loc("ZA")));
        assert_eq!(p("fast"), None);
        let olds: Vec<_> = cfg.deprecated.iter().map(|d| d.old.as_str()).collect();
        assert_eq!(
            olds,
            [
                "default_country",
                "default_city",
                "presets.home.country",
                "presets.work.country",
                "presets.work.city"
            ]
        );
        // One line, replacement named, old spellings grouped under it.
        insta::assert_snapshot!(deprecation_line(&cfg.deprecated).unwrap(), @"warning: deprecated spelling, still works for this release: default_country, default_city -> location; presets.home.country -> presets.home.location; presets.work.country, presets.work.city -> presets.work.location (see docs/CONFIG.md)");
        assert_eq!(deprecation_line(&[]), None);
        // The new spelling is not deprecated.
        assert_eq!(resolve("location = \"US\"").deprecated, []);
    }

    #[test]
    fn old_and_new_spelling_conflicts_are_errors() {
        for (n, text) in [
            "location = \"DE\"\ndefault_country = \"US\"\n",
            "default_city = \"Chicago\"\n",
            "[presets.work]\nlocation = \"DE\"\ncountry = \"DE\"\n",
            "[presets.work]\ncity = \"Berlin\"\n",
            "[presets.work]\nexit_ip_check = false\n",
            "[presets.work]\n[presets.work.tui]\nstart_expanded = true\n",
        ]
        .into_iter()
        .enumerate()
        {
            insta::assert_snapshot!(format!("alias_error_{n}"), err(text));
        }
    }

    #[test]
    fn precedence_flag_env_preset_file_default() {
        let file = FileConfig::parse(
            "location = \"DE/Berlin\"\npick = \"fastest\"\nexit_ip_check = false\n\
             [presets.za]\nlocation = \"ZA\"\n",
        )
        .unwrap();
        let none = Overrides::default();
        let env = Overrides {
            location: Some(loc("FR")),
            pick: Some(Pick::LeastLoad),
            exit_ip_check: Some(true),
            ..Overrides::default()
        };
        let flag = Overrides {
            location: Some(loc("US/Boston")),
            pick: Some(Pick::Fastest),
            ..Overrides::default()
        };
        let [file_only, with_env] = [&none, &env].map(|e| file.resolve(&none, e.clone()));
        let cases = [
            // (effective, preset, flags) -> (location, pick)
            (&file_only, None, &none, "DE/Berlin", Pick::Fastest),
            (&file_only, Some("za"), &none, "ZA", Pick::Fastest),
            (&with_env, None, &none, "FR", Pick::LeastLoad),
            (&with_env, Some("za"), &none, "FR", Pick::LeastLoad),
            (&with_env, Some("za"), &flag, "US/Boston", Pick::Fastest),
            (&file_only, None, &flag, "US/Boston", Pick::Fastest),
        ];
        for (eff, preset, flags, want_loc, want_pick) in cases {
            let p = preset.map(|n| eff.preset(n).unwrap());
            let r = eff.request(p, flags);
            assert_eq!(
                (r.location.unwrap().to_string(), r.pick),
                (want_loc.to_owned(), want_pick),
                "{preset:?} {flags:?}"
            );
        }
        // Reported source of the plain settings.
        assert_eq!(file_only.location.source, Source::File);
        assert_eq!(with_env.location.source, Source::Env);
        assert_eq!(
            file.resolve(&flag, env.clone()).location.source,
            Source::Flag
        );
        assert_eq!(
            (with_env.exit_ip_check.source, with_env.exit_ip_check.value),
            (Source::Env, true)
        );
        let e = FileConfig::default().resolve(&none, none.clone());
        assert_eq!(
            (e.location.value, e.location.source),
            (None, Source::Default)
        );
        assert!(e.exit_ip_check.value);
    }

    #[test]
    fn presets_parse_and_order() {
        let cfg = FileConfig::parse(
            "preset_order = [\"work\", \"home\"]\n\
             [presets.zeta]\nlabel = \"Last\"\n\
             [presets.home]\nlocation = \"za\"\npick = \"fastest\"\n\
             [presets.work]\nlocation = \"DE/Berlin\"\nbypass_extra = [\"203.0.113.0/24\"]\n\
             [presets.alpha]\n",
        )
        .unwrap();
        let eff = cfg.resolve(&Overrides::default(), Overrides::default());
        let names: Vec<_> = eff.presets.value.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["work", "home", "alpha", "zeta"]);
        assert_eq!(eff.presets.source, Source::File);
        assert_eq!(eff.preset("home").unwrap().pick, Some(Pick::Fastest));
        // Without preset_order: alphabetical. Without tables: default source.
        let alpha = FileConfig::parse("[presets.b]\n[presets.a]\n").unwrap();
        assert_eq!(alpha.ordered_presets()[0].name, "a");
        let none = FileConfig::default().resolve(&Overrides::default(), Overrides::default());
        assert_eq!(
            (none.presets.value.len(), none.presets.source),
            (0, Source::Default)
        );
    }

    #[test]
    fn preset_errors_name_key_and_line() {
        for (n, text) in [
            "[presets.work]\nlocation = \"DE\"\ncolour = \"red\"\n",
            "location = \"US\"\n[presets.Work]\nlocation = \"DE\"\n",
            "[presets.work]\nlocation = \"DE/a*b\"\n",
            "[presets.work]\nlocation = \"DE/Berlin/de1\"\n",
            "[presets.work]\nbypass_extra = [\"10.0.0.0/4\"]\n",
            "[presets.work]\nlabel = \"this label is much too long\"\n",
            "[presets.work]\npick = \"slowest\"\n",
            "preset_order = [\"home\"]\n[presets.work]\n",
            "preset_order = [\"work\", \"work\"]\n[presets.work]\n",
            "[presets.\"has space\"]\n",
        ]
        .into_iter()
        .enumerate()
        {
            insta::assert_snapshot!(format!("preset_error_{n}"), err(text));
        }
        let long = format!("[presets.{}]\n", "a".repeat(33));
        assert!(err(&long).contains("1 to 32 characters"));
    }

    #[test]
    fn env_parsing() {
        let env = |pairs: &'static [(&str, &str)]| {
            Overrides::from_env(move |k| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            })
        };
        let o = env(&[
            ("VPNLANE_LOCATION", "de/berlin"),
            ("VPNLANE_PICK", "Fastest"),
            ("VPNLANE_PRESET", "work"),
            ("VPNLANE_EXIT_IP_CHECK", "0"),
        ])
        .unwrap();
        assert_eq!(o.location, Some(loc("DE/berlin")));
        assert_eq!(
            (o.pick, o.preset.as_deref(), o.exit_ip_check),
            (Some(Pick::Fastest), Some("work"), Some(false))
        );
        assert_eq!(o.deprecated, []);
        assert_eq!(
            env(&[("VPNLANE_LOCATION", "  ")]).unwrap(),
            Overrides::default()
        );
        for (var, bad) in [
            ("VPNLANE_LOCATION", "usa"),
            ("VPNLANE_COUNTRY", "usa"),
            ("VPNLANE_PICK", "slowest"),
        ] {
            let pairs: &'static [(&str, &str)] = Box::leak(Box::new([(var, bad)]));
            let e = env(pairs).unwrap_err().to_string();
            assert!(e.contains(var), "{e}");
        }
        assert!(env(&[("VPNLANE_EXIT_IP_CHECK", "maybe")]).is_err());
    }

    #[test]
    fn vpnlane_country_is_an_alias_and_vpnlane_location_wins() {
        let env = |pairs: Vec<(&'static str, &'static str)>| {
            Overrides::from_env(move |k| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            })
            .unwrap()
        };
        let o = env(vec![("VPNLANE_COUNTRY", "de")]);
        assert_eq!(o.location, Some(loc("DE")));
        assert_eq!(
            deprecation_line(&o.deprecated).unwrap(),
            "warning: deprecated spelling, still works for this release: VPNLANE_COUNTRY -> VPNLANE_LOCATION (see docs/CONFIG.md)"
        );
        let o = env(vec![("VPNLANE_COUNTRY", "de"), ("VPNLANE_LOCATION", "fr")]);
        assert_eq!(o.location, Some(loc("FR")));
        // The deprecation reaches `Effective`, with the file's.
        let eff = FileConfig::parse("default_country = \"US\"")
            .unwrap()
            .resolve(&Overrides::default(), o);
        assert_eq!(eff.deprecated.len(), 2);
    }
}
