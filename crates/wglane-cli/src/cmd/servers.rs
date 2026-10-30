//! `wglane servers [CC]`: list countries, or ranked servers in one.

use std::fmt::Write as _;

use color_eyre::Result;
use serde_json::json;
use std::time::Duration;

use vpnlane_core::ports::{Prober, ServerCatalog, Tunnel};
use vpnlane_core::{City, CountryCode, Server, rank_by_load};
use vpnlane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "List countries, or ranked servers in one country",
    long_about = concat!("\
Without COUNTRY: every country the provider serves.
With COUNTRY: its WireGuard servers with their city, least loaded first (ties
keep Nord's distance order). --city keeps one city (name or identifier; case
and accents do not matter); `", brand_name!(), " ls COUNTRY` lists the cities.
--probe pings each listed server at once (about 2 s at most) and adds a PING
column; `-` means no reply. While a ", brand_name!(), " tunnel is up the pings are bound to
the physical interface, so they measure the path without the tunnel."),
    after_help = concat!("\
Examples:
  ", brand_name!(), " servers
  ", brand_name!(), " servers ZA
  ", brand_name!(), " servers US --city \"New York\" --limit 10 --json
  ", brand_name!(), " servers DE --probe --limit 10")
)]
pub(crate) struct Args {
    /// Country to list servers for; omit to list countries.
    pub(crate) country: Option<CountryCode>,

    /// Only servers in this city of COUNTRY.
    #[arg(long, value_name = "NAME", requires = "country")]
    pub(crate) city: Option<String>,

    /// How many servers to show.
    #[arg(long, default_value_t = 5)]
    pub(crate) limit: usize,

    /// Ping each listed server and show the round trip next to the load.
    #[arg(long, requires = "country")]
    pub(crate) probe: bool,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // No token needed: both endpoints are public.
    let prober = if args.probe {
        // shortcut: if the helper cannot say (not installed), no wglane tunnel
        // can be up, so probe unbound; revisit if tunnels get another owner.
        let helper = vpnlane_macos::HelperClient::new(vpnlane_macos::SystemRunner);
        Some(match helper.active() {
            Ok(_) => crate::cmd::fastest::real_prober(&helper)?,
            Err(_) => vpnlane_macos::PingProbe::new(vpnlane_macos::SystemRunner, None),
        })
    } else {
        None
    };
    let out = render(
        &NordApi::new(),
        args,
        ctx.json,
        prober.as_ref().map(|p| p as &(dyn Prober + Sync)),
    )?;
    println!("{out}");
    Ok(())
}

/// `CC  Name` per country, or JSON.
pub(crate) fn countries_listing<C: ServerCatalog>(catalog: &C, json: bool) -> Result<String> {
    let countries = catalog.countries()?;
    if json {
        let v: Vec<_> = countries
            .iter()
            .map(|c| json!({ "code": c.code, "name": c.name }))
            .collect();
        return Ok(serde_json::to_string_pretty(&v)?);
    }
    let mut out = String::new();
    for c in &countries {
        let _ = writeln!(out, "{}  {}", c.code, c.name);
    }
    Ok(out.trim_end().to_owned())
}

/// The city of `cc` matching `query`; the error lists the valid cities.
pub(crate) fn resolve_city<C: ServerCatalog>(
    catalog: &C,
    cc: CountryCode,
    query: &str,
) -> Result<City> {
    let countries = catalog.countries()?;
    let country = countries
        .iter()
        .find(|c| c.code == cc)
        .ok_or(vpnlane_core::Error::UnknownCountry(cc))?;
    Ok(country.find_city(query)?.clone())
}

