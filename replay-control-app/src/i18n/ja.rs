use super::Key;

pub fn translate(key: Key) -> &'static str {
    match key {
        // App chrome
        Key::NavGames => "ゲーム",
        Key::NavFavorites => "お気に入り",
        Key::NavSearch => "検索",
        Key::NavSettings => "設定",

        // Home page
        Key::HomeNowPlaying => "プレイ中",
        Key::HomeLastPlayed => "最後にプレイ",
        Key::HomeRecentlyPlayed => "最近プレイしたゲーム",
        Key::HomeLibrary => "ライブラリ",
        Key::HomeNoGamesPlayed => "まだゲームをプレイしていません",
        Key::HomeNoRecent => "最近のゲームなし",
        Key::HomeDeleteRecent => "最近から削除",
        Key::HomeDeleteRecentConfirm => "最近のゲームから削除しますか？",
        Key::HomeDiscover => "おすすめ",

        // Stats
        Key::StatsGames => "ゲーム",
        Key::StatsFavorites => "お気に入り",
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
        Key::OrganizeBoard => "基板別",
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
        Key::SettingsSectionLibraryGames => "ライブラリとゲーム",
        Key::SettingsSectionDeviceNetwork => "デバイスネットワーク",
        Key::SettingsSectionAccess => "アクセスとセキュリティ",
        Key::SettingsSectionSystem => "システム",
        Key::AccessSecurityTitle => "アクセスとセキュリティ",
        Key::AccessHttpsTitle => "HTTPS",
        Key::AccessHttpsEnabled => "ポート8443で安全なローカルアクセス",
        Key::AccessCertificateTitle => "証明書",
        Key::AccessCertificateLocal => "自己署名証明書を自動管理",
        Key::AccessCertificateMode => "モード",
        Key::AccessCertificateGenerated => "生成日",
        Key::AccessCertificateExpires => "有効期限",
        Key::AccessCertificateFingerprint => "SHA-256フィンガープリント",
        Key::AccessCertificateCoveredNames => "証明書に含まれる名前",
        Key::AccessCertificateCurrentNames => "現在のデバイス名",
        Key::AccessCertificateMissingCoverage => "不足している証明書範囲",
        Key::AccessCertificateCovered => "現在のアクセス名はすべて証明書に含まれています",
        Key::AccessCertificateRegenerate => "証明書を再生成",
        Key::AccessCertificateRegenerateConfirm => {
            "HTTPS証明書を再生成しますか？Replay Control が再起動し、このページは一時的に切断されます。少し待ってから再接続し、ローカルのセキュリティ例外をもう一度承認してください。"
        }
        Key::AccessCertificateRegenerated => {
            "証明書を再生成しました。Replay Control を再起動しています。少し待ってから再接続し、新しい証明書を承認してください。"
        }
        Key::AccessCertificateTrustHint => {
            "証明書を再生成すると、ローカルのセキュリティ例外を再度承認する必要がある場合があります。"
        }
        Key::AccessDowngradeUnavailableHint => {
            "アクセスコードでサインインして管理者に昇格した場合にのみ利用できます。"
        }
        Key::AccessNormalUserTitle => "通常ユーザーアクセス",
        Key::AccessNormalUserReplayOs => {
            "ゲーム起動とプレイヤー操作にはRePlayOS Net Controlのペアリングコードを使用します。"
        }
        Key::AccessManageReplayOs => "RePlayOS接続を管理",
        Key::AccessDevicePasswordTitle => "デバイスパスワード",
        Key::AccessDevicePasswordSummary => "デバイスレベルの変更に使うrootパスワード",
        Key::AccessDevicePasswordHint => {
            "管理者レベルのデバイスアクセスに使うRePlayOSのrootパスワードを変更します。"
        }
        Key::AccessAdminTimeoutTitle => "管理者解除の時間",
        Key::AccessAdminTimeoutHint => {
            "デバイスパスワードを入力した後、管理者アクセスを維持する時間です。"
        }
        Key::AccessAdminTimeoutOneHour => "1時間",
        Key::AccessAdminTimeoutThreeHours => "3時間",
        Key::AccessAdminTimeoutTwelveHours => "12時間",
        Key::LoginTitle => "サインイン",
        Key::LoginWelcomeTitle => "Replay Controlへようこそ",
        Key::LoginWelcomeBody => {
            "ここでサインインします。通常アクセスはNet Controlコード、管理者設定はデバイスパスワードを使います。"
        }
        Key::LoginUserTitle => "通常ユーザーアクセス",
        Key::LoginUserCodeLabel => "Net Controlコード",
        Key::LoginUserCodeHint => {
            "TVで SYSTEM > OPTIONS から NET CONTROL を有効にし、SYSTEM > INFORMATION の NET CONTROL CODE を入力します。"
        }
        Key::LoginUserSubmit => "コードでサインイン",
        Key::LoginAdminTitle => "管理者アクセス",
        Key::LoginAdminPasswordLabel => "デバイスパスワード",
        Key::LoginAdminHint => {
            "RePlayOSのrootパスワードを使います。Wi-Fi、NFS、ホスト名、アップデートなどのデバイス変更用です。"
        }
        Key::LoginAdminSubmit => "管理者としてサインイン",
        Key::LoginSuccessUser => "通常ユーザーとしてサインインしました",
        Key::LoginSuccessAdmin => "管理者としてサインインしました",
        Key::LoginDowngrade => "通常ユーザーに切り替え",
        Key::LoginDowngradeSuccess => "通常ユーザーアクセスに切り替えました",
        Key::LoginLogout => "サインアウト",
        Key::LoginLogoutAll => "すべてのセッションをサインアウト",
        Key::LoginLogoutAllConfirm => "すべてのReplay Controlセッションをサインアウトしますか？",
        Key::LoginContinue => "続行",
        Key::LoginCurrentRole => "現在のアクセス",
        Key::LoginAdminTimeRemaining => "管理者時間の残り",
        Key::LoginSessionTimeRemaining => "セッション時間の残り",
        Key::LoginStandaloneOpenTitle => "オープンなスタンドアロンモード",
        Key::LoginStandaloneOpenHint => {
            "このデバイス外のライブラリはローカルで開かれています。サインインはRePlayOSデバイス上でのみ使用します。"
        }
        Key::FirstSetupTitle => "初回セットアップ",
        Key::FirstSetupBody => {
            "ここでアクセスを設定します。管理者として続行するため、デバイスパスワードを一度確認してください。"
        }
        Key::FirstSetupPasswordTitle => "デバイスアクセスを確認",
        Key::FirstSetupPasswordHint => {
            "現在のRePlayOS rootパスワードを入力してください。新しいイメージのデフォルトパスワードはreplayosです。"
        }
        Key::FirstSetupSubmit => "管理者として続行",
        Key::AuthRoleAnonymous => "未サインイン",
        Key::AuthRoleOpen => "オープンアクセス",
        Key::AuthRoleUser => "通常ユーザー",
        Key::AuthRoleAdmin => "管理者",

        // More page (legacy keys)
        Key::MoreSectionGamePreferences => "ゲーム設定",
        Key::MoreWifi => "Wi-Fi設定",
        Key::MoreNfs => "NFS共有設定",
        Key::MoreStorage => "ストレージ",
        Key::MorePath => "パス",
        Key::MoreDiskTotal => "ディスク合計",
        Key::MoreDiskUsed => "ディスク使用量",
        Key::MoreDiskAvailable => "ディスク空き容量",
        Key::MoreEthernetIp => "Ethernet IP",
        Key::MoreWifiIp => "Wi-Fi IP",
        Key::MoreEthernetMac => "Ethernet MAC",
        Key::MoreWifiMac => "Wi-Fi MAC",
        Key::MoreNotConnected => "未接続",
        Key::MoreModel => "モデル",
        Key::MoreCpuTemperature => "CPU 温度",
        Key::MoreAvailableRam => "使用可能 RAM",
        Key::MoreUptime => "稼働時間",

        // App language (UI locale selector)
        Key::LocaleTitle => "アプリの言語",
        Key::LocaleSaved => "言語を保存しました",
        Key::LocaleAuto => "自動",
        Key::LocaleEn => "英語 - English",
        Key::LocaleEs => "スペイン語 - Español",
        Key::LocaleJa => "日本語",

        // Text size
        Key::MoreTextSize => "文字サイズ",
        Key::MoreTextSizeHint => "アプリの文字サイズを調整します",

        // Region preference
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

        // RetroAchievements settings
        Key::MoreRetroAchievements => "RetroAchievements",
        Key::RetroAchievementsTitle => "RetroAchievements",
        Key::RetroAchievementsUsername => "ユーザー名",
        Key::RetroAchievementsPassword => "パスワード",
        Key::RetroAchievementsPasswordSaved => "パスワード保存済み",
        Key::RetroAchievementsPasswordMissing => "パスワード未設定",
        Key::RetroAchievementsCredentialsRequired => {
            "ユーザー名とパスワードの両方を入力するか、「消去してRePlayOSを再起動」で保存済みアカウントを削除してください。"
        }
        Key::RetroAchievementsSaveRestart => "保存してRePlayOSを再起動",
        Key::RetroAchievementsClearRestart => "消去してRePlayOSを再起動",
        Key::RetroAchievementsSaved => "RetroAchievements認証情報を更新しました",

        // Skin
        Key::MoreSkin => "スキン",
        Key::SkinTitle => "スキン",
        Key::SkinCurrent => "現在",
        Key::SkinHint => "スキンを選択して適用します。",
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

        // Settings (shared)
        Key::SettingsSave => "保存",
        Key::SettingsSaveRestart => "保存してRePlayOSを再起動",
        Key::SettingsSaving => "保存中...",
        Key::SettingsSaved => "設定を保存しました",
        Key::SettingsRestarting => "再起動中...",
        Key::SettingsReplayRestartWarning => {
            "RePlayOSを再起動すると実行中のゲームが停止し、TVフロントエンドが一時的に切断されます。"
        }
        Key::SettingsReboot => "システムを再起動",
        Key::SettingsRebooting => "再起動中...",
        Key::SettingsDeviceOnlyDisabled => "RePlayOSデバイスでのみ利用できます。",
        Key::SettingsAdminOnlyDisabled => {
            "これらの設定を変更するには管理者としてサインインしてください。"
        }
        Key::SettingsPasswordEnter => "パスワードを入力",

        // Game detail page
        Key::GameDetailInfo => "ゲーム情報",
        Key::GameDetailSystem => "システム",
        Key::GameDetailFilename => "ファイル名",
        Key::GameDetailFileSize => "ファイルサイズ",
        Key::GameDetailFormat => "フォーマット",
        Key::GameDetailReleased => "発売日",
        Key::GameDetailOnePlayer => "1人プレイ",
        Key::GameDetailPlayerRange => "1-{0}人プレイ",
        Key::MonthJanShort => "1月",
        Key::MonthFebShort => "2月",
        Key::MonthMarShort => "3月",
        Key::MonthAprShort => "4月",
        Key::MonthMayShort => "5月",
        Key::MonthJunShort => "6月",
        Key::MonthJulShort => "7月",
        Key::MonthAugShort => "8月",
        Key::MonthSepShort => "9月",
        Key::MonthOctShort => "10月",
        Key::MonthNovShort => "11月",
        Key::MonthDecShort => "12月",
        Key::GameDetailPlayers => "プレイヤー数",
        Key::GameDetailRotation => "画面の向き",
        Key::GameDetailParentRom => "オリジナル版",
        Key::GameDetailBoard => "基板",
        Key::GameDetailRetroAchievements => "RetroAchievements",
        Key::GameDetailRetroAchievementsNoCore => {
            "このシステムのRePlayエミュレーターは実績に対応していません"
        }
        Key::GameDetailRetroAchievementsDiscFormat => {
            "RePlayはこのディスク形式からは実績をまだ記録できません"
        }
        Key::GameDetailGenre => "ジャンル",
        Key::GameDetailDeveloper => "開発元",
        Key::GameDetailPlaytime => "プレイ時間",

        Key::GameDetailEmulation => "互換性",
        Key::GameDetailRawCategory => "MAMEカテゴリ",
        Key::GameDetailRegion => "地域",
        Key::GameDetailDescription => "説明",
        Key::GameDetailShowMore => "さらに表示",
        Key::GameDetailShowLess => "折りたたむ",
        Key::GameDetailScreenshots => "スクリーンショット",
        Key::GameDetailTitleScreen => "タイトル画面",
        Key::GameDetailInGame => "ゲーム中",
        Key::GameDetailVideos => "動画",
        Key::GameDetailRemoveVideo => "削除",
        Key::GameDetailFindTrailers => "トレーラーを検索",
        Key::GameDetailFindGameplay => "ゲームプレイを検索",
        Key::GameDetailFind1cc => "1CCを検索",
        Key::GameDetailFindOnlineVideos => "オンラインで検索",
        Key::GameDetailNoResults => "動画が見つかりません",
        Key::GameDetailSearchError => "動画検索は利用できません。URLを直接貼り付けてください。",
        Key::GameDetailPinVideo => "固定",
        Key::GameDetailResources => "リソース",
        Key::GameDetailManualsGuidesAndLinks => "マニュアル・攻略・リンク",
        Key::GameDetailSuggestedResources => "おすすめリソース",
        Key::GameDetailAddResource => "リソースを追加またはアップロード",
        Key::GameDetailAddResourceSubmit => "追加",
        Key::GameDetailNoSavedResources => "保存済みリソースはまだありません",
        Key::GameDetailShowAllResources => "すべてのリソースを表示",
        Key::GameDetailResourceUrlPlaceholder => "動画、攻略、マニュアル、WebサイトのURLを貼り付け",
        Key::GameDetailUserCaptures => "マイキャプチャ",
        Key::GameDetailNoCaptures => {
            "RePlayOSでゲーム中にスクリーンショットを撮ると、ここに表示されます！"
        }
        Key::GameDetailViewAllCaptures => "すべて見る",
        Key::GameDetailDeleteCapture => "キャプチャを削除",
        Key::GameDetailDeleteCaptureConfirm => "このキャプチャを削除しますか？",
        Key::GameDetailManual => "マニュアル",
        Key::GameDetailNoManual => "マニュアルなし",
        Key::GameDetailSuggestedManuals => "おすすめ",
        Key::GameDetailAddManual => "マニュアルを追加",
        Key::GameDetailManualUrlPlaceholder => "PDFまたはテキストのマニュアルURLを貼り付け",
        Key::GameDetailUploadManual => "ファイルをアップロード",
        Key::GameDetailManualChooseFile => "先にPDFまたはテキストファイルを選択してください。",
        Key::GameDetailManualInvalidFileType => {
            "アップロードできるマニュアルはPDFまたはテキストファイルのみです。"
        }
        Key::GameDetailManualUploadBrowserOnly => "ここではマニュアルをアップロードできません。",
        Key::GameDetailViewManual => "表示",
        Key::GameDetailNoManualResults => "マニュアルが見つかりません",
        Key::GameDetailManualSaved => "マニュアルを保存しました",
        Key::GameDetailResourceSaved => "リソースを保存しました",
        Key::GameDetailResourceAlreadySaved => "このリソースはすでに保存されています。",
        Key::ManualConfirmDelete => "削除しますか？",
        Key::GameDetailLaunch => "TVで起動",
        Key::GameDetailAlreadyPlaying => "プレイ中",
        Key::GameDetailLaunching => "起動中...",
        Key::GameDetailLaunched => "起動しました！",
        Key::GameDetailLaunchError => "起動に失敗しました",
        Key::GameDetailLaunchNotReplayos => "RePlayOS上で動作していません",
        Key::GameDetailFavorite => "お気に入りに追加",
        Key::GameDetailUnfavorite => "お気に入りから削除",
        Key::GameDetailMoreActions => "その他の操作",
        Key::GameDetailConfirmDelete => "削除の確認",
        Key::GameDetailRegionalVariants => "地域別バリアント",
        Key::GameDetailArcadeVersions => "アーケードバージョン",
        Key::GameDetailTranslations => "翻訳版",
        Key::GameDetailHacks => "ハック版",
        Key::GameDetailSpecialVersions => "特別バージョン",
        Key::GameDetailAlternateVersions => "代替バージョン",
        Key::GameDetailAlsoAvailableOn => "他のシステムでも利用可能",
        Key::GameDetailRecommendations => "おすすめ",
        Key::GameDetailMoreLikeThis => "似たゲーム",
        Key::GameDetailMoreOnBoard => "この基板の他のゲーム",
        Key::GameDetailOtherVersions => "他のバージョン",
        Key::GameDetailMoreInSeries => "同シリーズの作品",
        Key::GameDetailMoreOfSeries => "{0}シリーズの作品",
        Key::GameDetailPlayOrder => "プレイ順",
        Key::GameDetailNotInLibrary => "ライブラリにありません",
        Key::GameDetailNOfM => "{1}中{0}",
        Key::GameDetailChangeCover => "カバーを変更",
        Key::GameDetailChooseBoxart => "ボックスアートを選択",
        Key::GameDetailResetDefault => "デフォルトに戻す",
        Key::GameDetailDownloading => "ダウンロード中...",
        Key::GameDetailNoVariants => "代替カバーが見つかりません",
        Key::GameDetailPublisher => "発売元",
        Key::GameDetailRating => "評価",
        Key::GameDetailGameFaqsLink => "GameFAQs で検索",
        Key::GameDetailShmupsWikiLink => "Shmups Wiki の攻略情報",
        Key::GameDetailShmupsWikiVideoIndexLink => "Shmups Wiki の動画インデックス",

        // Metadata management
        Key::MoreMetadata => "ゲームライブラリ",
        Key::MetadataTitle => "ゲームライブラリ",
        Key::MetadataDataSources => "データソース",
        Key::MetadataDescriptionsRatings => "説明・評価",
        Key::MetadataNoData => "未インポート",
        Key::MetadataEntriesSummary => "件",
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
        Key::MetadataThumbnailPhaseIndexing => "リスト取得中...",
        Key::MetadataThumbnailPhaseDownloading => "ダウンロード中...",
        Key::MetadataThumbnailComplete => "更新完了",
        Key::MetadataThumbnailFailed => "更新失敗",
        Key::MetadataThumbnailCancelled => "更新をキャンセルしました",
        Key::MetadataThumbnailDownloaded => "件ダウンロード済み",
        Key::MetadataThumbnailIndexed => "件インデックス済み",

        // Game library
        Key::MetadataRebuildGameLibrary => "ゲームライブラリを再構築",
        Key::MetadataRebuildingGameLibrary => "再構築中...",
        Key::MetadataConfirmRebuildGameLibrary => {
            "ゲームライブラリを再構築しますか？ディスクからすべてのゲームを再スキャンします。"
        }
        Key::MetadataRescanGameLibrary => "ライブラリを再スキャン",
        Key::MetadataRescanningGameLibrary => "再スキャン中...",
        Key::MetadataRescanGameLibraryHint => {
            "新しく追加された ROM をすぐに検出します。NFS 共有の自動検出には遅延があります。"
        }
        Key::MetadataBannerRebuildingLibrary => "ライブラリを再構築中...",
        Key::MetadataBannerRescanningLibrary => "ライブラリを再スキャン中...",
        Key::MetadataBannerEnrichingLibrary => "ライブラリのメタデータを取得中...",
        Key::MetadataBannerUpdatingMediaStats => "メディア統計を更新中...",
        Key::MetadataProgressVerbRebuilding => "再構築中",
        Key::MetadataProgressVerbRescanning => "再スキャン中",
        Key::MetadataProgressVerbEnriching => "メタデータ取得中",
        Key::MetadataProgressLibraryScanning => "ゲームライブラリをスキャン中",
        Key::MetadataProgressLibraryEnriching => "ゲームライブラリのメタデータを取得中",

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
        Key::MetadataBuiltinArcadeSummary => "件、MAME",
        Key::MetadataBuiltinConsoleSummaryEntries => "件のROMエントリ、",
        Key::MetadataBuiltinConsoleSummarySystems => "システム",
        Key::MetadataBuiltinWikidataEntries => "件のWikidataシリーズエントリ、",
        Key::MetadataBuiltinWikidataSeries => "シリーズ",
        Key::MetadataBuiltinManualLinks => {
            "件のMiSTer Manual Downloader / Retrokit由来のマニュアルリンク"
        }
        Key::MetadataBuiltinGuideLinks => "件のShmups Wikiガイド/動画リンク",
        Key::MetadataBuiltinHint => {
            "名前、ジャンル、開発元、発売元、プレイヤー数、マニュアルリンク、ガイドリンクなどのメタデータはアプリに内蔵されています。インポート不要です。"
        }

        // Library summary cards
        Key::MetadataSummaryTotalGames => "総ゲーム数",
        Key::MetadataSummaryEnrichment => "データ充実度",
        Key::MetadataSummaryCoOp => "協力プレイ",
        Key::MetadataSummaryYearSpan => "年代",
        Key::MetadataSummaryLibrarySize => "ライブラリ容量",
        Key::MetadataSummarySystems => "システム数",
        Key::MetadataSummaryStorage => "ストレージ",
        Key::MetadataSummaryDownloadedArt => "取得済み画像",
        Key::MetadataSummaryPlaytime => "プレイ時間",
        Key::PlaytimeUnavailable => "利用不可",

        // System accordion rows
        Key::MetadataSystemCoverage => "充実度",
        Key::MetadataRowGenre => "ジャンル",
        Key::MetadataRowDeveloper => "開発元",
        Key::MetadataRowPublisher => "発売元",
        Key::MetadataRowReleaseDate => "発売日",
        Key::MetadataRowRating => "評価",
        Key::MetadataRowDescription => "説明",
        Key::MetadataRowBoxArt => "カバー",
        Key::MetadataRowScreenshots => "スクリーンショット",
        Key::MetadataRowTitleScreens => "タイトル画面",
        Key::MetadataRowManuals => "マニュアル",
        Key::MetadataRowVideos => "動画",
        Key::MetadataRowPlaytime => "プレイ時間",
        Key::MetadataRowDownloadedMedia => "取得済みメディア:",
        Key::MetadataRowRegions => "地域:",
        Key::MetadataRowGenreGroups => "ジャンル:",
        Key::MetadataRowPlayers => "プレイヤー:",
        Key::MetadataRowStats => "統計:",
        Key::MetadataStatsRefreshing => "更新中",
        Key::MetadataStatsStale => "古い",
        Key::MetadataStatsFailed => "失敗",
        Key::MetadataRowUnique => "オリジナル",
        Key::MetadataRowClones => "クローン",
        Key::MetadataRowHacks => "改造版",
        Key::MetadataRowTranslations => "翻訳",
        Key::MetadataRowHomebrew => "自作",
        Key::MetadataRowUnlicensed => "非ライセンス",
        Key::MetadataRowSpecial => "特別版",
        Key::MetadataRowVerified => "検証済みゲーム",
        Key::MetadataRowRetroAchievements => "RetroAchievements",
        Key::MetadataRowNoRetroAchievements => "RetroAchievements非対応",
        Key::MetadataRowRetroAchievementsNoCore => "RePlayのエミュレーターでは実績を獲得できません",
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
            "ゲームメタデータはTheGamesDB、No-Intro、libretro-databaseから。説明・評価はLaunchBox提供。ボックスアート・スクリーンショットはlibretro-thumbnailsから。シリーズデータはWikidata（CC0）から。マニュアルリンクはMiSTer Manual DownloaderとRetrokitから取得し、PDFは保存時のみダウンロードされます。データはオフライン利用のためローカルにキャッシュされています。"
        }

        // Logs
        Key::MoreLogs => "システムログ",
        Key::LogsTitle => "システムログ",
        Key::LogsRefresh => "更新",
        Key::LogsCopy => "コピー",
        Key::LogsCopied => "ログをコピーしました",
        Key::LogsSourceAll => "すべてのサービス",
        Key::LogsSourceCompanion => "Replay Control",
        Key::LogsSourceReplay => "RePlayOS UI",
        Key::LogsEmpty => "このソースに利用できるログがありません。",
        Key::LogsReplayUnavailable => {
            "RePlayOSはこのデバイスにログを保存しないため、ここには表示できません。"
        }
        Key::LogsLevelTitle => "Replay Controlのログレベル",
        Key::LogsLevelInfo => "Info",
        Key::LogsLevelDebug => "Debug",
        Key::LogsLevelError => "Error",
        Key::LogsLevelWarn => "警告",
        Key::LogsLevelDisabled => "無効",
        Key::LogsLevelUnknown => "不明",
        Key::LogsLevelRestartHint => {
            "保存するとReplay Controlが再起動して変更が適用され、ページは再接続します。TVで実行中のゲームには影響しません。"
        }
        Key::LogsLevelSaveRestart => "保存して再起動",
        Key::LogsLevelRestarting => "保存しました — 再起動して再接続中…",
        Key::LogsReplayLevelTitle => "RePlayOS UIのログレベル",
        Key::LogsReplayLevelPrefix => "レベル",
        Key::LogsReplayLevelHint => {
            "TVの SYSTEM \u{2192} LOG LEVEL で変更してください。反映には再起動が必要な場合があります。"
        }
        Key::LogsReplayLevelUnavailable => "利用不可",

        // Search
        Key::SearchPlaceholder => "すべてのゲームを検索...",
        Key::SearchNoResults => "結果が見つかりません",
        Key::SearchResultsSummary => "件、",
        Key::SearchSystems => "システムで見つかりました",
        Key::SearchFilterPrefix => "フィルター",
        Key::SearchFilterRemove => "フィルターを解除",
        Key::SearchBrowsingGenre => "すべて閲覧中",
        Key::SearchGamesBy => "開発元のゲーム",
        Key::SearchGamesOn => "基板のゲーム",
        Key::SearchRandomGame => "おまかせゲーム",
        Key::SearchRecentSearches => "最近の検索",
        Key::SearchOtherDevelopers => "一致する他の開発元",
        Key::SearchOtherBoards => "一致する他の基板",

        // Filters
        Key::FilterHideHacks => "ハック版を非表示",
        Key::FilterGenreAll => "すべてのジャンル",
        Key::FilterHideTranslations => "翻訳版を非表示",
        Key::FilterHideBetas => "ベータ版を非表示",
        Key::FilterHideClones => "クローンを非表示",
        Key::FilterMultiplayer => "マルチプレイ",
        Key::FilterCoOp => "協力プレイ",
        Key::FilterHasAchievements => "実績あり",
        Key::FilterRatingAny => "評価を問わない",

        // Developer page
        Key::DeveloperNoGames => "この開発元のゲームが見つかりません",
        Key::DeveloperAllSystems => "すべて",

        // Board page
        Key::BoardNoGames => "この基板のゲームが見つかりません",

        // Metadata busy/scanning banners
        Key::MetadataBusyBanner => {
            "メタデータを更新中です \u{2014} 一部の情報が一時的に表示されない場合があります"
        }
        Key::MetadataBannerFetchingGameMetadata => "ゲームメタデータを取得中...",
        Key::MetadataBannerAlreadyUpToDate => "すでに最新です",

        // Common
        Key::CommonLoading => "読み込み中...",
        Key::CommonLoadingReplayControl => "Replay Control を読み込み中...",
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
        Key::PillBoard => "他の{0}",
        Key::SpotlightCoOp => "協力プレイ",
        Key::SpotlightBoard => "{0}のゲーム",

        // Analytics
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

        // Updates
        Key::MoreSectionUpdates => "アップデート",
        Key::UpdateAvailable => "バージョン {0} が利用可能です",
        Key::UpdateViewRelease => "GitHubで見る",
        Key::UpdateSkip => "このバージョンをスキップ",
        Key::UpdateNow => "今すぐアップデート",
        Key::UpdateCheckButton => "アップデートを確認",
        Key::UpdateChecking => "確認中...",
        Key::UpdateChannelStable => "安定版",
        Key::UpdateChannelBeta => "ベータ版",
        Key::UpdateCurrentVersion => "現在のバージョン: v{0} ({1})",
        Key::UpdateUpToDate => "最新です",
        Key::UpdateDownloading => "アップデートをダウンロード中...",
        Key::UpdateRestarting => "Replay Controlを再起動中...",
        Key::UpdateFailed => "アップデートに失敗しました",
        Key::UpdateDoNotNavigate => "このページを閉じたり移動したりしないでください",
        Key::UpdateReloadingIn => "{0}秒後にリロードします...",
        Key::UpdateWaitingForServer => "サーバーの応答を待っています...",
        Key::UpdateBackToSettings => "設定に戻る",
        Key::UpdatePageTitle => "Replay Controlをアップデート中",

        // Setup checklist (first-run)
        Key::SetupWelcome => "Replay Controlへようこそ！",
        Key::SetupIntro => "ライブラリを充実させ、Replay ControlをRePlayOSに接続します。",
        Key::SetupMetadataTitle => "メタデータソースをダウンロード",
        Key::SetupMetadataHint => "説明、評価、発売日、自動カバーアートを追加します",
        Key::SetupReplayosTitle => "RePlayOS連携を有効化",
        Key::SetupReplayosHint => {
            "RePlayOS Net Controlを有効化し、RePlayOSを再起動してNet Controlコードを読み取るため、操作は不要です"
        }
        Key::SetupPasswordTitle => "デフォルトのデバイスパスワードを変更",
        Key::SetupPasswordHint => {
            "管理者アクセスを保護します。RePlayOSのrootパスワードがまだデフォルトの場合に推奨されます。"
        }
        Key::SetupSkip => "スキップ",
        Key::SetupDismiss => "閉じる",
        Key::SetupComplete => "セットアップ完了！",
        Key::SetupInProgress => "処理中\u{2026}",
        Key::SetupBusyHint => {
            "別のライブラリ処理が実行中の間、セットアップ操作は一時停止されます。完了してから続けてください。"
        }
        Key::SetupStart => "開始",
        Key::SetupDone => "完了",

        Key::SetupReplayosManualLink => "またはコードを手動で入力",

        Key::ReplayOsSettingsTitle => "RePlayOS",
        Key::ReplayOsConnectionTitle => "接続",
        Key::ReplayOsActionsTitle => "操作",
        Key::ReplayOsConnectedHint => "これらの操作には RePlayOS Net Control の接続が必要です。",
        Key::ReplayOsMessageTitle => "画面メッセージ",
        Key::ReplayOsMessagePlaceholder => "TV に表示するメッセージ",
        Key::ReplayOsMessageSend => "メッセージを送信",
        Key::ReplayOsMessageClear => "メッセージを消去",
        Key::ReplayOsRestartGame => "現在のゲームを再起動",
        Key::ReplayOsRestartGameConfirm => "現在のゲームを再起動しますか？",
        Key::ReplayOsDeviceTitle => "デバイス",
        Key::ReplayOsDeviceHint => "これらの操作は RePlayOS デバイス全体に影響します。",
        Key::ReplayOsPowerOff => "電源を切る",
        Key::ReplayOsPowerOffConfirm => "RePlayOS デバイスの電源を切りますか？",
        Key::ReplayOsModeTitle => "モード",
        Key::ReplayOsKioskMode => "キオスクモード",
        Key::ReplayOsKioskHint => "ゲストが TV 側でシステム設定を変更できないようにします。",
        Key::ReplayApiStatusConnected => "{0} に接続済み",
        Key::ReplayApiStatusNotConnected => "未接続",
        Key::ReplayApiStatusRestarting => "RePlayOS を再起動しています…",
        Key::ReplayApiStatusUnauthorized => {
            "Net Control コードが無効になりました — TV側でリセットされた可能性があります"
        }
        Key::ReplayApiStatusUnsupported => {
            "リモート操作には新しい RePlayOS が必要です — TV で RePlayOS を更新してください"
        }
        Key::ReplayApiStatusError => "RePlayOS に接続できません",
        Key::ReplayApiCheckAgain => "再確認",
        Key::ReplayApiAutoTitle => "自動セットアップ",
        Key::ReplayApiAutoButton => "自動で有効にする",
        Key::ReplayApiAutoHint => {
            "Net Control を有効にして RePlayOS を再起動します。TV はメニューに戻り、プレイ中のゲームは終了します。RePlayOS はゲームが保存されているストレージに設定されている必要があります。"
        }
        Key::ReplayApiManualTitle => "手動セットアップ",
        Key::ReplayApiManualStep1 => "TV で SYSTEM > OPTIONS を開き、NET CONTROL を有効にする",
        Key::ReplayApiManualStep2 => "SYSTEM > INFORMATION を開き、NET CONTROL CODE を確認する",
        Key::ReplayApiManualStep3 => "コードをここに入力",
        Key::ReplayApiConnect => "接続",
        Key::ReplayApiConnecting => "接続中…",
        Key::ReplayApiReenterCode => "コードを再入力",
        Key::ReplayApiSetUpAgain => "再設定",

        Key::NowPlayingLabelPlaying => "プレイ中",
        Key::NowPlayingLabelPaused => "プレイ中（一時停止）",
        Key::NowPlayingLabelHalted => "停止中",
        Key::NowPlayingLabelInMenu => "プレイ中（メニュー）",
        Key::NowPlayingDisc => "ディスク {0}/{1}",
        Key::PlayerControlScreenshot => "撮影",
        Key::PlayerControlHalt => "停止",
        Key::PlayerControlMute => "ミュート",
        Key::PlayerControlVolumeDown => "音量 -",
        Key::PlayerControlVolumeUp => "音量 +",
        Key::PlayerControlReset => "リセット",
        Key::PlayerControlResetConfirm => "実行中のゲームをリセットしますか？",
        Key::PlayerControlSaveStates => "ステート保存",
        Key::PlayerControlMore => "その他",
        Key::SaveStatesSlot => "スロット",
        Key::SaveStatesEmpty => "空",
        Key::SaveStatesSaved => "保存済み",
        Key::SaveStatesJustNow => "たった今",
        Key::SaveStatesMinutesAgo => "{0}分前",
        Key::SaveStatesStatusPreview => "状態未取得",
        Key::SaveStatesPreviousSlot => "前のスロット",
        Key::SaveStatesNextSlot => "次のスロット",
        Key::SaveStatesSave => "ステートを保存",
        Key::SaveStatesLoad => "ステートを読み込み",
        Key::SaveStatesOverwriteTitle => "ステートを上書きしますか？",
        Key::SaveStatesOverwriteBody => {
            "{0} にはすでにステートがあります。保存すると上書きされます。"
        }
        Key::SaveStatesLoadTitle => "ステートを読み込みますか？",
        Key::SaveStatesLoadBody => "{0} を読み込むと、現在のゲーム状態が置き換わります。",
    }
}
