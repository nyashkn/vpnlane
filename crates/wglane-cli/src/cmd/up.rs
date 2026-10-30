//! `wglane up <CC>`: connect, regenerating the config if missing or stale.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use wglane_core::ports::{BypassProvider, Prober, RouteBypass, ServerCatalog, TokenSource, Tunnel};
use wglane_core::{
    BypassSet, Choice, City, CountryCode, Effective, Error, FASTEST_CANDIDATES, LastRequest,
    Location, MAX_BYPASS, Overrides, Pick, PresetUse, Source, generate_config, generate_config_by,
    is_stale, render_wg_conf,
};
use wglane_macos::{HelperClient, StaticBypass, SystemRunner, TailscaleBypass};
use wglane_nord::NordApi;

use crate::context::{Context, unix_now};

#[derive(Debug, clap::Args)]
#[command(
    about = "Connect to a location (switches if already connected)",
    long_about = "\
Connect to the least-loaded NordLynx server at LOCATION: a country (`DE`) or a
city in one (`DE/Berlin`; name or identifier, case and accents do not matter;
`wglane ls DE` lists them). `-` repeats the last request exactly (place, pick,
bypass routes, preset), like `cd -`; with no argument the configured
`location` is used. Changing between a city and the whole country regenerates
the config.

--city CITY is sugar for the city half: `up DE --city Berlin` is `up DE/Berlin`.
Without LOCATION it takes the country from the layers below (preset,
WGLANE_LOCATION, config.toml).

