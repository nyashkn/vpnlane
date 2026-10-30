//! Saved settings: the `config.toml` schema, validation and precedence.
//!
//! Pure: callers read the file and the environment and hand in strings.
//! Precedence is CLI flag > environment > file > built-in default.

use std::collections::BTreeMap;
use std::fmt;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::preset::check_name;
use crate::{CountryCode, Error, NamedPreset, Preset, Result};

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

impl fmt::Display for Pick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LeastLoad => "least-load",
            Self::Fastest => "fastest",
        })
    }
}

/// The `config.toml` file as written: every key optional.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Country for `wglane up` with no argument.
    pub default_country: Option<CountryCode>,
    /// City preference; belongs to `default_country` and applies only to it.
    pub default_city: Option<String>,
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

impl FileConfig {
    /// Parses and validates `text`. A parse error names the offending key and line.
    pub fn parse(text: &str) -> Result<Self> {
        let cfg: Self = toml::from_str(text).map_err(|e| Error::Config(e.to_string()))?;
        cfg.validate().map_err(|(key, msg)| {
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

    /// On failure: the key to locate in the file, and the message.
    fn validate(&self) -> Result<(), (String, String)> {
        let at = |key: &str| key.to_owned();
        if let Some(city) = &self.default_city {
            check_city("default_city", city).map_err(|m| (at("default_city"), m))?;
        }
        check_bypass("bypass_extra", self.bypass_extra.as_deref())
            .map_err(|m| (at("bypass_extra"), m))?;
        for (name, p) in self.presets.iter().flatten() {
            // Locate the table header; a name that needs quoting is found by its opening quote.
            let bare = name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            let key = format!("[presets.{}{name}", if bare { "" } else { "\"" });
            check_name(name)
                .map_err(|m| ("", m))
                .and_then(|()| p.validate(name))
                .map_err(|(field, m)| (format!("{key}\n{field}"), m))?;
        }
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
    pub fn resolve(&self, flags: Overrides, env: Overrides) -> Effective {
        let tui = self.tui.clone().unwrap_or_default();
        Effective {
            default_country: layer(
                flags.country.map(Some),
                env.country.map(Some),
                self.default_country.map(Some),
                None,
            ),
            file_default_country: self.default_country,
            default_city: layer(None, None, self.default_city.clone().map(Some), None),
            pick: layer(None, None, self.pick, Pick::default()),
            bypass_extra: layer(None, None, self.bypass_extra.clone(), Vec::new()),
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

/// City rule shared by `default_city` and preset cities; `label` names the key.
pub(crate) fn check_city(label: &str, city: &str) -> Result<(), String> {
    let ok = !city.is_empty()
        && city.chars().count() <= 64
        && city
            .chars()
            .all(|c| c.is_alphanumeric() || " -'.".contains(c));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{label} {city:?}: use 1 to 64 letters, digits, spaces, '-', ''' or '.'"
        ))
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `default_country`.
    pub country: Option<CountryCode>,
    /// `exit_ip_check`.
    pub exit_ip_check: Option<bool>,
}

impl Overrides {
    /// Reads `WGLANE_COUNTRY` and `WGLANE_EXIT_IP_CHECK` through `lookup`.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());
        let country = get("WGLANE_COUNTRY")
            .map(|v| {
                v.parse::<CountryCode>()
                    .map_err(|e| Error::Config(format!("WGLANE_COUNTRY: {e}")))
            })
            .transpose()?;
        let exit_ip_check = get("WGLANE_EXIT_IP_CHECK")
            .map(|v| match v.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" => Ok(false),
                _ => Err(Error::Config(format!(
                    "WGLANE_EXIT_IP_CHECK {v:?}: use true or false"
                ))),
            })
            .transpose()?;
        Ok(Self {
            country,
            exit_ip_check,
        })
    }
}

/// Every setting after precedence is applied.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Effective {
    /// Country for `up` with no argument; `None` when unset.
    pub default_country: Setting<Option<CountryCode>>,
    /// `default_country` as the file wrote it, before flag and environment.
    #[serde(skip)]
    pub file_default_country: Option<CountryCode>,
    /// City for `default_country` as written in the file; inactive when the
    /// country comes from elsewhere (see [`Effective::default_city_for`]).
    pub default_city: Setting<Option<String>>,
    /// Server selection rule.
    pub pick: Setting<Pick>,
    /// Extra bypass CIDRs (not the auto ones).
    pub bypass_extra: Setting<Vec<IpNet>>,
    /// Look up the exit IP.
    pub exit_ip_check: Setting<bool>,
    /// TUI settings.
    pub tui: TuiSettings,
    /// Presets in number-key order (`file` when any table exists).
    pub presets: Setting<Vec<NamedPreset>>,
}

impl Effective {
    /// `default_city` if it applies to `country`: it belongs to the file's
    /// `default_country`, so any other country (env, flag, preset) ignores it.
    pub fn default_city_for(&self, country: Option<CountryCode>) -> Option<String> {
        self.default_city
            .value
            .clone()
            .filter(|_| country.is_some() && country == self.file_default_country)
    }

    /// The preset called `name`.
    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets
            .value
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.preset)
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

/// What `wglane config init` writes. Every key is commented out, so a fresh
/// file changes nothing.
pub const SAMPLE_CONFIG: &str = r#"# wglane settings. Every key is optional; uncomment what you want.
# Precedence: CLI flag > environment > this file > built-in default.
# Reference: docs/CONFIG.md. The Nord token does NOT go here: it stays in
# <config dir>/token (mode 600) or WGLANE_TOKEN.

# Country `wglane up` connects to when none is given (two letters).
# Env: WGLANE_COUNTRY.
# default_country = "US"

# Preferred city for default_country; ignored when another country is used.
# default_city = "Chicago"

# Server choice: "least-load" or "fastest" (`up --fastest` for one run).
# pick = "least-load"

# Extra CIDRs that skip the tunnel, added to the automatic Tailscale ones.
# At most 1024 in total; IPv4 prefixes /8 or longer.
# bypass_extra = ["198.51.100.0/24"]

# Look up the exit IP in `status` and the TUI. Env: WGLANE_EXIT_IP_CHECK.
# exit_ip_check = true

# Presets: named bundles applied with `wglane preset apply NAME` or
# `wglane up --preset NAME`. Names: a-z, 0-9, '_' and '-', up to 32. All keys
# optional. The first 9 (preset_order, then alphabetical) get number keys.
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
# country = "DE"
# city = "Berlin"
# bypass_extra = ["203.0.113.0/24"]
# [presets.home]
# country = "ZA"
# pick = "fastest"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 12] = [
        "preset_order",
        "country",
        "city",
        "label",
        "default_country",
        "default_city",
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
        let eff = cfg.resolve(Overrides::default(), Overrides::default());
        assert_eq!(eff.default_country.value, Some("US".parse().unwrap()));
        assert_eq!(eff.default_city.value.as_deref(), Some("Chicago"));
        assert_eq!(eff.bypass_extra.value.len(), 1);
        assert_eq!(eff.pick.value, Pick::LeastLoad);
        assert!(eff.exit_ip_check.value);
        assert_eq!(eff.tui.graph_samples.value, DEFAULT_GRAPH_SAMPLES);
        assert!((eff.tui.warn_ratio.value - DEFAULT_WARN_RATIO).abs() < f64::EPSILON);
        assert_eq!(eff.exit_ip_check.source, Source::File);
    }

    #[test]
    fn parses_every_key() {
        let cfg = FileConfig::parse(
            "default_country = \"za\"\npick = \"fastest\"\nexit_ip_check = false\n\
             bypass_extra = [\"203.0.113.0/24\", \"203.0.113.9/32\"]\n\
             [tui]\nstart_expanded = true\ngraph_samples = 120\nwarn_ratio = 2.0\n",
        )
        .unwrap();
        let e = cfg.resolve(Overrides::default(), Overrides::default());
        assert_eq!(e.default_country.value.unwrap().as_str(), "ZA");
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
            "default_country = \"USA\"",
            "pick = \"slowest\"",
            "default_city = \"\"",
            "default_city = \"a/b\"",
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
    fn precedence_flag_env_file_default() {
        let file = FileConfig::parse("default_country = \"DE\"\nexit_ip_check = false").unwrap();
        let cc = |s: &str| Some(s.parse::<CountryCode>().unwrap());
        let none = Overrides::default();
        let env = Overrides {
            country: cc("FR"),
            exit_ip_check: Some(true),
        };
        let flag = Overrides {
            country: cc("US"),
            exit_ip_check: None,
        };

        let e = file.resolve(none, none);
        assert_eq!(
            (e.default_country.source, e.exit_ip_check.source),
            (Source::File, Source::File)
        );
        let e = file.resolve(none, env);
        assert_eq!(e.default_country.value, cc("FR"));
        assert_eq!(
            (e.default_country.source, e.exit_ip_check.value),
            (Source::Env, true)
        );
        let e = file.resolve(flag, env);
        assert_eq!(
            (e.default_country.value, e.default_country.source),
            (cc("US"), Source::Flag)
        );
        assert_eq!(
            e.exit_ip_check.source,
            Source::Env,
            "no flag: env beats file"
        );
        let e = FileConfig::default().resolve(none, none);
        assert_eq!(
            (e.default_country.value, e.default_country.source),
            (None, Source::Default)
        );
        assert!(e.exit_ip_check.value);
    }

    #[test]
    fn presets_parse_and_order() {
        let cfg = FileConfig::parse(
            "preset_order = [\"work\", \"home\"]\n\
             [presets.zeta]\nlabel = \"Last\"\n\
             [presets.home]\ncountry = \"za\"\npick = \"fastest\"\n\
             [presets.work]\ncountry = \"DE\"\ncity = \"Berlin\"\nbypass_extra = [\"203.0.113.0/24\"]\n\
             [presets.alpha]\n",
        )
        .unwrap();
        let eff = cfg.resolve(Overrides::default(), Overrides::default());
        let names: Vec<_> = eff.presets.value.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["work", "home", "alpha", "zeta"]);
        assert_eq!(eff.presets.source, Source::File);
        assert_eq!(eff.preset("home").unwrap().pick, Some(Pick::Fastest));
        // Without preset_order: alphabetical. Without tables: default source.
        let alpha = FileConfig::parse("[presets.b]\n[presets.a]\n").unwrap();
        assert_eq!(alpha.ordered_presets()[0].name, "a");
        let none = FileConfig::default().resolve(Overrides::default(), Overrides::default());
        assert_eq!(
            (none.presets.value.len(), none.presets.source),
            (0, Source::Default)
        );
    }

    #[test]
    fn preset_errors_name_key_and_line() {
        for (n, text) in [
            "[presets.work]\ncountry = \"DE\"\ncolour = \"red\"\n",
            "default_country = \"US\"\n[presets.Work]\ncountry = \"DE\"\n",
            "[presets.work]\ncity = \"Berlin\"\n",
            "[presets.work]\ncountry = \"DE\"\ncity = \"a/b\"\n",
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
        let o = env(&[("WGLANE_COUNTRY", "us"), ("WGLANE_EXIT_IP_CHECK", "0")]).unwrap();
        assert_eq!(
            (o.country.unwrap().as_str(), o.exit_ip_check),
            ("US", Some(false))
        );
        assert_eq!(
            env(&[("WGLANE_COUNTRY", "  ")]).unwrap(),
            Overrides::default()
        );
        assert!(
            env(&[("WGLANE_COUNTRY", "usa")])
                .unwrap_err()
                .to_string()
                .contains("WGLANE_COUNTRY")
        );
        assert!(env(&[("WGLANE_EXIT_IP_CHECK", "maybe")]).is_err());
    }
}
