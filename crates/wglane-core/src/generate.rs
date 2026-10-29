use crate::ports::{ServerCatalog, TokenSource};
use crate::{CountryCode, Result, TunnelConfig};

/// Builds a NordLynx config for `country`: fetch our key, pick the least
/// loaded recommended server, combine. Used by `gen` and by `up` auto-gen.
pub fn generate_config<C: ServerCatalog, T: TokenSource>(
    catalog: &C,
    token: &T,
    country: CountryCode,
) -> Result<TunnelConfig> {
    // TODO(bead): core-generate: token.token()? -> TokenMissing if None;
    // catalog.private_key(&token); catalog.recommended(country, 5); rank_by_load;
    // first or Error::NoServers; TunnelConfig::nordlynx. Test with testing::Fake*.
    let _ = (catalog, token, country);
    Err(crate::Error::NotImplemented("generate_config"))
}
