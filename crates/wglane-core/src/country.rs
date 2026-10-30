use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// ISO 3166-1 alpha-2 country code, stored upper case (`US`, `ZA`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CountryCode([u8; 2]);

impl CountryCode {
    /// Upper-case form, as the Nord API and users write it.
    pub fn as_str(&self) -> &str {
        // Both bytes are ASCII (checked in `from_str`), so this never falls back.
        std::str::from_utf8(&self.0).unwrap_or("??")
    }

    /// Lower-case form, used for config file names (`us.conf`).
    pub fn to_lowercase(self) -> String {
        self.as_str().to_ascii_lowercase()
    }
}

impl FromStr for CountryCode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().as_bytes() {
            [a, b] if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => {
                Ok(Self([a.to_ascii_uppercase(), b.to_ascii_uppercase()]))
            }
            _ => Err(Error::InvalidCountryCode(s.to_owned())),
        }
    }
}

impl TryFrom<String> for CountryCode {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<CountryCode> for String {
    fn from(c: CountryCode) -> Self {
        c.as_str().to_owned()
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A country the provider serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Country {
    /// ISO code.
    pub code: CountryCode,
    /// Display name, e.g. "South Africa".
    pub name: String,
    /// Cities with servers; empty when the provider did not say.
    #[serde(default)]
    pub cities: Vec<City>,
}

/// A city the provider has servers in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct City {
    /// Provider id, used to filter server lookups (0 when unknown).
    #[serde(default)]
    pub id: u32,
    /// Display name, e.g. "New York".
    #[serde(default)]
    pub name: String,
    /// Provider slug, e.g. `us_new-york`.
    #[serde(default)]
    pub identifier: String,
    /// How many servers the provider has there.
    #[serde(default)]
    pub server_count: u32,
}

/// Lower-cases and strips common Latin accents, for forgiving name matching.
// shortcut: Latin letters only, no unicode-normalization dep; add the crate if other scripts appear.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
            'ì'..='ï' | 'ī' | 'ı' => 'i',
            'ł' => 'l',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
            'ř' => 'r',
            'ś' | 'š' | 'ş' => 's',
            'ť' | 'ţ' => 't',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

impl Country {
    /// The city whose name or identifier equals `query`, ignoring case and
    /// accents. Fails with `UnknownCity`, which lists the valid names.
    pub fn find_city(&self, query: &str) -> Result<&City, Error> {
        let q = fold(query.trim());
        self.cities
            .iter()
            .find(|c| fold(&c.name) == q || fold(&c.identifier) == q)
            .ok_or_else(|| {
                let mut names: Vec<&str> = self.cities.iter().map(|c| c.name.as_str()).collect();
                names.sort_unstable();
                Error::UnknownCity {
                    country: self.code,
                    query: query.to_owned(),
                    valid: names.join(", "),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalises_case() {
        let cc: CountryCode = " za ".parse().unwrap();
        assert_eq!(cc.as_str(), "ZA");
        assert_eq!(cc.to_lowercase(), "za");
    }

    fn us() -> Country {
        let city = |id, name: &str, ident: &str| City {
            id,
            name: name.into(),
            identifier: ident.into(),
            server_count: 3,
        };
        Country {
            code: "US".parse().unwrap(),
            name: "United States".into(),
            cities: vec![
                city(1, "New York", "us_new-york"),
                city(2, "Zürich", "us_zurich"),
            ],
        }
    }

    #[test]
    fn city_match_ignores_case_and_accents_and_takes_identifiers() {
        let c = us();
        for q in ["new york", " NEW YORK ", "us_new-york", "zurich", "ZÜRICH"] {
            assert!(c.find_city(q).is_ok(), "{q}");
        }
        let err = c.find_city("york").unwrap_err().to_string();
        assert!(err.contains("New York, Zürich"), "{err}");
    }

    #[test]
    fn rejects_non_alpha2() {
        for bad in ["", "U", "USA", "U1", "ü"] {
            assert!(bad.parse::<CountryCode>().is_err(), "accepted {bad:?}");
        }
    }
}
