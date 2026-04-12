use super::Key;

pub fn translate(key: Key) -> &'static str {
    match key {
        // App chrome
        Key::AppTitle => "Replay Control",
        Key::NavHome => "Juegos",
        Key::NavGames => "Juegos",
        Key::NavFavorites => "Favs",
        Key::NavSearch => "Buscar",
        Key::NavMore => "Más",
        Key::NavSettings => "Ajustes",

        // Home page
        Key::HomeLastPlayed => "Último jugado",
        Key::HomeRecentlyPlayed => "Jugados recientemente",
        Key::HomeLibrary => "Biblioteca",
        Key::HomeNoGamesPlayed => "Aún no has jugado ningún juego",
        Key::HomeNoRecent => "Sin juegos recientes",
        Key::HomeNoSystems => "No hay sistemas con juegos",
        Key::HomeDiscover => "Descubrir",
        Key::HomeDiscoverRandom => "Redescubre tu biblioteca",
        Key::HomeDiscoverMultiplayer => "Multijugador",
        Key::HomeDiscoverGames => "juegos",

        // Stats
        Key::StatsGames => "Juegos",
        Key::StatsFavorites => "Favoritos",
        Key::StatsUsed => "Usado",
        Key::StatsStorage => "Almacenamiento",
        Key::StatsStorageUsed => "Almacenamiento usado",
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
        Key::MoreTitle => "Más",
        Key::MoreSectionPreferences => "Preferencias",
        Key::MoreSectionGamePreferences => "Preferencias de juego",
        Key::MoreSectionGameData => "Datos de juegos",
        Key::MoreSectionSystem => "Sistema",
        Key::MoreSectionSystemInfo => "Información del sistema",
        Key::MoreUpload => "Subir ROMs",
        Key::MoreWifi => "Configuración Wi-Fi",
        Key::MoreNfs => "Ajustes de recurso compartido NFS",
        Key::MoreSystemInfo => "Información del sistema",
        Key::MoreStorage => "Almacenamiento",
        Key::MorePath => "Ruta",
        Key::MoreDiskTotal => "Total en disco",
        Key::MoreDiskUsed => "Disco usado",
        Key::MoreDiskAvailable => "Disco disponible",
        Key::MoreEthernetIp => "IP Ethernet",
        Key::MoreWifiIp => "IP Wi-Fi",
        Key::MoreNotConnected => "No conectado",
        Key::MoreRefreshStorage => "Actualizar almacenamiento",
        Key::MoreRefreshing => "Actualizando...",
        Key::MoreStorageChanged => "Almacenamiento actualizado",
        Key::MoreStorageUnchanged => "Almacenamiento sin cambios",

        // App language (UI locale selector)
        Key::MoreLocale => "Idioma",
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
        Key::MoreRegion => "Región",
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

        // Skin
        Key::MoreSkin => "Tema visual",
        Key::SkinTitle => "Tema visual",
        Key::SkinCurrent => "Actual",
        Key::SkinHint => "Selecciona un tema para aplicarlo.",
        Key::SkinApplied => "Tema aplicado.",
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
        Key::NfsHint => "Es necesario reiniciar para que los cambios de NFS surtan efecto.",

        // Settings (shared)
        Key::SettingsSave => "Guardar",
        Key::SettingsSaving => "Guardando...",
        Key::SettingsSaved => "Ajustes guardados",
        Key::SettingsApplyHint => "Reinicia ReplayOS para aplicar los cambios.",
        Key::SettingsRestartUi => "Reiniciar ReplayOS",
        Key::SettingsRestarting => "Reiniciando...",
        Key::SettingsReboot => "Reiniciar sistema",
        Key::SettingsRebooting => "Reiniciando...",
        Key::SettingsRebootHint => "Es necesario reiniciar para que los cambios surtan efecto.",
        Key::SettingsPasswordEnter => "Introduce la contraseña",

        // Game detail page
        Key::GameDetailInfo => "Información del juego",
        Key::GameDetailSystem => "Sistema",
        Key::GameDetailFilename => "Nombre de archivo",
        Key::GameDetailFileSize => "Tamaño del archivo",
        Key::GameDetailFormat => "Formato",
        Key::GameDetailArcadeInfo => "Información arcade",
        Key::GameDetailYear => "Año de lanzamiento",
        Key::GameDetailManufacturer => "Fabricante",
        Key::GameDetailPlayers => "Jugadores",
        Key::GameDetailRotation => "Orientación",
        Key::GameDetailCategory => "Categoría",
        Key::GameDetailParentRom => "Versión original",
        Key::GameDetailMetadata => "Metadatos",
        Key::GameDetailGenre => "Género",
        Key::GameDetailDeveloper => "Desarrollador",

        Key::GameDetailEmulation => "Compatibilidad",
        Key::GameDetailRawCategory => "Categoría MAME",
        Key::GameDetailRegion => "Región",
        Key::GameDetailDescription => "Descripción",
        Key::GameDetailNoDescription => "Descripción no disponible",
        Key::GameDetailScreenshots => "Capturas de pantalla",
        Key::GameDetailTitleScreen => "Pantalla de título",
        Key::GameDetailInGame => "En juego",
        Key::GameDetailNoScreenshots => "Sin capturas de pantalla disponibles",
        Key::GameDetailVideos => "Vídeos",
        Key::GameDetailNoVideos => "Sin vídeos disponibles",
        Key::GameDetailMyVideos => "Mis vídeos",
        Key::GameDetailAddVideo => "Añadir",
        Key::GameDetailAddVideoPlaceholder => "Pega una URL de YouTube o Twitch...",
        Key::GameDetailAddVideoError => "URL no válida. Compatible con YouTube, Twitch y Vimeo.",
        Key::GameDetailAddVideoDuplicate => "Este vídeo ya está guardado.",
        Key::GameDetailVideoAdded => "Vídeo añadido",
        Key::GameDetailRemoveVideo => "Eliminar",
        Key::GameDetailFindTrailers => "Buscar tráilers",
        Key::GameDetailFindGameplay => "Buscar gameplay",
        Key::GameDetailFind1cc => "Buscar 1CC",
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
        Key::GameDetailFindManual => "Buscar manual",
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
        Key::GameDetailOtherVersions => "Otras versiones",
        Key::GameDetailMoreInSeries => "Más de esta saga",
        Key::GameDetailPlayOrder => "Orden de juego",
        Key::GameDetailNotInLibrary => "no está en la biblioteca",
        Key::GameDetailNOfM => "{0} de {1}",
        Key::GameDetailChangeCover => "Cambiar portada",
        Key::GameDetailChooseBoxart => "Elegir carátula",
        Key::GameDetailResetDefault => "Restablecer predeterminado",
        Key::GameDetailDownloading => "Descargando...",
        Key::GameDetailNoVariants => "No se encontraron portadas alternativas",
        Key::GameDetailBuildIndexFirst => "Descarga primero el índice de miniaturas",
        Key::GameDetailExternalMetadata => "Información adicional",
        Key::GameDetailPublisher => "Editora",
        Key::GameDetailRating => "Valoración",

        // Metadata management
        Key::MoreMetadata => "Metadatos de juegos",
        Key::MetadataTitle => "Datos de juegos",
        Key::MetadataDataSources => "Fuentes de datos",
        Key::MetadataDescriptionsLaunchbox => "Descripciones y valoraciones (LaunchBox)",
        Key::MetadataNoData => "Aún no importado",
        Key::MetadataEntriesSummary => "entradas",
        Key::MetadataLastUpdated => "última actualización",
        Key::MetadataDownloadingFile => "Descargando metadatos...",
        Key::MetadataMatched => "coincidencias",
        Key::MetadataDataManagement => "Gestión de datos",
        Key::MetadataBuildingIndex => "Construyendo índice de ROMs...",
        Key::MetadataParsingXml => "Procesando XML...",
        Key::MetadataImportComplete => "Importación completada",
        Key::MetadataImportFailed => "Error en la importación",
        Key::MetadataProcessed => "procesados",
        Key::MetadataSystemOverview => "Resumen por sistema",
        Key::MetadataColSystem => "Sistema",
        Key::MetadataColGames => "Juegos",
        Key::MetadataColDesc => "Desc.",
        Key::MetadataColThumb => "Min.",
        Key::MetadataNoSystems => "Aún no hay sistemas con datos.",

        // Thumbnails
        Key::MetadataThumbnailsLibretro => "Miniaturas (libretro)",
        Key::MetadataThumbnailSummary => "carátulas",
        Key::MetadataThumbnailSnaps => "capturas",
        Key::MetadataThumbnailOnDisk => "en disco",
        Key::MetadataThumbnailIndexSummary => "disponibles en",
        Key::MetadataThumbnailSystems => "sistemas",
        Key::MetadataThumbnailStop => "Detener",
        Key::MetadataThumbnailCancelling => "Cancelando...",
        Key::MetadataThumbnailPhaseIndexing => "Obteniendo índice...",
        Key::MetadataThumbnailPhaseDownloading => "Descargando...",
        Key::MetadataThumbnailComplete => "Actualización completada",
        Key::MetadataThumbnailFailed => "Error en la actualización",
        Key::MetadataThumbnailCancelled => "Actualización cancelada",
        Key::MetadataThumbnailDownloaded => "descargadas",
        Key::MetadataThumbnailIndexed => "indexadas",

        // Game library
        Key::MetadataRebuildGameLibrary => "Reconstruir biblioteca de juegos",
        Key::MetadataRebuildingGameLibrary => "Reconstruyendo...",
        Key::MetadataGameLibraryRebuilt => "Biblioteca de juegos reconstruida correctamente",
        Key::MetadataConfirmRebuildGameLibrary => {
            "¿Reconstruir la biblioteca de juegos? Se vuelven a escanear todos los juegos desde el disco."
        }

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
        Key::MetadataBuiltinArcade => "Base de datos arcade",
        Key::MetadataBuiltinArcadeSummary => "entradas, MAME",
        Key::MetadataBuiltinConsole => "Base de datos de consolas",
        Key::MetadataBuiltinConsoleSummaryEntries => "entradas de ROMs en",
        Key::MetadataBuiltinConsoleSummarySystems => "sistemas",
        Key::MetadataBuiltinWikidataEntries => "entradas de series de Wikidata en",
        Key::MetadataBuiltinWikidataSeries => "sagas",
        Key::MetadataBuiltinHint => {
            "Nombres, géneros, desarrolladores, distribuidores, número de jugadores y otros metadatos incluidos en la app. No se necesita importar nada."
        }

        Key::MetadataAttribution => "Atribución",
        Key::MetadataAttributionText => {
            "Metadatos de juegos de TheGamesDB, No-Intro y libretro-database. Descripciones y valoraciones de LaunchBox. Carátulas y capturas de libretro-thumbnails. Datos de series de Wikidata (CC0). Los datos se almacenan localmente para uso sin conexión."
        }

        // Logs
        Key::MoreLogs => "Registros del sistema",
        Key::LogsTitle => "Registros del sistema",
        Key::LogsRefresh => "Actualizar",
        Key::LogsSourceAll => "Todos los servicios",
        Key::LogsSourceCompanion => "Replay Control",
        Key::LogsSourceReplay => "RePlayOS UI",

        // Search
        Key::SearchTitle => "Buscar",
        Key::SearchPlaceholder => "Buscar todos los juegos...",
        Key::SearchNoResults => "No se encontraron resultados",
        Key::SearchNoResultsWithFilters => "Sin resultados. Intenta eliminar algunos filtros.",
        Key::SearchResultsSummary => "resultados en",
        Key::SearchSystems => "sistemas",
        Key::SearchBrowsingGenre => "Explorando todo",
        Key::SearchGamesBy => "Juegos de",
        Key::SearchRandomGame => "Juego aleatorio",
        Key::SearchRecentSearches => "Búsquedas recientes",
        Key::SearchClearRecent => "Borrar",
        Key::SearchOtherDevelopers => "Otros desarrolladores que coinciden con",

        // Filters
        Key::FilterHideHacks => "Ocultar hacks",
        Key::FilterGenre => "Género",
        Key::FilterGenreAll => "Todos los géneros",
        Key::FilterHideTranslations => "Ocultar traducciones",
        Key::FilterHideBetas => "Ocultar betas",
        Key::FilterHideClones => "Ocultar clones",
        Key::FilterMultiplayer => "Multijugador",
        Key::FilterCoOp => "Cooperativo",
        Key::FilterRatingAny => "Cualquier valoración",
        Key::FilterClearFilters => "Borrar filtros",
        Key::FilterActiveSearch => "Buscar",
        Key::FilterFilteredResults => "Resultados filtrados",

        // Developer page
        Key::DeveloperNoGames => "No se encontraron juegos de este desarrollador",
        Key::DeveloperAllSystems => "Todos",

        // Metadata busy/scanning banners
        Key::MetadataBusyBanner => {
            "Actualización de metadatos en curso \u{2014} puede que alguna información no esté disponible temporalmente"
        }
        Key::MetadataScanningBanner => "Escaneando biblioteca de juegos...",

        // Common
        Key::CommonLoading => "Cargando...",
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
        Key::SpotlightCoOp => "Juegos cooperativos",

        // Analytics
        Key::MoreSectionPrivacy => "Privacidad",
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
        Key::AnalyticsPrivacyLink => "Leer la política de privacidad completa",

        // Updates
        Key::MoreSectionUpdates => "Actualizaciones",
        Key::UpdateAvailable => "La versión {0} está disponible",
        Key::UpdateViewRelease => "Ver en GitHub",
        Key::UpdateSkip => "Omitir esta versión",
        Key::UpdateNow => "Actualizar ahora",
        Key::UpdateCheckButton => "Buscar actualizaciones",
        Key::UpdateChecking => "Comprobando...",
        Key::UpdateCheckFailed => "Error al buscar actualizaciones",
        Key::UpdateChannel => "Canal de actualización",
        Key::UpdateChannelStable => "Estable",
        Key::UpdateChannelBeta => "Beta",
        Key::UpdateCurrentVersion => "Versión actual: v{0} ({1})",
        Key::UpdateUpToDate => "Actualizado",
        Key::UpdateDownloading => "Descargando actualización...",
        Key::UpdateInstalling => "Instalando actualización...",
        Key::UpdateRestarting => "Reiniciando Replay Control...",
        Key::UpdateFailed => "Error en la actualización",
        Key::UpdateDoNotNavigate => "No cierres ni navegues fuera de esta página",
        Key::UpdateReloadingIn => "Recargando en {0} segundos...",
        Key::UpdateWaitingForServer => "Esperando al servidor...",
        Key::UpdateBackToSettings => "Volver a ajustes",
        Key::UpdateSystemBusy => {
            "El sistema está ocupado. Espera a que termine la operación actual."
        }
        Key::UpdatePageTitle => "Actualizando Replay Control",
    }
}
