use crate::ports::{ServerCatalog, TokenSource};
use crate::{CountryCode, Error, Result, TunnelConfig, rank_by_load};

/// How many recommended servers to rank; the provider's nearest few.
const CANDIDATES: usize = 5;

/// Builds a NordLynx config for `country`: fetch our key, pick the least
/// loaded recommended server, combine. Used by `gen` and by `up` auto-gen.
///
/// Fails with `TokenMissing` when the source has no token, `UnknownCountry`
/// or `NoServers` when the provider has nothing to offer.
pub fn generate_config<C: ServerCatalog, T: TokenSource>(
    catalog: &C,
    token: &T,
    country: CountryCode,
) -> Result<TunnelConfig> {
    let token = token.token()?.ok_or_else(|| Error::TokenMissing {
        hint: token.describe(),
    })?;
    let key = catalog.private_key(&token)?;
    let mut servers = catalog.recommended(country, CANDIDATES)?;
    rank_by_load(&mut servers);
    let best = servers.first().ok_or(Error::NoServers(country))?;
    Ok(TunnelConfig::nordlynx(key, best))
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
        let conf = generate_config(&c, &FakeToken(Some("tok".into())), "ZA".parse().unwrap())
            .expect("generates");
        assert_eq!(conf.peer_public_key.0, "pk-near");
        assert_eq!(conf.endpoint.to_string(), "192.0.2.2:51820");
        assert_eq!(conf.private_key.expose(), "priv");
    }

    #[test]
    fn missing_token_is_token_missing() {
        let c = catalog(vec![srv("a", 1, "192.0.2.1")]);
        let err = generate_config(&c, &FakeToken(None), "ZA".parse().unwrap()).unwrap_err();
        assert!(matches!(err, Error::TokenMissing { hint } if hint == "fake token"));
    }

    #[test]
    fn empty_and_unknown_country() {
        let tok = FakeToken(Some("tok".into()));
        let c = catalog(vec![]);
        let err = generate_config(&c, &tok, "ZA".parse().unwrap()).unwrap_err();
        assert!(matches!(err, Error::NoServers(_)));
        let err = generate_config(&c, &tok, "DE".parse().unwrap()).unwrap_err();
        assert!(matches!(err, Error::UnknownCountry(_)));
    }

    #[test]
    fn auth_failure_propagates() {
        let mut c = catalog(vec![srv("a", 1, "192.0.2.1")]);
        c.private_key = None;
        let err =
            generate_config(&c, &FakeToken(Some("tok".into())), "ZA".parse().unwrap()).unwrap_err();
        assert!(matches!(err, Error::Api(_)));
    }
}
