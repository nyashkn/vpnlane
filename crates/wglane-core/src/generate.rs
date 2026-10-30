use crate::ports::{ServerCatalog, TokenSource};
use crate::{City, CountryCode, Error, Result, Server, TunnelConfig, rank_by_load};

/// How many recommended servers to rank; the provider's nearest few.
const CANDIDATES: usize = 5;

/// Builds a NordLynx config for `country` (or only `city`): fetch our key,
/// pick the least loaded recommended server, combine. Also returns the server
/// picked. Used by `gen` and by `up` auto-gen.
///
/// Fails with `TokenMissing` when the source has no token, `UnknownCountry`
/// or `NoServers` when the provider has nothing to offer.
pub fn generate_config<C: ServerCatalog, T: TokenSource>(
    catalog: &C,
    token: &T,
    country: CountryCode,
    city: Option<&City>,
) -> Result<(TunnelConfig, Server)> {
    let token = token.token()?.ok_or_else(|| Error::TokenMissing {
        hint: token.describe(),
    })?;
    let key = catalog.private_key(&token)?;
    let mut servers = catalog.recommended(country, city, CANDIDATES)?;
    rank_by_load(&mut servers);
    if servers.is_empty() {
        return Err(Error::NoServers(country));
    }
    let best = servers.remove(0);
    Ok((TunnelConfig::nordlynx(key, &best), best))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PublicKey;
    use crate::Server;
    use crate::testing::{FakeCatalog, FakeToken};

    fn srv(host: &str, load: u8, ip: &str) -> Server {
        Server {
            hostname: host.into(),
            station: ip.parse().unwrap(),
            load,
            public_key: PublicKey(format!("pk-{host}")),
            country: "ZA".parse().unwrap(),
            city: None,
        }
    }

    fn catalog(servers: Vec<Server>) -> FakeCatalog {
        FakeCatalog {
            servers: [("ZA".parse().unwrap(), servers)].into(),
            private_key: Some("priv".into()),
            ..Default::default()
        }
    }

    #[test]
    fn picks_least_loaded_and_keeps_nearest_on_ties() {
        let c = catalog(vec![
            srv("busy", 50, "192.0.2.1"),
            srv("near", 9, "192.0.2.2"),
            srv("far", 9, "192.0.2.3"),
        ]);
        let conf = generate_config(
            &c,
            &FakeToken(Some("tok".into())),
            "ZA".parse().unwrap(),
            None,
        )
        .expect("generates");
        let conf = conf.0;
        assert_eq!(conf.peer_public_key.0, "pk-near");
        assert_eq!(conf.endpoint.to_string(), "192.0.2.2:51820");
        assert_eq!(conf.private_key.expose(), "priv");
    }

    #[test]
    fn city_restricts_the_pick() {
        let mut far = srv("far", 1, "192.0.2.9");
        far.city = Some("Cape Town".into());
        let mut near = srv("near", 50, "192.0.2.8");
        near.city = Some("Johannesburg".into());
        let c = catalog(vec![far, near]);
        let city = City {
            id: 7,
            name: "Johannesburg".into(),
            identifier: "za_johannesburg".into(),
            server_count: 1,
        };
        let tok = FakeToken(Some("tok".into()));
        let (_, got) = generate_config(&c, &tok, "ZA".parse().unwrap(), Some(&city)).unwrap();
        assert_eq!(got.hostname, "near");
        let (_, got) = generate_config(&c, &tok, "ZA".parse().unwrap(), None).unwrap();
        assert_eq!(got.hostname, "far");
    }

    #[test]
    fn missing_token_is_token_missing() {
        let c = catalog(vec![srv("a", 1, "192.0.2.1")]);
        let err = generate_config(&c, &FakeToken(None), "ZA".parse().unwrap(), None).unwrap_err();
        assert!(matches!(err, Error::TokenMissing { hint } if hint == "fake token"));
    }

    #[test]
    fn empty_and_unknown_country() {
        let tok = FakeToken(Some("tok".into()));
        let c = catalog(vec![]);
        let err = generate_config(&c, &tok, "ZA".parse().unwrap(), None).unwrap_err();
        assert!(matches!(err, Error::NoServers(_)));
        let err = generate_config(&c, &tok, "DE".parse().unwrap(), None).unwrap_err();
        assert!(matches!(err, Error::UnknownCountry(_)));
    }

    #[test]
    fn auth_failure_propagates() {
        let mut c = catalog(vec![srv("a", 1, "192.0.2.1")]);
        c.private_key = None;
        let err = generate_config(
            &c,
            &FakeToken(Some("tok".into())),
            "ZA".parse().unwrap(),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Api(_)));
    }
}
