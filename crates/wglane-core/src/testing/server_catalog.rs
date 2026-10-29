use std::collections::HashMap;

use crate::ports::ServerCatalog;
use crate::{Country, CountryCode, Error, PrivateKey, Result, Server, Token};

/// In-memory [`ServerCatalog`].
#[derive(Clone, Debug, Default)]
pub struct FakeCatalog {
    /// Returned by `countries`.
    pub countries: Vec<Country>,
    /// Returned by `recommended`, per country.
    pub servers: HashMap<CountryCode, Vec<Server>>,
    /// Returned by `private_key`; `None` simulates an auth failure.
    pub private_key: Option<String>,
}

impl ServerCatalog for FakeCatalog {
    fn countries(&self) -> Result<Vec<Country>> {
        Ok(self.countries.clone())
    }

    fn recommended(&self, country: CountryCode, limit: usize) -> Result<Vec<Server>> {
        let servers = self
            .servers
            .get(&country)
            .ok_or(Error::UnknownCountry(country))?;
        Ok(servers.iter().take(limit).cloned().collect())
    }

    fn private_key(&self, _token: &Token) -> Result<PrivateKey> {
        self.private_key
            .as_deref()
            .map(PrivateKey::new)
            .ok_or_else(|| Error::Api("401 Unauthorized".into()))
    }
}
