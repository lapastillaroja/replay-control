//! Runtime genre normalizer.
//!
//! Maps raw genre strings from any source (baked-in databases, LaunchBox,
//! catver.ini) to a small set of ~20 canonical genre groups. This is the single
//! source of truth for genre normalization — at runtime AND at catalog build
//! time (`tools/build-catalog` calls `normalize_genre` directly rather than
//! keeping its own copy).
//!
//! Handles:
//!  - LaunchBox semicolon separators ("Action; Platform" -> normalize first part)
//!  - catver.ini slash separators ("Maze / Shooter" -> normalize first part)
//!  - Direct genre strings ("Fighting", "Shoot'em Up", etc.)

/// Canonical name of the gambling genre group. Kept as a single named constant
/// (rather than a scattered magic string) so the normalizer and any consumer
/// reference the one literal.
pub const GENRE_GROUP_GAMBLING: &str = "Gambling";

/// True when a raw genre string is the Mahjong sub-category — the catver
/// "Tabletop / Mahjong" form or a direct "Mahjong" primary. Keyed on the PRIMARY
/// segment, not a bare substring, so device categories that merely mention it
/// (e.g. "Game Console / Home Mahjong", "Handheld / Plug n' Play TV Game /
/// Mahjong") fall through to their real (non-tabletop) primary and become
/// "Other". Internal to `normalize_genre`, which all producers route through.
fn is_mahjong_primary(raw: &str) -> bool {
    // Case-insensitive compare on the borrowed primary segment — no allocation
    // on the common path; only the rare "tabletop" branch lowercases the whole.
    let primary = raw.split(" / ").next().unwrap_or("").trim();
    primary.eq_ignore_ascii_case("mahjong")
        || (primary.eq_ignore_ascii_case("tabletop")
            && raw.to_ascii_lowercase().contains("mahjong"))
}

/// True when a catver category carries the `* Mature *` content marker (adult /
/// strip mahjong, etc.). The companion to `is_mahjong_primary` — keeps the
/// catver content-marker rules together; used by `build-catalog` to set
/// `arcade_game.is_mature`.
pub fn is_mature_category(raw: &str) -> bool {
    raw.contains("* Mature *")
}

/// Normalize a raw genre string to one of the canonical genre groups.
///
/// Takes the first genre from semicolon or slash-separated lists, then maps
/// it to the shared taxonomy. Returns an empty string for empty input.
pub fn normalize_genre(raw: &str) -> &'static str {
    let raw = raw.trim();
    if raw.is_empty() {
        return "";
    }

    // Mahjong is detected before the first-segment split below would fold the
    // "Tabletop / Mahjong" sub-category into Tabletop.
    if is_mahjong_primary(raw) {
        return "Mahjong";
    }

    // Extract the first genre segment:
    // - LaunchBox uses "; " (e.g., "Action; Platform")
    // - catver.ini uses " / " (e.g., "Maze / Shooter")
    let primary = raw.split(';').next().unwrap_or(raw).trim();
    let primary = primary.split(" / ").next().unwrap_or(primary).trim();

    normalize_single(primary)
}

