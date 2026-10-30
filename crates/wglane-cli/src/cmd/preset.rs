//! `wglane preset`: named bundles of settings from `[presets.*]` in config.toml.

use std::fmt::Write as _;

use color_eyre::Result;
use serde_json::json;
use wglane_core::ports::Tunnel;
use wglane_core::{
    ActiveState, BypassSet, CountryCode, Effective, EffectiveRequest, KEYED_PRESETS, NamedPreset,
    Overrides, describe_change,
};
use wglane_macos::{HelperClient, SystemRunner};

use crate::cmd::up;
use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "List, inspect and apply presets (named bundles of settings)",
    long_about = "\
A preset is a [presets.NAME] table in config.toml that bundles a connection
request: country, city, pick and extra bypass CIDRs, plus a short label.
Every key is optional; what a preset leaves out comes from the top-level
settings (see docs/CONFIG.md). The first 9 presets (preset_order, then
alphabetical) have a number key, shown by `list`; the TUI uses it too.

`apply` is `up` with that request: same code path, same flags and
environment otherwise. The name of the last applied preset is kept in
<config dir>/active-preset and cleared by `down` or by an `up` that no longer
matches it (different country, city or bypass routes). `status` shows it.

Only the shape of a preset's city is checked when the file is read; it is
checked against Nord's list when you apply, and the error lists valid cities.",
    after_help = "\
Examples:
  wglane preset list              numbered key, name, label, summary; * marks the active one
  wglane preset show work         the request, and what applying it would change now
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
            let active = active_name(ctx, &HelperClient::new(SystemRunner));
            println!("{}", list(&settings, active.as_deref(), ctx.json)?);
        }
        Action::Show { name } => {
            let settings = ctx.settings(Overrides::default())?;
            let tunnel = HelperClient::new(SystemRunner);
            let active = active_name(ctx, &tunnel);
            let current = current_state(ctx, &tunnel, &settings);
            println!(
                "{}",
                show(
                    &settings,
                    name,
                    active.as_deref(),
                    current.as_ref(),
                    ctx.json
                )?
            );
        }
        Action::Apply { name } => up::run(
            ctx,
            &up::Args {
                country: None,
                city: None,
                preset: Some(name.clone()),
                fastest: false,
            },
        )?,
    }
    Ok(())
}

/// The active preset, for the TUI header and `list`: the recorded name, if the
/// preset still exists and a tunnel is up (or cannot be read).
pub(crate) fn active_name<T: Tunnel>(ctx: &Context, tunnel: &T) -> Option<String> {
    let name = ctx.recorded_preset()?;
    match tunnel.active() {
        Ok(a) if a.is_empty() => None,
        _ => Some(name),
    }
}

/// The connection as it is now; `None` when no tunnel is up. The city is the
/// one asked for, as `up` recorded it.
pub(crate) fn current_state<T: Tunnel>(
    ctx: &Context,
    tunnel: &T,
    settings: &Effective,
) -> Option<ActiveState> {
    let cc: CountryCode = tunnel.active().ok()?.first()?.parse().ok()?;
    let base = ctx
        .recorded_preset()
        .and_then(|n| settings.preset(&n).cloned())
        .unwrap_or_default()
        .resolve(settings);
    Some(ActiveState {
        country: Some(cc),
        city: ctx.recorded_request(&cc.to_lowercase()),
        ..base
    })
}

