//! `wglane up <CC>`: connect, regenerating the config if missing or stale.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use wglane_core::ports::{BypassProvider, Prober, RouteBypass, ServerCatalog, TokenSource, Tunnel};
use wglane_core::{
    BypassSet, Choice, City, CountryCode, Error, FASTEST_CANDIDATES, MAX_BYPASS, Overrides, Pick,
    generate_config, generate_config_by, is_stale, render_wg_conf,
};
use wglane_macos::{HelperClient, StaticBypass, SystemRunner, TailscaleBypass};
use wglane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Connect to a country (switches if already connected)",
    long_about = "\
Connect to the least-loaded NordLynx server in COUNTRY, or in one of its
cities with --city (name or identifier; case and accents do not matter;
`wglane ls COUNTRY` lists them). Changing between a city and the whole
country regenerates the config.

With --fastest (or `pick = \"fastest\"` in config.toml) the 10 least-loaded
candidates are pinged at once (about 2 s at most) and the lowest round trip
wins; ties go to the lower load, and if no server answers the least-loaded one
is used and the summary says so. It always picks anew, so it regenerates the
config even when a fresh one exists. While a wglane tunnel is up, pings are
bound to the physical interface so they do not measure the tunnel's exit; if
every ping fails while connected, run `wglane down` first.

If the config for COUNTRY is missing or older than 7 days it is regenerated
first (no separate `gen` step). An existing wglane tunnel is brought down.

With --preset NAME the request comes from [presets.NAME] in config.toml
(`wglane preset list`); the preset's bypass_extra is added to the top-level
one. Precedence for what to connect to: explicit COUNTRY and --city, then the
preset, then WGLANE_COUNTRY, then config.toml. A COUNTRY other than the
preset's drops the preset's city. Presets are applied the same way by
`wglane preset apply NAME`; the active one is shown by `wglane status`.

Why Tailscale breaks, and what wglane does:
  A full tunnel (AllowedIPs 0.0.0.0/0) also captures Tailscale's underlay:
  the control server and DERP relays. netcheck then reports UDP false and the
  tailnet dies, although 100.64.0.0/10 still routes. wglane adds host routes
  for those IPs via your physical gateway, so the tailnet keeps working and
  everything else exits through the VPN. The config has no DNS= line, so
  MagicDNS keeps working too.",
    after_help = "\
Examples:
  wglane up ZA          South Africa
  wglane up us          case does not matter
  wglane up             use default_country from config.toml (or WGLANE_COUNTRY)
  wglane up US --city \"New York\"
  wglane up DE --fastest          lowest latency among the least-loaded servers
  wglane up --preset work         apply [presets.work] from config.toml
  wglane up --preset work FR      the preset, but exit in France
  wglane -v up DE       show each step"
)]
pub(crate) struct Args {
    /// Two-letter country code, e.g. US, DE, ZA (not UK: use GB).
    /// Default: `default_country` from `config.toml` or `WGLANE_COUNTRY`.
    pub(crate) country: Option<CountryCode>,

    /// City in COUNTRY to connect to (see `wglane ls COUNTRY`).
    /// Default: `default_city` from `config.toml`, for the default country.
    #[arg(long, value_name = "NAME")]
    pub(crate) city: Option<String>,

    /// Connect with a preset from `[presets.NAME]` in `config.toml`. An
    /// explicit COUNTRY or --city overrides the preset's.
    #[arg(long, value_name = "NAME")]
    pub(crate) preset: Option<String>,

    /// Pick the lowest-latency server among the least loaded, not just the
    /// least loaded. Overrides `pick` in `config.toml` and in the preset.
    #[arg(long)]
    pub(crate) fastest: bool,
}

/// What `up` will do, from the arguments and the saved settings.
#[derive(Debug)]
pub(crate) struct Plan {
    cc: CountryCode,
    /// City name or identifier, not yet checked against the network.
    city: Option<String>,
    /// Config `bypass_extra` plus the preset's.
    bypass: BypassSet,
    preset: Option<String>,
    /// `--fastest`, else the preset's `pick`, else the top-level `pick`.
    pick: Pick,
}

/// Precedence: COUNTRY/--city > preset > `WGLANE_COUNTRY` > config file.
pub(crate) fn plan(ctx: &Context, args: &Args) -> Result<Plan> {
    let settings = ctx.settings(Overrides::default())?;
    let preset = args
        .preset
        .as_deref()
        .map(|n| {
            settings.preset(n).ok_or_else(|| {
                let names: Vec<_> = settings
                    .presets
                    .value
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();
                eyre!(
                    "unknown preset {n:?}: {} (see `wglane preset list`)",
                    if names.is_empty() {
                        "none defined in config.toml".to_owned()
                    } else {
                        format!("defined: {}", names.join(", "))
                    }
                )
            })
        })
        .transpose()?;
    let mut req = preset.cloned().unwrap_or_default().resolve(&settings);
    if let Some(c) = args.country {
        // A city belongs to its country: keep it only if the preset itself names this one.
        if preset.is_none_or(|p| p.country != Some(c)) {
            req.city = None;
        }
        req.country = Some(c);
    }
    if args.city.is_some() {
        req.city.clone_from(&args.city);
    }
    let cc = req.country.ok_or_else(|| {
        eyre!(
            "no country given: run `wglane up ZA`, or set default_country in {} (see `wglane config init`)",
            ctx.config_file().display()
        )
    })?;
    Ok(Plan {
        cc,
        city: req.city,
        bypass: req.bypass_extra.iter().copied().collect(),
        preset: args.preset.clone(),
        pick: if args.fastest {
            Pick::Fastest
        } else {
            req.pick
        },
    })
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let plan = plan(ctx, args)?;
    let cc = plan.cc;
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

/// `up` against real adapters. Prints nothing, so the TUI can call it.
#[cfg(feature = "tui")]
pub(crate) fn connect_real(
    ctx: &Context,
    cc: CountryCode,
    city: Option<&City>,
) -> Result<Connected> {
    let helper = HelperClient::new(SystemRunner);
    connect_with(
        ctx,
        &helper,
        &helper,
        &TailscaleBypass::new(SystemRunner),
        &NordApi::new(),
        &ctx.token_source(),
        (cc, city),
        &ctx.static_bypass()?,
        None,
        None,
    )
}

/// Resolves the plan's city (the only network check of a preset's city, so
/// it happens before any tunnel is touched), then connects.
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
    let city = plan
        .city
        .as_deref()
        .map(|q| crate::cmd::servers::resolve_city(catalog, plan.cc, q))
        .transpose()?;
    connect_with(
        ctx,
        tunnel,
        routes,
        tailscale,
        catalog,
        token,
        (plan.cc, city.as_ref()),
        &StaticBypass(plan.bypass),
        plan.preset.as_deref(),
        prober,
    )
}

/// `connect_recorded`, then notes which preset (if any) the connection matches.
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
    hint: Option<&str>,
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
    crate::cmd::preset::settle(ctx, hint, cc, city.map(|c| c.name.as_str()), &extra.0)?;
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
