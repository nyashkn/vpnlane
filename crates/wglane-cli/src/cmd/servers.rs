//! `wglane servers [CC]`: list countries, or ranked servers in one.

use std::fmt::Write as _;

use color_eyre::Result;
use serde_json::json;
use wglane_core::ports::ServerCatalog;
use wglane_core::{CountryCode, rank_by_load};
use wglane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "List countries, or ranked servers in one country",
    long_about = "\
Without COUNTRY: every country the provider serves.
With COUNTRY: its WireGuard servers, least loaded first (ties keep
Nord's distance order).",
    after_help = "\
Examples:
  wglane servers
  wglane servers ZA
  wglane ls DE --limit 10 --json",
    visible_alias = "ls"
)]
pub(crate) struct Args {
    /// Country to list servers for; omit to list countries.
    pub(crate) country: Option<CountryCode>,

    /// How many servers to show.
    #[arg(long, default_value_t = 5)]
    pub(crate) limit: usize,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // No token needed: both endpoints are public.
    let out = render(&NordApi::new(), args, ctx.json)?;
    println!("{out}");
    Ok(())
}

/// The command's stdout, without the trailing newline.
fn render<C: ServerCatalog>(catalog: &C, args: &Args, json: bool) -> Result<String> {
    let Some(cc) = args.country else {
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
        return Ok(out.trim_end().to_owned());
    };
    let mut servers = catalog.recommended(cc, args.limit)?;
    rank_by_load(&mut servers);
    if json {
        let v: Vec<_> = servers
            .iter()
            .map(|s| json!({ "hostname": s.hostname, "load": s.load, "ip": s.station }))
            .collect();
        return Ok(serde_json::to_string_pretty(&v)?);
    }
    let width = servers
        .iter()
        .map(|s| s.hostname.len())
        .max()
        .unwrap_or(0)
        .max(4);
    let mut out = format!("{:<width$}  LOAD  IP", "HOST");
    for s in &servers {
        let load = format!("{}%", s.load);
        let _ = write!(out, "\n{:<width$}  {load:<4}  {}", s.hostname, s.station);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use wglane_core::testing::FakeCatalog;
    use wglane_core::{Country, PublicKey, Server};

    use super::*;

    fn fake() -> FakeCatalog {
        let srv = |h: &str, load, ip: &str| Server {
            hostname: h.into(),
            station: ip.parse().unwrap(),
            load,
            public_key: PublicKey(String::new()),
            country: "ZA".parse().unwrap(),
        };
        FakeCatalog {
            countries: vec![
                Country {
                    code: "DE".parse().unwrap(),
                    name: "Germany".into(),
                },
                Country {
                    code: "ZA".parse().unwrap(),
                    name: "South Africa".into(),
                },
            ],
            servers: [(
                "ZA".parse().unwrap(),
                vec![
                    srv("za12.nordvpn.com", 40, "192.0.2.1"),
                    srv("za3.nordvpn.com", 9, "192.0.2.2"),
                    srv("za7.nordvpn.com", 9, "192.0.2.3"),
                ],
            )]
            .into(),
            private_key: None,
        }
    }

    fn args(country: Option<&str>, limit: usize) -> Args {
        Args {
            country: country.map(|c| c.parse().unwrap()),
            limit,
        }
    }

    #[test]
    fn countries_table_and_json() {
        insta::assert_snapshot!(render(&fake(), &args(None, 5), false).unwrap());
        insta::assert_snapshot!(render(&fake(), &args(None, 5), true).unwrap());
    }

    #[test]
    fn servers_ranked_table_and_json() {
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA"), 5), false).unwrap());
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA"), 5), true).unwrap());
    }

    #[test]
    fn limit_applies_and_unknown_country_errors() {
        let out = render(&fake(), &args(Some("ZA"), 1), true).unwrap();
        assert_eq!(out.matches("hostname").count(), 1);
        assert!(render(&fake(), &args(Some("US"), 5), false).is_err());
    }
}
