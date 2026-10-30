//! Nord API JSON shapes and their mapping to domain types.
//!
//! Pure functions over `&str` so tests run against `fixtures/` with no network.
//! Fields are the subset wglane reads; unknown fields are ignored so additive
//! API changes do not break us, missing fields surface as `Error::Api`.

use serde::Deserialize;
use wglane_core::{Country, Error, ExitInsights, PrivateKey, PublicKey, Result, Server};

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
    #[serde(default)]
    pub(crate) country_code: Option<String>,
    #[serde(default)]
    pub(crate) protected: Option<bool>,
}

/// `GET /v1/servers/recommendations` element.
#[derive(Debug, Deserialize)]
struct WireServer {
    hostname: String,
    station: std::net::IpAddr,
    load: u8,
    #[serde(default)]
    technologies: Vec<WireTechnology>,
    locations: Vec<WireLocation>,
}

#[derive(Debug, Deserialize)]
struct WireTechnology {
    identifier: String,
    #[serde(default)]
    metadata: Vec<WireMeta>,
}

#[derive(Debug, Deserialize)]
struct WireMeta {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct WireLocation {
    country: WireLocationCountry,
}

#[derive(Debug, Deserialize)]
struct WireLocationCountry {
    code: String,
}

fn bad_code(code: &str) -> Error {
    Error::Api(format!(
        "unexpected response shape (Nord API changed?): bad country code {code:?}"
    ))
}

/// Countries plus Nord's numeric id, which `recommended` needs for its filter.
pub(crate) fn parse_countries(json: &str) -> Result<Vec<(u32, Country)>> {
    let wire: Vec<WireCountry> = serde_json::from_str(json).map_err(|e| schema_drift(&e))?;
    wire.into_iter()
        .map(|c| {
            let code = c.code.parse().map_err(|_| bad_code(&c.code))?;
            Ok((c.id, Country { code, name: c.name }))
        })
        .collect()
}

/// Recommendations -> servers. Skips servers without a `wireguard_udp`
/// technology carrying a `public_key` metadata entry.
pub(crate) fn parse_servers(json: &str) -> Result<Vec<Server>> {
    let wire: Vec<WireServer> = serde_json::from_str(json).map_err(|e| schema_drift(&e))?;
    let mut out = Vec::new();
    for s in wire {
        let key = s
            .technologies
            .iter()
            .filter(|t| t.identifier == "wireguard_udp")
            .flat_map(|t| &t.metadata)
            .find(|m| m.name == "public_key");
        let (Some(key), Some(loc)) = (key, s.locations.first()) else {
            continue;
        };
        out.push(Server {
            hostname: s.hostname,
            station: s.station,
            load: s.load,
            public_key: PublicKey(key.value.clone()),
            country: loc
                .country
                .code
                .parse()
                .map_err(|_| bad_code(&loc.country.code))?,
        });
    }
    Ok(out)
}

/// Credentials -> our private key.
pub(crate) fn parse_private_key(json: &str) -> Result<PrivateKey> {
    let c: WireCredentials = serde_json::from_str(json).map_err(|e| schema_drift(&e))?;
    Ok(PrivateKey::new(c.nordlynx_private_key))
}

/// Insights -> the few fields we show. City, ISP and coordinates are dropped here.
pub(crate) fn parse_insights(json: &str) -> Result<ExitInsights> {
    let i: WireInsights = serde_json::from_str(json).map_err(|e| schema_drift(&e))?;
    Ok(ExitInsights {
        ip: i.ip,
        country: i.country_code.and_then(|c| c.parse().ok()),
        protected: i.protected,
    })
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
        let i = parse_insights(include_str!("../fixtures/insights.json")).unwrap();
        assert_eq!(i.ip.to_string(), "198.51.100.7");
        assert_eq!(i.country.map(|c| c.to_string()), Some("ZA".to_owned()));
        assert_eq!(i.protected, Some(true));
    }

    #[test]
    fn insights_tolerates_missing_and_bad_optional_fields() {
        let i = parse_insights(r#"{"ip":"198.51.100.7","country_code":"???"}"#).unwrap();
        assert_eq!((i.country, i.protected), (None, None));
    }

    #[test]
    fn drift_is_api_error() {
        assert!(matches!(parse_private_key("{}"), Err(Error::Api(_))));
    }

    #[test]
    fn countries_fixture() {
        let v = parse_countries(include_str!("../fixtures/countries.json")).unwrap();
        assert!(
            v.iter()
                .any(|(id, c)| c.code.as_str() == "ZA" && *id == 2 && c.name == "South Africa")
        );
    }

    #[test]
    fn servers_fixture_skips_non_wireguard() {
        let v = parse_servers(include_str!("../fixtures/recommendations_za.json")).unwrap();
        let hosts: Vec<_> = v.iter().map(|s| s.hostname.as_str()).collect();
        assert_eq!(hosts, ["za0001.example.invalid", "za0002.example.invalid"]);
        assert_eq!(v[1].load, 9);
        assert_eq!(v[0].country.as_str(), "ZA");
        assert!(v[0].public_key.0.starts_with("U1lOVEhFVElD"));
    }

    #[test]
    fn bad_country_code_is_api_error() {
        let j = r#"[{"id":1,"name":"X","code":"XYZ"}]"#;
        assert!(matches!(parse_countries(j), Err(Error::Api(_))));
    }
}