/// The command's stdout, without the trailing newline.
fn render<C: ServerCatalog>(
    catalog: &C,
    args: &Args,
    json: bool,
    prober: Option<&(dyn Prober + Sync)>,
) -> Result<String> {
    let Some(cc) = args.country else {
        return countries_listing(catalog, json);
    };
    let city = args
        .city
        .as_deref()
        .map(|q| resolve_city(catalog, cc, q))
        .transpose()?;
    let mut servers = catalog.recommended(cc, city.as_ref(), args.limit)?;
    rank_by_load(&mut servers);
    let pings = prober.map(|p| crate::cmd::fastest::probe_all(p, &servers, |_, _| {}));
    let ping_of = |i: usize| pings.as_ref().and_then(|p| p[i]);
    if json {
        let v: Vec<_> = servers
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut o = json!({ "hostname": s.hostname, "city": s.city, "load": s.load, "ip": s.station });
                if pings.is_some() {
                    o["latency_ms"] = json!(ping_of(i).map(|d| d.as_millis()));
                }
                o
            })
            .collect();
        return Ok(serde_json::to_string_pretty(&v)?);
    }
    let city_of = |s: &Server| s.city.clone().unwrap_or_else(|| "-".into());
    let width = |f: &dyn Fn(&Server) -> usize, min: usize| {
        servers.iter().map(f).max().unwrap_or(0).max(min)
    };
    let (hw, cw) = (
        width(&|s| s.hostname.len(), 4),
        width(&|s| city_of(s).chars().count(), 4),
    );
    let ping_col = |d: Option<Duration>| d.map_or("-".into(), |d| format!("{} ms", d.as_millis()));
    let mut out = format!("{:<hw$}  {:<cw$}  LOAD  ", "HOST", "CITY");
    if pings.is_some() {
        out.push_str("PING    ");
    }
    out.push_str("IP");
    for (i, s) in servers.iter().enumerate() {
        let load = format!("{}%", s.load);
        let _ = write!(
            out,
            "\n{:<hw$}  {:<cw$}  {load:<4}  ",
            s.hostname,
            city_of(s)
        );
        if pings.is_some() {
            let _ = write!(out, "{:<6}  ", ping_col(ping_of(i)));
        }
        let _ = write!(out, "{}", s.station);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use vpnlane_core::testing::FakeCatalog;
    use vpnlane_core::{City, Country, PublicKey, Server};

    use super::*;

    /// ZA with two cities (one accented), DE with none listed.
    pub(crate) fn fake() -> FakeCatalog {
        let srv = |h: &str, load, ip: &str, city: &str| Server {
            hostname: h.into(),
            station: ip.parse().unwrap(),
            load,
            public_key: PublicKey(String::new()),
            country: "ZA".parse().unwrap(),
            city: Some(city.into()),
        };
        let city = |id, name: &str, ident: &str, n| City {
            id,
            name: name.into(),
            identifier: ident.into(),
            server_count: n,
        };
        FakeCatalog {
            countries: vec![
                Country {
                    code: "DE".parse().unwrap(),
                    name: "Germany".into(),
                    cities: vec![],
                },
                Country {
                    code: "ZA".parse().unwrap(),
                    name: "South Africa".into(),
                    cities: vec![
                        city(1, "Johannesburg", "za_johannesburg", 12),
                        city(2, "Pretória", "za_pretoria", 3),
                    ],
                },
            ],
            servers: [(
                "ZA".parse().unwrap(),
                vec![
                    srv("za12.nordvpn.com", 40, "192.0.2.1", "Johannesburg"),
                    srv("za3.nordvpn.com", 9, "192.0.2.2", "Johannesburg"),
                    srv("za7.nordvpn.com", 9, "192.0.2.3", "Pretória"),
                ],
            )]
            .into(),
            private_key: None,
        }
    }

    fn args(country: Option<&str>, city: Option<&str>, limit: usize) -> Args {
        Args {
            country: country.map(|c| c.parse().unwrap()),
            city: city.map(Into::into),
            limit,
            probe: false,
        }
    }

    #[test]
    fn countries_table_and_json() {
        insta::assert_snapshot!(render(&fake(), &args(None, None, 5), false, None).unwrap());
        insta::assert_snapshot!(render(&fake(), &args(None, None, 5), true, None).unwrap());
    }

    #[test]
    fn servers_ranked_table_and_json() {
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA"), None, 5), false, None).unwrap());
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA"), None, 5), true, None).unwrap());
    }

    #[test]
    fn city_filter_matches_loosely_and_lists_valid_cities_on_error() {
        let out = render(&fake(), &args(Some("ZA"), Some("PRETORIA"), 5), false, None).unwrap();
        insta::assert_snapshot!(out);
        let err = render(&fake(), &args(Some("ZA"), Some("Durban"), 5), false, None)
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "unknown city \"Durban\" in ZA; valid cities: Johannesburg, Pretória"
        );
    }

    #[test]
    fn limit_applies_and_unknown_country_errors() {
        let out = render(&fake(), &args(Some("ZA"), None, 1), true, None).unwrap();
        assert_eq!(out.matches("hostname").count(), 1);
        assert!(render(&fake(), &args(Some("US"), None, 5), false, None).is_err());
    }

    #[test]
    fn probe_adds_a_latency_column_and_json_field() {
        let p =
            vpnlane_core::testing::FakeProber::with_ms(&[("192.0.2.2", 41), ("192.0.2.1", 380)]);
        let prober: Option<&(dyn Prober + Sync)> = Some(&p);
        insta::assert_snapshot!(
            render(&fake(), &args(Some("ZA"), None, 5), false, prober).unwrap()
        );
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA"), None, 5), true, prober).unwrap());
    }
}
