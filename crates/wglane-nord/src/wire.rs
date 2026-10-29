//! Nord API JSON shapes and their mapping to domain types.
//!
//! Pure functions over `&str` so tests run against `fixtures/` with no network.
//! Fields are the subset wglane reads; unknown fields are ignored so additive
//! API changes do not break us, missing fields surface as `Error::Api`.

use serde::Deserialize;
use wglane_core::{Country, Error, PrivateKey, Result, Server};

/// `GET /v1/servers/countries` element.
#[derive(Debug, Deserialize)]
pub(crate) struct WireCountry {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) code: String,
}

/// `GET /v1/users/services/credentials` body.
#[derive(Debug, Deserialize)]
pub(crate) struct WireCredentials {
    pub(crate) nordlynx_private_key: String,
}

/// `GET /v1/helpers/ips/insights` body.
#[derive(Debug, Deserialize)]
pub(crate) struct WireInsights {
    pub(crate) ip: std::net::IpAddr,
}

/// Countries plus Nord's numeric id, which `recommended` needs for its filter.
pub(crate) fn parse_countries(json: &str) -> Result<Vec<(u32, Country)>> {
    // TODO(bead): nord-wire-countries: serde_json::from_str::<Vec<WireCountry>>,
    // parse `code` into CountryCode, map errors to Error::Api("schema drift: ...").
    // Test against fixtures/countries.json.
    let _ = json;
    Err(Error::NotImplemented("wire::parse_countries"))
}

/// Recommendations -> servers. Skips servers without a `wireguard_udp`
/// technology carrying a `public_key` metadata entry.
pub(crate) fn parse_servers(json: &str) -> Result<Vec<Server>> {
    // TODO(bead): nord-wire-servers: define WireServer {hostname, station, load,
    // technologies[{identifier, metadata[{name,value}]}], locations[0].country.code};
    // map to Server. Test against fixtures/recommendations_za.json (incl. the
    // server with no wireguard_udp, which must be skipped).
    let _ = json;
    Err(Error::NotImplemented("wire::parse_servers"))
}

/// Credentials -> our private key.
pub(crate) fn parse_private_key(json: &str) -> Result<PrivateKey> {
    let c: WireCredentials = serde_json::from_str(json).map_err(|e| schema_drift(&e))?;
    Ok(PrivateKey::new(c.nordlynx_private_key))
}

/// Insights -> our exit IP.
pub(crate) fn parse_exit_ip(json: &str) -> Result<std::net::IpAddr> {
    let i: WireInsights = serde_json::from_str(json).map_err(|e| schema_drift(&e))?;
    Ok(i.ip)
}

fn schema_drift(e: &serde_json::Error) -> Error {
    Error::Api(format!(
        "unexpected response shape (Nord API changed?): {e}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_fixture() {
        let k = parse_private_key(include_str!("../fixtures/credentials.json")).unwrap();
        assert!(k.expose().starts_with("U1lOVEhFVElD"));
    }

    #[test]
    fn insights_fixture() {
        let ip = parse_exit_ip(include_str!("../fixtures/insights.json")).unwrap();
        assert_eq!(ip.to_string(), "198.51.100.7");
    }

    #[test]
    fn drift_is_api_error() {
        assert!(matches!(parse_private_key("{}"), Err(Error::Api(_))));
    }

    #[test]
    fn countries_fixture_is_valid_json() {
        let v: Vec<WireCountry> =
            serde_json::from_str(include_str!("../fixtures/countries.json")).unwrap();
        assert!(
            v.iter()
                .any(|c| c.code == "ZA" && c.id > 0 && !c.name.is_empty())
        );
    }
}
