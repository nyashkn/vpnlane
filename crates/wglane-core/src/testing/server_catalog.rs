use std::collections::HashMap;

use crate::ports::ServerCatalog;
use crate::{City, Country, CountryCode, Error, PrivateKey, Result, Server, Token};

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

    fn recommended(
        &self,
        country: CountryCode,
        city: Option<&City>,
        limit: usize,
    ) -> Result<Vec<Server>> {
        let servers = self
            .servers
            .get(&country)
            .ok_or(Error::UnknownCountry(country))?;
        Ok(servers
            .iter()
            .filter(|s| city.is_none_or(|c| s.city.as_deref() == Some(c.name.as_str())))
            .take(limit)
            .cloned()
            .collect())
    }

    fn private_key(&self, _token: &Token) -> Result<PrivateKey> {
        self.private_key
            .as_deref()
            .map(PrivateKey::new)
            .ok_or_else(|| Error::Api("401 Unauthorized".into()))
    }
}
