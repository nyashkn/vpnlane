use std::net::IpAddr;
use std::time::Duration;

use wglane_core::ports::{ExitIpProbe, ServerCatalog};
use wglane_core::{Country, CountryCode, Error, PrivateKey, Result, Server, Token};

use crate::wire;

const BASE: &str = "https://api.nordvpn.com/v1";
const TIMEOUT: Duration = Duration::from_secs(20);

/// Blocking client for Nord's public API.
///
/// Endpoints (undocumented by Nord, may change):
/// - `GET /users/services/credentials` - Basic auth `token:<TOKEN>`
/// - `GET /servers/countries`
/// - `GET /servers/recommendations?filters[country_id]=<id>&filters[servers_technologies][identifier]=wireguard_udp&limit=<n>`
/// - `GET /helpers/ips/insights`
#[derive(Debug)]
pub struct NordApi {
    agent: ureq::Agent,
    base: String,
}

impl NordApi {
    /// Client for the production API, using the macOS system trust store.
    pub fn new() -> Self {
        Self::with_base(BASE)
    }

    /// Client for another base URL (a local fixture server, for example).
    pub fn with_base(base: impl Into<String>) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_global(Some(TIMEOUT))
            .user_agent(concat!("wglane/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            agent,
            base: base.into(),
        }
    }
}

impl Default for NordApi {
    fn default() -> Self {
        Self::new()
    }
}

impl NordApi {
    /// GET `{base}{path}` and return the body. Errors never echo headers.
    fn get(&self, path: &str, query: &[(&str, &str)], auth: Option<&str>) -> Result<String> {
        let mut req = self.agent.get(format!("{}{path}", self.base));
        for (k, v) in query {
            req = req.query(k, v);
        }
        if let Some(a) = auth {
            req = req.header("Authorization", a);
        }
        let mut resp = req.call().map_err(|e| match e {
            ureq::Error::StatusCode(401) => {
                Error::Api("token rejected: create a new one in Nord account > Access token".into())
            }
            ureq::Error::StatusCode(c) => Error::Api(format!("GET {path} failed: HTTP {c}")),
            other => Error::Api(format!("GET {path} failed: {other}")),
        })?;
        resp.body_mut()
            .read_to_string()
            .map_err(|e| Error::Api(format!("GET {path}: reading body: {e}")))
    }

    fn countries_with_ids(&self) -> Result<Vec<(u32, Country)>> {
        wire::parse_countries(&self.get("/servers/countries", &[], None)?)
    }
}

impl ServerCatalog for NordApi {
    fn countries(&self) -> Result<Vec<Country>> {
        Ok(self
            .countries_with_ids()?
            .into_iter()
            .map(|(_, c)| c)
            .collect())
    }

    fn recommended(&self, country: CountryCode, limit: usize) -> Result<Vec<Server>> {
        let id = self
            .countries_with_ids()?
            .into_iter()
            .find(|(_, c)| c.code == country)
            .map(|(id, _)| id.to_string())
            .ok_or(Error::UnknownCountry(country))?;
        let limit = limit.to_string();
        let body = self.get(
            "/servers/recommendations",
            &[
                ("filters[country_id]", &id),
                ("filters[servers_technologies][identifier]", "wireguard_udp"),
                ("limit", &limit),
            ],
            None,
        )?;
        let servers = wire::parse_servers(&body)?;
        if servers.is_empty() {
            return Err(Error::NoServers(country));
        }
        Ok(servers)
    }

    fn private_key(&self, token: &Token) -> Result<PrivateKey> {
        let auth = format!(
            "Basic {}",
            base64(format!("token:{}", token.expose()).as_bytes())
        );
        wire::parse_private_key(&self.get("/users/services/credentials", &[], Some(&auth))?)
    }
}

impl ExitIpProbe for NordApi {
    fn exit_ip(&self) -> Result<IpAddr> {
        wire::parse_exit_ip(&self.get("/helpers/ips/insights", &[], None)?)
    }
}

/// Standard padded base64; only used for the Basic auth header.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;

    const COUNTRIES: &str = include_str!("../fixtures/countries.json");
    const RECS: &str = include_str!("../fixtures/recommendations_za.json");

    /// Localhost-only fake API. Answers by path prefix from `routes`
    /// (404 otherwise) and sends each raw request to the receiver.
    fn serve(routes: Vec<(&'static str, u16, &'static str)>) -> (String, mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!("http://{}", l.local_addr().expect("local addr"));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = req.split_whitespace().nth(1).unwrap_or("");
                let (code, body) = routes
                    .iter()
                    .find(|(p, ..)| path.starts_with(p))
                    .map_or((404, ""), |(_, c, b)| (*c, *b));
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tx.send(req);
            }
        });
        (base, rx)
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"token:abc"), "dG9rZW46YWJj");
    }

    #[test]
    fn countries_over_http() {
        let (base, _rx) = serve(vec![("/servers/countries", 200, COUNTRIES)]);
        assert_eq!(NordApi::with_base(base).countries().unwrap().len(), 3);
    }

    #[test]
    fn recommended_sends_filters_and_parses() {
        let (base, rx) = serve(vec![
            ("/servers/countries", 200, COUNTRIES),
            ("/servers/recommendations", 200, RECS),
        ]);
        let got = NordApi::with_base(base)
            .recommended("za".parse().unwrap(), 5)
            .unwrap();
        assert_eq!(got.len(), 2);
        let _countries = rx.recv().unwrap();
        let req = rx.recv().unwrap();
        assert!(req.contains("filters%5Bcountry_id%5D=2"), "{req}");
        assert!(req.contains("wireguard_udp"), "{req}");
        assert!(req.contains("limit=5"), "{req}");
    }

    #[test]
    fn recommended_unknown_country_and_empty() {
        let (base, _rx) = serve(vec![
            ("/servers/countries", 200, COUNTRIES),
            ("/servers/recommendations", 200, "[]"),
        ]);
        let api = NordApi::with_base(base);
        assert!(matches!(
            api.recommended("FR".parse().unwrap(), 1),
            Err(Error::UnknownCountry(_))
        ));
        assert!(matches!(
            api.recommended("ZA".parse().unwrap(), 1),
            Err(Error::NoServers(_))
        ));
    }

    #[test]
    fn private_key_uses_basic_auth_and_maps_401() {
        let (base, rx) = serve(vec![(
            "/users/services/credentials",
            200,
            include_str!("../fixtures/credentials.json"),
        )]);
        let tok = Token::new("deadbeef").unwrap();
        let key = NordApi::with_base(base).private_key(&tok).unwrap();
        assert!(key.expose().starts_with("U1lOVEhFVElD"));
        let req = rx.recv().unwrap().to_ascii_lowercase();
        let want = format!("authorization: basic {}", base64(b"token:deadbeef"));
        assert!(req.contains(&want.to_ascii_lowercase()), "{req}");

        let (base, _rx) = serve(vec![("/users/services/credentials", 401, "")]);
        let err = NordApi::with_base(base).private_key(&tok).unwrap_err();
        assert!(err.to_string().contains("token rejected"), "{err}");
    }

    #[test]
    fn exit_ip_over_http() {
        let (base, _rx) = serve(vec![(
            "/helpers/ips/insights",
            200,
            include_str!("../fixtures/insights.json"),
        )]);
        let ip = NordApi::with_base(base).exit_ip().unwrap();
        assert_eq!(ip.to_string(), "198.51.100.7");
    }
}
