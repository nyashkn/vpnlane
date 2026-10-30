//! `wglane preset`: named bundles of settings from `[presets.*]` in config.toml.

use std::fmt::Write as _;

use color_eyre::Result;
use serde_json::json;
use wglane_core::ports::Tunnel;
use wglane_core::{
    ActiveState, CountryCode, Effective, KEYED_PRESETS, Location, NamedPreset, Overrides, Pick,
    PresetUse, describe_change,
};
use wglane_macos::{HelperClient, SystemRunner};

use crate::cmd::up;
use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "List, inspect and apply presets (named bundles of settings)",
    long_about = "\
A preset is a [presets.NAME] table in config.toml: an overlay of the top-level
connection keys (location, pick, bypass_extra) plus a short label. Every key
is optional and means what it means at the top level; scalars replace, and
bypass_extra is added to the top-level list. The first 9 presets
(preset_order, then alphabetical) have a number key, shown by `list`; the TUI
uses it too.

`apply` is `up --preset NAME`: same code path, same environment. The applied
preset is recorded in <config dir>/state.json (never in config.toml), shown by
`status` (as `NAME (location overridden)` when you named another place), and
cleared by `down`.

Only the shape of a preset's city is checked when the file is read; it is
checked against Nord's list when you apply, and the error lists valid cities.",
    after_help = "\
Examples:
  wglane preset list              numbered key, name, label, summary; * marks the active one
  wglane preset show work         the resolved request, and what applying it would change now
  wglane preset apply work        connect (same as `wglane up --preset work`)
  wglane preset list --json

Config:
  preset_order = [\"work\", \"home\"]
  [presets.work]
  label = \"Office exit\"
  country = \"DE\"
  city = \"Berlin\""
)]
pub(crate) struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// List presets in key order; `*` marks the active one.
    List,
    /// Show a preset's effective request and what applying it would change.
    Show {
        /// Preset name.
        name: String,
    },
    /// Connect with a preset (same as `wglane up --preset NAME`).
    Apply {
        /// Preset name.
        name: String,
    },
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    match &args.action {
        Action::List => {
            let settings = ctx.settings(Overrides::default())?;
            let active = active_preset(ctx, &HelperClient::new(SystemRunner));
            println!(
                "{}",
                list(
                    &settings,
                    active.as_ref().map(|p| p.name.as_str()),
                    ctx.json
                )?
            );
        }
        Action::Show { name } => {
            let settings = ctx.settings(Overrides::default())?;
            let tunnel = HelperClient::new(SystemRunner);
            let active = active_preset(ctx, &tunnel);
            let current = current_state(ctx, &tunnel);
            println!(
                "{}",
                show(&settings, name, active.as_ref(), current.as_ref(), ctx.json)?
            );
        }
        Action::Apply { name } => up::run(
            ctx,
            &up::Args {
                target: None,
                city: None,
                preset: Some(name.clone()),
                pick: None,
                fastest: false,
            },
        )?,
    }
    Ok(())
}

/// The active preset, for the TUI header, `list` and `status`: the one the
/// live connection was applied from, with what was overridden. `None` when no
/// preset was used, `down` has run, or the helper says no tunnel is up.
pub(crate) fn active_preset<T: Tunnel>(ctx: &Context, tunnel: &T) -> Option<PresetUse> {
    let preset = ctx.active_preset()?;
    match tunnel.active() {
        Ok(a) if a.is_empty() => None,
        _ => Some(preset),
    }
}

/// The connection as it is now; `None` when no tunnel is up. Taken from the
/// last request when it is for the tunnel's country, else just the country.
pub(crate) fn current_state<T: Tunnel>(ctx: &Context, tunnel: &T) -> Option<ActiveState> {
    let cc: CountryCode = tunnel.active().ok()?.first()?.parse().ok()?;
    Some(
        match ctx.last_request().filter(|l| l.location.country() == cc) {
            Some(l) => ActiveState {
                location: Some(l.location),
                pick: l.pick,
                bypass_extra: l.bypass_extra,
            },
            None => ActiveState {
                location: Some(Location::country_only(cc)),
                pick: Pick::default(),
                bypass_extra: Vec::new(),
            },
        },
    )
}

fn key_of(index: usize) -> Option<usize> {
    (index < KEYED_PRESETS).then_some(index + 1)
}

