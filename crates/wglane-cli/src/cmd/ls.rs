//! `wglane ls [CC]`: list countries, or the cities of one with server counts.

use std::fmt::Write as _;

use color_eyre::Result;
use serde_json::json;
use wglane_core::CountryCode;
use wglane_core::ports::ServerCatalog;
use wglane_nord::NordApi;

use super::servers::countries_listing;
use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "List countries, or the cities of one country with server counts",
    long_about = concat!("\
Without COUNTRY: every country the provider serves.
With COUNTRY: its cities and how many servers each has. Use a name with
`", brand_name!(), " up COUNTRY --city NAME`."),
    after_help = concat!("\
Examples:
  ", brand_name!(), " ls
  ", brand_name!(), " ls US
  ", brand_name!(), " ls DE --json")
)]
pub(crate) struct Args {
    /// Country whose cities to list; omit to list countries.
    pub(crate) country: Option<CountryCode>,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    println!("{}", render(&NordApi::new(), args, ctx.json)?);
    Ok(())
}

/// The command's stdout, without the trailing newline.
fn render<C: ServerCatalog>(catalog: &C, args: &Args, json: bool) -> Result<String> {
    let Some(cc) = args.country else {
        return countries_listing(catalog, json);
    };
    let countries = catalog.countries()?;
    let country = countries
        .iter()
        .find(|c| c.code == cc)
        .ok_or(wglane_core::Error::UnknownCountry(cc))?;
    let mut cities: Vec<_> = country.cities.iter().collect();
    cities.sort_by(|a, b| a.name.cmp(&b.name));
    if json {
        let v: Vec<_> = cities
            .iter()
            .map(|c| json!({ "name": c.name, "identifier": c.identifier, "servers": c.server_count }))
            .collect();
        return Ok(serde_json::to_string_pretty(&v)?);
    }
    let width = cities
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);
    let mut out = format!("{:<width$}  SERVERS", "CITY");
    for c in cities {
        let _ = write!(out, "\n{:<width$}  {}", c.name, c.server_count);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::servers::tests::fake;
    use super::*;

    fn args(cc: Option<&str>) -> Args {
        Args {
            country: cc.map(|c| c.parse().unwrap()),
        }
    }

    #[test]
    fn cities_table_and_json() {
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA")), false).unwrap());
        insta::assert_snapshot!(render(&fake(), &args(Some("ZA")), true).unwrap());
    }

    #[test]
    fn no_country_lists_countries_and_unknown_errors() {
        assert!(
            render(&fake(), &args(None), false)
                .unwrap()
                .contains("DE  Germany")
        );
        assert!(render(&fake(), &args(Some("US")), false).is_err());
    }
}
