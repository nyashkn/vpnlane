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

    #[test]
    fn rejects_non_alpha2() {
        for bad in ["", "U", "USA", "U1", "ü"] {
            assert!(bad.parse::<CountryCode>().is_err(), "accepted {bad:?}");
        }
    }
}