/// After a connection: keep the active-preset note if the connection still
/// matches that preset (the one just applied, else the recorded one), else clear it.
pub(crate) fn settle(
    ctx: &Context,
    hint: Option<&str>,
    cc: CountryCode,
    city: Option<&str>,
    bypass: &BypassSet,
) -> Result<()> {
    let settings = ctx.settings(Overrides::default())?;
    let used = EffectiveRequest {
        country: Some(cc),
        city: city.map(Into::into),
        pick: settings.pick.value,
        bypass_extra: bypass.iter().copied().collect(),
    };
    let name = hint
        .map(str::to_owned)
        .or_else(|| ctx.recorded_preset())
        .filter(|n| {
            settings
                .preset(n)
                .is_some_and(|p| p.resolve(&settings).same_connection(&used))
        });
    ctx.record_preset(name.as_deref())
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
    active: Option<&str>,
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
    let req = preset.resolve(settings);
    let changes: Vec<String> = describe_change(current, preset)
        .iter()
        .map(ToString::to_string)
        .collect();
    let is_active = active == Some(name);
    if json {
        return Ok(serde_json::to_string_pretty(&json!({
            "name": name,
            "key": key_of(index),
            "label": preset.label,
            "active": is_active,
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
        if is_active { " (active)" } else { "" }
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
        "country       {}",
        req.country.map_or("(unset)".to_owned(), |c| c.to_string())
    );
    let _ = writeln!(out, "city          {}", req.city.as_deref().unwrap_or("-"));
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
    use wglane_core::{City, Country, FileConfig, Pick, PublicKey, Server};

    use super::*;

    const CFG: &str = "preset_order = [\"work\"]\nbypass_extra = [\"203.0.113.0/24\"]\n\
        [presets.work]\nlabel = \"Office exit\"\ncountry = \"DE\"\ncity = \"Berlin\"\n\
        bypass_extra = [\"203.0.113.9/32\", \"203.0.113.10/32\"]\n\
        [presets.home]\ncountry = \"ZA\"\npick = \"fastest\"\n\
        [presets.ghost]\ncountry = \"DE\"\ncity = \"Atlantis\"\n";

    fn settings() -> Effective {
        FileConfig::parse(CFG)
            .unwrap()
            .resolve(Overrides::default(), Overrides::default())
    }

    #[test]
    fn list_and_show_snapshots() {
        let s = settings();
        insta::assert_snapshot!("list", list(&s, Some("home"), false).unwrap());
        insta::assert_snapshot!("list_json", list(&s, None, true).unwrap());
        let cur = ActiveState {
            country: Some("ZA".parse().unwrap()),
            city: None,
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
            show(&s, "work", Some("work"), Some(&cur), true).unwrap()
        );
        let e = show(&s, "nope", None, None, false).unwrap_err().to_string();
        assert_eq!(
            e,
            "unknown preset \"nope\": defined: work, ghost, home (see `wglane preset list`)"
        );
        assert!(
            list(
                &FileConfig::default().resolve(Overrides::default(), Overrides::default()),
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
            countries: vec![country("DE", vec![berlin]), country("ZA", vec![jnb])],
            servers: [
                (cc("DE"), vec![server("DE", "Berlin")]),
                (cc("ZA"), vec![server("ZA", "Johannesburg")]),
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

        fn up(
            &self,
            country: Option<&str>,
            city: Option<&str>,
            preset: Option<&str>,
        ) -> Result<up::Connected> {
            let ctx = self.ctx();
            let args = up::Args {
                country: country.map(cc),
                city: city.map(Into::into),
                preset: preset.map(Into::into),
                fastest: false,
            };
            up::connect_target(
                &ctx,
                &self.tunnel,
                &self.routes,
                &FakeBypassProvider(BypassSet::default()),
                &catalog(),
                &FakeToken(Some("tok".into())),
                up::plan(&ctx, &args)?,
                None,
            )
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
        assert_eq!(ctx.recorded_preset().as_deref(), Some("work"));
        assert_eq!(active_name(&ctx, &rig.tunnel).as_deref(), Some("work"));
        assert_eq!(
            current_state(&ctx, &rig.tunnel, &settings())
                .unwrap()
                .city
                .as_deref(),
            Some("Berlin")
        );

        // A plain `up` to the same place lacks the preset's bypass routes: no longer "work".
        rig.up(Some("DE"), Some("berlin"), None).unwrap();
        assert_eq!(ctx.recorded_preset(), None);
    }

    #[test]
    fn active_preset_is_cleared_by_a_different_up_and_by_down() {
        let rig = Rig::new();
        let ctx = rig.ctx();
        rig.up(None, None, Some("home")).unwrap();
        assert_eq!(ctx.recorded_preset().as_deref(), Some("home"));
        // Explicit flags beat the preset: no longer "home".
        rig.up(Some("ZA"), Some("Johannesburg"), Some("home"))
            .unwrap();
        assert_eq!(ctx.recorded_preset(), None);
        rig.up(None, None, Some("home")).unwrap();
        // Same country, plain up: matches home exactly (no city, same routes).
        rig.up(Some("ZA"), None, None).unwrap();
        assert_eq!(ctx.recorded_preset().as_deref(), Some("home"));
        // Different country: cleared.
        rig.up(Some("DE"), None, None).unwrap();
        assert_eq!(ctx.recorded_preset(), None);
        // Preset with a flag override is not the preset any more.
        rig.up(Some("ZA"), None, Some("work")).unwrap();
        assert_eq!(ctx.recorded_preset(), None);
        rig.up(None, None, Some("work")).unwrap();
        assert_eq!(ctx.recorded_preset().as_deref(), Some("work"));
        crate::cmd::down::disconnect_recorded(&ctx, &rig.tunnel).unwrap();
        assert_eq!(ctx.recorded_preset(), None);
        assert_eq!(active_name(&ctx, &rig.tunnel), None);
    }

    #[test]
    fn a_stale_record_is_not_active_when_nothing_is_up() {
        let rig = Rig::new();
        let ctx = rig.ctx();
        ctx.record_preset(Some("work")).unwrap();
        assert_eq!(active_name(&ctx, &rig.tunnel), None);
        assert!(current_state(&ctx, &rig.tunnel, &settings()).is_none());
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
        assert_eq!(rig.ctx().recorded_preset(), None);
    }

    #[test]
    fn explicit_country_drops_the_presets_city() {
        let rig = Rig::new();
        rig.up(Some("ZA"), None, Some("work")).unwrap();
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert_eq!(
            rig.ctx().recorded_request("za"),
            None,
            "preset city Berlin dropped"
        );
        // Same country as the preset: its city stays.
        rig.up(Some("DE"), None, Some("work")).unwrap();
        assert_eq!(rig.ctx().recorded_request("de").as_deref(), Some("Berlin"));
    }
}
