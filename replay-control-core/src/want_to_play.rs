use serde::{Deserialize, Serialize};

/// A game added to the user's backlog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WantToPlayEntry {
    pub system: String,
    pub rom_filename: String,
    pub base_title: String,
    pub added_at: u64,
}

/// Completion-time data fetched from HowLongToBeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HltbData {
    pub game_id: u64,
    /// Main story only (seconds).
    pub main_secs: Option<u64>,
    /// Main story + extras (seconds).
    pub main_extra_secs: Option<u64>,
    /// Full completionist run (seconds).
    pub completionist_secs: Option<u64>,
}

impl HltbData {
    /// Format seconds as a human-readable duration string ("7½h", "12h", "45m").
    pub fn format_hours(secs: u64) -> String {
        if secs == 0 {
            return "—".to_string();
        }
        let total_mins = secs / 60;
        let hours = total_mins / 60;
        let mins = total_mins % 60;

        if hours == 0 {
            format!("{mins}m")
        } else if mins < 8 {
            format!("{hours}h")
        } else if mins < 23 {
            format!("{hours}¼h")
        } else if mins < 38 {
            format!("{hours}½h")
        } else if mins < 53 {
            format!("{hours}¾h")
        } else {
            format!("{}h", hours + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_hours_rounds_to_quarter() {
        assert_eq!(HltbData::format_hours(27000), "7½h"); // 7h30m
        assert_eq!(HltbData::format_hours(3600), "1h"); // 1h0m
        assert_eq!(HltbData::format_hours(45 * 60), "45m"); // 45m
        assert_eq!(HltbData::format_hours(0), "—");
    }
}
