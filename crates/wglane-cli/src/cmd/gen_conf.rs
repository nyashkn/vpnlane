//! `wglane gen <CC>...`: write configs without connecting.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use serde_json::json;
use wglane_core::ports::{ServerCatalog, TokenSource};
use wglane_core::{
    Country, CountryCode, PrivateKey, Server, Token, generate_config, rank_by_load, render_wg_conf,
};
use wglane_nord::NordApi;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Write tunnel configs without connecting",
    long_about = "\
Fetch your NordLynx key and the best server for each COUNTRY and write
<config-dir>/<cc>.conf (mode 600). `up` does this automatically; use `gen`
to prepare configs ahead of time or to force a refresh.",
    after_help = "\
Examples:
  wglane gen ZA US DE
  wglane gen us --config-dir /tmp/wg"
)]
pub(crate) struct Args {
    /// One or more two-letter country codes.
    #[arg(required = true, num_args = 1..)]
    pub(crate) countries: Vec<CountryCode>,
}

/// One written config.
#[derive(Debug)]
struct Generated {
    country: CountryCode,
    hostname: String,
    load: u8,
    path: PathBuf,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let store = ctx.conf_store();
    let done = generate_all(
        &NordApi::new(),
        &ctx.token_source(),
        &args.countries,
        |cc, body| Ok(store.write(cc, body)?),
    )?;
    println!("{}", report(&done, ctx.json, &home_relative)?);
    Ok(())
}

/// Generates and writes a config per country. Stops at the first failure.
fn generate_all<C: ServerCatalog, T: TokenSource>(
    catalog: &C,
    token: &T,
    countries: &[CountryCode],
    mut write: impl FnMut(CountryCode, &str) -> Result<PathBuf>,
) -> Result<Vec<Generated>> {
    let cached = Cached::new(catalog);
    let mut done = Vec::new();
    for &cc in countries {
        let conf = generate_config(&cached, token, cc)?;
        let path = write(cc, &render_wg_conf(&conf))?;
        let best = cached.best(cc)?;
        done.push(Generated {
            country: cc,
            hostname: best.hostname,
            load: best.load,
            path,
        });
    }
    Ok(done)
}

fn report(
    done: &[Generated],
    json: bool,
    display_path: &dyn Fn(&Path) -> String,
) -> Result<String> {
    if json {
        let v: Vec<_> = done
            .iter()
            .map(|g| {
                json!({
                    "country": g.country,
                    "hostname": g.hostname,
                    "load": g.load,
                    "path": display_path(&g.path),
                })
            })
            .collect();
        return Ok(serde_json::to_string_pretty(&v)?);
    }
    Ok(done
        .iter()
        .map(|g| {
            format!(
                "{} {} load {}% -> {}",
                g.country,
                g.hostname,
                g.load,
                display_path(&g.path)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `/Users/me/.config/x` -> `~/.config/x`.
fn home_relative(p: &Path) -> String {
    std::env::var_os("HOME")
        .and_then(|h| p.strip_prefix(h).ok().map(|r| format!("~/{}", r.display())))
        .unwrap_or_else(|| p.display().to_string())
}

/// Catalog wrapper that fetches the private key and each country's server
/// list once, so `gen ZA US DE` makes one key request and `best` costs nothing.
struct Cached<'a, C> {
    inner: &'a C,
    key: RefCell<Option<PrivateKey>>,
    servers: RefCell<HashMap<CountryCode, Vec<Server>>>,
}

impl<'a, C: ServerCatalog> Cached<'a, C> {
    fn new(inner: &'a C) -> Self {
        Self {
            inner,
            key: RefCell::default(),
            servers: RefCell::default(),
        }
    }

    /// The server `generate_config` picked: least loaded of the cached list.
    fn best(&self, cc: CountryCode) -> wglane_core::Result<Server> {
        let mut v = self.recommended(cc, 5)?;
        rank_by_load(&mut v);
        v.into_iter()
            .next()
            .ok_or(wglane_core::Error::NoServers(cc))
    }
}

impl<C: ServerCatalog> ServerCatalog for Cached<'_, C> {
    fn countries(&self) -> wglane_core::Result<Vec<Country>> {
        self.inner.countries()
    }

    fn recommended(&self, cc: CountryCode, limit: usize) -> wglane_core::Result<Vec<Server>> {
        if let Some(v) = self.servers.borrow().get(&cc) {
            return Ok(v.clone());
        }
        let v = self.inner.recommended(cc, limit)?;
        self.servers.borrow_mut().insert(cc, v.clone());
        Ok(v)
    }

    fn private_key(&self, token: &Token) -> wglane_core::Result<PrivateKey> {
        if let Some(k) = self.key.borrow().as_ref() {
            return Ok(k.clone());
        }
        let k = self.inner.private_key(token)?;
        *self.key.borrow_mut() = Some(k.clone());
        Ok(k)
    }
}

#[cfg(test)]
mod tests {
    use wglane_core::testing::{FakeCatalog, FakeToken};
    use wglane_core::{Error, PublicKey};

    use super::*;

    fn fake() -> FakeCatalog {
        let srv = |h: &str, load, ip: &str, cc: &str| Server {
            hostname: h.into(),
            station: ip.parse().unwrap(),
            load,
            public_key: PublicKey(format!("pk-{h}")),
            country: cc.parse().unwrap(),
        };
        FakeCatalog {
            servers: [
                (
                    "ZA".parse().unwrap(),
                    vec![
                        srv("za12.nordvpn.com", 40, "192.0.2.1", "ZA"),
                        srv("za3.nordvpn.com", 9, "192.0.2.2", "ZA"),
                    ],
                ),
                (
                    "DE".parse().unwrap(),
                    vec![srv("de7.nordvpn.com", 21, "192.0.2.3", "DE")],
                ),
            ]
            .into(),
            private_key: Some("priv-key".into()),
            ..Default::default()
        }
    }

    type Written = Vec<(CountryCode, String)>;

    fn run_gen(ccs: &[&str], token: Option<&str>) -> Result<(Vec<Generated>, Written)> {
        let ccs: Vec<CountryCode> = ccs.iter().map(|c| c.parse().unwrap()).collect();
        let mut written = Vec::new();
        let done = generate_all(
            &fake(),
            &FakeToken(token.map(Into::into)),
            &ccs,
            |cc, body| {
                written.push((cc, body.to_owned()));
                Ok(PathBuf::from(format!("/cfg/{}.conf", cc.to_lowercase())))
            },
        )?;
        Ok((done, written))
    }

    #[test]
    fn writes_one_conf_per_country_with_best_server() {
        let (done, written) = run_gen(&["za", "de"], Some("tok")).unwrap();
        assert_eq!(written.len(), 2);
        assert!(written[0].1.contains("PublicKey = pk-za3.nordvpn.com"));
        assert!(written[0].1.contains("PrivateKey = priv-key"));
        assert_eq!(done[0].hostname, "za3.nordvpn.com");
        insta::assert_snapshot!(report(&done, false, &|p| p.display().to_string()).unwrap());
    }

    #[test]
    fn json_output_is_stable() {
        let (done, _) = run_gen(&["ZA", "DE"], Some("tok")).unwrap();
        insta::assert_snapshot!(report(&done, true, &|p| p.display().to_string()).unwrap());
    }

    #[test]
    fn missing_token_and_unknown_country_fail_without_writing() {
        let err = run_gen(&["ZA"], None).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<Error>(),
            Some(Error::TokenMissing { .. })
        ));
        let err = run_gen(&["US"], Some("tok")).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<Error>(),
            Some(Error::UnknownCountry(_))
        ));
    }
}
