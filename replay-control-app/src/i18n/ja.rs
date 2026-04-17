use super::Key;

pub fn translate(key: Key) -> &'static str {
    match key {
        // App chrome
        Key::AppTitle => "Replay Control",
        Key::NavHome => "ゲーム",
        Key::NavGames => "ゲーム",
        Key::NavFavorites => "お気に入り",
        Key::NavSearch => "検索",
        Key::NavMore => "その他",
        Key::NavSettings => "設定",

        // Home page
        Key::HomeLastPlayed => "最後にプレイ",
        Key::HomeRecentlyPlayed => "最近プレイしたゲーム",
        Key::HomeLibrary => "ライブラリ",
        Key::HomeNoGamesPlayed => "まだゲームをプレイしていません",
        Key::HomeNoRecent => "最近のゲームなし",
        Key::HomeNoSystems => "ゲームのあるシステムなし",
        Key::HomeDiscover => "おすすめ",
        Key::HomeDiscoverRandom => "あなたのライブラリから",
        Key::HomeDiscoverMultiplayer => "マルチプレイ",
        Key::HomeDiscoverGames => "ゲーム",

        // Stats
        Key::StatsGames => "ゲーム",
        Key::StatsFavorites => "お気に入り",
        Key::StatsUsed => "使用中",
        Key::StatsStorage => "ストレージ",
        Key::StatsStorageUsed => "使用ストレージ",
        Key::CountGames => "{0} ゲーム",
        Key::CountGamesPartial => "{0} / {1} ゲーム",
        Key::CountFavorites => "{0} お気に入り",
        Key::CountFavoritesPartial => "{0} / {1} お気に入り",

        // Games page
        Key::GamesSearchPlaceholder => "ゲームを検索...",
        Key::GamesBack => "\u{2190} 戻る",
        Key::GamesNoGames => "ゲームなし",
        Key::GamesLoadingRoms => "ROM読み込み中...",
        Key::GamesLoadMore => "もっと読み込む",

        // Favorites page
        Key::FavoritesTitle => "お気に入り",
        Key::FavoritesViewGrouped => "表示：グループ",
        Key::FavoritesViewFlat => "表示：フラット",
        Key::FavoritesEmpty => "お気に入りはまだありません",
        Key::FavoritesLatestAdded => "最近追加",
        Key::FavoritesRecentlyAdded => "最近追加されたもの",
        Key::FavoritesBySystem => "システム別",
        Key::FavoritesAll => "すべてのお気に入り",

        // Organize favorites
        Key::OrganizeTitle => "お気に入りを整理",
        Key::OrganizeDescription => "サブフォルダを作成してお気に入りを整理します",
        Key::OrganizePrimary => "整理方法",
        Key::OrganizeSecondary => "次の基準（任意）",
        Key::OrganizeNone => "なし",
        Key::OrganizeSystem => "システム別",
        Key::OrganizeGenre => "ジャンル別",
        Key::OrganizePlayers => "プレイヤー数別",
        Key::OrganizeRating => "評価別",
        Key::OrganizeAlphabetical => "アルファベット順",
        Key::OrganizeDeveloper => "開発元別",
        Key::OrganizeKeepOriginals => "ルートにコピーを残す",
        Key::OrganizeKeepHint => {
            "RePlayOS UIがすべてのお気に入りを表示し続けられるよう、元のファイルをルートに残します"
        }
        Key::OrganizeApply => "整理する",
        Key::OrganizeOrganizing => "整理中...",
        Key::OrganizeFlatten => "すべて平坦化",
        Key::OrganizeFlattening => "平坦化中...",
        Key::OrganizeDone => "件整理済み",
        Key::OrganizeFlattened => "件のお気に入りをルートに移動",
        Key::OrganizeAlreadyFlat => "すべてのお気に入りはすでにルートにあります",
        Key::OrganizePreview => "プレビュー",
        Key::OrganizePreviewUnknown => "不明",

        // Hostname settings
        Key::MoreHostname => "ホスト名",
        Key::HostnameTitle => "ホスト名",
        Key::HostnameLabel => "ホスト名",
        Key::HostnameHint => {
            "このRePlayOSシステムのネットワーク名を設定します。小文字、数字、ハイフンのみ使用できます（例：replay-living-room）。"
        }
        Key::HostnameSaved => {
            "ホスト名を更新しました。mDNS（.local）が完全に反映されるまで再起動が必要な場合があります。"
        }
        Key::HostnameInvalid => "ホスト名が無効です",

        // Password change
        Key::MorePassword => "パスワード変更",
        Key::PasswordTitle => "パスワード変更",
        Key::PasswordCurrent => "現在のパスワード",
        Key::PasswordNew => "新しいパスワード",
        Key::PasswordConfirm => "新しいパスワードを確認",
        Key::PasswordSave => "パスワードを変更",
        Key::PasswordSuccess => "パスワードを変更しました",
        Key::PasswordMismatch => "パスワードが一致しません",
        Key::PasswordWrongCurrent => "現在のパスワードが正しくありません",
        Key::PasswordEmpty => "パスワードを空にすることはできません",
        Key::PasswordDevSkip => "開発モードではパスワード変更は使用できません",
        Key::PasswordDeployHint => {
            "パスワード変更後は、dev.shやinstall.shを実行する際にPI_PASS=パスワードを指定してください。"
        }

        // GitHub API key
        Key::MoreGithub => "GitHub APIキー",
        Key::GithubTitle => "GitHub APIキー",
        Key::GithubLabel => "個人アクセストークン",
        Key::GithubHint => {
            "任意。サムネイルのインデックス作成時のGitHub APIレート制限を60から5,000リクエスト/時間に増やします。github.com/settings/tokensでトークンを作成してください（スコープ不要）。"
        }

        // Settings page
        Key::SettingsTitle => "設定",
        Key::SettingsSectionAppearance => "外観",
        Key::SettingsSectionNetwork => "ネットワークとセキュリティ",
        Key::SettingsSectionSystem => "システム",

        // More page (legacy keys)
        Key::MoreTitle => "その他",
        Key::MoreSectionPreferences => "設定",
        Key::MoreSectionGamePreferences => "ゲーム設定",
        Key::MoreSectionGameData => "ゲームデータ",
        Key::MoreSectionSystem => "システム",
        Key::MoreSectionSystemInfo => "システム情報",
        Key::MoreUpload => "ROMをアップロード",
        Key::MoreWifi => "Wi-Fi設定",
        Key::MoreNfs => "NFS共有設定",
        Key::MoreSystemInfo => "システム情報",
        Key::MoreStorage => "ストレージ",
        Key::MorePath => "パス",
        Key::MoreDiskTotal => "ディスク合計",
        Key::MoreDiskUsed => "ディスク使用量",
        Key::MoreDiskAvailable => "ディスク空き容量",
        Key::MoreEthernetIp => "Ethernet IP",
        Key::MoreWifiIp => "Wi-Fi IP",
        Key::MoreNotConnected => "未接続",
        Key::MoreRefreshStorage => "ストレージを更新",
        Key::MoreRefreshing => "更新中...",
        Key::MoreStorageChanged => "ストレージを更新しました",
        Key::MoreStorageUnchanged => "ストレージに変更なし",

        // App language (UI locale selector)
        Key::MoreLocale => "言語",
        Key::LocaleTitle => "アプリの言語",
        Key::LocaleSaved => "言語を保存しました",
        Key::LocaleAuto => "ブラウザと同じ",
        Key::LocaleEn => "英語 - English",
        Key::LocaleEs => "スペイン語 - Español",
        Key::LocaleJa => "日本語",

        // Text size
        Key::MoreTextSize => "文字サイズ",
        Key::MoreTextSizeHint => "アプリの文字サイズを調整します",

        // Region preference
        Key::MoreRegion => "地域",
        Key::RegionTitle => "地域設定",
        Key::RegionHint => {
            "優先地域のゲームが先に表示されます。第二地域は優先地域が利用できない場合の代替として使用されます。"
        }
        Key::RegionPrimaryLabel => "第一地域",
        Key::RegionSecondaryLabel => "第二地域",
        Key::RegionUsa => "USA",
        Key::RegionEurope => "ヨーロッパ",
        Key::RegionJapan => "日本",
        Key::RegionWorld => "ワールド",
        Key::RegionSaved => "地域設定を保存しました",
        Key::RegionNone => "なし（デフォルト順）",

        // Language preference (for game documents)
        Key::LanguageTitle => "ドキュメント言語",
        Key::LanguageHint => {
            "ゲームのマニュアルやドキュメントの優先言語。「自動」は地域設定に基づいて決まります。"
        }
        Key::LanguagePrimaryLabel => "第一言語",
        Key::LanguageSecondaryLabel => "第二言語",
        Key::LanguageAuto => "自動（地域から）",
        Key::LanguageEn => "英語",
        Key::LanguageEs => "スペイン語",
        Key::LanguageFr => "フランス語",
        Key::LanguageDe => "ドイツ語",
        Key::LanguageIt => "イタリア語",
        Key::LanguageJa => "日本語",
        Key::LanguagePt => "ポルトガル語",
        Key::LanguageSaved => "言語設定を保存しました",

        // Skin
        Key::MoreSkin => "スキン",
        Key::SkinTitle => "スキン",
        Key::SkinCurrent => "現在",
        Key::SkinHint => "スキンを選択して適用します。",
        Key::SkinApplied => "スキンを適用しました。",
        Key::SkinSync => "RePlayOSと同期",
        Key::SkinSyncHint => "有効にすると、アプリのスキンがRePlayOSのスキン設定に従います。",

        // WiFi configuration
        Key::WifiTitle => "Wi-Fi設定",
        Key::WifiSsid => "ネットワーク名（SSID）",
        Key::WifiPassword => "パスワード",
        Key::WifiCountry => "国コード",
        Key::WifiMode => "セキュリティモード",
        Key::WifiHidden => "非表示ネットワーク",

        // NFS settings
        Key::NfsTitle => "NFS共有設定",
        Key::NfsServer => "サーバーアドレス",
        Key::NfsShare => "共有パス",
        Key::NfsVersion => "NFSバージョン",
        Key::NfsHint => "NFS設定を反映するには再起動が必要です。",

        // Settings (shared)
        Key::SettingsSave => "保存",
        Key::SettingsSaving => "保存中...",
        Key::SettingsSaved => "設定を保存しました",
        Key::SettingsApplyHint => "変更を反映するにはRePlayOSを再起動してください。",
        Key::SettingsRestartUi => "RePlayOSを再起動",
        Key::SettingsRestarting => "再起動中...",
        Key::SettingsReboot => "システムを再起動",
        Key::SettingsRebooting => "再起動中...",
        Key::SettingsRebootHint => "変更を反映するには再起動が必要です。",
        Key::SettingsPasswordEnter => "パスワードを入力",

        // Game detail page
        Key::GameDetailInfo => "ゲーム情報",
        Key::GameDetailSystem => "システム",
        Key::GameDetailFilename => "ファイル名",
        Key::GameDetailFileSize => "ファイルサイズ",
        Key::GameDetailFormat => "フォーマット",
        Key::GameDetailArcadeInfo => "アーケード情報",
        Key::GameDetailYear => "発売年",
        Key::GameDetailManufacturer => "メーカー",
        Key::GameDetailPlayers => "プレイヤー数",
        Key::GameDetailRotation => "画面の向き",
        Key::GameDetailCategory => "カテゴリ",
        Key::GameDetailParentRom => "オリジナル版",
        Key::GameDetailMetadata => "メタデータ",
        Key::GameDetailGenre => "ジャンル",
        Key::GameDetailDeveloper => "開発元",

        Key::GameDetailEmulation => "互換性",
        Key::GameDetailRawCategory => "MAMEカテゴリ",
        Key::GameDetailRegion => "地域",
        Key::GameDetailDescription => "説明",
        Key::GameDetailNoDescription => "説明はありません",
        Key::GameDetailScreenshots => "スクリーンショット",
        Key::GameDetailTitleScreen => "タイトル画面",
        Key::GameDetailInGame => "ゲーム中",
        Key::GameDetailNoScreenshots => "スクリーンショットなし",
        Key::GameDetailVideos => "動画",
        Key::GameDetailNoVideos => "動画なし",
        Key::GameDetailMyVideos => "マイ動画",
        Key::GameDetailAddVideo => "追加",
        Key::GameDetailAddVideoPlaceholder => "YouTubeまたはTwitchのURLを貼り付け...",
        Key::GameDetailAddVideoError => "無効なURL。対応サービス：YouTube、Twitch、Vimeo。",
        Key::GameDetailAddVideoDuplicate => "この動画はすでに保存されています。",
        Key::GameDetailVideoAdded => "動画を追加しました",
        Key::GameDetailRemoveVideo => "削除",
        Key::GameDetailFindTrailers => "トレーラーを検索",
        Key::GameDetailFindGameplay => "ゲームプレイを検索",
        Key::GameDetailFind1cc => "1CCを検索",
        Key::GameDetailNoResults => "動画が見つかりません",
        Key::GameDetailSearchError => "動画検索は利用できません。URLを直接貼り付けてください。",
        Key::GameDetailPinVideo => "固定",
        Key::GameDetailPinned => "固定済み",
        Key::GameDetailShowAllVideos => "すべて表示",
        Key::GameDetailUserCaptures => "マイキャプチャ",
        Key::GameDetailNoCaptures => {
            "RePlayOSでゲーム中にスクリーンショットを撮ると、ここに表示されます！"
        }
        Key::GameDetailViewAllCaptures => "すべて見る",
        Key::GameDetailManual => "マニュアル",
        Key::GameDetailNoManual => "マニュアルなし",
        Key::GameDetailFindManual => "マニュアルを検索",
        Key::GameDetailViewManual => "表示",
        Key::GameDetailNoManualResults => "マニュアルが見つかりません",
        Key::GameDetailManualSaved => "マニュアルを保存しました",
        Key::ManualConfirmDelete => "削除しますか？",
        Key::GameDetailLaunch => "TVで起動",
        Key::GameDetailLaunching => "起動中...",
        Key::GameDetailLaunched => "起動しました！",
        Key::GameDetailLaunchError => "起動に失敗しました",
        Key::GameDetailLaunchNotReplayos => "RePlayOS上で動作していません",
        Key::GameDetailFavorite => "お気に入りに追加",
        Key::GameDetailUnfavorite => "お気に入りから削除",
        Key::GameDetailConfirmDelete => "削除の確認",
        Key::GameDetailRegionalVariants => "地域別バリアント",
        Key::GameDetailArcadeVersions => "アーケードバージョン",
        Key::GameDetailTranslations => "翻訳版",
        Key::GameDetailHacks => "ハック版",
        Key::GameDetailSpecialVersions => "特別バージョン",
        Key::GameDetailAlternateVersions => "代替バージョン",
        Key::GameDetailAlsoAvailableOn => "他のシステムでも利用可能",
        Key::GameDetailMoreLikeThis => "似たゲーム",
        Key::GameDetailOtherVersions => "他のバージョン",
        Key::GameDetailMoreInSeries => "同シリーズの作品",
        Key::GameDetailPlayOrder => "プレイ順",
        Key::GameDetailNotInLibrary => "ライブラリにありません",
        Key::GameDetailNOfM => "{1}中{0}",
        Key::GameDetailChangeCover => "カバーを変更",
        Key::GameDetailChooseBoxart => "ボックスアートを選択",
        Key::GameDetailResetDefault => "デフォルトに戻す",
        Key::GameDetailDownloading => "ダウンロード中...",
        Key::GameDetailNoVariants => "代替カバーが見つかりません",
        Key::GameDetailBuildIndexFirst => "先にサムネイルインデックスをダウンロードしてください",
        Key::GameDetailExternalMetadata => "追加情報",
        Key::GameDetailPublisher => "発売元",
        Key::GameDetailRating => "評価",

        // Metadata management
        Key::MoreMetadata => "ゲームメタデータ",
        Key::MetadataTitle => "ゲームデータ",
        Key::MetadataDataSources => "データソース",
        Key::MetadataDescriptionsRatings => "説明・評価",
        Key::MetadataNoData => "未インポート",
        Key::MetadataEntriesSummary => "件",
        Key::MetadataLastUpdated => "最終更新",
        Key::MetadataDownloadingFile => "メタデータをダウンロード中...",
        Key::MetadataMatched => "件一致",
        Key::MetadataDataManagement => "データ管理",
        Key::MetadataBuildingIndex => "ROMインデックスを構築中...",
        Key::MetadataParsingXml => "XML解析中...",
        Key::MetadataImportComplete => "インポート完了",
        Key::MetadataImportFailed => "インポート失敗",
        Key::MetadataProcessed => "件処理済み",
        Key::MetadataSystemOverview => "システム概要",

        // Thumbnails
        Key::MetadataThumbnailsLibretro => "サムネイル（libretro）",
        Key::MetadataThumbnailSummary => "ボックスアート",
        Key::MetadataThumbnailSnaps => "スクリーンショット",
        Key::MetadataThumbnailOnDisk => "ディスク上",
        Key::MetadataThumbnailIndexSummary => "件、",
        Key::MetadataThumbnailSystems => "システムで利用可能",
        Key::MetadataThumbnailStop => "停止",
        Key::MetadataThumbnailCancelling => "キャンセル中...",
        Key::MetadataThumbnailPhaseIndexing => "インデックス取得中...",
        Key::MetadataThumbnailPhaseDownloading => "ダウンロード中...",
        Key::MetadataThumbnailComplete => "更新完了",
        Key::MetadataThumbnailFailed => "更新失敗",
        Key::MetadataThumbnailCancelled => "更新をキャンセルしました",
        Key::MetadataThumbnailDownloaded => "件ダウンロード済み",
        Key::MetadataThumbnailIndexed => "件インデックス済み",

        // Game library
        Key::MetadataRebuildGameLibrary => "ゲームライブラリを再構築",
        Key::MetadataRebuildingGameLibrary => "再構築中...",
        Key::MetadataGameLibraryRebuilt => "ゲームライブラリを再構築しました",
        Key::MetadataConfirmRebuildGameLibrary => {
            "ゲームライブラリを再構築しますか？ディスクからすべてのゲームを再スキャンします。"
        }

        // Advanced data management
        Key::MetadataAdvancedActions => "詳細設定",

        // Data management
        Key::MetadataClearImages => "ダウンロード済み画像を削除",
        Key::MetadataClearedImages => "画像を削除しました",
        Key::MetadataConfirmClearImages => {
            "ダウンロード済みのボックスアートとスクリーンショットをすべて削除しますか？"
        }
        Key::MetadataCleanupOrphans => "孤立した画像を削除",
        Key::MetadataCleaningOrphans => "クリーンアップ中...",
        Key::MetadataConfirmCleanupOrphans => "存在しないROMの画像とメタデータを削除しますか？",
        Key::MetadataClearIndex => "サムネイルインデックスを削除",
        Key::MetadataIndexCleared => "サムネイルインデックスを削除しました",
        Key::MetadataConfirmClearIndex => {
            "サムネイルインデックスを削除しますか？「更新」をクリックすれば再構築できます。"
        }
        Key::MetadataClearMetadata => "メタデータを削除",
        Key::MetadataMetadataCleared => "メタデータを削除しました",
        Key::MetadataConfirmClearMetadata => "すべてのゲームの説明と評価を削除しますか？",

        // Built-in metadata
        Key::MetadataBuiltin => "内蔵ゲームデータ",
        Key::MetadataBuiltinArcade => "アーケードデータベース",
        Key::MetadataBuiltinArcadeSummary => "件、MAME",
        Key::MetadataBuiltinConsole => "コンソールデータベース",
        Key::MetadataBuiltinConsoleSummaryEntries => "件のROMエントリ、",
        Key::MetadataBuiltinConsoleSummarySystems => "システム",
        Key::MetadataBuiltinWikidataEntries => "件のWikidataシリーズエントリ、",
        Key::MetadataBuiltinWikidataSeries => "シリーズ",
        Key::MetadataBuiltinHint => {
            "名前、ジャンル、開発元、発売元、プレイヤー数などのメタデータはアプリに内蔵されています。インポート不要です。"
        }

        // Library summary cards
        Key::MetadataSummaryTotalGames => "総ゲーム数",
        Key::MetadataSummaryEnrichment => "データ充実度",
        Key::MetadataSummaryCoOp => "協力プレイ",
        Key::MetadataSummaryYearSpan => "年代",
        Key::MetadataSummaryLibrarySize => "ライブラリ容量",
        Key::MetadataSummarySystems => "システム数",

        // System accordion rows
        Key::MetadataSystemCoverage => "充実度",
        Key::MetadataRowGenre => "ジャンル",
        Key::MetadataRowDeveloper => "開発元",
        Key::MetadataRowRating => "評価",
        Key::MetadataRowDescription => "説明",
        Key::MetadataRowBoxArt => "カバー",
        Key::MetadataRowUnique => "オリジナル",
        Key::MetadataRowClones => "クローン",
        Key::MetadataRowHacks => "改造版",
        Key::MetadataRowTranslations => "翻訳",
        Key::MetadataRowSpecial => "特別版",
        Key::MetadataRowVerified => "検証済み",
        Key::MetadataRowCoOp => "協力プレイ",
        Key::MetadataRowDrivers => "ドライバ:",
        Key::MetadataDriverWorking => "動作",
        Key::MetadataDriverImperfect => "不完全",
        Key::MetadataDriverPreliminary => "予備",
        Key::MetadataDriverUnknown => "不明",
        Key::MetadataExpandAll => "すべて展開",
        Key::MetadataCollapseAll => "すべて折りたたむ",

        Key::MetadataAttribution => "出典",
        Key::MetadataAttributionText => {
            "ゲームメタデータはTheGamesDB、No-Intro、libretro-databaseから。説明・評価はLaunchBox提供。ボックスアート・スクリーンショットはlibretro-thumbnailsから。シリーズデータはWikidata（CC0）から。データはオフライン利用のためローカルにキャッシュされています。"
        }

        // Logs
        Key::MoreLogs => "システムログ",
        Key::LogsTitle => "システムログ",
        Key::LogsRefresh => "更新",
        Key::LogsSourceAll => "すべてのサービス",
        Key::LogsSourceCompanion => "Replay Control",
        Key::LogsSourceReplay => "RePlayOS UI",

        // Search
        Key::SearchTitle => "検索",
        Key::SearchPlaceholder => "すべてのゲームを検索...",
        Key::SearchNoResults => "結果が見つかりません",
        Key::SearchNoResultsWithFilters => "結果なし。フィルターを外してみてください。",
        Key::SearchResultsSummary => "件、",
        Key::SearchSystems => "システムで見つかりました",
        Key::SearchBrowsingGenre => "すべて閲覧中",
        Key::SearchGamesBy => "開発元のゲーム",
        Key::SearchRandomGame => "おまかせゲーム",
        Key::SearchRecentSearches => "最近の検索",
        Key::SearchClearRecent => "消去",
        Key::SearchOtherDevelopers => "一致する他の開発元",

        // Filters
        Key::FilterHideHacks => "ハック版を非表示",
        Key::FilterGenre => "ジャンル",
        Key::FilterGenreAll => "すべてのジャンル",
        Key::FilterHideTranslations => "翻訳版を非表示",
        Key::FilterHideBetas => "ベータ版を非表示",
        Key::FilterHideClones => "クローンを非表示",
        Key::FilterMultiplayer => "マルチプレイ",
        Key::FilterCoOp => "協力プレイ",
        Key::FilterRatingAny => "評価を問わない",
        Key::FilterClearFilters => "フィルターをクリア",
        Key::FilterActiveSearch => "検索",
        Key::FilterFilteredResults => "フィルター結果",

        // Developer page
        Key::DeveloperNoGames => "この開発元のゲームが見つかりません",
        Key::DeveloperAllSystems => "すべて",

        // Metadata busy/scanning banners
        Key::MetadataBusyBanner => {
            "メタデータを更新中です \u{2014} 一部の情報が一時的に表示されない場合があります"
        }
        Key::MetadataScanningBanner => "ゲームライブラリをスキャン中...",

        // Common
        Key::CommonLoading => "読み込み中...",
        Key::CommonError => "エラー",
        Key::CommonSeeAll => "すべて見る",
        Key::CommonSystems => "システム",
        Key::CommonClearing => "削除中...",
        Key::CommonUpdating => "更新中...",
        Key::CommonUpdate => "更新",
        Key::CommonSearching => "検索中...",
        Key::CommonCancel => "キャンセル",
        Key::CommonDelete => "削除",
        Key::CommonRename => "名前変更",
        Key::CommonActions => "操作",
        Key::CommonSave => "保存",

        // Recommendation section / discover pill titles
        Key::SpotlightBestGenre => "ベスト{0}",
        Key::SpotlightBestOf => "{0}のベスト",
        Key::SpotlightGamesBy => "{0}のゲーム",
        Key::SpotlightHiddenGems => "隠れた名作",
        Key::SpotlightTopRated => "高評価",
        Key::SpotlightRediscover => "あなたのライブラリから",
        Key::SpotlightBecauseYouLove => "{0}が好きな方へ",
        Key::SpotlightMoreFrom => "{0}シリーズのその他のゲーム",
        Key::PillClassics => "{0}年代の名作",
        Key::PillBestOf => "{0}のベスト",
        Key::PillGamesBy => "{0}のゲーム",
        Key::PillMultiplayer => "マルチプレイ",
        Key::PillCoOp => "協力プレイ",
        Key::SpotlightCoOp => "協力プレイ",

        // Analytics
        Key::MoreSectionPrivacy => "プライバシー",
        Key::AnalyticsTitle => "匿名の使用統計",
        Key::AnalyticsDescription => "匿名のインストール統計を送信してReplay Controlの改善に貢献",
        Key::AnalyticsSaved => "設定を保存しました",
        Key::AnalyticsWhatSent => "送信されるデータは？",
        Key::AnalyticsFieldInstallId => {
            "ランダムなインストールID（あなたやデバイスに紐付けられません）"
        }
        Key::AnalyticsFieldVersion => "アプリのバージョン",
        Key::AnalyticsFieldArch => "CPUアーキテクチャ",
        Key::AnalyticsFieldChannel => "アップデートチャンネル",
        Key::AnalyticsNotCollected => {
            "収集されないもの：IPアドレス、ゲームライブラリ、使用パターン、個人情報。"
        }
        Key::AnalyticsPrivacyLink => "プライバシーポリシー全文を読む",

        // Updates
        Key::MoreSectionUpdates => "アップデート",
        Key::UpdateAvailable => "バージョン {0} が利用可能です",
        Key::UpdateViewRelease => "GitHubで見る",
        Key::UpdateSkip => "このバージョンをスキップ",
        Key::UpdateNow => "今すぐアップデート",
        Key::UpdateCheckButton => "アップデートを確認",
        Key::UpdateChecking => "確認中...",
        Key::UpdateCheckFailed => "アップデートの確認に失敗しました",
        Key::UpdateChannel => "チャンネル",
        Key::UpdateChannelStable => "安定版",
        Key::UpdateChannelBeta => "ベータ版",
        Key::UpdateCurrentVersion => "現在のバージョン: v{0} ({1})",
        Key::UpdateUpToDate => "最新です",
        Key::UpdateDownloading => "アップデートをダウンロード中...",
        Key::UpdateInstalling => "アップデートをインストール中...",
        Key::UpdateRestarting => "Replay Controlを再起動中...",
        Key::UpdateFailed => "アップデートに失敗しました",
        Key::UpdateDoNotNavigate => "このページを閉じたり移動したりしないでください",
        Key::UpdateReloadingIn => "{0}秒後にリロードします...",
        Key::UpdateWaitingForServer => "サーバーの応答を待っています...",
        Key::UpdateBackToSettings => "設定に戻る",
        Key::UpdateSystemBusy => "システムがビジーです。現在の操作が完了するまでお待ちください。",
        Key::UpdatePageTitle => "Replay Controlをアップデート中",

        // Setup checklist (first-run)
        Key::SetupWelcome => "Replay Controlへようこそ！",
        Key::SetupIntro => {
            "オプションのダウンロードでライブラリを最大限に活用しましょう。メタデータページからいつでも実行できます。"
        }
        Key::SetupMetadataTitle => "ゲーム情報と評価をダウンロード",
        Key::SetupMetadataHint => "約100 MB — 説明、ジャンル、評価、発売日を追加",
        Key::SetupThumbnailTitle => "カバーアートのインデックスを更新",
        Key::SetupThumbnailHint => "カバーアートの自動ダウンロードを有効にします",
        Key::SetupSkip => "スキップ",
        Key::SetupDismiss => "閉じる",
        Key::SetupComplete => "セットアップ完了！",
        Key::SetupInProgress => "処理中\u{2026}",
        Key::SetupTaskDone => "完了",
        Key::SetupStart => "開始",
    }
}