fn list(settings: &Effective, active: Option<&str>, json: bool) -> Result<String> {
    let presets = &settings.presets.value;
    if json {
        let rows: Vec<_> = presets
            .iter()
            .enumerate()
            .map(|(i, p)| {
                json!({
                    "key": key_of(i),
                    "name": p.name,
                    "label": p.preset.label,
                    "summary": p.preset.summary(),
                    "active": active == Some(p.name.as_str()),
                })
            })
            .collect();
        return Ok(serde_json::to_string_pretty(&rows)?);
    }
    if presets.is_empty() {
        return Ok(
            "no presets: add [presets.NAME] tables to config.toml (see `wglane config init`)"
                .to_owned(),
        );
    }
    let w = |f: fn(&NamedPreset) -> usize| presets.iter().map(f).max().unwrap_or(0);
    let (nw, lw) = (
        w(|p| p.name.len()).max(4),
        w(|p| p.preset.label.as_deref().map_or(0, |l| l.chars().count())).max(5),
    );
    let mut out = format!("  KEY  {:<nw$}  {:<lw$}  SUMMARY\n", "NAME", "LABEL");
    for (i, p) in presets.iter().enumerate() {
        let mark = if active == Some(p.name.as_str()) {
            '*'
        } else {
            ' '
        };
        let key = key_of(i).map_or("-".to_owned(), |k| k.to_string());
        let _ = writeln!(
            out,
            "{mark} {key:<4} {:<nw$}  {:<lw$}  {}",
            p.name,
            p.preset.label.as_deref().unwrap_or(""),
            p.preset.summary()
        );
    }
    Ok(out.trim_end().to_owned())
}

