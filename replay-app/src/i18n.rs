use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Supported locales. English is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    #[default]
    En,
}

impl Locale {
    pub fn code(&self) -> &'static str {
        match self {
            Locale::En => "en",
        }
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// Provides the i18n context to the component tree.
/// Call this once at the App root level.
pub fn provide_i18n() {
    let (locale, set_locale) = signal(Locale::default());
    provide_context(I18nContext { locale, set_locale });
}

/// Retrieves the current i18n context.
pub fn use_i18n() -> I18nContext {
    expect_context::<I18nContext>()
}

#[derive(Clone, Copy)]
pub struct I18nContext {
    pub locale: ReadSignal<Locale>,
    pub set_locale: WriteSignal<Locale>,
}

/// Translation function. Returns the localized string for the given key.
/// Currently only English is supported — add match arms per locale to extend.
pub fn t(locale: Locale, key: &str) -> &'static str {
    // All keys fall through to English for now.
    // When adding a new locale, add a match on (locale, key) before the default.
    let _ = locale;
    match key {
        // App chrome
        "app.title" => "Replay",
        "nav.home" => "Home",
        "nav.games" => "Games",
        "nav.favorites" => "Favs",
        "nav.more" => "More",

        // Home page
        "home.last_played" => "Last Played",
        "home.recently_played" => "Recently Played",
        "home.library" => "Library",
        "home.systems" => "Systems",
        "home.no_games_played" => "No games played yet",
        "home.no_recent" => "No recent games",
        "home.no_systems" => "No systems with games",

        // Stats
        "stats.games" => "Games",
        "stats.systems" => "Systems",
        "stats.favorites" => "Favorites",
        "stats.used" => "Used",
        "stats.storage" => "Storage",

        // Games page
        "games.systems" => "Systems",
        "games.search_placeholder" => "Search games...",
        "games.back" => "\u{2190} Back",
        "games.no_games" => "No games",
        "games.loading_roms" => "Loading ROMs...",
        "games.delete_confirm" => "Delete this ROM?",
        "games.deleted" => "ROM deleted",
        "games.renamed" => "ROM renamed",
        "games.rename" => "Rename",
        "games.delete" => "Delete",
        "games.cancel" => "Cancel",
        "games.confirm" => "Confirm",
        "games.actions" => "Actions",
        "games.load_more" => "Load more",

        // Favorites page
        "favorites.title" => "Favorites",
        "favorites.view_grouped" => "View: Grouped",
        "favorites.view_flat" => "View: Flat",
        "favorites.empty" => "No favorites yet",
        "favorites.latest_added" => "Latest Added",
        "favorites.recently_added" => "Recently Added",
        "favorites.by_system" => "By System",
        "favorites.all" => "All Favorites",

        // More page
        "more.title" => "More",
        "more.upload" => "Upload ROMs",
        "more.backup" => "Backup & Restore",
        "more.wifi" => "Wi-Fi Configuration",
        "more.nfs" => "NFS Share Settings",
        "more.system_info" => "System Info",
        "more.storage" => "Storage",
        "more.path" => "Path",
        "more.disk_total" => "Disk Total",
        "more.disk_used" => "Disk Used",
        "more.disk_available" => "Disk Available",
        "more.refresh_storage" => "Refresh Storage",
        "more.refreshing" => "Refreshing...",
        "more.storage_changed" => "Storage updated",
        "more.storage_unchanged" => "Storage unchanged",

        // WiFi configuration
        "wifi.title" => "Wi-Fi Configuration",
        "wifi.ssid" => "Network Name (SSID)",
        "wifi.password" => "Password",
        "wifi.country" => "Country Code",
        "wifi.mode" => "Security Mode",
        "wifi.hidden" => "Hidden Network",

        // NFS settings
        "nfs.title" => "NFS Share Settings",
        "nfs.server" => "Server Address",
        "nfs.share" => "Share Path",
        "nfs.version" => "NFS Version",
        "nfs.hint" => "NFS changes take effect after switching storage to NFS and restarting the ReplayOS UI.",

        // Settings (shared)
        "settings.save" => "Save",
        "settings.saving" => "Saving...",
        "settings.saved" => "Settings saved",
        "settings.apply_hint" => "Restart the ReplayOS UI to apply changes.",
        "settings.restart_ui" => "Restart ReplayOS UI",
        "settings.restarting" => "Restarting...",
        "settings.password_keep" => "Leave empty to keep current",
        "settings.password_enter" => "Enter password",

        // Game detail page
        "game_detail.info" => "Game Info",
        "game_detail.system" => "System",
        "game_detail.filename" => "Filename",
        "game_detail.file_size" => "File Size",
        "game_detail.format" => "Format",
        "game_detail.arcade_info" => "Arcade Info",
        "game_detail.year" => "Year",
        "game_detail.manufacturer" => "Manufacturer",
        "game_detail.players" => "Players",
        "game_detail.rotation" => "Rotation",
        "game_detail.category" => "Category",
        "game_detail.parent_rom" => "Parent ROM",
        "game_detail.description" => "Description",
        "game_detail.no_description" => "No description available",
        "game_detail.screenshots" => "Screenshots",
        "game_detail.no_screenshots" => "No screenshots available",
        "game_detail.videos" => "Videos",
        "game_detail.no_videos" => "No videos available",
        "game_detail.music" => "Music / Soundtrack",
        "game_detail.no_music" => "No soundtrack available",
        "game_detail.manual" => "Manual",
        "game_detail.no_manual" => "No manual available",
        "game_detail.actions" => "Actions",
        "game_detail.favorite" => "Favorite",
        "game_detail.unfavorite" => "Unfavorite",
        "game_detail.rename" => "Rename",
        "game_detail.delete" => "Delete",
        "game_detail.confirm_delete" => "Confirm Delete",

        // Common
        "common.loading" => "Loading...",
        "common.error" => "Error",

        _ => "???",
    }
}
