use super::Key;

pub fn translate(key: Key) -> &'static str {
    match key {
        // App chrome
        Key::AppTitle => "Replay Control",
        Key::NavHome => "Games",
        Key::NavGames => "Games",
        Key::NavFavorites => "Favs",
        Key::NavMyGames => "My Games",
        Key::NavSearch => "Search",
        Key::NavMore => "More",
        Key::NavSettings => "Settings",

        // Home page
        Key::HomeLastPlayed => "Last Played",
        Key::HomeRecentlyPlayed => "Recently Played",
        Key::HomeLibrary => "Library",
        Key::HomeNoGamesPlayed => "No games played yet",
        Key::HomeNoRecent => "No recent games",
        Key::HomeNoSystems => "No systems with games",
        Key::HomeDiscover => "Discover",
        Key::HomeDiscoverRandom => "Rediscover Your Library",
        Key::HomeDiscoverMultiplayer => "Multiplayer",
        Key::HomeDiscoverGames => "games",

        // Stats
        Key::StatsGames => "Games",
        Key::StatsFavorites => "Favorites",
        Key::StatsUsed => "Used",
        Key::StatsStorage => "Storage",
        Key::StatsStorageUsed => "Storage Used",
        Key::CountGames => "{0} games",
        Key::CountGamesPartial => "{0} / {1} games",
        Key::CountFavorites => "{0} favorites",
        Key::CountFavoritesPartial => "{0} / {1} favorites",

        // Games page
        Key::GamesSearchPlaceholder => "Search games...",
        Key::GamesBack => "\u{2190} Back",
        Key::GamesNoGames => "No games",
        Key::GamesLoadingRoms => "Loading ROMs...",
        Key::GamesLoadMore => "Load more",

        // Favorites page
        Key::FavoritesTitle => "Favorites",
        Key::FavoritesViewGrouped => "View: Grouped",
        Key::FavoritesViewFlat => "View: Flat",
        Key::FavoritesEmpty => "No favorites yet",
        Key::FavoritesLatestAdded => "Latest Added",
        Key::FavoritesRecentlyAdded => "Recently Added",
        Key::FavoritesBySystem => "By System",
        Key::FavoritesAll => "All Favorites",

        // Organize favorites
        Key::OrganizeTitle => "Organize Favorites",
        Key::OrganizeDescription => "Create subfolders to organize your favorites",
        Key::OrganizePrimary => "Organize by",
        Key::OrganizeSecondary => "Then by (optional)",
        Key::OrganizeNone => "None",
        Key::OrganizeSystem => "By System",
        Key::OrganizeGenre => "By Genre",
        Key::OrganizePlayers => "By Players",
        Key::OrganizeRating => "By Rating",
        Key::OrganizeAlphabetical => "Alphabetical",
        Key::OrganizeDeveloper => "By Developer",
        Key::OrganizeKeepOriginals => "Keep copy in root",
        Key::OrganizeKeepHint => {
            "Keeps original files at root so RePlayOS UI still shows all favorites"
        }
        Key::OrganizeApply => "Organize",
        Key::OrganizeOrganizing => "Organizing...",
        Key::OrganizeFlatten => "Flatten All",
        Key::OrganizeFlattening => "Flattening...",
        Key::OrganizeDone => "organized",
        Key::OrganizeFlattened => "favorites moved to root",
        Key::OrganizeAlreadyFlat => "All favorites are already at root",
        Key::OrganizePreview => "Preview",
        Key::OrganizePreviewUnknown => "Unknown",

        // Hostname settings
        Key::MoreHostname => "Hostname",
        Key::HostnameTitle => "Hostname",
        Key::HostnameLabel => "Hostname",
        Key::HostnameHint => {
            "Sets the network name for this RePlayOS system. Use lowercase letters, digits, and hyphens (e.g., replay-living-room)."
        }
        Key::HostnameSaved => {
            "Hostname updated. Reboot may be needed for mDNS (.local) to fully update."
        }
        Key::HostnameInvalid => "Invalid hostname",

        // Password change
        Key::MorePassword => "Change Password",
        Key::PasswordTitle => "Change Password",
        Key::PasswordCurrent => "Current Password",
        Key::PasswordNew => "New Password",
        Key::PasswordConfirm => "Confirm New Password",
        Key::PasswordSave => "Change Password",
        Key::PasswordSuccess => "Password changed successfully",
        Key::PasswordMismatch => "Passwords do not match",
        Key::PasswordWrongCurrent => "Current password is incorrect",
        Key::PasswordEmpty => "Password cannot be empty",
        Key::PasswordDevSkip => "Password change not available in development mode",
        Key::PasswordDeployHint => {
            "After changing the password, use PI_PASS=yourpassword when running dev.sh or install.sh."
        }

        // GitHub API key
        Key::MoreGithub => "GitHub API Key",
        Key::GithubTitle => "GitHub API Key",
        Key::GithubLabel => "Personal Access Token",
        Key::GithubHint => {
            "Optional. Increases the GitHub API rate limit from 60 to 5,000 requests/hour for thumbnail indexing. Create a token at github.com/settings/tokens (no scopes needed)."
        }

        // Settings page
        Key::SettingsTitle => "Settings",
        Key::SettingsSectionAppearance => "Appearance",
        Key::SettingsSectionNetwork => "Network & Security",
        Key::SettingsSectionSystem => "System",

        // More page (legacy keys)
        Key::MoreTitle => "More",
        Key::MoreSectionPreferences => "Preferences",
        Key::MoreSectionGamePreferences => "Game Preferences",
        Key::MoreSectionGameData => "Game Data",
        Key::MoreSectionSystem => "System",
        Key::MoreSectionSystemInfo => "System Info",
        Key::MoreUpload => "Upload ROMs",
        Key::MoreWifi => "Wi-Fi Configuration",
        Key::MoreNfs => "NFS Share Settings",
        Key::MoreSystemInfo => "System Info",
        Key::MoreStorage => "Storage",
        Key::MorePath => "Path",
        Key::MoreDiskTotal => "Disk Total",
        Key::MoreDiskUsed => "Disk Used",
        Key::MoreDiskAvailable => "Disk Available",
        Key::MoreEthernetIp => "Ethernet IP",
        Key::MoreWifiIp => "Wi-Fi IP",
        Key::MoreNotConnected => "Not connected",
        Key::MoreRefreshStorage => "Refresh Storage",
        Key::MoreRefreshing => "Refreshing...",
        Key::MoreStorageChanged => "Storage updated",
        Key::MoreStorageUnchanged => "Storage unchanged",

        // App language (UI locale selector)
        Key::MoreLocale => "Language",
        Key::LocaleTitle => "App Language",
        Key::LocaleSaved => "Language saved",
        Key::LocaleAuto => "Same as browser",
        Key::LocaleEn => "English",
        Key::LocaleEs => "Spanish - Español",
        Key::LocaleJa => "Japanese - 日本語",

        // Text size
        Key::MoreTextSize => "Text Size",
        Key::MoreTextSizeHint => "Adjust the app text size",

        // Region preference
        Key::MoreRegion => "Region",
        Key::RegionTitle => "Region Preferences",
        Key::RegionHint => {
            "Games from your primary region appear first. The secondary region is used as a fallback when your primary isn't available."
        }
        Key::RegionPrimaryLabel => "Primary",
        Key::RegionSecondaryLabel => "Secondary",
        Key::RegionUsa => "USA",
        Key::RegionEurope => "Europe",
        Key::RegionJapan => "Japan",
        Key::RegionWorld => "World",
        Key::RegionSaved => "Region preference saved",
        Key::RegionNone => "None (use default order)",

        // Language preference (for game documents)
        Key::LanguageTitle => "Language",
        Key::LanguageHint => {
            "Preferred language for game manuals and documents. Auto derives from your region preference."
        }
        Key::LanguagePrimaryLabel => "Primary",
        Key::LanguageSecondaryLabel => "Secondary",
        Key::LanguageAuto => "Auto (from region)",
        Key::LanguageEn => "English",
        Key::LanguageEs => "Spanish",
        Key::LanguageFr => "French",
        Key::LanguageDe => "German",
        Key::LanguageIt => "Italian",
        Key::LanguageJa => "Japanese",
        Key::LanguagePt => "Portuguese",
        Key::LanguageSaved => "Language preference saved",

        // Skin
        Key::MoreSkin => "Skin",
        Key::SkinTitle => "Skin",
        Key::SkinCurrent => "Current",
        Key::SkinHint => "Select a skin to apply it.",
        Key::SkinApplied => "Skin applied.",
        Key::SkinSync => "Sync with ReplayOS",
        Key::SkinSyncHint => "When enabled, the app skin follows the ReplayOS skin setting.",

        // WiFi configuration
        Key::WifiTitle => "Wi-Fi Configuration",
        Key::WifiSsid => "Network Name (SSID)",
        Key::WifiPassword => "Password",
        Key::WifiCountry => "Country Code",
        Key::WifiMode => "Security Mode",
        Key::WifiHidden => "Hidden Network",

        // NFS settings
        Key::NfsTitle => "NFS Share Settings",
        Key::NfsServer => "Server Address",
        Key::NfsShare => "Share Path",
        Key::NfsVersion => "NFS Version",
        Key::NfsHint => "A reboot is required for NFS changes to take effect.",

        // Settings (shared)
        Key::SettingsSave => "Save",
        Key::SettingsSaving => "Saving...",
        Key::SettingsSaved => "Settings saved",
        Key::SettingsApplyHint => "Restart ReplayOS to apply changes.",
        Key::SettingsRestartUi => "Restart ReplayOS",
        Key::SettingsRestarting => "Restarting...",
        Key::SettingsReboot => "Reboot System",
        Key::SettingsRebooting => "Rebooting...",
        Key::SettingsRebootHint => "A reboot is required for changes to take effect.",
        Key::SettingsPasswordEnter => "Enter password",

        // Game detail page
        Key::GameDetailInfo => "Game Info",
        Key::GameDetailSystem => "System",
        Key::GameDetailFilename => "Filename",
        Key::GameDetailFileSize => "File Size",
        Key::GameDetailFormat => "Format",
        Key::GameDetailArcadeInfo => "Arcade Info",
        Key::GameDetailReleased => "Released",
        Key::MonthJanShort => "Jan",
        Key::MonthFebShort => "Feb",
        Key::MonthMarShort => "Mar",
        Key::MonthAprShort => "Apr",
        Key::MonthMayShort => "May",
        Key::MonthJunShort => "Jun",
        Key::MonthJulShort => "Jul",
        Key::MonthAugShort => "Aug",
        Key::MonthSepShort => "Sep",
        Key::MonthOctShort => "Oct",
        Key::MonthNovShort => "Nov",
        Key::MonthDecShort => "Dec",
        Key::GameDetailManufacturer => "Manufacturer",
        Key::GameDetailPlayers => "Players",
        Key::GameDetailRotation => "Orientation",
        Key::GameDetailCategory => "Category",
        Key::GameDetailParentRom => "Original Version",
        Key::GameDetailMetadata => "Metadata",
        Key::GameDetailGenre => "Genre",
        Key::GameDetailDeveloper => "Developer",

        Key::GameDetailEmulation => "Compatibility",
        Key::GameDetailRawCategory => "MAME Category",
        Key::GameDetailRegion => "Region",
        Key::GameDetailDescription => "Description",
        Key::GameDetailNoDescription => "No description available",
        Key::GameDetailScreenshots => "Screenshots",
        Key::GameDetailTitleScreen => "Title Screen",
        Key::GameDetailInGame => "In-Game",
        Key::GameDetailNoScreenshots => "No screenshots available",
        Key::GameDetailVideos => "Videos",
        Key::GameDetailNoVideos => "No videos available",
        Key::GameDetailMyVideos => "My Videos",
        Key::GameDetailAddVideo => "Add",
        Key::GameDetailAddVideoPlaceholder => "Paste a YouTube or Twitch URL...",
        Key::GameDetailAddVideoError => "Invalid URL. Supported: YouTube, Twitch, Vimeo.",
        Key::GameDetailAddVideoDuplicate => "This video is already saved.",
        Key::GameDetailVideoAdded => "Video added",
        Key::GameDetailRemoveVideo => "Remove",
        Key::GameDetailFindTrailers => "Find Trailers",
        Key::GameDetailFindGameplay => "Find Gameplay",
        Key::GameDetailFind1cc => "Find 1CC",
        Key::GameDetailNoResults => "No videos found",
        Key::GameDetailSearchError => "Video search unavailable. Paste URLs directly.",
        Key::GameDetailPinVideo => "Pin",
        Key::GameDetailPinned => "Pinned",
        Key::GameDetailShowAllVideos => "Show all",
        Key::GameDetailUserCaptures => "Your Captures",
        Key::GameDetailNoCaptures => {
            "Take screenshots during gameplay on your RePlayOS \u{2014} they'll appear here!"
        }
        Key::GameDetailViewAllCaptures => "View all",
        Key::GameDetailManual => "Manual",
        Key::GameDetailNoManual => "No manual available",
        Key::GameDetailFindManual => "Find Manual",
        Key::GameDetailViewManual => "View",
        Key::GameDetailNoManualResults => "No manuals found",
        Key::GameDetailManualSaved => "Manual saved",
        Key::ManualConfirmDelete => "Delete?",
        Key::GameDetailLaunch => "Launch on TV",
        Key::GameDetailLaunching => "Launching...",
        Key::GameDetailLaunched => "Launched!",
        Key::GameDetailLaunchError => "Failed to launch",
        Key::GameDetailLaunchNotReplayos => "Not running on RePlayOS",
        Key::GameDetailFavorite => "Favorite",
        Key::GameDetailUnfavorite => "Unfavorite",
        Key::GameDetailConfirmDelete => "Confirm Delete",
        Key::GameDetailRegionalVariants => "Regional Variants",
        Key::GameDetailArcadeVersions => "Arcade Versions",
        Key::GameDetailTranslations => "Translations",
        Key::GameDetailHacks => "Hacks",
        Key::GameDetailSpecialVersions => "Special Versions",
        Key::GameDetailAlternateVersions => "Alternate Versions",
        Key::GameDetailAlsoAvailableOn => "Also Available On",
        Key::GameDetailMoreLikeThis => "More Like This",
        Key::GameDetailOtherVersions => "Other Versions",
        Key::GameDetailMoreInSeries => "More in this Series",
        Key::GameDetailMoreOfSeries => "More of {0}",
        Key::GameDetailPlayOrder => "Play Order",
        Key::GameDetailNotInLibrary => "not in library",
        Key::GameDetailNOfM => "{0} of {1}",
        Key::GameDetailChangeCover => "Change cover",
        Key::GameDetailChooseBoxart => "Choose Box Art",
        Key::GameDetailResetDefault => "Reset to Default",
        Key::GameDetailDownloading => "Downloading...",
        Key::GameDetailNoVariants => "No alternative covers found",
        Key::GameDetailBuildIndexFirst => "Download thumbnail index first",
        Key::GameDetailExternalMetadata => "Additional Info",
        Key::GameDetailPublisher => "Publisher",
        Key::GameDetailRating => "Rating",

        // Metadata management
        Key::MoreMetadata => "Game Metadata",
        Key::MetadataTitle => "Game Data",
        Key::MetadataDataSources => "Data Sources",
        Key::MetadataDescriptionsRatings => "Descriptions & Ratings",
        Key::MetadataNoData => "Not imported yet",
        Key::MetadataEntriesSummary => "entries",
        Key::MetadataLastUpdated => "last updated",
        Key::MetadataDownloadingFile => "Downloading metadata...",
        Key::MetadataMatched => "matched",
        Key::MetadataDataManagement => "Data Management",
        Key::MetadataBuildingIndex => "Building ROM index...",
        Key::MetadataParsingXml => "Parsing XML...",
        Key::MetadataImportComplete => "Import complete",
        Key::MetadataImportFailed => "Import failed",
        Key::MetadataProcessed => "processed",
        Key::MetadataSystemOverview => "System Overview",

        // Thumbnails
        Key::MetadataThumbnailsLibretro => "Thumbnails (libretro)",
        Key::MetadataThumbnailSummary => "boxart",
        Key::MetadataThumbnailSnaps => "snaps",
        Key::MetadataThumbnailOnDisk => "on disk",
        Key::MetadataThumbnailIndexSummary => "available across",
        Key::MetadataThumbnailSystems => "systems",
        Key::MetadataThumbnailStop => "Stop",
        Key::MetadataThumbnailCancelling => "Cancelling...",
        Key::MetadataThumbnailPhaseIndexing => "Fetching list...",
        Key::MetadataThumbnailPhaseDownloading => "Downloading...",
        Key::MetadataThumbnailComplete => "Update complete",
        Key::MetadataThumbnailFailed => "Update failed",
        Key::MetadataThumbnailCancelled => "Update cancelled",
        Key::MetadataThumbnailDownloaded => "downloaded",
        Key::MetadataThumbnailIndexed => "indexed",

        // Game library
        Key::MetadataRebuildGameLibrary => "Rebuild Game Library",
        Key::MetadataRebuildingGameLibrary => "Rebuilding...",
        Key::MetadataGameLibraryRebuilt => "Game library rebuilt successfully",
        Key::MetadataConfirmRebuildGameLibrary => {
            "Rebuild the game library? This re-scans all games from disk."
        }

        // Advanced data management
        Key::MetadataAdvancedActions => "Advanced",

        // Data management
        Key::MetadataClearImages => "Clear Downloaded Images",
        Key::MetadataClearedImages => "Images cleared",
        Key::MetadataConfirmClearImages => "Delete all downloaded box art and screenshots?",
        Key::MetadataCleanupOrphans => "Cleanup Orphaned Images",
        Key::MetadataCleaningOrphans => "Cleaning up...",
        Key::MetadataConfirmCleanupOrphans => {
            "Delete images and metadata for ROMs that no longer exist?"
        }
        Key::MetadataClearIndex => "Clear Thumbnail Index",
        Key::MetadataIndexCleared => "Thumbnail index cleared",
        Key::MetadataConfirmClearIndex => {
            "Delete the thumbnail index? It can be rebuilt by clicking Update."
        }
        Key::MetadataClearMetadata => "Clear Metadata",
        Key::MetadataMetadataCleared => "Metadata cleared",
        Key::MetadataConfirmClearMetadata => "Delete all game descriptions and ratings?",

        // Built-in metadata
        Key::MetadataBuiltin => "Built-in Game Data",
        Key::MetadataBuiltinArcade => "Arcade Database",
        Key::MetadataBuiltinArcadeSummary => "entries, MAME",
        Key::MetadataBuiltinConsole => "Console Database",
        Key::MetadataBuiltinConsoleSummaryEntries => "ROM entries across",
        Key::MetadataBuiltinConsoleSummarySystems => "systems",
        Key::MetadataBuiltinWikidataEntries => "Wikidata series entries across",
        Key::MetadataBuiltinWikidataSeries => "series",
        Key::MetadataBuiltinHint => {
            "Names, genres, developers, publishers, player counts, and other metadata compiled into the app. No import needed."
        }

        // Library summary cards
        Key::MetadataSummaryTotalGames => "Total Games",
        Key::MetadataSummaryEnrichment => "Enrichment",
        Key::MetadataSummaryCoOp => "Co-op Games",
        Key::MetadataSummaryYearSpan => "Year Span",
        Key::MetadataSummaryLibrarySize => "Library Size",
        Key::MetadataSummarySystems => "Systems",
        Key::MetadataSummaryStorage => "Storage",

        // System accordion rows
        Key::MetadataSystemCoverage => "coverage",
        Key::MetadataRowGenre => "Genre",
        Key::MetadataRowDeveloper => "Developer",
        Key::MetadataRowRating => "Rating",
        Key::MetadataRowDescription => "Description",
        Key::MetadataRowBoxArt => "Box Art",
        Key::MetadataRowUnique => "unique",
        Key::MetadataRowClones => "clones",
        Key::MetadataRowHacks => "hacks",
        Key::MetadataRowTranslations => "trans",
        Key::MetadataRowSpecial => "special",
        Key::MetadataRowVerified => "verified",
        Key::MetadataRowCoOp => "co-op",
        Key::MetadataRowDrivers => "Drivers:",
        Key::MetadataDriverWorking => "working",
        Key::MetadataDriverImperfect => "imperfect",
        Key::MetadataDriverPreliminary => "preliminary",
        Key::MetadataDriverUnknown => "unknown",
        Key::MetadataExpandAll => "Expand all",
        Key::MetadataCollapseAll => "Collapse all",

        Key::MetadataAttribution => "Attribution",
        Key::MetadataAttributionText => {
            "Game metadata from TheGamesDB, No-Intro, and libretro-database. Descriptions and ratings from LaunchBox. Box art and screenshots from libretro-thumbnails. Series data from Wikidata (CC0). Data is cached locally for offline use."
        }

        // Logs
        Key::MoreLogs => "System Logs",
        Key::LogsTitle => "System Logs",
        Key::LogsRefresh => "Refresh",
        Key::LogsSourceAll => "All Services",
        Key::LogsSourceCompanion => "Replay Control",
        Key::LogsSourceReplay => "RePlayOS UI",

        // Search
        Key::SearchTitle => "Search",
        Key::SearchPlaceholder => "Search all games...",
        Key::SearchNoResults => "No results found",
        Key::SearchNoResultsWithFilters => "No results. Try removing some filters.",
        Key::SearchResultsSummary => "results across",
        Key::SearchSystems => "systems",
        Key::SearchBrowsingGenre => "Browsing all",
        Key::SearchGamesBy => "Games by",
        Key::SearchRandomGame => "Random Game",
        Key::SearchRecentSearches => "Recent Searches",
        Key::SearchClearRecent => "Clear",
        Key::SearchOtherDevelopers => "Other developers matching",

        // Filters
        Key::FilterHideHacks => "Hide Hacks",
        Key::FilterGenre => "Genre",
        Key::FilterGenreAll => "All Genres",
        Key::FilterHideTranslations => "Hide Translations",
        Key::FilterHideBetas => "Hide Betas",
        Key::FilterHideClones => "Hide Clones",
        Key::FilterMultiplayer => "Multiplayer",
        Key::FilterCoOp => "Co-op",
        Key::FilterRatingAny => "Any Rating",
        Key::FilterClearFilters => "Clear Filters",
        Key::FilterActiveSearch => "Search",
        Key::FilterFilteredResults => "Filtered results",

        // Developer page
        Key::DeveloperNoGames => "No games found for this developer",
        Key::DeveloperAllSystems => "All",

        // Metadata busy/scanning banners
        Key::MetadataBusyBanner => {
            "Metadata update in progress \u{2014} some info may be temporarily unavailable"
        }
        Key::MetadataScanningBanner => "Scanning game library...",

        // Common
        Key::CommonLoading => "Loading...",
        Key::CommonError => "Error",
        Key::CommonSeeAll => "See all",
        Key::CommonSystems => "Systems",
        Key::CommonClearing => "Clearing...",
        Key::CommonUpdating => "Updating...",
        Key::CommonUpdate => "Update",
        Key::CommonSearching => "Searching...",
        Key::CommonCancel => "Cancel",
        Key::CommonDelete => "Delete",
        Key::CommonRename => "Rename",
        Key::CommonActions => "Actions",
        Key::CommonSave => "Save",

        // Recommendation section / discover pill titles
        Key::SpotlightBestGenre => "Best {0}",
        Key::SpotlightBestOf => "Best of {0}",
        Key::SpotlightGamesBy => "Games by {0}",
        Key::SpotlightHiddenGems => "Hidden Gems",
        Key::SpotlightTopRated => "Top Rated",
        Key::SpotlightRediscover => "Rediscover Your Library",
        Key::SpotlightBecauseYouLove => "Because You Love {0}",
        Key::SpotlightMoreFrom => "More from {0}",
        Key::PillClassics => "{0}s Classics",
        Key::PillBestOf => "Best of {0}",
        Key::PillGamesBy => "Games by {0}",
        Key::PillMultiplayer => "Multiplayer",
        Key::PillCoOp => "Co-op Games",
        Key::SpotlightCoOp => "Co-op Games",

        // Analytics
        Key::MoreSectionPrivacy => "Privacy",
        Key::AnalyticsTitle => "Anonymous usage statistics",
        Key::AnalyticsDescription => {
            "Help improve Replay Control by sending anonymous install statistics"
        }
        Key::AnalyticsSaved => "Preference saved",
        Key::AnalyticsWhatSent => "What data is sent?",
        Key::AnalyticsFieldInstallId => "Random install ID (not tied to you or your device)",
        Key::AnalyticsFieldVersion => "App version",
        Key::AnalyticsFieldArch => "CPU architecture",
        Key::AnalyticsFieldChannel => "Update channel",
        Key::AnalyticsNotCollected => {
            "Not collected: IP addresses, game library, usage patterns, or any personal information."
        }
        Key::AnalyticsPrivacyLink => "Read the full privacy policy",

        // Updates
        Key::MoreSectionUpdates => "Updates",
        Key::UpdateAvailable => "Version {0} is available",
        Key::UpdateViewRelease => "View on GitHub",
        Key::UpdateSkip => "Skip this version",
        Key::UpdateNow => "Update Now",
        Key::UpdateCheckButton => "Check for Updates",
        Key::UpdateChecking => "Checking...",
        Key::UpdateCheckFailed => "Update check failed",
        Key::UpdateChannel => "Update Channel",
        Key::UpdateChannelStable => "Stable",
        Key::UpdateChannelBeta => "Beta",
        Key::UpdateCurrentVersion => "Current version: v{0} ({1})",
        Key::UpdateUpToDate => "Up to date",
        Key::UpdateDownloading => "Downloading update...",
        Key::UpdateInstalling => "Installing update...",
        Key::UpdateRestarting => "Restarting Replay Control...",
        Key::UpdateFailed => "Update failed",
        Key::UpdateDoNotNavigate => "Please do not close or navigate away from this page",
        Key::UpdateReloadingIn => "Reloading in {0} seconds...",
        Key::UpdateWaitingForServer => "Waiting for server...",
        Key::UpdateBackToSettings => "Back to settings",
        Key::UpdateSystemBusy => "System is busy. Please wait for the current operation to finish.",
        Key::UpdatePageTitle => "Updating Replay Control",

        // Setup checklist (first-run)
        Key::SetupWelcome => "Welcome to Replay Control!",
        Key::SetupIntro => {
            "Get the most out of your library with these optional downloads. You can always do this later from the Metadata page."
        }
        Key::SetupMetadataTitle => "Download game descriptions & ratings",
        Key::SetupMetadataHint => {
            "~100 MB \u{2014} adds descriptions, genres, ratings, and release dates"
        }
        Key::SetupThumbnailTitle => "Update box art index",
        Key::SetupThumbnailHint => "Enables automatic box art downloads",
        Key::SetupSkip => "Skip",
        Key::SetupDismiss => "Dismiss",
        Key::SetupComplete => "Setup complete!",
        Key::SetupInProgress => "In progress\u{2026}",
        Key::SetupTaskDone => "Done",
        Key::SetupStart => "Start",
        Key::SetupUpdate => "Update",

        // Game notes
        Key::GameNotesTitle => "Notes",
        Key::GameNotesPlaceholder => "Add notes about this game...",
        Key::GameNotesSave => "Save",
        Key::GameNotesSaving => "Saving...",
        Key::GameNotesEdit => "Edit",
        Key::GameNotesClear => "Clear",
        Key::GameNotesAdd => "Add Note",
        Key::GameNotesEmpty => "No notes yet",

        // Game status
        Key::GameStatusTitle => "My Progress",
        Key::GameStatusNone => "Not set",
        Key::GameStatusWantToPlay => "Want to Play",
        Key::GameStatusInProgress => "In Progress",
        Key::GameStatusCompleted => "Completed",
        Key::GameStatusPlatinum => "Platinum",
        Key::GameStatusSetStatus => "Set Status",
        Key::GameStatusClear => "Clear Status",
        Key::MyGamesTitle => "My Games",
        Key::MyGamesAll => "All",
        Key::MyGamesWantToPlay => "Want to Play",
        Key::MyGamesInProgress => "In Progress",
        Key::MyGamesCompleted => "Completed",
        Key::MyGamesPlatinum => "Platinum",
        Key::MyGamesEmpty => "No games with this status yet",
        Key::MyGamesUpdated => "Status updated",
    }
}
