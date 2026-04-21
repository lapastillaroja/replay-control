//! Date precision marker for partial/full release dates.
//!
//! `release_date` strings are ISO 8601 but may be truncated: `"1991"` (year),
//! `"1991-08"` (month), or `"1991-08-23"` (day). `DatePrecision` captures
//! which truncation level the data represents, so callers can format the
//! date correctly without guessing.

/// Precision level of a `release_date` string.
///
/// Serialization (JSON / SQLite TEXT column) uses the lowercase name:
/// `"year" | "month" | "day"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatePrecision {
    Year,
    Month,
    Day,
}

impl DatePrecision {
    /// Parse from the lowercase DB / JSON representation.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "year" => Some(Self::Year),
            "month" => Some(Self::Month),
            "day" => Some(Self::Day),
            _ => None,
        }
    }

    /// Lowercase string for DB / JSON storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Year => "year",
            Self::Month => "month",
            Self::Day => "day",
        }
    }

    /// Rank for comparison: higher is more precise.
    pub fn rank(&self) -> u8 {
        match self {
            Self::Day => 3,
            Self::Month => 2,
            Self::Year => 1,
        }
    }
}
