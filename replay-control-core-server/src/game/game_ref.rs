//! Native `GameRef` constructor — resolves display_name via the arcade/game
//! DBs and delegates to `GameRef::from_parts` (pure, in core).
//!
//! The `GameRef` struct, pure helpers, and inherent `from_parts` /
//! `new_with_display` methods live in `replay_control_core::game_ref`.

pub use replay_control_core::game_ref::GameRef;

use crate::arcade_db;
use crate::game_db;
use replay_control_core::systems;

/// Create a new GameRef, resolving display_name from the appropriate DB.
///
/// For arcade systems, uses the arcade DB (zip filename lookup).
/// For non-arcade systems with game DB coverage, uses the game DB
/// (No-Intro filename lookup).
pub async fn new(system: &str, rom_filename: String, rom_path: String) -> GameRef {
    let resolved_name = if systems::is_arcade_system(system) {
        let resolved = arcade_db::arcade_display_name_for_system(system, &rom_filename).await;
        if resolved != rom_filename {
            Some(resolved)
        } else {
            None
        }
    } else {
        game_db::game_display_name(system, &rom_filename).await
    };

    GameRef::from_parts(system, rom_filename, rom_path, resolved_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn display_name_uninverts_m3u() {
        crate::catalog_pool::init_test_catalog().await;
        let game_ref = new(
            "sharp_x68k",
            "4th Unit Act 2, The.m3u".to_string(),
            "/roms/sharp_x68k/4th Unit Act 2, The.m3u".to_string(),
        )
        .await;
        assert_eq!(game_ref.display_name.as_deref(), Some("The 4th Unit Act 2"));
    }

    #[tokio::test]
    async fn display_name_uninverts_dim() {
        crate::catalog_pool::init_test_catalog().await;
        let game_ref = new(
            "sharp_x68k",
            "Emerald Dragon, The (1990)(Glodia)(Disk 1 of 5).dim".to_string(),
            "/roms/sharp_x68k/Emerald Dragon, The (1990)(Glodia)(Disk 1 of 5).dim".to_string(),
        )
        .await;
        assert_eq!(
            game_ref.display_name.as_deref(),
            Some("The Emerald Dragon [Disk 1 of 5]")
        );
    }

    #[tokio::test]
    async fn display_name_no_comma_no_change() {
        crate::catalog_pool::init_test_catalog().await;
        let game_ref = new(
            "sharp_x68k",
            "Alshark.m3u".to_string(),
            "/roms/sharp_x68k/Alshark.m3u".to_string(),
        )
        .await;
        assert_eq!(game_ref.display_name.as_deref(), Some("Alshark"));
    }

    #[tokio::test]
    async fn display_name_side_a() {
        crate::catalog_pool::init_test_catalog().await;
        let game_ref = new(
            "amstrad_cpc",
            "Arkanoid (1987)(Imagine)(GB)(Side A).dsk".to_string(),
            "/roms/amstrad_cpc/Arkanoid (1987)(Imagine)(GB)(Side A).dsk".to_string(),
        )
        .await;
        assert_eq!(
            game_ref.display_name.as_deref(),
            Some("Arkanoid (UK) [Side A]")
        );
    }

    #[tokio::test]
    async fn display_name_no_disc_label() {
        crate::catalog_pool::init_test_catalog().await;
        let game_ref = new(
            "amstrad_cpc",
            "Commando (1985)(Elite)(GB).dsk".to_string(),
            "/roms/amstrad_cpc/Commando (1985)(Elite)(GB).dsk".to_string(),
        )
        .await;
        assert_eq!(game_ref.display_name.as_deref(), Some("Commando (UK)"));
    }
}
