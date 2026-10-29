use std::net::IpAddr;
use std::time::Duration;

use wglane_core::ports::{ExitIpProbe, ServerCatalog};
use wglane_core::{Country, CountryCode, Error, PrivateKey, Result, Server, Token};

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
    #[allow(dead_code, reason = "used once nord-api beads land")]
    agent: ureq::Agent,
    #[allow(dead_code, reason = "used once nord-api beads land")]
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

impl ServerCatalog for NordApi {
    fn countries(&self) -> Result<Vec<Country>> {
        // TODO(bead): nord-api-countries: GET {base}/servers/countries, body ->
        // wire::parse_countries, drop the ids. Map ureq errors to Error::Api with
        // the status code; never include request headers in messages.
        Err(Error::NotImplemented("NordApi::countries"))
    }

    fn recommended(&self, country: CountryCode, limit: usize) -> Result<Vec<Server>> {
        // TODO(bead): nord-api-recommended: resolve country -> Nord id via
        // countries (UnknownCountry if absent), GET recommendations with the
        // wireguard_udp filter and limit, wire::parse_servers, NoServers if empty.
        let _ = (country, limit);
        Err(Error::NotImplemented("NordApi::recommended"))
    }

    fn private_key(&self, token: &Token) -> Result<PrivateKey> {
        // TODO(bead): nord-api-credentials: GET credentials with header
        // `Authorization: Basic base64("token:" + token.expose())` (hand-roll
        // base64 or add the `base64` crate), wire::parse_private_key. 401 ->
        // Error::Api("token rejected: create a new one in Nord account > Access token").
        let _ = token;
        Err(Error::NotImplemented("NordApi::private_key"))
    }
}

impl ExitIpProbe for NordApi {
    fn exit_ip(&self) -> Result<IpAddr> {
        // TODO(bead): nord-api-insights: GET {base}/helpers/ips/insights, wire::parse_exit_ip.
        Err(Error::NotImplemented("NordApi::exit_ip"))
    }
}
