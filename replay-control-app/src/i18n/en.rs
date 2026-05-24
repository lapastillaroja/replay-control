use super::Key;

pub fn translate(key: Key) -> &'static str {
    match key {
        // App chrome
        Key::NavGames => "Games",
        Key::NavFavorites => "Favs",
        Key::NavSearch => "Search",
        Key::NavSettings => "Settings",

        // Home page
        Key::HomeNowPlaying => "Now Playing",
        Key::HomeLastPlayed => "Last Played",
        Key::HomeRecentlyPlayed => "Recently Played",
        Key::HomeLibrary => "Library",
        Key::HomeNoGamesPlayed => "No games played yet",
        Key::HomeNoRecent => "No recent games",
        Key::HomeDiscover => "Discover",

        // Stats
        Key::StatsGames => "Games",
        Key::StatsFavorites => "Favorites",
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
        Key::MoreSectionGamePreferences => "Game Preferences",
        Key::MoreWifi => "Wi-Fi Configuration",
        Key::MoreNfs => "NFS Share Settings",
        Key::MoreStorage => "Storage",
        Key::MorePath => "Path",
        Key::MoreDiskTotal => "Disk Total",
        Key::MoreDiskUsed => "Disk Used",
        Key::MoreDiskAvailable => "Disk Available",
        Key::MoreEthernetIp => "Ethernet IP",
        Key::MoreWifiIp => "Wi-Fi IP",
        Key::MoreNotConnected => "Not connected",

        // App language (UI locale selector)
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

        // RetroAchievements settings
        Key::MoreRetroAchievements => "RetroAchievements",
        Key::RetroAchievementsTitle => "RetroAchievements",
        Key::RetroAchievementsUsername => "Username",
        Key::RetroAchievementsPassword => "Password",
        Key::RetroAchievementsPasswordSaved => "Password saved",
        Key::RetroAchievementsPasswordMissing => "No password saved",
        Key::RetroAchievementsCredentialsRequired => {
            "Enter both username and password, or use Clear & Restart RePlayOS to remove the saved account."
        }
        Key::RetroAchievementsSaveRestart => "Save & Restart RePlayOS",
        Key::RetroAchievementsClearRestart => "Clear & Restart RePlayOS",
        Key::RetroAchievementsSaved => "RetroAchievements credentials updated",

        // Skin
        Key::MoreSkin => "Skin",
        Key::SkinTitle => "Skin",
        Key::SkinCurrent => "Current",
        Key::SkinHint => "Select a skin to apply it.",
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

        // Settings (shared)
        Key::SettingsSave => "Save",
        Key::SettingsSaveRestart => "Save & Restart RePlayOS",
        Key::SettingsSaving => "Saving...",
        Key::SettingsSaved => "Settings saved",
        Key::SettingsRestarting => "Restarting...",
        Key::SettingsReplayRestartWarning => {
            "Restarting RePlayOS stops any running game and briefly disconnects the TV frontend."
        }
        Key::SettingsReboot => "Reboot System",
        Key::SettingsRebooting => "Rebooting...",
        Key::SettingsPasswordEnter => "Enter password",

        // Game detail page
        Key::GameDetailInfo => "Game Info",
        Key::GameDetailSystem => "System",
        Key::GameDetailFilename => "Filename",
        Key::GameDetailFileSize => "File Size",
        Key::GameDetailFormat => "Format",
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
        Key::GameDetailPlayers => "Players",
        Key::GameDetailRotation => "Orientation",
        Key::GameDetailParentRom => "Original Version",
        Key::GameDetailGenre => "Genre",
        Key::GameDetailDeveloper => "Developer",

        Key::GameDetailEmulation => "Compatibility",
        Key::GameDetailRawCategory => "MAME Category",
        Key::GameDetailRegion => "Region",
        Key::GameDetailDescription => "Description",
        Key::GameDetailScreenshots => "Screenshots",
        Key::GameDetailTitleScreen => "Title Screen",
        Key::GameDetailInGame => "In-Game",
        Key::GameDetailVideos => "Videos",
        Key::GameDetailNoVideos => "No videos available",
        Key::GameDetailAddVideo => "Add",
        Key::GameDetailAddVideoPlaceholder => "Paste a YouTube or Twitch URL...",
        Key::GameDetailAddVideoError => "Invalid URL. Supported: YouTube, Twitch, Vimeo.",
        Key::GameDetailAddVideoDuplicate => "This video is already saved.",
        Key::GameDetailVideoAdded => "Video added",
        Key::GameDetailRemoveVideo => "Remove",
        Key::GameDetailFindTrailers => "Find Trailers",
        Key::GameDetailFindGameplay => "Find Gameplay",
        Key::GameDetailFind1cc => "Find 1CC",
        Key::GameDetailSuggestedVideos => "Suggested",
        Key::GameDetailAddVideoUrl => "Add a video",
        Key::GameDetailFindOnlineVideos => "Find online",
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
        Key::GameDetailSuggestedManuals => "Suggested",
        Key::GameDetailAddManual => "Add a manual",
        Key::GameDetailManualUrlPlaceholder => "Paste a PDF or text manual URL",
        Key::GameDetailUploadManual => "Upload file",
        Key::GameDetailManualChooseFile => "Choose a PDF or text file first.",
        Key::GameDetailManualInvalidFileType => "Manual uploads must be PDF or text files.",
        Key::GameDetailManualUploadBrowserOnly => "Manual upload is only available in the browser.",
        Key::GameDetailViewManual => "View",
        Key::GameDetailNoManualResults => "No manuals found",
        Key::GameDetailManualSaved => "Manual saved",
        Key::ManualConfirmDelete => "Delete?",
        Key::GameDetailLaunch => "Launch on TV",
        Key::GameDetailLaunching => "Launching...",
        Key::GameDetailLaunched => "Launched!",
        Key::GameDetailLaunchError => "Failed to launch",
        Key::GameDetailLaunchNotReplayos => "Not running on RePlayOS",
        Key::GameDetailNowPlaying => "Now Playing",
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
        Key::GameDetailPublisher => "Publisher",
        Key::GameDetailRating => "Rating",
        Key::GameDetailGameFaqsLink => "Look up on GameFAQs",
        Key::GameDetailShmupsWikiLink => "Strategy guide on Shmups Wiki",
        Key::GameDetailShmupsWikiVideoIndexLink => "Video index on Shmups Wiki",

        // Metadata management
        Key::MoreMetadata => "Game Metadata",
        Key::MetadataTitle => "Game Data",
        Key::MetadataDataSources => "Data Sources",
        Key::MetadataDescriptionsRatings => "Descriptions & Ratings",
        Key::MetadataNoData => "Not imported yet",
        Key::MetadataEntriesSummary => "entries",
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
        Key::MetadataConfirmRebuildGameLibrary => {
            "Rebuild the game library? This re-scans all games from disk."
        }
        Key::MetadataRescanGameLibrary => "Rescan Library",
        Key::MetadataRescanningGameLibrary => "Rescanning...",
        Key::MetadataRescanGameLibraryHint => {
            "Pick up newly added ROMs without waiting. NFS auto-detection runs on a delay."
        }
        Key::MetadataBannerRebuildingLibrary => "Rebuilding library...",
        Key::MetadataBannerRescanningLibrary => "Rescanning library...",
        Key::MetadataBannerEnrichingLibrary => "Enriching library...",
        Key::MetadataProgressVerbRebuilding => "Rebuilding",
        Key::MetadataProgressVerbRescanning => "Rescanning",
        Key::MetadataProgressVerbEnriching => "Enriching",
        Key::MetadataProgressLibraryScanning => "Scanning game library",
        Key::MetadataProgressLibraryEnriching => "Enriching game library",

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
        Key::MetadataBuiltinArcadeSummary => "entries, MAME",
        Key::MetadataBuiltinConsoleSummaryEntries => "ROM entries across",
        Key::MetadataBuiltinConsoleSummarySystems => "systems",
        Key::MetadataBuiltinWikidataEntries => "Wikidata series entries across",
        Key::MetadataBuiltinWikidataSeries => "series",
        Key::MetadataBuiltinManualLinks => {
            "manual links from MiSTer Manual Downloader and Retrokit"
        }
        Key::MetadataBuiltinGuideLinks => "Shmups Wiki guide/video links",
        Key::MetadataBuiltinHint => {
            "Names, genres, developers, publishers, player counts, manual links, guide links, and other metadata compiled into the app. No import needed."
        }

        // Library summary cards
        Key::MetadataSummaryTotalGames => "Total Games",
        Key::MetadataSummaryEnrichment => "Enrichment",
        Key::MetadataSummaryCoOp => "Co-op Games",
        Key::MetadataSummaryYearSpan => "Year Span",
        Key::MetadataSummaryLibrarySize => "Library Size",
        Key::MetadataSummarySystems => "Systems",
        Key::MetadataSummaryStorage => "Storage",
        Key::MetadataSummaryDownloadedArt => "Downloaded Art",

        // System accordion rows
        Key::MetadataSystemCoverage => "coverage",
        Key::MetadataRowGenre => "Genre",
        Key::MetadataRowDeveloper => "Developer",
        Key::MetadataRowPublisher => "Publisher",
        Key::MetadataRowReleaseDate => "Release Date",
        Key::MetadataRowRating => "Rating",
        Key::MetadataRowDescription => "Description",
        Key::MetadataRowBoxArt => "Box Art",
        Key::MetadataRowScreenshots => "Screenshots",
        Key::MetadataRowTitleScreens => "Title Screens",
        Key::MetadataRowManuals => "Manuals",
        Key::MetadataRowVideos => "Videos",
        Key::MetadataRowDownloadedMedia => "Downloaded media:",
        Key::MetadataRowRegions => "Regions:",
        Key::MetadataRowGenreGroups => "Genres:",
        Key::MetadataRowPlayers => "Players:",
        Key::MetadataRowStats => "Stats:",
        Key::MetadataStatsRefreshing => "refreshing",
        Key::MetadataStatsStale => "stale",
        Key::MetadataStatsFailed => "failed",
        Key::MetadataRowUnique => "unique",
        Key::MetadataRowClones => "clones",
        Key::MetadataRowHacks => "hacks",
        Key::MetadataRowTranslations => "trans",
        Key::MetadataRowHomebrew => "homebrew",
        Key::MetadataRowUnlicensed => "unlicensed",
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
            "Game metadata from TheGamesDB, No-Intro, and libretro-database. Descriptions and ratings from LaunchBox. Box art and screenshots from libretro-thumbnails. Series data from Wikidata (CC0). Manual links from MiSTer Manual Downloader and Retrokit; PDFs are downloaded only when saved. Data is cached locally for offline use."
        }

        // Logs
        Key::MoreLogs => "System Logs",
        Key::LogsTitle => "System Logs",
        Key::LogsRefresh => "Refresh",
        Key::LogsCopy => "Copy",
        Key::LogsCopied => "Logs copied",
        Key::LogsSourceAll => "All Services",
        Key::LogsSourceCompanion => "Replay Control",
        Key::LogsSourceReplay => "RePlayOS UI",
        Key::LogsLevelTitle => "Replay Control log level",
        Key::LogsLevelInfo => "Info",
        Key::LogsLevelDebug => "Debug",
        Key::LogsLevelRebootHint => "Saved changes are applied after the system is rebooted.",

        // Search
        Key::SearchPlaceholder => "Search all games...",
        Key::SearchNoResults => "No results found",
        Key::SearchResultsSummary => "results across",
        Key::SearchSystems => "systems",
        Key::SearchBrowsingGenre => "Browsing all",
        Key::SearchGamesBy => "Games by",
        Key::SearchRandomGame => "Random Game",
        Key::SearchRecentSearches => "Recent Searches",
        Key::SearchOtherDevelopers => "Other developers matching",

        // Filters
        Key::FilterHideHacks => "Hide Hacks",
        Key::FilterGenreAll => "All Genres",
        Key::FilterHideTranslations => "Hide Translations",
        Key::FilterHideBetas => "Hide Betas",
        Key::FilterHideClones => "Hide Clones",
        Key::FilterMultiplayer => "Multiplayer",
        Key::FilterCoOp => "Co-op",
        Key::FilterRatingAny => "Any Rating",

        // Developer page
        Key::DeveloperNoGames => "No games found for this developer",
        Key::DeveloperAllSystems => "All",

        // Metadata busy/scanning banners
        Key::MetadataBusyBanner => {
            "Metadata update in progress \u{2014} some info may be temporarily unavailable"
        }
        Key::MetadataBannerFetchingGameMetadata => "Fetching game metadata...",
        Key::MetadataBannerAlreadyUpToDate => "Already up to date",

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

        // Updates
        Key::MoreSectionUpdates => "Updates",
        Key::UpdateAvailable => "Version {0} is available",
        Key::UpdateViewRelease => "View on GitHub",
        Key::UpdateSkip => "Skip this version",
        Key::UpdateNow => "Update Now",
        Key::UpdateCheckButton => "Check for Updates",
        Key::UpdateChecking => "Checking...",
        Key::UpdateChannelStable => "Stable",
        Key::UpdateChannelBeta => "Beta",
        Key::UpdateCurrentVersion => "Current version: v{0} ({1})",
        Key::UpdateUpToDate => "Up to date",
        Key::UpdateDownloading => "Downloading update...",
        Key::UpdateRestarting => "Restarting Replay Control...",
        Key::UpdateFailed => "Update failed",
        Key::UpdateDoNotNavigate => "Please do not close or navigate away from this page",
        Key::UpdateReloadingIn => "Reloading in {0} seconds...",
        Key::UpdateWaitingForServer => "Waiting for server...",
        Key::UpdateBackToSettings => "Back to settings",
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
        Key::SetupStart => "Start",
        Key::SetupUpdate => "Update",
    }
}