fn show(
    settings: &Effective,
    name: &str,
    active: Option<&PresetUse>,
    current: Option<&ActiveState>,
    json: bool,
) -> Result<String> {
    let Some(index) = settings.presets.value.iter().position(|p| p.name == name) else {
        let names: Vec<_> = settings
            .presets
            .value
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        return Err(color_eyre::eyre::eyre!(
            "unknown preset {name:?}: {} (see `wglane preset list`)",
            if names.is_empty() {
                "none defined in config.toml".to_owned()
            } else {
                format!("defined: {}", names.join(", "))
            }
        ));
    };
    let preset = &settings.presets.value[index].preset;
    let req = settings.resolved_presets().swap_remove(index).request;
    let changes: Vec<String> = describe_change(current, preset, settings)
        .iter()
        .map(ToString::to_string)
        .collect();
    let active = active.filter(|a| a.name == name);
    let is_active = active.is_some();
    if json {
        return Ok(serde_json::to_string_pretty(&json!({
            "name": name,
            "key": key_of(index),
            "label": preset.label,
            "active": is_active,
            "overridden": active.map(|a| &a.overridden),
            "request": req,
            "connected": current.is_some(),
            "changes": changes,
        }))?);
    }
    let nets = if req.bypass_extra.is_empty() {
        "[]".to_owned()
    } else {
        req.bypass_extra
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "preset        {name}{}",
        active.map_or(String::new(), |a| format!(
            " (active{})",
            if a.overridden.is_empty() {
                String::new()
            } else {
                format!(", {} overridden", a.overridden.join(", "))
            }
        ))
    );
    let _ = writeln!(
        out,
        "key           {}",
        key_of(index).map_or("-".to_owned(), |k| k.to_string())
    );
    if let Some(l) = &preset.label {
        let _ = writeln!(out, "label         {l}");
    }
    let _ = writeln!(
        out,
        "location      {}",
        req.location.map_or("(unset)".to_owned(), |l| l.to_string())
    );
    let _ = writeln!(out, "pick          {}", req.pick);
    let _ = writeln!(out, "bypass_extra  {nets}");
    let _ = writeln!(
        out,
        "\nwould change{}:",
        if current.is_some() {
            ""
        } else {
            " (not connected)"
        }
    );
    for c in &changes {
        let _ = writeln!(out, "  {c}");
    }
    Ok(out.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use wglane_core::testing::{
        FakeBypassProvider, FakeCatalog, FakeRoutes, FakeToken, FakeTunnel,
    };
    use wglane_core::{City, Country, FileConfig, PublicKey, Server};

    use super::*;

    const CFG: &str = "preset_order = [\"work\"]\nbypass_extra = [\"203.0.113.0/24\"]\n\
        [presets.work]\nlabel = \"Office exit\"\nlocation = \"DE/Berlin\"\n\
        bypass_extra = [\"203.0.113.9/32\", \"203.0.113.10/32\"]\n\
        [presets.home]\nlocation = \"ZA\"\npick = \"fastest\"\n\
        [presets.ghost]\nlocation = \"DE/Atlantis\"\n";

    fn settings() -> Effective {
        FileConfig::parse(CFG)
            .unwrap()
            .resolve(&Overrides::default(), Overrides::default())
    }

    fn use_of(name: &str, overridden: &[&str]) -> PresetUse {
        PresetUse {
            name: name.into(),
            overridden: overridden.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn list_and_show_snapshots() {
        let s = settings();
        insta::assert_snapshot!("list", list(&s, Some("home"), false).unwrap());
        insta::assert_snapshot!("list_json", list(&s, None, true).unwrap());
        let cur = ActiveState {
            location: Some("ZA".parse().unwrap()),
            pick: Pick::LeastLoad,
            bypass_extra: vec!["203.0.113.0/24".parse().unwrap()],
        };
        insta::assert_snapshot!(
            "show_connected",
            show(&s, "work", None, Some(&cur), false).unwrap()
        );
        insta::assert_snapshot!("show_down", show(&s, "home", None, None, false).unwrap());
        insta::assert_snapshot!(
            "show_json",
            show(&s, "work", Some(&use_of("work", &[])), Some(&cur), true).unwrap()
        );
        insta::assert_snapshot!(
            "show_overridden",
            show(
                &s,
                "work",
                Some(&use_of("work", &["location"])),
                Some(&cur),
                false
            )
            .unwrap()
        );
        let e = show(&s, "nope", None, None, false).unwrap_err().to_string();
        assert_eq!(
            e,
            "unknown preset \"nope\": defined: work, ghost, home (see `wglane preset list`)"
        );
        assert!(
            list(
                &FileConfig::default().resolve(&Overrides::default(), Overrides::default()),
                None,
                false
            )
            .unwrap()
            .starts_with("no presets")
        );
    }

    fn cc(s: &str) -> CountryCode {
        s.parse().unwrap()
    }

    fn server(country: &str, city: &str) -> Server {
        Server {
            hostname: format!("{}1.nordvpn.com", country.to_lowercase()),
            station: "192.0.2.2".parse().unwrap(),
            load: 9,
            public_key: PublicKey("pk".into()),
            country: cc(country),
            city: Some(city.into()),
        }
    }

    fn catalog() -> FakeCatalog {
        let berlin = City {
            id: 1,
            name: "Berlin".into(),
            identifier: "de_berlin".into(),
            server_count: 1,
        };
        let jnb = City {
            id: 2,
            name: "Johannesburg".into(),
            identifier: "za_jnb".into(),
            server_count: 1,
        };
        let country = |code: &str, cities| Country {
            name: code.into(),
            code: cc(code),
            cities,
        };
        FakeCatalog {
            countries: vec![
                country("DE", vec![berlin]),
                country("ZA", vec![jnb]),
                country("FR", vec![]),
            ],
            servers: [
                (cc("DE"), vec![server("DE", "Berlin")]),
                (cc("ZA"), vec![server("ZA", "Johannesburg")]),
                (cc("FR"), vec![server("FR", "Paris")]),
            ]
            .into(),
            private_key: Some("priv".into()),
        }
    }

    struct Rig {
        dir: tempfile::TempDir,
        tunnel: FakeTunnel,
        routes: FakeRoutes,
    }

    impl Rig {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("config.toml"), CFG).unwrap();
            Self {
                dir,
                tunnel: FakeTunnel::default(),
                routes: FakeRoutes::new("192.0.2.254".parse::<IpAddr>().unwrap()),
            }
        }

        fn ctx(&self) -> Context {
            Context {
                json: false,
                verbose: 0,
                config_dir: self.dir.path().to_owned(),
            }
        }

        fn args(target: Option<&str>, city: Option<&str>, preset: Option<&str>) -> up::Args {
            up::Args {
                target: target.map(|t| t.parse().unwrap()),
                city: city.map(Into::into),
                preset: preset.map(Into::into),
                pick: None,
                fastest: false,
            }
        }

        fn run(&self, args: &up::Args) -> Result<up::Connected> {
            let ctx = self.ctx();
            up::connect_target(
                &ctx,
                &self.tunnel,
                &self.routes,
                &FakeBypassProvider(wglane_core::BypassSet::default()),
                &catalog(),
                &FakeToken(Some("tok".into())),
                up::plan(&ctx, args)?,
                None,
            )
        }

        fn up(
            &self,
            target: Option<&str>,
            city: Option<&str>,
            preset: Option<&str>,
        ) -> Result<up::Connected> {
            self.run(&Self::args(target, city, preset))
        }

        fn active(&self) -> Option<String> {
            active_preset(&self.ctx(), &self.tunnel).map(|p| p.describe())
        }

        fn last(&self) -> String {
            self.ctx()
                .last_request()
                .map_or("-".to_owned(), |l| l.location.to_string())
        }
    }

    #[test]
    fn apply_connects_with_the_presets_request_and_records_it() {
        let rig = Rig::new();
        let ctx = rig.ctx();
        let done = rig.up(None, None, Some("work")).unwrap();
        assert!(done.summary(cc("DE")).starts_with("connected to DE"));
        assert_eq!(*rig.tunnel.up.borrow(), ["de"]);
        // Base route plus the preset's two.
        assert_eq!(rig.routes.installed.borrow().len(), 3);
        assert_eq!(ctx.recorded_request("de").as_deref(), Some("Berlin"));
        assert_eq!(rig.active().as_deref(), Some("work"));
        let cur = current_state(&ctx, &rig.tunnel).unwrap();
        assert_eq!(cur.location.unwrap().to_string(), "DE/Berlin");
        assert_eq!(cur.bypass_extra.len(), 3);

        // A plain `up` to the same place is not a preset apply: no preset routes.
        rig.up(Some("DE/berlin"), None, None).unwrap();
        assert_eq!(rig.active(), None);
        assert_eq!(ctx.last_request().unwrap().bypass_extra.len(), 1);
    }

    #[test]
    fn active_preset_follows_apply_override_and_down() {
        let rig = Rig::new();
        let ctx = rig.ctx();
        rig.up(None, None, Some("home")).unwrap();
        assert_eq!(rig.active().as_deref(), Some("home"));
        // The user overrode the preset's place: still the preset, and it says so.
        rig.up(Some("FR"), None, Some("home")).unwrap();
        assert_eq!(rig.active().as_deref(), Some("home (location overridden)"));
        // The same place spelled another way is not an override.
        rig.up(Some("za"), None, Some("home")).unwrap();
        assert_eq!(rig.active().as_deref(), Some("home"));
        // A --pick that differs from the preset's.
        let mut a = Rig::args(None, None, Some("home"));
        a.pick = Some(Pick::LeastLoad);
        rig.run(&a).unwrap();
        assert_eq!(rig.active().as_deref(), Some("home (pick overridden)"));
        crate::cmd::down::disconnect_recorded(&ctx, &rig.tunnel).unwrap();
        assert_eq!(ctx.active_preset(), None);
        assert_eq!(active_preset(&ctx, &rig.tunnel), None);
    }

    #[test]
    fn up_dash_repeats_the_last_request_even_after_down() {
        let rig = Rig::new();
        let ctx = rig.ctx();
        let e = rig.up(Some("-"), None, None).unwrap_err().to_string();
        assert!(e.contains("no previous connection"), "{e}");

        rig.up(Some("FR"), None, Some("work")).unwrap();
        assert_eq!(rig.last(), "FR");
        crate::cmd::down::disconnect_recorded(&ctx, &rig.tunnel).unwrap();
        assert!(rig.tunnel.up.borrow().is_empty());

        let done = rig.up(Some("-"), None, None).unwrap();
        assert!(done.summary(cc("FR")).starts_with("connected to FR"));
        assert_eq!(*rig.tunnel.up.borrow(), ["fr"]);
        assert_eq!(
            rig.routes.installed.borrow().len(),
            3,
            "the preset's routes came back"
        );
        assert_eq!(rig.active().as_deref(), Some("work (location overridden)"));

        // Plain `up` is the base config again, not "last".
        let e = rig.up(None, None, None).unwrap_err().to_string();
        assert!(e.contains("no location given"), "{e}");
        // `-` takes no companions.
        for bad in [
            (Some("-"), Some("Berlin"), None),
            (Some("-"), None, Some("work")),
        ] {
            let e = rig.up(bad.0, bad.1, bad.2).unwrap_err().to_string();
            assert!(e.contains("repeats the last request"), "{e}");
        }
        let mut a = Rig::args(Some("-"), None, None);
        a.fastest = true;
        assert!(rig.run(&a).is_err());
    }

    #[test]
    fn city_is_sugar_for_the_second_half_of_location() {
        let rig = Rig::new();
        rig.up(Some("DE"), Some("berlin"), None).unwrap();
        assert_eq!(rig.last(), "DE/Berlin");
        rig.up(Some("DE/Berlin"), None, None).unwrap();
        assert_eq!(rig.last(), "DE/Berlin");
        // Alone, --city takes the country from the preset.
        rig.up(None, Some("Johannesburg"), Some("home")).unwrap();
        assert_eq!(rig.last(), "ZA/Johannesburg");
        let e = rig.up(Some("DE/Berlin"), Some("Berlin"), None).unwrap_err();
        insta::assert_snapshot!("city_twice", e.to_string());
        let e = rig.up(None, Some("Berlin"), None).unwrap_err();
        insta::assert_snapshot!("city_without_country", e.to_string());
    }

    #[test]
    fn a_positional_location_replaces_the_presets_whole_location() {
        let rig = Rig::new();
        // work is DE/Berlin; naming DE drops the city (replace, not a special case).
        rig.up(Some("DE"), None, Some("work")).unwrap();
        assert_eq!(rig.last(), "DE");
        assert_eq!(rig.ctx().recorded_request("de"), None);
        assert_eq!(rig.active().as_deref(), Some("work (location overridden)"));
        // The preset's own place, spelled differently, is not an override.
        rig.up(Some("de/BERLIN"), None, Some("work")).unwrap();
        assert_eq!(rig.active().as_deref(), Some("work"));
    }

    #[test]
    fn pick_flag_and_its_fastest_alias() {
        let rig = Rig::new();
        let ctx = rig.ctx();
        let pick = |a: &up::Args| format!("{:?}", up::plan(&ctx, a).unwrap());
        let mut a = Rig::args(Some("ZA"), None, None);
        assert!(pick(&a).contains("pick: LeastLoad"));
        a.fastest = true;
        assert!(pick(&a).contains("pick: Fastest"));
        // The preset's pick applies unless a flag says otherwise.
        let mut home = Rig::args(None, None, Some("home"));
        assert!(pick(&home).contains("pick: Fastest"));
        home.pick = Some(Pick::LeastLoad);
        assert!(pick(&home).contains("pick: LeastLoad"));
    }

    #[test]
    fn a_tui_connect_elsewhere_keeps_the_presets_routes() {
        let rig = Rig::new();
        rig.up(None, None, Some("work")).unwrap();
        let ctx = rig.ctx();
        let plan = up::plan_from_last(&ctx, "ZA/Johannesburg".parse().unwrap()).unwrap();
        up::connect_target(
            &ctx,
            &rig.tunnel,
            &rig.routes,
            &FakeBypassProvider(wglane_core::BypassSet::default()),
            &catalog(),
            &FakeToken(Some("tok".into())),
            plan,
            None,
        )
        .unwrap();
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert_eq!(
            rig.routes.installed.borrow().len(),
            3,
            "base + the preset's two"
        );
        assert_eq!(rig.active().as_deref(), Some("work (location overridden)"));
        assert_eq!(ctx.applied_bypass().unwrap().0.len(), 3);
    }

    #[test]
    fn unknown_preset_and_unknown_city_fail_before_the_tunnel_is_touched() {
        let rig = Rig::new();
        rig.tunnel.up.borrow_mut().push("us".into());
        let e = rig.up(None, None, Some("nope")).unwrap_err().to_string();
        assert!(e.contains("unknown preset"), "{e}");
        let e = rig.up(None, None, Some("ghost")).unwrap_err().to_string();
        assert!(e.contains("Atlantis") && e.contains("Berlin"), "{e}");
        assert_eq!(*rig.tunnel.up.borrow(), ["us"]);
        assert_eq!(rig.ctx().active_preset(), None);
        assert_eq!(rig.ctx().last_request(), None);
    }
}
