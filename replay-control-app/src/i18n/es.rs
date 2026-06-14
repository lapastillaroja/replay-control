use super::Key;

pub fn translate(key: Key) -> &'static str {
    match key {
        // App chrome
        Key::NavGames => "Juegos",
        Key::NavFavorites => "Favs",
        Key::NavSearch => "Buscar",
        Key::NavSettings => "Ajustes",

        // Home page
        Key::HomeNowPlaying => "Jugando ahora",
        Key::HomeLastPlayed => "Último jugado",
        Key::HomeRecentlyPlayed => "Jugados recientemente",
        Key::HomeLibrary => "Biblioteca",
        Key::HomeNoGamesPlayed => "Aún no has jugado ningún juego",
        Key::HomeNoRecent => "Sin juegos recientes",
        Key::HomeDiscover => "Descubrir",

        // Stats
        Key::StatsGames => "Juegos",
        Key::StatsFavorites => "Favoritos",
        Key::CountGames => "{0} juegos",
        Key::CountGamesPartial => "{0} / {1} juegos",
        Key::CountFavorites => "{0} favoritos",
        Key::CountFavoritesPartial => "{0} / {1} favoritos",

        // Games page
        Key::GamesSearchPlaceholder => "Buscar juegos...",
        Key::GamesBack => "\u{2190} Volver",
        Key::GamesNoGames => "Sin juegos",
        Key::GamesLoadingRoms => "Cargando ROMs...",
        Key::GamesLoadMore => "Cargar más",

        // Favorites page
        Key::FavoritesTitle => "Favoritos",
        Key::FavoritesViewGrouped => "Vista: agrupada",
        Key::FavoritesViewFlat => "Vista: plana",
        Key::FavoritesEmpty => "Aún no tienes favoritos",
        Key::FavoritesLatestAdded => "Últimos añadidos",
        Key::FavoritesRecentlyAdded => "Añadidos recientemente",
        Key::FavoritesBySystem => "Por sistema",
        Key::FavoritesAll => "Todos los favoritos",

        // Organize favorites
        Key::OrganizeTitle => "Organizar favoritos",
        Key::OrganizeDescription => "Crea subcarpetas para organizar tus favoritos",
        Key::OrganizePrimary => "Organizar por",
        Key::OrganizeSecondary => "Luego por (opcional)",
        Key::OrganizeNone => "Ninguno",
        Key::OrganizeSystem => "Por sistema",
        Key::OrganizeGenre => "Por género",
        Key::OrganizePlayers => "Por jugadores",
        Key::OrganizeRating => "Por valoración",
        Key::OrganizeAlphabetical => "Alfabético",
        Key::OrganizeDeveloper => "Por desarrollador",
        Key::OrganizeKeepOriginals => "Mantener copia en raíz",
        Key::OrganizeKeepHint => {
            "Mantiene los archivos originales en la raíz para que la interfaz de RePlayOS siga mostrando todos los favoritos"
        }
        Key::OrganizeApply => "Organizar",
        Key::OrganizeOrganizing => "Organizando...",
        Key::OrganizeFlatten => "Aplanar todo",
        Key::OrganizeFlattening => "Aplanando...",
        Key::OrganizeDone => "organizados",
        Key::OrganizeFlattened => "favoritos movidos a la raíz",
        Key::OrganizeAlreadyFlat => "Todos los favoritos ya están en la raíz",
        Key::OrganizePreview => "Vista previa",
        Key::OrganizePreviewUnknown => "Desconocido",

        // Hostname settings
        Key::MoreHostname => "Nombre del host",
        Key::HostnameTitle => "Nombre del host",
        Key::HostnameLabel => "Nombre del host",
        Key::HostnameHint => {
            "Establece el nombre de red de este sistema RePlayOS. Usa minúsculas, dígitos y guiones (p. ej., replay-salon)."
        }
        Key::HostnameSaved => {
            "Nombre del host actualizado. Puede que sea necesario reiniciar para que mDNS (.local) se actualice completamente."
        }
        Key::HostnameInvalid => "Nombre de host no válido",

        // Password change
        Key::MorePassword => "Cambiar contraseña",
        Key::PasswordTitle => "Cambiar contraseña",
        Key::PasswordCurrent => "Contraseña actual",
        Key::PasswordNew => "Nueva contraseña",
        Key::PasswordConfirm => "Confirmar nueva contraseña",
        Key::PasswordSave => "Cambiar contraseña",
        Key::PasswordSuccess => "Contraseña cambiada correctamente",
        Key::PasswordMismatch => "Las contraseñas no coinciden",
        Key::PasswordWrongCurrent => "La contraseña actual es incorrecta",
        Key::PasswordEmpty => "La contraseña no puede estar vacía",
        Key::PasswordDevSkip => "El cambio de contraseña no está disponible en modo desarrollo",
        Key::PasswordDeployHint => {
            "Tras cambiar la contraseña, usa PI_PASS=tucontraseña al ejecutar dev.sh o install.sh."
        }

        // GitHub API key
        Key::MoreGithub => "Clave de API de GitHub",
        Key::GithubTitle => "Clave de API de GitHub",
        Key::GithubLabel => "Token de acceso personal",
        Key::GithubHint => {
            "Opcional. Aumenta el límite de la API de GitHub de 60 a 5.000 solicitudes/hora para la indexación de miniaturas. Crea un token en github.com/settings/tokens (no se necesitan permisos)."
        }

        // Settings page
        Key::SettingsTitle => "Ajustes",
        Key::SettingsSectionAppearance => "Apariencia",
        Key::SettingsSectionNetwork => "Red y seguridad",
        Key::SettingsSectionSystem => "Sistema",

        // More page (legacy keys)
        Key::MoreSectionGamePreferences => "Preferencias de juego",
        Key::MoreWifi => "Configuración Wi-Fi",
        Key::MoreNfs => "Ajustes de recurso compartido NFS",
        Key::MoreStorage => "Almacenamiento",
        Key::MorePath => "Ruta",
        Key::MoreDiskTotal => "Total en disco",
        Key::MoreDiskUsed => "Disco usado",
        Key::MoreDiskAvailable => "Disco disponible",
        Key::MoreEthernetIp => "IP Ethernet",
        Key::MoreWifiIp => "IP Wi-Fi",
        Key::MoreNotConnected => "No conectado",

        // App language (UI locale selector)
        Key::LocaleTitle => "Idioma de la app",
        Key::LocaleSaved => "Idioma guardado",
        Key::LocaleAuto => "Igual que el navegador",
        Key::LocaleEn => "Inglés - English",
        Key::LocaleEs => "Español",
        Key::LocaleJa => "Japonés - 日本語",

        // Text size
        Key::MoreTextSize => "Tamaño de texto",
        Key::MoreTextSizeHint => "Ajusta el tamaño del texto de la app",

        // Region preference
        Key::RegionTitle => "Preferencias de región",
        Key::RegionHint => {
            "Los juegos de tu región principal aparecen primero. La región secundaria se usa como alternativa cuando la principal no está disponible."
        }
        Key::RegionPrimaryLabel => "Principal",
        Key::RegionSecondaryLabel => "Secundaria",
        Key::RegionUsa => "EE. UU.",
        Key::RegionEurope => "Europa",
        Key::RegionJapan => "Japón",
        Key::RegionWorld => "Mundial",
        Key::RegionSaved => "Preferencia de región guardada",
        Key::RegionNone => "Ninguna (orden predeterminado)",

        // Language preference (for game documents)
        Key::LanguageTitle => "Idioma de documentos",
        Key::LanguageHint => {
            "Idioma preferido para manuales y documentos de juegos. «Auto» lo deduce de tu preferencia de región."
        }
        Key::LanguagePrimaryLabel => "Principal",
        Key::LanguageSecondaryLabel => "Secundario",
        Key::LanguageAuto => "Auto (según región)",
        Key::LanguageEn => "Inglés",
        Key::LanguageEs => "Español",
        Key::LanguageFr => "Francés",
        Key::LanguageDe => "Alemán",
        Key::LanguageIt => "Italiano",
        Key::LanguageJa => "Japonés",
        Key::LanguagePt => "Portugués",
        Key::LanguageSaved => "Preferencia de idioma guardada",

        // RetroAchievements settings
        Key::MoreRetroAchievements => "RetroAchievements",
        Key::RetroAchievementsTitle => "RetroAchievements",
        Key::RetroAchievementsUsername => "Usuario",
        Key::RetroAchievementsPassword => "Contraseña",
        Key::RetroAchievementsPasswordSaved => "Contraseña guardada",
        Key::RetroAchievementsPasswordMissing => "No hay contraseña guardada",
        Key::RetroAchievementsCredentialsRequired => {
            "Introduce usuario y contraseña, o usa Borrar y reiniciar RePlayOS para quitar la cuenta guardada."
        }
        Key::RetroAchievementsSaveRestart => "Guardar y reiniciar RePlayOS",
        Key::RetroAchievementsClearRestart => "Borrar y reiniciar RePlayOS",
        Key::RetroAchievementsSaved => "Credenciales de RetroAchievements actualizadas",

        // Skin
        Key::MoreSkin => "Tema visual",
        Key::SkinTitle => "Tema visual",
        Key::SkinCurrent => "Actual",
        Key::SkinHint => "Selecciona un tema para aplicarlo.",
        Key::SkinSync => "Sincronizar con ReplayOS",
        Key::SkinSyncHint => "Al activarlo, el tema de la app sigue el ajuste de tema de ReplayOS.",

        // WiFi configuration
        Key::WifiTitle => "Configuración Wi-Fi",
        Key::WifiSsid => "Nombre de red (SSID)",
        Key::WifiPassword => "Contraseña",
        Key::WifiCountry => "Código de país",
        Key::WifiMode => "Modo de seguridad",
        Key::WifiHidden => "Red oculta",

        // NFS settings
        Key::NfsTitle => "Ajustes de recurso compartido NFS",
        Key::NfsServer => "Dirección del servidor",
        Key::NfsShare => "Ruta del recurso compartido",
        Key::NfsVersion => "Versión NFS",

        // Settings (shared)
        Key::SettingsSave => "Guardar",
        Key::SettingsSaveRestart => "Guardar y reiniciar RePlayOS",
        Key::SettingsSaving => "Guardando...",
        Key::SettingsSaved => "Ajustes guardados",
        Key::SettingsRestarting => "Reiniciando...",
        Key::SettingsReplayRestartWarning => {
            "Reiniciar RePlayOS detiene cualquier juego en ejecución y desconecta brevemente el dispositivo."
        }
        Key::SettingsReboot => "Reiniciar sistema",
        Key::SettingsRebooting => "Reiniciando...",
        Key::SettingsDeviceOnlyDisabled => "Disponible solo en el dispositivo RePlayOS.",
        Key::SettingsPasswordEnter => "Introduce la contraseña",

        // Game detail page
        Key::GameDetailInfo => "Información del juego",
        Key::GameDetailSystem => "Sistema",
        Key::GameDetailFilename => "Nombre de archivo",
        Key::GameDetailFileSize => "Tamaño del archivo",
        Key::GameDetailFormat => "Formato",
        Key::GameDetailReleased => "Lanzamiento",
        Key::MonthJanShort => "ene",
        Key::MonthFebShort => "feb",
        Key::MonthMarShort => "mar",
        Key::MonthAprShort => "abr",
        Key::MonthMayShort => "may",
        Key::MonthJunShort => "jun",
        Key::MonthJulShort => "jul",
        Key::MonthAugShort => "ago",
        Key::MonthSepShort => "sep",
        Key::MonthOctShort => "oct",
        Key::MonthNovShort => "nov",
        Key::MonthDecShort => "dic",
        Key::GameDetailPlayers => "Jugadores",
        Key::GameDetailRotation => "Orientación",
        Key::GameDetailParentRom => "Versión original",
        Key::GameDetailBoard => "Placa",
        Key::GameDetailGenre => "Género",
        Key::GameDetailDeveloper => "Desarrollador",

        Key::GameDetailEmulation => "Compatibilidad",
        Key::GameDetailRawCategory => "Categoría MAME",
        Key::GameDetailRegion => "Región",
        Key::GameDetailDescription => "Descripción",
        Key::GameDetailScreenshots => "Capturas de pantalla",
        Key::GameDetailTitleScreen => "Pantalla de título",
        Key::GameDetailInGame => "En juego",
        Key::GameDetailVideos => "Vídeos",
        Key::GameDetailNoVideos => "Sin vídeos disponibles",
        Key::GameDetailAddVideo => "Añadir",
        Key::GameDetailAddVideoPlaceholder => "Pega una URL de YouTube o Twitch...",
        Key::GameDetailAddVideoError => "URL no válida. Compatible con YouTube, Twitch y Vimeo.",
        Key::GameDetailAddVideoDuplicate => "Este vídeo ya está guardado.",
        Key::GameDetailVideoAdded => "Vídeo añadido",
        Key::GameDetailRemoveVideo => "Eliminar",
        Key::GameDetailFindTrailers => "Buscar tráilers",
        Key::GameDetailFindGameplay => "Buscar gameplay",
        Key::GameDetailFind1cc => "Buscar 1CC",
        Key::GameDetailSuggestedVideos => "Sugeridos",
        Key::GameDetailAddVideoUrl => "Añadir vídeo",
        Key::GameDetailFindOnlineVideos => "Buscar en línea",
        Key::GameDetailNoResults => "No se encontraron vídeos",
        Key::GameDetailSearchError => {
            "Búsqueda de vídeo no disponible. Pega las URLs directamente."
        }
        Key::GameDetailPinVideo => "Fijar",
        Key::GameDetailPinned => "Fijado",
        Key::GameDetailShowAllVideos => "Ver todos",
        Key::GameDetailUserCaptures => "Mis capturas",
        Key::GameDetailNoCaptures => {
            "Haz capturas de pantalla durante el juego en tu RePlayOS \u{2014} ¡aparecerán aquí!"
        }
        Key::GameDetailViewAllCaptures => "Ver todas",
        Key::GameDetailManual => "Manual",
        Key::GameDetailNoManual => "Manual no disponible",
        Key::GameDetailSuggestedManuals => "Sugeridos",
        Key::GameDetailAddManual => "Añadir manual",
        Key::GameDetailManualUrlPlaceholder => "Pega una URL de manual PDF o texto",
        Key::GameDetailUploadManual => "Subir archivo",
        Key::GameDetailManualChooseFile => "Elige primero un archivo PDF o de texto.",
        Key::GameDetailManualInvalidFileType => {
            "Los manuales subidos deben ser archivos PDF o de texto."
        }
        Key::GameDetailManualUploadBrowserOnly => {
            "La subida de manuales solo está disponible en el navegador."
        }
        Key::GameDetailViewManual => "Ver",
        Key::GameDetailNoManualResults => "No se encontraron manuales",
        Key::GameDetailManualSaved => "Manual guardado",
        Key::ManualConfirmDelete => "¿Eliminar?",
        Key::GameDetailLaunch => "Lanzar en TV",
        Key::GameDetailLaunching => "Lanzando...",
        Key::GameDetailLaunched => "¡Lanzado!",
        Key::GameDetailLaunchError => "Error al lanzar",
        Key::GameDetailLaunchNotReplayos => "No se está ejecutando en RePlayOS",
        Key::GameDetailFavorite => "Favorito",
        Key::GameDetailUnfavorite => "Quitar de favoritos",
        Key::GameDetailConfirmDelete => "Confirmar eliminación",
        Key::GameDetailRegionalVariants => "Variantes regionales",
        Key::GameDetailArcadeVersions => "Versiones arcade",
        Key::GameDetailTranslations => "Traducciones",
        Key::GameDetailHacks => "Hacks",
        Key::GameDetailSpecialVersions => "Versiones especiales",
        Key::GameDetailAlternateVersions => "Versiones alternativas",
        Key::GameDetailAlsoAvailableOn => "También disponible en",
        Key::GameDetailMoreLikeThis => "Más como este",
        Key::GameDetailMoreOnBoard => "Más de esta placa",
        Key::GameDetailOtherVersions => "Otras versiones",
        Key::GameDetailMoreInSeries => "Más de esta saga",
        Key::GameDetailMoreOfSeries => "Más de {0}",
        Key::GameDetailPlayOrder => "Orden de juego",
        Key::GameDetailNotInLibrary => "no está en la biblioteca",
        Key::GameDetailNOfM => "{0} de {1}",
        Key::GameDetailChangeCover => "Cambiar portada",
        Key::GameDetailChooseBoxart => "Elegir carátula",
        Key::GameDetailResetDefault => "Restablecer predeterminado",
        Key::GameDetailDownloading => "Descargando...",
        Key::GameDetailNoVariants => "No se encontraron portadas alternativas",
        Key::GameDetailPublisher => "Editora",
        Key::GameDetailRating => "Valoración",
        Key::GameDetailGameFaqsLink => "Buscar en GameFAQs",
        Key::GameDetailShmupsWikiLink => "Guía en Shmups Wiki",
        Key::GameDetailShmupsWikiVideoIndexLink => "Índice de vídeos en Shmups Wiki",

        // Metadata management
        Key::MoreMetadata => "Metadatos de juegos",
        Key::MetadataTitle => "Datos de juegos",
        Key::MetadataDataSources => "Fuentes de datos",
        Key::MetadataDescriptionsRatings => "Descripciones y valoraciones",
        Key::MetadataNoData => "Aún no importado",
        Key::MetadataEntriesSummary => "entradas",
        Key::MetadataDownloadingFile => "Descargando metadatos...",
        Key::MetadataMatched => "coincidencias",
        Key::MetadataDataManagement => "Gestión de datos",
        Key::MetadataBuildingIndex => "Construyendo índice de ROMs...",
        Key::MetadataParsingXml => "Procesando XML...",
        Key::MetadataImportComplete => "Importación completada",
        Key::MetadataImportFailed => "Error en la importación",
        Key::MetadataProcessed => "procesados",
        Key::MetadataSystemOverview => "Resumen por sistema",

        // Thumbnails
        Key::MetadataThumbnailsLibretro => "Miniaturas (libretro)",
        Key::MetadataThumbnailSummary => "carátulas",
        Key::MetadataThumbnailSnaps => "capturas",
        Key::MetadataThumbnailOnDisk => "en disco",
        Key::MetadataThumbnailIndexSummary => "disponibles en",
        Key::MetadataThumbnailSystems => "sistemas",
        Key::MetadataThumbnailStop => "Detener",
        Key::MetadataThumbnailCancelling => "Cancelando...",
        Key::MetadataThumbnailPhaseIndexing => "Obteniendo lista...",
        Key::MetadataThumbnailPhaseDownloading => "Descargando...",
        Key::MetadataThumbnailComplete => "Actualización completada",
        Key::MetadataThumbnailFailed => "Error en la actualización",
        Key::MetadataThumbnailCancelled => "Actualización cancelada",
        Key::MetadataThumbnailDownloaded => "descargadas",
        Key::MetadataThumbnailIndexed => "indexadas",

        // Game library
        Key::MetadataRebuildGameLibrary => "Reconstruir biblioteca de juegos",
        Key::MetadataRebuildingGameLibrary => "Reconstruyendo...",
        Key::MetadataConfirmRebuildGameLibrary => {
            "¿Reconstruir la biblioteca de juegos? Se vuelven a escanear todos los juegos desde el disco."
        }
        Key::MetadataRescanGameLibrary => "Reescanear biblioteca",
        Key::MetadataRescanningGameLibrary => "Reescaneando...",
        Key::MetadataRescanGameLibraryHint => {
            "Detecta ROMs añadidas recientemente sin esperar. La detección automática en NFS tiene cierto retraso."
        }
        Key::MetadataBannerRebuildingLibrary => "Reconstruyendo biblioteca...",
        Key::MetadataBannerRescanningLibrary => "Reescaneando biblioteca...",
        Key::MetadataBannerEnrichingLibrary => "Enriqueciendo biblioteca...",
        Key::MetadataBannerUpdatingMediaStats => "Actualizando estadísticas multimedia...",
        Key::MetadataProgressVerbRebuilding => "Reconstruyendo",
        Key::MetadataProgressVerbRescanning => "Reescaneando",
        Key::MetadataProgressVerbEnriching => "Enriqueciendo",
        Key::MetadataProgressLibraryScanning => "Escaneando biblioteca de juegos",
        Key::MetadataProgressLibraryEnriching => "Enriqueciendo biblioteca de juegos",

        // Advanced data management
        Key::MetadataAdvancedActions => "Avanzado",

        // Data management
        Key::MetadataClearImages => "Borrar imágenes descargadas",
        Key::MetadataClearedImages => "Imágenes borradas",
        Key::MetadataConfirmClearImages => "¿Eliminar todas las carátulas y capturas descargadas?",
        Key::MetadataCleanupOrphans => "Limpiar imágenes huérfanas",
        Key::MetadataCleaningOrphans => "Limpiando...",
        Key::MetadataConfirmCleanupOrphans => {
            "¿Eliminar imágenes y metadatos de ROMs que ya no existen?"
        }
        Key::MetadataClearIndex => "Borrar índice de miniaturas",
        Key::MetadataIndexCleared => "Índice de miniaturas borrado",
        Key::MetadataConfirmClearIndex => {
            "¿Eliminar el índice de miniaturas? Se puede reconstruir pulsando Actualizar."
        }
        Key::MetadataClearMetadata => "Borrar metadatos",
        Key::MetadataMetadataCleared => "Metadatos borrados",
        Key::MetadataConfirmClearMetadata => {
            "¿Eliminar todas las descripciones y valoraciones de juegos?"
        }

        // Built-in metadata
        Key::MetadataBuiltin => "Datos de juegos integrados",
        Key::MetadataBuiltinArcadeSummary => "entradas, MAME",
        Key::MetadataBuiltinConsoleSummaryEntries => "entradas de ROMs en",
        Key::MetadataBuiltinConsoleSummarySystems => "sistemas",
        Key::MetadataBuiltinWikidataEntries => "entradas de series de Wikidata en",
        Key::MetadataBuiltinWikidataSeries => "sagas",
        Key::MetadataBuiltinManualLinks => {
            "enlaces a manuales de MiSTer Manual Downloader y Retrokit"
        }
        Key::MetadataBuiltinGuideLinks => "enlaces de guías/videos de Shmups Wiki",
        Key::MetadataBuiltinHint => {
            "Nombres, géneros, desarrolladores, distribuidores, número de jugadores, enlaces a manuales, enlaces a guías y otros metadatos incluidos en la app. No se necesita importar nada."
        }

        // Library summary cards
        Key::MetadataSummaryTotalGames => "Total de juegos",
        Key::MetadataSummaryEnrichment => "Enriquecimiento",
        Key::MetadataSummaryCoOp => "Juegos coop.",
        Key::MetadataSummaryYearSpan => "Rango de años",
        Key::MetadataSummaryLibrarySize => "Tamaño",
        Key::MetadataSummarySystems => "Sistemas",
        Key::MetadataSummaryStorage => "Almacenamiento",
        Key::MetadataSummaryDownloadedArt => "Imágenes descargadas",

        // System accordion rows
        Key::MetadataSystemCoverage => "cobertura",
        Key::MetadataRowGenre => "Género",
        Key::MetadataRowDeveloper => "Desarrollador",
        Key::MetadataRowPublisher => "Distribuidor",
        Key::MetadataRowReleaseDate => "Fecha",
        Key::MetadataRowRating => "Valoración",
        Key::MetadataRowDescription => "Descripción",
        Key::MetadataRowBoxArt => "Carátula",
        Key::MetadataRowScreenshots => "Capturas",
        Key::MetadataRowTitleScreens => "Pantallas de título",
        Key::MetadataRowManuals => "Manuales",
        Key::MetadataRowVideos => "Vídeos",
        Key::MetadataRowDownloadedMedia => "Medios descargados:",
        Key::MetadataRowRegions => "Regiones:",
        Key::MetadataRowGenreGroups => "Géneros:",
        Key::MetadataRowPlayers => "Jugadores:",
        Key::MetadataRowStats => "Estadísticas:",
        Key::MetadataStatsRefreshing => "actualizando",
        Key::MetadataStatsStale => "desactualizadas",
        Key::MetadataStatsFailed => "error",
        Key::MetadataRowUnique => "únicos",
        Key::MetadataRowClones => "clones",
        Key::MetadataRowHacks => "hacks",
        Key::MetadataRowTranslations => "trad.",
        Key::MetadataRowHomebrew => "homebrew",
        Key::MetadataRowUnlicensed => "sin licencia",
        Key::MetadataRowSpecial => "especiales",
        Key::MetadataRowVerified => "verificados",
        Key::MetadataRowCoOp => "coop.",
        Key::MetadataRowDrivers => "Controladores:",
        Key::MetadataDriverWorking => "funcionan",
        Key::MetadataDriverImperfect => "imperfectos",
        Key::MetadataDriverPreliminary => "preliminares",
        Key::MetadataDriverUnknown => "desconocidos",
        Key::MetadataExpandAll => "Expandir todo",
        Key::MetadataCollapseAll => "Contraer todo",

        Key::MetadataAttribution => "Atribución",
        Key::MetadataAttributionText => {
            "Metadatos de juegos de TheGamesDB, No-Intro y libretro-database. Descripciones y valoraciones de LaunchBox. Carátulas y capturas de libretro-thumbnails. Datos de series de Wikidata (CC0). Enlaces a manuales de MiSTer Manual Downloader y Retrokit; los PDF solo se descargan al guardarlos. Los datos se almacenan localmente para uso sin conexión."
        }

        // Logs
        Key::MoreLogs => "Registros del sistema",
        Key::LogsTitle => "Registros del sistema",
        Key::LogsRefresh => "Actualizar",
        Key::LogsCopy => "Copiar",
        Key::LogsCopied => "Registros copiados",
        Key::LogsSourceAll => "Todos los servicios",
        Key::LogsSourceCompanion => "Replay Control",
        Key::LogsSourceReplay => "RePlayOS UI",
        Key::LogsEmpty => "No hay registros disponibles para esta fuente.",
        Key::LogsReplayUnavailable => {
            "RePlayOS no guarda sus registros en este dispositivo, por lo que no se pueden mostrar aquí."
        }
        Key::LogsLevelTitle => "Nivel de registro de Replay Control",
        Key::LogsLevelInfo => "Info",
        Key::LogsLevelDebug => "Debug",
        Key::LogsLevelRebootHint => {
            "Los cambios guardados se aplican después de reiniciar el sistema."
        }

        // Search
        Key::SearchPlaceholder => "Buscar todos los juegos...",
        Key::SearchNoResults => "No se encontraron resultados",
        Key::SearchResultsSummary => "resultados en",
        Key::SearchSystems => "sistemas",
        Key::SearchFilterPrefix => "Filtro",
        Key::SearchFilterRemove => "Quitar filtro",
        Key::SearchBrowsingGenre => "Explorando todo",
        Key::SearchGamesBy => "Juegos de",
        Key::SearchGamesOn => "Juegos en",
        Key::SearchRandomGame => "Juego aleatorio",
        Key::SearchRecentSearches => "Búsquedas recientes",
        Key::SearchOtherDevelopers => "Otros desarrolladores que coinciden con",
        Key::SearchOtherBoards => "Otras placas arcade que coinciden con",

        // Filters
        Key::FilterHideHacks => "Ocultar hacks",
        Key::FilterGenreAll => "Todos los géneros",
        Key::FilterHideTranslations => "Ocultar traducciones",
        Key::FilterHideBetas => "Ocultar betas",
        Key::FilterHideClones => "Ocultar clones",
        Key::FilterMultiplayer => "Multijugador",
        Key::FilterCoOp => "Cooperativo",
        Key::FilterRatingAny => "Cualquier valoración",

        // Developer page
        Key::DeveloperNoGames => "No se encontraron juegos de este desarrollador",
        Key::DeveloperAllSystems => "Todos",

        // Board page
        Key::BoardNoGames => "No se encontraron juegos en esta placa arcade",

        // Metadata busy/scanning banners
        Key::MetadataBusyBanner => {
            "Actualización de metadatos en curso \u{2014} puede que alguna información no esté disponible temporalmente"
        }
        Key::MetadataBannerFetchingGameMetadata => "Obteniendo metadatos de juegos...",
        Key::MetadataBannerAlreadyUpToDate => "Ya está actualizado",

        // Common
        Key::CommonLoading => "Cargando...",
        Key::CommonLoadingReplayControl => "Cargando Replay Control...",
        Key::CommonError => "Error",
        Key::CommonSeeAll => "Ver todo",
        Key::CommonSystems => "Sistemas",
        Key::CommonClearing => "Borrando...",
        Key::CommonUpdating => "Actualizando...",
        Key::CommonUpdate => "Actualizar",
        Key::CommonSearching => "Buscando...",
        Key::CommonCancel => "Cancelar",
        Key::CommonDelete => "Eliminar",
        Key::CommonRename => "Renombrar",
        Key::CommonActions => "Acciones",
        Key::CommonSave => "Guardar",

        // Recommendation section / discover pill titles
        Key::SpotlightBestGenre => "Mejor {0}",
        Key::SpotlightBestOf => "Lo mejor de {0}",
        Key::SpotlightGamesBy => "Juegos de {0}",
        Key::SpotlightHiddenGems => "Joyas ocultas",
        Key::SpotlightTopRated => "Mejor valorados",
        Key::SpotlightRediscover => "Redescubre tu biblioteca",
        Key::SpotlightBecauseYouLove => "Si te gusta {0}",
        Key::SpotlightMoreFrom => "Más de {0}",
        Key::PillClassics => "Clásicos de los {0}",
        Key::PillBestOf => "Lo mejor de {0}",
        Key::PillGamesBy => "Juegos de {0}",
        Key::PillMultiplayer => "Multijugador",
        Key::PillCoOp => "Juegos cooperativos",
        Key::PillBoard => "Más {0}",
        Key::SpotlightCoOp => "Juegos cooperativos",
        Key::SpotlightBoard => "Juegos en {0}",

        // Analytics
        Key::AnalyticsTitle => "Estadísticas de uso anónimas",
        Key::AnalyticsDescription => {
            "Ayuda a mejorar Replay Control enviando estadísticas de instalación anónimas"
        }
        Key::AnalyticsSaved => "Preferencia guardada",
        Key::AnalyticsWhatSent => "¿Qué datos se envían?",
        Key::AnalyticsFieldInstallId => {
            "ID de instalación aleatorio (no vinculado a ti ni a tu dispositivo)"
        }
        Key::AnalyticsFieldVersion => "Versión de la aplicación",
        Key::AnalyticsFieldArch => "Arquitectura de CPU",
        Key::AnalyticsFieldChannel => "Canal de actualización",
        Key::AnalyticsNotCollected => {
            "No se recopila: direcciones IP, biblioteca de juegos, patrones de uso ni información personal."
        }

        // Updates
        Key::MoreSectionUpdates => "Actualizaciones",
        Key::UpdateAvailable => "La versión {0} está disponible",
        Key::UpdateViewRelease => "Ver en GitHub",
        Key::UpdateSkip => "Omitir esta versión",
        Key::UpdateNow => "Actualizar ahora",
        Key::UpdateCheckButton => "Buscar actualizaciones",
        Key::UpdateChecking => "Comprobando...",
        Key::UpdateChannelStable => "Estable",
        Key::UpdateChannelBeta => "Beta",
        Key::UpdateCurrentVersion => "Versión actual: v{0} ({1})",
        Key::UpdateUpToDate => "Actualizado",
        Key::UpdateDownloading => "Descargando actualización...",
        Key::UpdateRestarting => "Reiniciando Replay Control...",
        Key::UpdateFailed => "Error en la actualización",
        Key::UpdateDoNotNavigate => "No cierres ni navegues fuera de esta página",
        Key::UpdateReloadingIn => "Recargando en {0} segundos...",
        Key::UpdateWaitingForServer => "Esperando al servidor...",
        Key::UpdateBackToSettings => "Volver a ajustes",
        Key::UpdatePageTitle => "Actualizando Replay Control",

        // Setup checklist (first-run)
        Key::SetupWelcome => "\u{00a1}Bienvenido a Replay Control!",
        Key::SetupIntro => {
            "Saca el m\u{00e1}ximo partido a tu biblioteca y conecta Replay Control con RePlayOS."
        }
        Key::SetupMetadataTitle => "Descargar fuentes de metadatos",
        Key::SetupMetadataHint => {
            "A\u{00f1}ade descripciones, valoraciones, fechas y car\u{00e1}tulas autom\u{00e1}ticas"
        }
        Key::SetupReplayosTitle => "Activar integraci\u{00f3}n con RePlayOS",
        Key::SetupReplayosHint => {
            "Activa Net Control de RePlayOS, reinicia RePlayOS y lee el c\u{00f3}digo de Net Control para que no tengas que hacer nada."
        }
        Key::SetupSkip => "Omitir",
        Key::SetupDismiss => "Cerrar",
        Key::SetupComplete => "\u{00a1}Configuraci\u{00f3}n completada!",
        Key::SetupInProgress => "En progreso\u{2026}",
        Key::SetupBusyHint => {
            "Las acciones de configuraci\u{00f3}n se pausan mientras se ejecuta otra tarea de biblioteca. Espera a que termine y contin\u{00fa}a."
        }
        Key::SetupStart => "Iniciar",
        Key::SetupDone => "Listo",

        Key::SetupReplayosManualLink => "o introduce el código manualmente",

        Key::ReplayOsSettingsTitle => "RePlayOS",
        Key::ReplayOsConnectionTitle => "Conexión",
        Key::ReplayOsActionsTitle => "Acciones",
        Key::ReplayOsConnectedHint => {
            "Estos controles requieren que RePlayOS Net Control esté conectado."
        }
        Key::ReplayOsMessageTitle => "Mensaje en pantalla",
        Key::ReplayOsMessagePlaceholder => "Mensaje que se muestra en la TV",
        Key::ReplayOsMessageSend => "Enviar mensaje",
        Key::ReplayOsMessageClear => "Borrar mensaje",
        Key::ReplayOsRestartGame => "Reiniciar juego actual",
        Key::ReplayOsRestartGameConfirm => "¿Reiniciar el juego actual?",
        Key::ReplayOsDeviceTitle => "Dispositivo",
        Key::ReplayOsDeviceHint => "Estas acciones afectan a todo el dispositivo RePlayOS.",
        Key::ReplayOsPowerOff => "Apagar",
        Key::ReplayOsPowerOffConfirm => "¿Apagar el dispositivo RePlayOS?",
        Key::ReplayOsModeTitle => "Modo",
        Key::ReplayOsKioskMode => "Modo kiosco",
        Key::ReplayOsKioskHint => {
            "Limita la interfaz de la TV para que los invitados no cambien ajustes del sistema."
        }
        Key::ReplayApiStatusConnected => "Conectado a {0}",
        Key::ReplayApiStatusNotConnected => "No conectado",
        Key::ReplayApiStatusRestarting => "RePlayOS se está reiniciando…",
        Key::ReplayApiStatusUnauthorized => {
            "El código de Net Control dejó de funcionar — puede que se haya restablecido en la TV"
        }
        Key::ReplayApiStatusUnsupported => {
            "El control remoto requiere una versión más reciente de RePlayOS — actualiza RePlayOS en la TV"
        }
        Key::ReplayApiStatusError => "No se pudo conectar con RePlayOS",
        Key::ReplayApiCheckAgain => "Comprobar de nuevo",
        Key::ReplayApiAutoTitle => "Configuración automática",
        Key::ReplayApiAutoButton => "Activar automáticamente",
        Key::ReplayApiAutoHint => {
            "Activa Net Control y reinicia RePlayOS: la TV vuelve al menú y cualquier partida en curso se detiene. RePlayOS debe estar configurado con el almacenamiento donde están tus juegos."
        }
        Key::ReplayApiManualTitle => "Configuración manual",
        Key::ReplayApiManualStep1 => "En la TV, abre SYSTEM > OPTIONS y activa NET CONTROL",
        Key::ReplayApiManualStep2 => "Abre SYSTEM > INFORMATION y consulta el NET CONTROL CODE",
        Key::ReplayApiManualStep3 => "Introduce el código aquí",
        Key::ReplayApiConnect => "Conectar",
        Key::ReplayApiConnecting => "Conectando…",
        Key::ReplayApiReenterCode => "Volver a introducir el código",
        Key::ReplayApiSetUpAgain => "Configurar de nuevo",

        Key::NowPlayingLabelPlaying => "Jugando",
        Key::NowPlayingLabelPaused => "Pausado",
        Key::NowPlayingLabelHalted => "Detenido",
        Key::NowPlayingLabelInMenu => "Jugando (en el menú)",
        Key::NowPlayingDisc => "Disco {0}/{1}",
        Key::PlayerControlScreenshot => "Captura",
        Key::PlayerControlHalt => "Halt",
        Key::PlayerControlMute => "Silenciar",
        Key::PlayerControlVolumeDown => "Vol -",
        Key::PlayerControlVolumeUp => "Vol +",
        Key::PlayerControlReset => "Reset",
        Key::PlayerControlResetConfirm => "¿Reiniciar el juego en curso?",
        Key::PlayerControlSaveStates => "Estados",
        Key::PlayerControlMore => "Más",
        Key::SaveStatesSlot => "Slot",
        Key::SaveStatesEmpty => "Vacío",
        Key::SaveStatesSaved => "Guardado",
        Key::SaveStatesJustNow => "ahora",
        Key::SaveStatesMinutesAgo => "hace {0} min",
        Key::SaveStatesStatusPreview => "estado no cargado",
        Key::SaveStatesPreviousSlot => "Ranura anterior",
        Key::SaveStatesNextSlot => "Ranura siguiente",
        Key::SaveStatesSave => "Guardar estado",
        Key::SaveStatesLoad => "Cargar estado",
        Key::SaveStatesOverwriteTitle => "¿Sobrescribir estado?",
        Key::SaveStatesOverwriteBody => "{0} ya tiene un estado. Guardar ahora lo reemplazará.",
        Key::SaveStatesLoadTitle => "¿Cargar estado?",
        Key::SaveStatesLoadBody => "Cargar {0} reemplazará el estado actual del juego.",
    }
}