/// Normalize a single genre term (no separators) to the canonical taxonomy.
fn normalize_single(genre: &str) -> &'static str {
    // Exact matches first (case-sensitive for the common case).
    match genre {
        // Already-normalized genres (pass-through).
        "Action" => return "Action",
        "Adventure" => return "Adventure",
        "Beat'em Up" => return "Beat'em Up",
        "Gambling" => return "Gambling",
        "Tabletop" => return "Tabletop",
        "Driving" => return "Driving",
        "Educational" => return "Educational",
        "Fighting" => return "Fighting",
        "Maze" => return "Maze",
        "Music" => return "Music",
        "Pinball" => return "Pinball",
        "Platform" => return "Platform",
        "Puzzle" => return "Puzzle",
        "Quiz" => return "Quiz",
        "Role-Playing" => return "Role-Playing",
        "Shooter" => return "Shooter",
        "Simulation" => return "Simulation",
        "Sports" => return "Sports",
        "Strategy" => return "Strategy",
        "Other" => return "Other",
        _ => {}
    }

    // Case-insensitive matching for variant spellings.
    let lower = genre.to_ascii_lowercase();
    match lower.as_str() {
        // ── Action ──
        "action" | "ball & paddle" | "breakout" | "compilation" | "party" | "sandbox"
        | "stealth" | "horror" | "mmo" | "family" | "comedy" => "Action",

        // ── Adventure ──
        "adventure" => "Adventure",

        // ── Beat'em Up ──
        "beat'em up" | "beat-'em-up" | "beat 'em up" | "beatemup" => "Beat'em Up",

        // ── Gambling ── (slots, casino, gambling-cards/lottery/bingo via first segment)
        "casino" | "gambling" | "slot machine" | "slot" => "Gambling",

        // ── Tabletop ── (board games, chess, shanghai, hanafuda, non-gambling cards)
        "board" | "card" | "board game" | "cards" | "tabletop" => "Tabletop",

        // ── Driving ──
        "driving" | "racing" => "Driving",

        // ── Educational ──
        "educational" => "Educational",

        // ── Fighting ──
        "fighting" | "fighter" => "Fighting",

        // ── Maze ──
        "maze" => "Maze",

        // ── Music ──
        "music" | "rhythm" => "Music",

        // ── Pinball ──
        "pinball" => "Pinball",

        // ── Platform ──
        "platform" | "climbing" => "Platform",

        // ── Puzzle ──
        "puzzle" => "Puzzle",

        // ── Quiz ──
        "quiz" | "trivia" => "Quiz",

        // ── Role-Playing ──
        "role-playing" | "role-playing (rpg)" | "rpg" | "role playing" => "Role-Playing",

        // ── Shooter ──
        "shooter" | "shoot-'em-up" | "shoot'em up" | "lightgun shooter" | "run & gun"
        | "shoot 'em up" | "shmup" => "Shooter",

        // ── Simulation ──
        "simulation"
        | "flight simulator"
        | "virtual life"
        | "flight"
        | "construction and management simulation" => "Simulation",

        // ── Sports ──
        "sports" | "fitness" => "Sports",

        // ── Strategy ──
        "strategy" => "Strategy",

        // ── Non-game / system categories ──
        "system" | "bios" | "utilities" | "electromechanical" | "device" | "rewritable"
        | "not coverage" | "mature" | "other" => "Other",

        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Direct genre strings ──

    #[test]
    fn empty_input() {
        assert_eq!(normalize_genre(""), "");
        assert_eq!(normalize_genre("  "), "");
    }

    #[test]
    fn passthrough_normalized_genres() {
        assert_eq!(normalize_genre("Action"), "Action");
        assert_eq!(normalize_genre("Adventure"), "Adventure");
        assert_eq!(normalize_genre("Beat'em Up"), "Beat'em Up");
        assert_eq!(normalize_genre("Gambling"), "Gambling");
        assert_eq!(normalize_genre("Tabletop"), "Tabletop");
        assert_eq!(normalize_genre("Mahjong"), "Mahjong");
        assert_eq!(normalize_genre("Driving"), "Driving");
        assert_eq!(normalize_genre("Educational"), "Educational");
        assert_eq!(normalize_genre("Fighting"), "Fighting");
        assert_eq!(normalize_genre("Maze"), "Maze");
        assert_eq!(normalize_genre("Music"), "Music");
        assert_eq!(normalize_genre("Pinball"), "Pinball");
        assert_eq!(normalize_genre("Platform"), "Platform");
        assert_eq!(normalize_genre("Puzzle"), "Puzzle");
        assert_eq!(normalize_genre("Quiz"), "Quiz");
        assert_eq!(normalize_genre("Role-Playing"), "Role-Playing");
        assert_eq!(normalize_genre("Shooter"), "Shooter");
        assert_eq!(normalize_genre("Simulation"), "Simulation");
        assert_eq!(normalize_genre("Sports"), "Sports");
        assert_eq!(normalize_genre("Strategy"), "Strategy");
        assert_eq!(normalize_genre("Other"), "Other");
    }

    // ── Arcade catver.ini categories ──

    #[test]
    fn arcade_fighter_category() {
        assert_eq!(normalize_genre("Fighter"), "Fighting");
        assert_eq!(normalize_genre("Fighter / Versus"), "Fighting");
        assert_eq!(normalize_genre("Fighter / 2D"), "Fighting");
    }

    #[test]
    fn arcade_platform_climbing() {
        assert_eq!(normalize_genre("Platform"), "Platform");
        assert_eq!(normalize_genre("Platform / Run Jump"), "Platform");
        assert_eq!(normalize_genre("Climbing"), "Platform");
    }

    #[test]
    fn arcade_shooter_category() {
        assert_eq!(normalize_genre("Shooter"), "Shooter");
        assert_eq!(normalize_genre("Shooter / Flying Vertical"), "Shooter");
    }

    #[test]
    fn arcade_driving_racing() {
        assert_eq!(normalize_genre("Driving"), "Driving");
        assert_eq!(normalize_genre("Racing"), "Driving");
        assert_eq!(normalize_genre("Driving / 1st Person"), "Driving");
    }

    #[test]
    fn arcade_maze() {
        assert_eq!(normalize_genre("Maze"), "Maze");
        assert_eq!(normalize_genre("Maze / Collect"), "Maze");
        assert_eq!(normalize_genre("Maze / Shooter"), "Maze");
    }

    #[test]
    fn arcade_gambling_tabletop_mahjong() {
        // Gambling — first segment carries it (Gambling / *, Slot Machine / *).
        assert_eq!(normalize_genre("Casino"), "Gambling");
        assert_eq!(normalize_genre("Slot Machine"), "Gambling");
        assert_eq!(normalize_genre("Slot Machine / Video Slot"), "Gambling");
        assert_eq!(normalize_genre("Gambling / Cards"), "Gambling");
        // Tabletop — board/chess/shanghai/hanafuda, non-gambling cards.
        assert_eq!(normalize_genre("Tabletop"), "Tabletop");
        assert_eq!(normalize_genre("Board Game"), "Tabletop");
        assert_eq!(normalize_genre("Board Game / Chess Machine"), "Tabletop");
        assert_eq!(normalize_genre("Tabletop / Shanghai"), "Tabletop");
        assert_eq!(normalize_genre("Tabletop / Hanafuda"), "Tabletop");
        assert_eq!(normalize_genre("Cards"), "Tabletop");
        // Mahjong — split out of the Tabletop sub-category by the full-string check.
        assert_eq!(normalize_genre("Tabletop / Mahjong"), "Mahjong");
        assert_eq!(normalize_genre("Mahjong"), "Mahjong");
        // Device categories that merely mention mahjong stay "Other" (overmatch guard).
        assert_eq!(normalize_genre("Game Console / Home Mahjong"), "Other");
        assert_eq!(
            normalize_genre("Handheld / Plug n' Play TV Game / Mahjong"),
            "Other"
        );
    }

    #[test]
    fn arcade_quiz_trivia() {
        assert_eq!(normalize_genre("Quiz"), "Quiz");
        assert_eq!(normalize_genre("Trivia"), "Quiz");
    }

    #[test]
    fn arcade_ball_paddle_breakout() {
        assert_eq!(normalize_genre("Ball & Paddle"), "Action");
        assert_eq!(normalize_genre("Breakout"), "Action");
    }

    #[test]
    fn arcade_beatemup_variants() {
        assert_eq!(normalize_genre("Beat'em Up"), "Beat'em Up");
        assert_eq!(normalize_genre("BeatEmUp"), "Beat'em Up");
    }

    #[test]
    fn arcade_non_game_categories() {
        assert_eq!(normalize_genre("System"), "Other");
        assert_eq!(normalize_genre("BIOS"), "Other");
        assert_eq!(normalize_genre("Utilities"), "Other");
        assert_eq!(normalize_genre("Electromechanical"), "Other");
        assert_eq!(normalize_genre("Mature"), "Other");
    }

    // ── Console genre strings (libretro/TGDB) ──

    #[test]
    fn console_beatemup_variants() {
        assert_eq!(normalize_genre("Beat-'Em-Up"), "Beat'em Up");
        assert_eq!(normalize_genre("Beat 'Em Up"), "Beat'em Up");
    }

    #[test]
    fn console_board_card_variants() {
        assert_eq!(normalize_genre("Board"), "Tabletop");
        assert_eq!(normalize_genre("Card"), "Tabletop");
        assert_eq!(normalize_genre("Gambling"), "Gambling");
    }

    #[test]
    fn console_rpg_variants() {
        assert_eq!(normalize_genre("Role-Playing (RPG)"), "Role-Playing");
        assert_eq!(normalize_genre("RPG"), "Role-Playing");
    }

    #[test]
    fn console_shooter_variants() {
        assert_eq!(normalize_genre("Shoot-'Em-Up"), "Shooter");
        assert_eq!(normalize_genre("Shoot'em Up"), "Shooter");
        assert_eq!(normalize_genre("Lightgun Shooter"), "Shooter");
        assert_eq!(normalize_genre("Run & Gun"), "Shooter");
        assert_eq!(normalize_genre("Shoot 'Em Up"), "Shooter");
    }

    #[test]
    fn console_simulation_variants() {
        assert_eq!(normalize_genre("Flight Simulator"), "Simulation");
        assert_eq!(normalize_genre("Virtual Life"), "Simulation");
    }

    #[test]
    fn console_compilation_party() {
        assert_eq!(normalize_genre("Compilation"), "Action");
        assert_eq!(normalize_genre("Party"), "Action");
    }

    #[test]
    fn console_misc_mapped_to_action() {
        assert_eq!(normalize_genre("Sandbox"), "Action");
        assert_eq!(normalize_genre("Stealth"), "Action");
        assert_eq!(normalize_genre("Horror"), "Action");
    }

    // ── LaunchBox semicolon-separated genres ──

    #[test]
    fn launchbox_semicolon_separated() {
        assert_eq!(normalize_genre("Action; Platform"), "Action");
        assert_eq!(
            normalize_genre("Construction and Management Simulation; Strategy"),
            "Simulation"
        );
        assert_eq!(
            normalize_genre("Role-Playing (RPG); Action"),
            "Role-Playing"
        );
        assert_eq!(normalize_genre("Sports; Racing"), "Sports");
    }

    // ── catver.ini slash-separated categories ──

    #[test]
    fn catver_slash_separated() {
        assert_eq!(normalize_genre("Shooter / Flying Vertical"), "Shooter");
        assert_eq!(normalize_genre("Maze / Collect"), "Maze");
        assert_eq!(normalize_genre("Fighter / Versus"), "Fighting");
        assert_eq!(normalize_genre("Platform / Run Jump"), "Platform");
    }

    // ── Whitespace handling ──

    #[test]
    fn whitespace_trimming() {
        assert_eq!(normalize_genre("  Action  "), "Action");
        assert_eq!(normalize_genre("  Shooter / Flying  "), "Shooter");
    }

    // ── Unknown genres ──

    #[test]
    fn unknown_genres_map_to_other() {
        assert_eq!(normalize_genre("SomeNewGenre"), "Other");
        assert_eq!(normalize_genre("Totally Unknown"), "Other");
    }
}
