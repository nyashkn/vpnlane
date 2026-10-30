//! Static ISO code to continent table for the server tree. Nord's API has no
//! region field; unknown codes land in [`Continent::Other`] so a new country
//! never disappears from the list.

use std::fmt;

use vpnlane_core::CountryCode;

/// A server-tree group, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Continent {
    Africa,
    Asia,
    Europe,
    MiddleEast,
    NorthAmerica,
    Oceania,
    SouthAmerica,
    Other,
}

impl Continent {
    /// Every group, in display order.
    pub(crate) const ALL: [Self; 8] = [
        Self::Africa,
        Self::Asia,
        Self::Europe,
        Self::MiddleEast,
        Self::NorthAmerica,
        Self::Oceania,
        Self::SouthAmerica,
        Self::Other,
    ];
}

impl fmt::Display for Continent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Africa => "Africa",
            Self::Asia => "Asia",
            Self::Europe => "Europe",
            Self::MiddleEast => "Middle East",
            Self::NorthAmerica => "North America",
            Self::Oceania => "Oceania",
            Self::SouthAmerica => "South America",
            Self::Other => "Other",
        })
    }
}

/// The group a country is listed under.
pub(crate) fn continent(code: CountryCode) -> Continent {
    use Continent::{Africa, Asia, Europe, MiddleEast, NorthAmerica, Oceania, Other, SouthAmerica};
    match code.as_str() {
        "AO" | "BF" | "BJ" | "BW" | "CV" | "DZ" | "EG" | "ET" | "GH" | "GM" | "KE" | "KM"
        | "LR" | "LY" | "MA" | "MG" | "MR" | "MU" | "MW" | "MZ" | "NG" | "SC" | "SL" | "SN"
        | "SO" | "TD" | "TN" | "TZ" | "ZA" | "ZM" => Africa,
        "AF" | "AM" | "AZ" | "BD" | "BN" | "BT" | "GE" | "HK" | "ID" | "IN" | "JP" | "KG"
        | "KH" | "KR" | "KZ" | "LA" | "LK" | "MM" | "MN" | "MV" | "MY" | "NP" | "PH" | "PK"
        | "SG" | "TH" | "TJ" | "TW" | "UZ" | "VN" => Asia,
        "AD" | "AL" | "AT" | "BA" | "BE" | "BG" | "CH" | "CY" | "CZ" | "DE" | "DK" | "EE"
        | "ES" | "FI" | "FR" | "GB" | "GR" | "HR" | "HU" | "IE" | "IM" | "IS" | "IT" | "JE"
        | "LI" | "LT" | "LU" | "LV" | "MC" | "MD" | "ME" | "MK" | "MT" | "NL" | "NO" | "PL"
        | "PT" | "RO" | "RS" | "SE" | "SI" | "SK" | "TR" | "UA" => Europe,
        "AE" | "BH" | "IL" | "IQ" | "JO" | "KW" | "LB" | "QA" | "YE" => MiddleEast,
        "AG" | "BB" | "BM" | "BS" | "BZ" | "CA" | "CR" | "DO" | "GL" | "GT" | "HN" | "JM"
        | "KY" | "LC" | "MX" | "PA" | "PR" | "SV" | "TT" | "US" => NorthAmerica,
        "AU" | "FJ" | "GU" | "NZ" | "PG" => Oceania,
        "AR" | "BO" | "BR" | "CL" | "CO" | "EC" | "GY" | "PE" | "PY" | "SR" | "UY" | "VE" => {
            SouthAmerica
        }
        _ => Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code `GET /v1/servers/countries` returned (150, checked 2026-10).
    /// When Nord adds one, it shows under "Other" until it is added here.
    const NORD: &str = "AD AE AF AG AL AM AO AR AT AU AZ BA BB BD BE BF BG BH BJ BM BN BO BR \
        BS BT BW BZ CA CH CL CO CR CV CY CZ DE DK DO DZ EC EE EG ES ET FI FJ FR GB GE GH GL GM \
        GR GT GU GY HK HN HR HU ID IE IL IM IN IQ IS IT JE JM JO JP KE KG KH KM KR KW KY KZ LA \
        LB LC LI LK LR LT LU LV LY MA MC MD ME MG MK MM MN MR MT MU MV MW MX MY MZ NG NL NO NP \
        NZ PA PE PG PH PK PL PR PT PY QA RO RS SC SE SG SI SK SL SN SO SR SV TD TH TJ TN TR TT \
        TW TZ UA US UY UZ VE VN YE ZA ZM";

    #[test]
    fn every_listed_code_has_a_continent() {
        let codes: Vec<&str> = NORD.split_whitespace().collect();
        assert_eq!(codes.len(), 150);
        for code in codes {
            let cc: CountryCode = code.parse().unwrap();
            assert_ne!(continent(cc), Continent::Other, "{code}");
        }
    }

    #[test]
    fn unknown_codes_fall_back_to_other() {
        assert_eq!(continent("XK".parse().unwrap()), Continent::Other);
    }
}
