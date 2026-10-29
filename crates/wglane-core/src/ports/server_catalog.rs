use crate::{Country, CountryCode, PrivateKey, Result, Server, Token};

/// The VPN provider: what it serves and our key for it.
///
/// Implemented by `wglane_nord::NordApi`; faked by
/// [`crate::testing::FakeCatalog`].
pub trait ServerCatalog {
    /// Every country the provider serves.
    fn countries(&self) -> Result<Vec<Country>>;

    /// Up to `limit` WireGuard servers in `country`, in the provider's
    /// recommended order. Errors with `UnknownCountry` if it is not served.
    fn recommended(&self, country: CountryCode, limit: usize) -> Result<Vec<Server>>;

    /// Our WireGuard private key, fetched with the access token.
    fn private_key(&self, token: &Token) -> Result<PrivateKey>;
}
