//! Where to connect: the one value every layer (argument, environment, file,
//! preset) spells the same way.
//!
//! Grammar: `CC` or `CC/City`. `CC/City/hostname` and a bare hostname are
//! reserved for later and rejected with a "not yet" error, so a file written
//! for a future release fails loudly instead of meaning something else.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{CountryCode, Error, Result, fold};

const MAX_CITY: usize = 64;

/// A country, optionally narrowed to a city. The city is checked for shape
/// here and against the provider's list when the connection is applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Location {
    country: CountryCode,
    city: Option<String>,
}

impl Location {
    /// `country`, narrowed to `city` when given. Fails on a city of the wrong shape.
    pub fn new(country: CountryCode, city: Option<String>) -> Result<Self> {
        if let Some(c) = city.as_deref() {
            check_city(c)?;
        }
        Ok(Self { country, city })
    }

    /// The whole country.
    pub fn country_only(country: CountryCode) -> Self {
        Self {
            country,
            city: None,
        }
    }

    /// The country.
    pub fn country(&self) -> CountryCode {
        self.country
    }

    /// The city, as written.
    pub fn city(&self) -> Option<&str> {
        self.city.as_deref()
    }

    /// True when both name the same place: same country, same city ignoring
    /// case and accents.
    pub fn same_place(&self, other: &Self) -> bool {
        self.country == other.country
            && self.city.as_deref().map(fold) == other.city.as_deref().map(fold)
    }
}

/// 1 to 64 letters, digits, spaces, `-`, `_`, `'` or `.`; no `/`.
fn check_city(city: &str) -> Result<()> {
    let ok = !city.is_empty()
        && city.chars().count() <= MAX_CITY
        && city
            .chars()
            // Combining marks, so a decomposed "Zu\u{308}rich" is accepted too.
            .all(|c| {
                c.is_alphanumeric() || " -_'.".contains(c) || ('\u{300}'..='\u{36f}').contains(&c)
            });
    if ok {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "city {city:?}: use 1 to {MAX_CITY} letters, digits, spaces, '-', '_', ''' or '.'"
        )))
    }
}

impl FromStr for Location {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        let bad = |why: &str| Error::Config(format!("location {s:?}: {why}"));
        let mut parts = s.split('/');
        let (cc, city, rest) = (parts.next(), parts.next(), parts.next());
        if rest.is_some() {
            return Err(bad(
                "CC/City/hostname is not supported yet; use CC or CC/City",
            ));
        }
        let Some(city) = city else {
            // No slash: a country code, or a hostname-shaped word we do not take yet.
            return s
                .parse::<CountryCode>()
                .map(Self::country_only)
                .map_err(|_| {
                    if s.chars().any(|c| c.is_ascii_digit() || c == '.') {
                        bad("a bare hostname is not supported yet; use CC or CC/City")
                    } else {
                        bad("use two letters (US, DE) or CC/City (DE/Berlin); not UK, use GB")
                    }
                });
        };
        let cc = cc
            .unwrap_or_default()
            .parse::<CountryCode>()
            .map_err(|_| bad("the part before '/' must be a two-letter country code"))?;
        Self::new(cc, Some(city.trim().to_owned())).map_err(|_| bad("empty or invalid city"))
    }
}

impl TryFrom<String> for Location {
    type Error = Error;

    fn try_from(s: String) -> Result<Self> {
        s.parse()
    }
}

impl From<Location> for String {
    fn from(l: Location) -> Self {
        l.to_string()
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.city {
            Some(c) => write!(f, "{}/{c}", self.country),
            None => write!(f, "{}", self.country),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(s: &str) -> Location {
        s.parse().unwrap()
    }

    #[test]
    fn parses_country_and_city() {
        assert_eq!(loc("de").to_string(), "DE");
        assert_eq!(loc(" DE/Berlin ").to_string(), "DE/Berlin");
        let l = loc("us/New York");
        assert_eq!((l.country().as_str(), l.city()), ("US", Some("New York")));
        assert_eq!(loc("us/new_york").city(), Some("new_york"));
        assert_eq!(loc("CH/Zürich").city(), Some("Zürich"));
    }

    #[test]
    fn city_matching_ignores_case_and_accents() {
        assert!(loc("CH/Zürich").same_place(&loc("ch/ZURICH")));
        // Decomposed (u + combining diaeresis) as a terminal or macOS filename may send it.
        let decomposed = loc("DE/Zu\u{308}rich");
        assert_eq!(decomposed.city(), Some("Zu\u{308}rich"));
        assert!(decomposed.same_place(&loc("de/zürich")));
        assert!(decomposed.same_place(&loc("DE/ZURICH")));
        assert!(!loc("CH/Zürich").same_place(&loc("CH")));
        assert!(!loc("DE/Berlin").same_place(&loc("AT/Berlin")));
    }

    #[test]
    fn rejects_bad_locations_with_a_reason() {
        for (bad, why) in [
            ("", "use two letters"),
            ("USA", "use two letters"),
            ("DE/", "empty or invalid city"),
            ("DE/a*b", "empty or invalid city"),
            ("/Berlin", "two-letter country code"),
            ("DE/Berlin/de1234", "not supported yet"),
            ("de1234", "bare hostname is not supported yet"),
            ("de1234.nordvpn.com", "bare hostname is not supported yet"),
        ] {
            let e = bad.parse::<Location>().unwrap_err().to_string();
            assert!(e.contains(why), "{bad}: {e}");
        }
    }

    #[test]
    fn serializes_as_a_string() {
        let l = loc("DE/Berlin");
        assert_eq!(serde_json::to_string(&l).unwrap(), "\"DE/Berlin\"");
    }
}
