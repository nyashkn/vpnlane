use std::fmt::Write as _;

use zeroize::Zeroizing;

use crate::TunnelConfig;

/// Renders a wg-quick config file. The result holds the private key, so it
/// is zeroized on drop; write it with mode 0600.
pub fn render_wg_conf(conf: &TunnelConfig) -> Zeroizing<String> {
    let allowed = conf
        .allowed_ips
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = Zeroizing::new(String::new());
    // `write!` to a String cannot fail.
    let _ = write!(
        out,
        "[Interface]\nPrivateKey = {}\nAddress = {}\n\n[Peer]\nPublicKey = {}\nAllowedIPs = {}\nEndpoint = {}\n",
        conf.private_key.expose(),
        conf.address,
        conf.peer_public_key.0,
        allowed,
        conf.endpoint,
    );
    if let Some(secs) = conf.persistent_keepalive {
        let _ = writeln!(out, "PersistentKeepalive = {secs}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PrivateKey, PublicKey, Server};

    fn server(station: &str) -> Server {
        Server {
            hostname: "us0001.example.invalid".into(),
            station: station.parse().unwrap(),
            load: 12,
            public_key: PublicKey("U1lOVEhFVElDLVBFRVItUFVCTElDLUtFWS0wMDAwMDE=".into()),
            country: "US".parse().unwrap(),
        }
    }

    fn key() -> PrivateKey {
        PrivateKey::new("U1lOVEhFVElDLVBSSVZBVEUtS0VZLU5PVC1SRUFMMDE=")
    }

    #[test]
    fn nordlynx_ipv4() {
        let conf = TunnelConfig::nordlynx(key(), &server("192.0.2.10"));
        insta::assert_snapshot!(render_wg_conf(&conf).as_str());
    }

    #[test]
    fn nordlynx_ipv6_endpoint_is_bracketed() {
        let conf = TunnelConfig::nordlynx(key(), &server("2001:db8::10"));
        assert!(render_wg_conf(&conf).contains("Endpoint = [2001:db8::10]:51820\n"));
    }

    #[test]
    fn never_writes_dns() {
        let conf = TunnelConfig::nordlynx(key(), &server("192.0.2.10"));
        assert!(
            !render_wg_conf(&conf).contains("DNS"),
            "DNS= breaks MagicDNS"
        );
    }

    #[test]
    fn keepalive_is_optional() {
        let mut conf = TunnelConfig::nordlynx(key(), &server("192.0.2.10"));
        conf.persistent_keepalive = None;
        assert!(!render_wg_conf(&conf).contains("PersistentKeepalive"));
    }
}