With --pick fastest (or --fastest, WGLANE_PICK, or `pick = \"fastest\"` in
config.toml) the 10 least-loaded candidates are pinged at once (about 2 s at
most) and the lowest round trip wins; ties go to the lower load, and if no
server answers the least-loaded one is used and the summary says so. It always
picks anew, so it regenerates the config even when a fresh one exists. While a
wglane tunnel is up, pings are bound to the physical interface so they do not
measure the tunnel's exit; if every ping fails while connected, run `wglane
down` first.

If the config for the country is missing or older than 7 days it is
regenerated first (no separate `gen` step). An existing wglane tunnel is
brought down.

With --preset NAME (or WGLANE_PRESET) the request is the top-level settings
with [presets.NAME] laid over them: the preset's `location` and `pick`
replace, its `bypass_extra` is added. Then WGLANE_LOCATION and WGLANE_PICK,
then LOCATION, --pick and --fastest, win over the preset. A LOCATION replaces
the preset's whole location, city included. When something was overridden,
`wglane status` shows `NAME (location overridden)`. `wglane preset apply NAME`
is the same code path. Precedence for every key is in docs/CONFIG.md.

Why Tailscale breaks, and what wglane does:
  A full tunnel (AllowedIPs 0.0.0.0/0) also captures Tailscale's underlay:
  the control server and DERP relays. netcheck then reports UDP false and the
  tailnet dies, although 100.64.0.0/10 still routes. wglane adds host routes
  for those IPs via your physical gateway, so the tailnet keeps working and
  everything else exits through the VPN. The config has no DNS= line, so
  MagicDNS keeps working too.",
    after_help = "\
Examples:
  wglane up ZA            South Africa
  wglane up us            case does not matter
  wglane up DE/Berlin     a city
  wglane up US --city \"New York\"
  wglane up               the configured location (or WGLANE_LOCATION)
  wglane up -             the last request again, as it was
  wglane up DE --pick fastest     lowest latency among the least-loaded servers
  wglane up --preset work         apply [presets.work] from config.toml
  wglane up --preset work FR      the preset, but exit in France
  wglane -v up DE         show each step"
)]
pub(crate) struct Args {
    /// `CC` (US, DE, ZA; not UK, use GB), `CC/City`, or `-` for the last
    /// request. Default: `location` from `config.toml` or `WGLANE_LOCATION`.
    #[arg(value_name = "LOCATION")]
    pub(crate) target: Option<Target>,

    /// City in the country (see `wglane ls CC`); same as `CC/City`.
    #[arg(long, value_name = "NAME")]
    pub(crate) city: Option<String>,

    /// Connect with a preset from `[presets.NAME]` in `config.toml`. An
    /// explicit LOCATION, --city or --pick overrides the preset's.
    /// Default: `WGLANE_PRESET`.
    #[arg(long, value_name = "NAME")]
    pub(crate) preset: Option<String>,

    /// How to choose the server: `least-load` or `fastest` (lowest latency
    /// among the least loaded). Overrides `pick` in `config.toml`, the preset
    /// and `WGLANE_PICK`.
    #[arg(long, value_name = "RULE")]
    pub(crate) pick: Option<Pick>,

    /// Short for `--pick fastest`.
    #[arg(long, conflicts_with = "pick")]
    pub(crate) fastest: bool,
}

/// The positional argument: a place, or `-` for the last request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// `-`.
    Last,
    /// `CC` or `CC/City`.
    Place(Location),
}

impl std::str::FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> wglane_core::Result<Self> {
        if s == "-" {
            Ok(Self::Last)
        } else {
            s.parse().map(Self::Place)
        }
    }
}

/// What `up` will do, from the arguments and the saved settings.
#[derive(Debug)]
pub(crate) struct Plan {
    /// Where; the city is not yet checked against the network.
    location: Location,
    /// Config `bypass_extra` plus the preset's.
    bypass: BypassSet,
    preset: Option<PresetUse>,
    pick: Pick,
}

/// Keys of the preset overlay the final request differs from.
fn overridden(settings: &Effective, preset: &str, location: &Location, pick: Pick) -> Vec<String> {
    let want = settings.overlay(settings.preset(preset));
    let mut out = Vec::new();
    if want
        .location
        .as_ref()
        .is_none_or(|l| !l.same_place(location))
    {
        out.push("location".to_owned());
    }
    if want.pick != pick {
        out.push("pick".to_owned());
    }
    out
}

fn unknown_preset(settings: &Effective, name: &str) -> color_eyre::Report {
    let names: Vec<_> = settings
        .presets
        .value
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let from = match settings.preset.source {
        Source::Env => " (from WGLANE_PRESET)",
        _ => "",
    };
    eyre!(
        "unknown preset {name:?}{from}: {} (see `wglane preset list`)",
        if names.is_empty() {
            "none defined in config.toml".to_owned()
        } else {
            format!("defined: {}", names.join(", "))
        }
    )
}

/// `up -`: the last request again, untouched.
fn replay(ctx: &Context, args: &Args) -> Result<Plan> {
    if args.city.is_some() || args.preset.is_some() || args.pick.is_some() || args.fastest {
        return Err(eyre!(
            "`up -` repeats the last request as it was: drop --city, --preset, --pick and --fastest, or name a location"
        ));
    }
    let last = ctx
        .last_request()
        .ok_or_else(|| eyre!("no previous connection to repeat: run `wglane up CC` first"))?;
    Ok(Plan {
        location: last.location,
        bypass: last.bypass_extra.into_iter().collect(),
        preset: last.preset,
        pick: last.pick,
    })
}

/// Layers, lowest first: config.toml, the preset, `WGLANE_LOCATION` and
/// `WGLANE_PICK`, then LOCATION, `--city`, `--pick`. `up -` skips them all.
pub(crate) fn plan(ctx: &Context, args: &Args) -> Result<Plan> {
    if args.target == Some(Target::Last) {
        return replay(ctx, args);
    }
    let settings = ctx.settings(Overrides {
        preset: args.preset.clone(),
        ..Overrides::default()
    })?;
    let preset_name = settings.preset.value.clone();
    let preset = preset_name
        .as_deref()
        .map(|n| {
            settings
                .preset(n)
                .ok_or_else(|| unknown_preset(&settings, n))
        })
        .transpose()?;
    let mut flags = Overrides {
        pick: args.pick.or(args.fastest.then_some(Pick::Fastest)),
        ..Overrides::default()
    };
    flags.location = match (&args.target, &args.city) {
        (Some(Target::Place(l)), None) => Some(l.clone()),
        (Some(Target::Place(l)), Some(city)) if l.city().is_none() => {
            Some(Location::new(l.country(), Some(city.clone()))?)
        }
        (Some(Target::Place(l)), Some(_)) => {
            return Err(eyre!(
                "{l} already names a city: drop --city, or pass the country alone"
            ));
        }
        (None, Some(city)) => {
            let below = settings.request(preset, &Overrides::default()).location;
            let l = below.ok_or_else(|| {
                eyre!("--city {city:?} needs a country: run `wglane up DE/{city}`")
            })?;
            Some(Location::new(l.country(), Some(city.clone()))?)
        }
        _ => None,
    };
    let req = settings.request(preset, &flags);
    let location = req.location.ok_or_else(|| {
        eyre!(
            "no location given: run `wglane up ZA`, or set location in {} (see `wglane config init`)",
            ctx.config_file().display()
        )
    })?;
    let preset = preset_name.map(|name| PresetUse {
        overridden: overridden(&settings, &name, &location, req.pick),
        name,
    });
    Ok(Plan {
        location,
        bypass: req.bypass_extra.iter().copied().collect(),
        preset,
        pick: req.pick,
    })
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let plan = plan(ctx, args)?;
    let cc = plan.location.country();
    let helper = HelperClient::new(SystemRunner);
    let prober = match plan.pick {
        Pick::Fastest => Some(crate::cmd::fastest::real_prober(&helper)?),
        Pick::LeastLoad => None,
    };
    let done = connect_target(
        ctx,
        &helper,
        &helper,
        &TailscaleBypass::new(SystemRunner),
        &NordApi::new(),
        &ctx.token_source(),
        plan,
        prober.as_ref().map(|p| p as &(dyn Prober + Sync)),
    )?;
    println!("{}", done.summary(cc));
    Ok(())
}

/// The request behind a TUI connect: the new place, everything else from the
/// last request (pick aside: the TUI does not probe, so the server is chosen
/// by load). A connect to another country thus keeps the preset's routes, and
/// the preset is marked as overridden when the place differs from its own.
#[cfg(any(feature = "tui", test))]
pub(crate) fn plan_from_last(ctx: &Context, location: Location) -> Result<Plan> {
    let settings = ctx.settings(Overrides::default())?;
    let pick = Pick::LeastLoad;
    let last = ctx.last_request();
    let bypass = match &last {
        Some(l) => l.bypass_extra.iter().copied().collect(),
        None => settings.bypass_extra.value.iter().copied().collect(),
    };
    let preset = last.and_then(|l| l.preset).map(|p| PresetUse {
        overridden: overridden(&settings, &p.name, &location, pick),
        name: p.name,
    });
    Ok(Plan {
        location,
        bypass,
        preset,
        pick,
    })
}

/// `up` against real adapters, for the TUI. Prints nothing.
#[cfg(feature = "tui")]
pub(crate) fn connect_real(
    ctx: &Context,
    cc: CountryCode,
    city: Option<&City>,
) -> Result<Connected> {
    let helper = HelperClient::new(SystemRunner);
    let plan = plan_from_last(ctx, Location::new(cc, city.map(|c| c.name.clone()))?)?;
    connect_with(
        ctx,
        &helper,
        &helper,
        &TailscaleBypass::new(SystemRunner),
        &NordApi::new(),
        &ctx.token_source(),
        (cc, city),
        &StaticBypass(plan.bypass),
        plan.preset,
        None,
    )
}

/// Resolves the plan's city (the only network check of a city, so it happens
/// before any tunnel is touched), then connects.
#[allow(
    clippy::too_many_arguments,
    reason = "ports plus the target; a struct for two callers would only move the list"
)]
pub(crate) fn connect_target<T: Tunnel, R: RouteBypass, C: ServerCatalog, S: TokenSource>(
    ctx: &Context,
    tunnel: &T,
    routes: &R,
    tailscale: &dyn BypassProvider,
    catalog: &C,
    token: &S,
    plan: Plan,
    prober: Option<&(dyn Prober + Sync)>,
) -> Result<Connected> {
    let cc = plan.location.country();
    let city = plan
        .location
        .city()
        .map(|q| crate::cmd::servers::resolve_city(catalog, cc, q))
        .transpose()?;
    connect_with(
        ctx,
        tunnel,
        routes,
        tailscale,
        catalog,
        token,
        (cc, city.as_ref()),
        &StaticBypass(plan.bypass),
        plan.preset,
        prober,
    )
}

/// `connect_recorded`, then records what was applied as the last request.
#[allow(
    clippy::too_many_arguments,
    reason = "ports plus the target; a struct for two callers would only move the list"
)]
fn connect_with<T: Tunnel, R: RouteBypass, C: ServerCatalog, S: TokenSource>(
    ctx: &Context,
    tunnel: &T,
    routes: &R,
    tailscale: &dyn BypassProvider,
    catalog: &C,
    token: &S,
    (cc, city): (CountryCode, Option<&City>),
    extra: &StaticBypass,
    preset: Option<PresetUse>,
    prober: Option<&(dyn Prober + Sync)>,
) -> Result<Connected> {
    let done = connect_recorded(
        ctx,
        tunnel,
        routes,
        &[tailscale, extra],
        catalog,
        token,
        cc,
        city,
        prober,
    )?;
    // shortcut: a city name the shape check rejects is recorded as its country; widen the check if one turns up.
    let location = Location::new(cc, city.map(|c| c.name.clone()))
        .unwrap_or_else(|_| Location::country_only(cc));
    ctx.record_applied(LastRequest {
        location,
        pick: if prober.is_some() {
            Pick::Fastest
        } else {
            Pick::LeastLoad
        },
        bypass_extra: extra.0.iter().copied().collect(),
        preset,
        at: unix_now(),
    })?;
    Ok(done)
}

/// The helper takes at most [`MAX_BYPASS`] routes; say which setting to trim.
pub(crate) fn check_limit(set: &BypassSet) -> wglane_core::Result<()> {
    if set.len() > MAX_BYPASS {
        return Err(Error::Config(format!(
            "{} bypass routes, the limit is {MAX_BYPASS}: shorten bypass_extra in config.toml",
            set.len()
        )));
    }
    Ok(())
}

/// `connect` plus the config store and the route-count record `status` reads.
#[allow(
    clippy::too_many_arguments,
    reason = "ports plus the target; a struct for two callers would only move the list"
)]
pub(crate) fn connect_recorded<T: Tunnel, R: RouteBypass, C: ServerCatalog, S: TokenSource>(
    ctx: &Context,
    tunnel: &T,
    routes: &R,
    providers: &[&dyn BypassProvider],
    catalog: &C,
    token: &S,
    cc: CountryCode,
    city: Option<&City>,
    prober: Option<&(dyn Prober + Sync)>,
) -> Result<Connected> {
    let store = ctx.conf_store();
    // The city is not part of the config's name: a cached config is reusable
    // only if it was generated for the same request (no city, or this city).
    let want = city.map(|c| c.name.as_str());
    let same = ctx.recorded_request(&cc.to_lowercase()).as_deref() == want;
    // Choosing by latency means measuring now: never reuse a cached pick.
    let age = if same && prober.is_none() {
        store.age(cc)?
    } else {
        None
    };
    let picked = RefCell::new(None);
    let mut done = connect(tunnel, routes, providers, cc, store.dir(), age, || {
        let (path, choice) = regenerate(catalog, token, cc, city, prober, |body| {
            Ok(store.write(cc, body)?)
        })?;
        *picked.borrow_mut() = Some(choice);
        Ok(path)
    })?;
    if done.regenerated {
        let choice = picked.into_inner();
        let server_city = choice.as_ref().and_then(|c| c.server.city.as_deref());
        ctx.record_city(&cc.to_lowercase(), want, server_city)?;
        done.picked = choice;
    }
    ctx.record_routes(done.routes)?;
    Ok(done)
}

/// What `connect` did, for the one-line report.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Connected {
    regenerated: bool,
    /// Other tunnels taken down first.
    switched_from: Vec<String>,
    /// The tunnel for this country was already up and kept.
    already_up: bool,
    pub(crate) routes: usize,
    /// The server a regenerated config points at, and how it was chosen.
    picked: Option<Choice>,
}

impl Connected {
    pub(crate) fn summary(&self, cc: CountryCode) -> String {
        let verb = if self.already_up {
            "already connected to"
        } else if self.switched_from.is_empty() {
            "connected to"
        } else {
            "switched to"
        };
        let regen = if self.regenerated {
            ", config regenerated"
        } else {
            ""
        };
        format!(
            "{verb} {cc}{regen}, {} bypass routes{}",
            self.routes,
            self.picked.as_ref().map_or_else(String::new, server_note)
        )
    }
}

/// `, server za3.nordvpn.com load 9% latency 41 ms` for the summary line.
fn server_note(c: &Choice) -> String {
    let mut note = format!(", server {} load {}%", c.server.hostname, c.server.load);
    if let Some(l) = c.latency {
        let _ = write!(note, " latency {} ms", l.as_millis());
    }
    if c.fell_back {
        note.push_str(" (no probe answered: picked the least loaded)");
    }
    note
}

/// Fetches a fresh config for `cc` (only `city` when given), hands the
/// rendered body to `write`, and returns the path plus the choice made. With a
/// `prober` the fastest of the [`FASTEST_CANDIDATES`] least-loaded wins,
/// else the least loaded.
pub(crate) fn regenerate<C: ServerCatalog, T: TokenSource>(
    catalog: &C,
    token: &T,
    cc: CountryCode,
    city: Option<&City>,
    prober: Option<&(dyn Prober + Sync)>,
    write: impl FnOnce(&str) -> Result<PathBuf>,
) -> Result<(PathBuf, Choice)> {
    let (conf, choice) = if let Some(p) = prober {
        generate_config_by(catalog, token, cc, city, FASTEST_CANDIDATES, |servers| {
            crate::cmd::fastest::choose(p, servers)
        })?
    } else {
        let (conf, server) = generate_config(catalog, token, cc, city)?;
        (conf, Choice::least_load(vec![server]))
    };
    Ok((write(&render_wg_conf(&conf))?, choice))
}

/// The whole `up` flow against ports. `conf_age` is `None` when the config is
/// missing; `regen` writes a new config and returns its path.
pub(crate) fn connect<T: Tunnel, R: RouteBypass>(
    tunnel: &T,
    routes: &R,
    providers: &[&dyn BypassProvider],
    cc: CountryCode,
    conf_dir: &Path,
    conf_age: Option<Duration>,
    regen: impl FnOnce() -> Result<PathBuf>,
) -> Result<Connected> {
    // Validate the bypass set first: an oversized one must fail before any
    // tunnel is touched, regenerated config written or other tunnel taken down.
    let mut set = BypassSet::new();
    for p in providers {
        set.merge(p.collect()?);
    }
    check_limit(&set)?;

    let regenerated = is_stale(conf_age);
    let conf = if regenerated {
        regen()?
    } else {
        conf_dir.join(format!("{}.conf", cc.to_lowercase()))
    };
    let name = cc.to_lowercase();

    let mut switched_from = Vec::new();
    let mut already_up = false;
    for active in tunnel.active()? {
        // A regenerated config must replace the running tunnel, not keep it.
        if active == name && !regenerated {
            already_up = true;
        } else {
            tunnel.down(&conf_dir.join(format!("{active}.conf")))?;
            if active != name {
                switched_from.push(active);
            }
        }
    }

    // Gateway first: once the tunnel is up, the default route is the tunnel.
    let gateway = routes.physical_gateway()?;
    if !already_up {
        tunnel.up(&conf)?;
    }
    let routes = match routes.apply(&set, gateway) {
        Ok(n) => n,
        Err(e) => {
            // A full tunnel without its bypass routes kills the tailnet.
            return Err(match tunnel.down(&conf) {
                Ok(()) => eyre!("tunnel {name} was removed because bypass routes failed: {e}"),
                Err(d) => {
                    eyre!("bypass routes failed ({e}) and tunnel {name} could not be removed: {d}")
                }
            });
        }
    };
    Ok(Connected {
        regenerated,
        switched_from,
        already_up,
        routes,
        picked: None,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::net::IpAddr;

    use wglane_core::testing::{
        FakeBypassProvider, FakeCatalog, FakeProber, FakeRoutes, FakeToken, FakeTunnel,
    };
    use wglane_core::{Error, MAX_CONF_AGE, PublicKey, Server};

    use super::*;
    use crate::context::Context;

    const DIR: &str = "/cfg";

    fn cc(s: &str) -> CountryCode {
        s.parse().unwrap()
    }

    fn catalog() -> FakeCatalog {
        FakeCatalog {
            servers: [(
                cc("ZA"),
                vec![Server {
                    hostname: "za3.nordvpn.com".into(),
                    station: "192.0.2.2".parse().unwrap(),
                    load: 9,
                    public_key: PublicKey("pk".into()),
                    country: cc("ZA"),
                    city: None,
                }],
            )]
            .into(),
            private_key: Some("priv".into()),
            ..Default::default()
        }
    }

    fn bypass() -> FakeBypassProvider {
        FakeBypassProvider(["198.51.100.1/32".parse().unwrap()].into_iter().collect())
    }

    struct Rig {
        tunnel: FakeTunnel,
        routes: FakeRoutes,
        writes: Cell<u32>,
    }

    impl Rig {
        fn new(up: &[&str]) -> Self {
            let tunnel = FakeTunnel::default();
            tunnel
                .up
                .borrow_mut()
                .extend(up.iter().map(|s| (*s).to_owned()));
            Self {
                tunnel,
                routes: FakeRoutes::new("192.0.2.254".parse::<IpAddr>().unwrap()),
                writes: Cell::new(0),
            }
        }

        fn up(&self, to: &str, age: Option<Duration>, token: Option<&str>) -> Result<Connected> {
            connect(
                &self.tunnel,
                &self.routes,
                &[&bypass()],
                cc(to),
                Path::new(DIR),
                age,
                || {
                    regenerate(
                        &catalog(),
                        &FakeToken(token.map(Into::into)),
                        cc(to),
                        None,
                        None,
                        |body| {
                            assert!(body.contains("PrivateKey = priv"));
                            self.writes.set(self.writes.get() + 1);
                            Ok(Path::new(DIR).join(format!("{}.conf", to.to_lowercase())))
                        },
                    )
                    .map(|(path, _)| path)
                },
            )
        }
    }

    const FRESH: Option<Duration> = Some(Duration::from_secs(60));

    #[test]
    fn missing_conf_is_generated_then_connected() {
        let rig = Rig::new(&[]);
        let done = rig.up("ZA", None, Some("tok")).unwrap();
        assert_eq!(rig.writes.get(), 1);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert_eq!(rig.routes.installed.borrow().len(), 1);
        assert_eq!(
            done.summary(cc("ZA")),
            "connected to ZA, config regenerated, 1 bypass routes"
        );
    }

    #[test]
    fn stale_conf_is_regenerated() {
        let rig = Rig::new(&[]);
        rig.up(
            "ZA",
            Some(MAX_CONF_AGE + Duration::from_secs(1)),
            Some("tok"),
        )
        .unwrap();
        assert_eq!(rig.writes.get(), 1);
    }

    #[test]
    fn fresh_conf_is_reused_without_a_token() {
        let rig = Rig::new(&[]);
        let done = rig.up("ZA", FRESH, None).unwrap();
        assert_eq!(rig.writes.get(), 0);
        assert!(!done.regenerated);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
    }

    #[test]
    fn already_connected_keeps_tunnel_and_refreshes_routes() {
        let rig = Rig::new(&["za"]);
        let done = rig.up("ZA", FRESH, None).unwrap();
        assert!(done.already_up);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert_eq!(rig.routes.installed.borrow().len(), 1);
    }

    #[test]
    fn stale_while_connected_bounces_the_tunnel() {
        let rig = Rig::new(&["za"]);
        let done = rig.up("ZA", None, Some("tok")).unwrap();
        assert!(!done.already_up);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
    }

    #[test]
    fn switching_country_downs_the_old_tunnel() {
        let rig = Rig::new(&["us"]);
        let done = rig.up("ZA", None, Some("tok")).unwrap();
        assert_eq!(done.switched_from, ["us"]);
        assert_eq!(*rig.tunnel.up.borrow(), ["za"]);
        assert!(done.summary(cc("ZA")).starts_with("switched to ZA"));
    }

    #[test]
    fn missing_token_fails_before_touching_the_tunnel() {
        let rig = Rig::new(&["us"]);
        let err = rig.up("ZA", None, None).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<Error>(),
            Some(Error::TokenMissing { .. })
        ));
        assert_eq!(*rig.tunnel.up.borrow(), ["us"]);
        assert!(rig.routes.installed.borrow().is_empty());
    }

    #[test]
    fn gateway_is_read_before_the_tunnel_comes_up() {
        let rig = Rig::new(&[]);
        let before: IpAddr = "192.0.2.254".parse().unwrap();
        let gw = rig.routes.gateway.clone();
        // Once the tunnel is up the default route is the tunnel.
        rig.tunnel
            .on_up
            .borrow_mut()
            .push(Box::new(move || gw.set("10.5.0.1".parse().unwrap())));
        rig.up("ZA", FRESH, None).unwrap();
        assert_eq!(rig.routes.installed.borrow()[0].1, before);
    }

    #[test]
    fn failed_bypass_removes_the_tunnel() {
        let rig = Rig::new(&[]);
        rig.routes.fail_applies.set(1);
        let err = rig.up("ZA", FRESH, None).unwrap_err().to_string();
        assert!(err.contains("tunnel za was removed because bypass routes failed"));
        assert!(rig.tunnel.up.borrow().is_empty());
    }

    #[test]
    fn oversized_bypass_set_fails_before_the_tunnel_is_touched() {
        let rig = Rig::new(&["us"]);
        let big = FakeBypassProvider(
            (0..=MAX_BYPASS)
                .map(|i| format!("10.{}.{}.1/32", i / 256, i % 256).parse().unwrap())
                .collect(),
        );
        let err = connect(
            &rig.tunnel,
            &rig.routes,
            &[&big],
            cc("ZA"),
            Path::new(DIR),
            None,
            || panic!("no config may be written"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("bypass_extra"), "{err}");
        assert_eq!(
            *rig.tunnel.up.borrow(),
            ["us"],
            "existing tunnel untouched, none brought up"
        );
        assert_eq!(rig.routes.apply_calls.get(), 0);
    }

    fn two_servers() -> FakeCatalog {
        let srv = |h: &str, load, ip: &str| Server {
            hostname: h.into(),
            station: ip.parse().unwrap(),
            load,
            public_key: PublicKey("pk".into()),
            country: cc("ZA"),
            city: None,
        };
        FakeCatalog {
            servers: [(
                cc("ZA"),
                vec![
                    srv("za3.nordvpn.com", 9, "192.0.2.2"),
                    srv("za9.nordvpn.com", 30, "192.0.2.9"),
                ],
            )]
            .into(),
            private_key: Some("priv".into()),
            ..Default::default()
        }
    }

    #[test]
    fn fastest_beats_least_load_re_picks_every_time_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context {
            json: false,
            verbose: 0,
            config_dir: dir.path().to_owned(),
        };
        let rig = Rig::new(&[]);
        let prober = FakeProber::with_ms(&[("192.0.2.2", 200), ("192.0.2.9", 15)]);
        let go = |p: Option<&(dyn Prober + Sync)>| {
            connect_recorded(
                &ctx,
                &rig.tunnel,
                &rig.routes,
                &[&bypass()],
                &two_servers(),
                &FakeToken(Some("tok".into())),
                cc("ZA"),
                None,
                p,
            )
            .unwrap()
            .summary(cc("ZA"))
        };
        let fast = go(Some(&prober));
        assert!(
            fast.ends_with("server za9.nordvpn.com load 30% latency 15 ms"),
            "{fast}"
        );
        // A fresh config exists now, but --fastest measures again...
        assert!(go(Some(&prober)).contains("regenerated"));
        // ...while the default reuses it, and a regenerating default says the load.
        assert!(!go(None).contains("regenerated"));
    }

    #[test]
    fn fastest_with_no_answer_says_it_fell_back() {
        let (_, choice) = regenerate(
            &two_servers(),
            &FakeToken(Some("tok".into())),
            cc("ZA"),
            None,
            Some(&FakeProber::default()),
            |_| Ok(PathBuf::from("/cfg/za.conf")),
        )
        .unwrap();
        assert_eq!(choice.server.hostname, "za3.nordvpn.com");
        assert!(server_note(&choice).ends_with("(no probe answered: picked the least loaded)"));
    }
}
