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
        Key::HomeDeleteRecent => "Eliminar de recientes",
        Key::HomeDeleteRecentConfirm => "¿Eliminar este juego de los recientes?",
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
        Key::GamesLoadError => "No se pudieron cargar los juegos",
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
        Key::FavoritesRemoveConfirm => "¿Quitar {0} de favoritos?",

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
        Key::OrganizeBoard => "Por placa",
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
        Key::SettingsSectionLibraryGames => "Biblioteca y juegos",
        Key::StandaloneStorageLocation => "Ubicación de almacenamiento",
        Key::StandaloneStorageLocalHint => {
            "Para cambiar el almacenamiento, abre Replay Control en este ordenador usando localhost."
        }
        Key::StorageSetupTitle => "Elige la ubicación de almacenamiento",
        Key::StorageSetupBack => "← Ajustes",
        Key::StorageSetupStep => "CONFIGURACIÓN · PASO 1 DE 2",
        Key::StorageSetupIntro => {
            "Selecciona la carpeta que contiene roms/. Replay Control también usará carpetas del mismo nivel, como bios/, saves/ y captures/, si existen."
        }
        Key::StorageSetupNext => {
            "Después, Replay Control descargará los metadatos de los juegos. La descarga y la base de datos se guardan en /var/lib/replay-control/ de este ordenador; las imágenes de la biblioteca permanecen en la ubicación elegida."
        }
        Key::StorageSetupPathLabel => "Carpeta raíz de almacenamiento en este ordenador",
        Key::StorageSetupBrowse => "Examinar",
        Key::StorageSetupParent => "Carpeta superior",
        Key::StorageSetupSelect => "Usar esta carpeta",
        Key::StorageSetupCurrent => "Almacenamiento actual: {0}",
        Key::StorageSetupNone => "Aún no se ha elegido una ubicación de almacenamiento.",
        Key::StorageSetupUnavailable => "El almacenamiento configurado no está disponible: {0}",
        Key::StorageSetupFound => {
            "Se encontró la carpeta raíz. Replay Control leerá los juegos de {0}/roms/."
        }
        Key::StorageSetupInvalid => "Esta carpeta no contiene un directorio roms/ legible.",
        Key::StorageSetupBrowseFailed => "No se pudieron explorar las carpetas.",
        Key::StorageSetupSwitching => {
            "Cambiando el almacenamiento y comprobando la nueva biblioteca…"
        }
        Key::StorageSetupSwitchTimeout => {
            "El cambio está tardando más de lo esperado. Comprueba replay-control.service en este ordenador y actualiza esta página."
        }
        Key::StorageSetupSwitchFailed => "El cambio de almacenamiento no terminó.",
        Key::StorageSetupCouldNotSwitch => "No se pudo cambiar el almacenamiento.",
        Key::StorageSetupSelectDirectory => "Selecciona un directorio.",
        Key::StorageSetupAlreadySelected => "Esta carpeta ya está seleccionada.",
        Key::StorageSetupInvalidPath => "La ruta contiene un carácter no válido.",
        Key::StorageSetupSwitchBusy => {
            "Ya hay un cambio de almacenamiento en curso. Vuelve a intentarlo cuando termine."
        }
        Key::StorageSetupValidationRequired => {
            "Esta carpeta debe contener un directorio roms/ legible."
        }
        Key::StorageSetupSwitchRestored => {
            "La carpeta seleccionada no pudo iniciar Replay Control. Se restauró la ubicación anterior."
        }
        Key::StorageSetupSwitchInterrupted => {
            "El cambio de almacenamiento se detuvo sin informar del resultado. Reinicia Replay Control para restaurar la ubicación anterior."
        }
        Key::StorageSetupSwitchStartFailed => "No se pudo iniciar el cambio de almacenamiento.",
        Key::StorageSetupPackageOnly => {
            "El cambio de almacenamiento está disponible en instalaciones con paquete."
        }
        Key::SettingsSectionDeviceNetwork => "Red del dispositivo",
        Key::SettingsSectionAccess => "Acceso y seguridad",
        Key::SettingsSectionSystem => "Sistema",
        Key::AccessSecurityTitle => "Acceso y seguridad",
        Key::AccessHttpsTitle => "HTTPS",
        Key::AccessHttpsEnabled => "Acceso local seguro en el puerto 8443",
        Key::AccessCertificateTitle => "Certificado",
        Key::AccessCertificateLocal => "Certificado autofirmado gestionado automáticamente",
        Key::AccessCertificateMode => "Modo",
        Key::AccessCertificateGenerated => "Generado",
        Key::AccessCertificateExpires => "Caduca",
        Key::AccessCertificateFingerprint => "Huella SHA-256",
        Key::AccessCertificateCoveredNames => "Nombres cubiertos",
        Key::AccessCertificateCurrentNames => "Nombres actuales del dispositivo",
        Key::AccessCertificateMissingCoverage => "Cobertura faltante",
        Key::AccessCertificateCovered => "Todos los nombres de acceso actuales están cubiertos",
        Key::AccessCertificateRegenerate => "Regenerar certificado",
        Key::AccessCertificateRegenerateConfirm => {
            "¿Regenerar el certificado HTTPS? Replay Control se reiniciará y esta página se desconectará brevemente; vuelve a conectarte en un momento y acepta de nuevo la excepción de seguridad local."
        }
        Key::AccessCertificateRegenerated => {
            "Certificado regenerado. Replay Control se está reiniciando; vuelve a conectarte en un momento y acepta el nuevo certificado."
        }
        Key::AccessCertificateTrustHint => {
            "Al regenerar el certificado, puede que tengas que aceptar de nuevo la excepción de seguridad local."
        }
        Key::AccessDowngradeUnavailableHint => {
            "Disponible solo cuando inicias sesión con el código de acceso y luego subes a administrador."
        }
        Key::AccessNormalUserTitle => "Acceso de usuario normal",
        Key::AccessNormalUserReplayOs => {
            "Usa el código de emparejamiento de RePlayOS Net Control para iniciar juegos y usar los controles."
        }
        Key::AccessManageReplayOs => "Gestionar conexión de RePlayOS",
        Key::AccessDevicePasswordTitle => "Contraseña del dispositivo",
        Key::AccessDevicePasswordSummary => "Contraseña root para cambios del dispositivo",
        Key::AccessDevicePasswordHint => {
            "Cambia la contraseña root de RePlayOS usada para el acceso de administración del dispositivo."
        }
        Key::AccessAdminTimeoutTitle => "Duración del acceso admin",
        Key::AccessAdminTimeoutHint => {
            "Cuánto tiempo queda desbloqueado el acceso de administrador tras introducir la contraseña del dispositivo."
        }
        Key::AccessAdminTimeoutOneHour => "1 hora",
        Key::AccessAdminTimeoutThreeHours => "3 horas",
        Key::AccessAdminTimeoutTwelveHours => "12 horas",
        Key::LoginTitle => "Iniciar sesión",
        Key::LoginWelcomeTitle => "Bienvenido a Replay Control",
        Key::LoginWelcomeBody => {
            "Inicia sesión aquí. Usa el código de Net Control para acceso normal, o la contraseña del dispositivo para ajustes de administrador."
        }
        Key::LoginUserTitle => "Acceso de usuario normal",
        Key::LoginUserCodeLabel => "Código de Net Control",
        Key::LoginUserCodeHint => {
            "En la TV, activa NET CONTROL en SYSTEM > OPTIONS. Después lee el NET CONTROL CODE en SYSTEM > INFORMATION."
        }
        Key::LoginUserSubmit => "Entrar con código",
        Key::LoginAdminTitle => "Acceso de administrador",
        Key::LoginAdminPasswordLabel => "Contraseña del dispositivo",
        Key::LoginAdminHint => {
            "Usa la contraseña root de RePlayOS. El acceso de administrador es para Wi-Fi, NFS, nombre del host, actualizaciones y cambios del dispositivo."
        }
        Key::LoginAdminSubmit => "Entrar como administrador",
        Key::LoginSuccessUser => "Sesión iniciada con acceso de usuario normal",
        Key::LoginSuccessAdmin => "Sesión iniciada con acceso de administrador",
        Key::LoginDowngrade => "Cambiar a usuario normal",
        Key::LoginDowngradeSuccess => "Cambiado a acceso de usuario normal",
        Key::LoginLogout => "Cerrar sesión",
        Key::LoginLogoutAll => "Cerrar todas las sesiones",
        Key::LoginLogoutAllConfirm => "¿Cerrar todas las sesiones de Replay Control?",
        Key::LoginContinue => "Continuar",
        Key::LoginLanguage => "Idioma",
        Key::LoginCurrentRole => "Acceso actual",
        Key::LoginAdminTimeRemaining => "Tiempo admin restante",
        Key::LoginSessionTimeRemaining => "Tiempo de sesión restante",
        Key::LoginStandaloneOpenTitle => "Modo independiente abierto",
        Key::LoginStandaloneOpenHint => {
            "Esta biblioteca fuera del dispositivo está abierta localmente. El inicio de sesión solo se usa en un dispositivo RePlayOS."
        }
        Key::FirstSetupTitle => "Primera configuración",
        Key::FirstSetupBody => {
            "Configura el acceso aquí. Confirma la contraseña del dispositivo una vez para continuar como administrador."
        }
        Key::FirstSetupPasswordTitle => "Confirmar acceso al dispositivo",
        Key::FirstSetupPasswordHint => {
            "Introduce la contraseña root actual de RePlayOS. En una imagen nueva, la contraseña predeterminada es replayos."
        }
        Key::FirstSetupSubmit => "Continuar como administrador",
        Key::AuthRoleAnonymous => "Sin sesión iniciada",
        Key::AuthRoleOpen => "Acceso abierto",
        Key::AuthRoleUser => "Usuario normal",
        Key::AuthRoleAdmin => "Administrador",

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
        Key::MoreEthernetMac => "MAC Ethernet",
        Key::MoreWifiMac => "MAC Wi-Fi",
        Key::MoreNotConnected => "No conectado",
        Key::MoreModel => "Modelo",
        Key::MoreCpuTemperature => "Temperatura de CPU",
        Key::MoreAvailableRam => "RAM disponible",
        Key::MoreUptime => "Tiempo activo",

        // App language (UI locale selector)
        Key::LocaleTitle => "Idioma de la app",
        Key::LocaleSaved => "Idioma guardado",
        Key::LocaleAuto => "Automático",
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
        Key::ReplayInsiderTitle => "RePlayOS Insider",
        Key::ReplayInsiderActive => "El acceso Insider está activo",
        Key::ReplayInsiderInactive => "El acceso Insider no está activo",
        Key::ReplayInsiderUnknown => "El estado de activación Insider no está disponible",
        Key::ReplayInsiderRequired => {
            "Iniciar este sistema requiere acceso activo a RePlayOS Insider. Puedes seguir consultando sus juegos y metadatos."
        }
        Key::ReplayCapabilitiesChecking => "Comprobando las funciones de RePlayOS…",
        Key::ReplayFeatureRequiresVersion => "Esta función requiere RePlayOS {0} o posterior.",
        Key::ReplayFeatureUnknownVersion => {
            "No se pudo verificar la compatibilidad porque se desconoce la versión de RePlayOS."
        }
        Key::ReplayCapabilitiesUnavailable => {
            "No se pudieron comprobar las funciones de RePlayOS. Revisa la conexión e inténtalo de nuevo."
        }
        Key::ReplayVulkanUnavailable => {
            "Este sistema necesita Vulkan, que RePlayOS actualmente solo admite en Raspberry Pi 5."
        }
        Key::ReplayVulkanRequired => {
            "Este sistema necesita Vulkan. En el televisor, abre Opciones de RePlay → Vídeo → Renderizador preferido y selecciona Vulkan. Después, inténtalo de nuevo."
        }
        Key::ReplayInsiderHint => {
            "Introduce el token Insider proporcionado por RePlayOS para acceder a versiones beta disponibles. El token es de solo escritura y nunca se devuelve al navegador."
        }
        Key::ReplayInsiderToken => "Token Insider",
        Key::ReplayInsiderTokenSaved => "Token Insider guardado",
        Key::ReplayInsiderTokenMissing => "No hay ningún token Insider guardado",
        Key::ReplayInsiderSaveRestart => "Guardar y reiniciar RePlayOS",
        Key::ReplayInsiderClearRestart => "Borrar y reiniciar RePlayOS",
        Key::ReplayInsiderSaved => "Token Insider de RePlayOS actualizado",
        Key::LinkPlayTitle => "Juego enlazado",
        Key::LinkPlayHint => {
            "Juega entre dispositivos RePlay en la misma red con juegos cuyos núcleos admitan Juego enlazado. Configura un dispositivo como anfitrión y los demás como clientes. Los cambios se aplican inmediatamente."
        }
        Key::LinkPlayConfigGuide => "Guía de configuración de Juego enlazado de RePlayOS",
        Key::LinkPlayMode => "Modo",
        Key::LinkPlayDisabled => "Desactivado",
        Key::LinkPlayHost => "Anfitrión",
        Key::LinkPlayClient => "Cliente",
        Key::LinkPlayAddress => "Dirección del anfitrión",
        Key::LinkPlayAddressHint => {
            "Obligatoria en modo cliente. Introduce un nombre de host o una dirección IP."
        }
        Key::LinkPlayPort => "Puerto",
        Key::LinkPlayDiscovery => "Detección LAN",
        Key::LinkPlayDiscoveryHint => {
            "Anuncia los anfitriones activos y permite encontrarlos desde el menú Juego enlazado."
        }
        Key::LinkPlaySave => "Aplicar ajustes de Juego enlazado",

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
        Key::SettingsAdminOnlyDisabled => {
            "Inicia sesión como administrador para cambiar estos ajustes."
        }
        Key::SettingsPasswordEnter => "Introduce la contraseña",

        // Game detail page
        Key::GameDetailInfo => "Información del juego",
        Key::GameDetailSystem => "Sistema",
        Key::GameDetailFilename => "Nombre de archivo",
        Key::GameDetailStorageSize => "Tamaño en disco",
        Key::GameDetailRomCapacity => "Capacidad ROM",
        Key::GameDetailFormat => "Formato",
        Key::GameDetailReleased => "Lanzamiento",
        Key::GameDetailOnePlayer => "1 jugador",
        Key::GameDetailPlayerRange => "1-{0} jugadores",
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
        Key::GameDetailRetroAchievements => "RetroAchievements",
        Key::GameDetailRetroAchievementsNoCore => {
            "El emulador de RePlay para este sistema no admite logros"
        }
        Key::GameDetailGenre => "Género",
        Key::GameDetailDeveloper => "Desarrollador",
        Key::GameDetailPlaytime => "Tiempo de juego",
        Key::GameSettingsTitle => "Ajustes de vídeo y audio",
        Key::GameSettingsHint => "Para este juego y pantalla.",
        Key::GameSettingsGameScope => "Juego",
        Key::GameSettingsSystemScope => "Sistema",
        Key::GameSettingsDefaultNotice => {
            "«Predeterminado» usa el ajuste global de RePlayOS, no el del sistema."
        }
        Key::SystemSettingsHint => "Para este sistema y pantalla.",
        Key::SystemSettingsGameOverrideNotice => {
            "Los ajustes del juego tienen prioridad. Elimínalos para usar los del sistema."
        }
        Key::SystemSettingsApply => "Aplicar",
        Key::SystemSettingsSaved => "Ajustes del sistema guardados",
        Key::GameSettingsInherit => "Predeterminado",
        Key::GameSettingsOff => "Desactivado",
        Key::GameSettingsAmbiscan => "Ambiscan",
        Key::GameSettingsAmbiscanFlat => "Plano",
        Key::GameSettingsAmbiscanRadial => "Radial",
        Key::GameSettingsAmbiscanSmart => "Inteligente",
        Key::GameSettingsIntegerScale => "Escala entera (solo LCD)",
        Key::GameSettingsScaleVertical => "Vertical",
        Key::GameSettingsScaleHorizontal => "Horizontal",
        Key::GameSettingsScaleFull => "Completa",
        Key::GameSettingsScaleFullOverscan => "Overscan completo",
        Key::GameSettingsScaleVerticalOverscan => "Overscan vertical",
        Key::GameSettingsVideoFilter => "Filtro de vídeo (solo LCD)",
        Key::GameSettingsFilterLight => "Ligero",
        Key::GameSettingsFilterMedium => "Medio",
        Key::GameSettingsFilterStrong => "Fuerte",
        Key::GameSettingsFilterBlack => "Negro",
        Key::GameSettingsGamma => "Gamma",
        Key::GameSettingsMonitorX => "Posición horizontal",
        Key::GameSettingsMonitorY => "Posición vertical",
        Key::GameSettingsVolume => "Volumen",
        Key::GameSettingsResetAll => "Restablecer todo",
        Key::RemoveAndRestart => "Eliminar y reiniciar",
        Key::RemoveGameSettings => "Eliminar ajustes del juego",
        Key::RemoveGameSettingsConfirm => {
            "¿Eliminar todos los ajustes de este juego, incluidas las opciones del emulador, y reiniciarlo? Puede perderse el progreso no guardado."
        }
        Key::GameSettingsRemoved => "Ajustes del juego eliminados; reiniciando el juego",
        Key::GameSettingsAlreadyRemoved => "Este juego no tiene ajustes guardados",
        Key::GameSettingsRemovedRefresh => {
            "Ajustes del juego eliminados. Actualiza esta página después de que el juego se reinicie."
        }
        Key::RemoveSystemSettings => "Eliminar ajustes del sistema",
        Key::RemoveSystemSettingsConfirm => {
            "¿Eliminar todos los ajustes de este sistema y pantalla, incluidas las opciones del emulador, y reiniciar el juego? Puede perderse el progreso no guardado."
        }
        Key::SystemSettingsRemoved => "Ajustes del sistema eliminados; reiniciando el juego",
        Key::SystemSettingsAlreadyRemoved => "Este sistema no tiene ajustes guardados",
        Key::SystemSettingsRemovedRefresh => {
            "Ajustes del sistema eliminados. Actualiza esta página después de que el juego se reinicie."
        }
        Key::GameSettingsApply => "Aplicar",
        Key::GameSettingsUnavailable => {
            "Los ajustes no están disponibles. Comprueba la conexión e inténtalo de nuevo."
        }
        Key::GameSettingsChanged => {
            "El juego activo ha cambiado. No se aplicaron los ajustes; se ha actualizado el panel."
        }
        Key::GameSettingsInvalid => {
            "Algunos valores no son válidos. Vuelve a cargar los ajustes e inténtalo de nuevo."
        }
        Key::LinkPlayApplied => "Ajustes de Juego enlazado aplicados",
        Key::LinkPlayInvalidPort => "El puerto debe estar entre 1 y 65535",
        Key::LinkPlayInvalidMode => "Selecciona un modo de Juego enlazado válido",
        Key::LinkPlayInvalidAddress => {
            "Introduce un nombre de host o una dirección IP válidos para el modo cliente"
        }
        Key::ReplaySettingsRestarting => "Ajustes guardados; RePlayOS se está reiniciando…",
        Key::ReplaySettingsFailed => {
            "No se pudo completar la solicitud. Comprueba la conexión y los ajustes guardados antes de reintentar."
        }
        Key::ReplayInsiderInvalid => "Token Insider no válido",
        Key::GameSettingsApplied => "Ajustes del juego aplicados",

        Key::GameDetailEmulation => "Compatibilidad",
        Key::GameDetailRawCategory => "Categoría MAME",
        Key::GameDetailMatureCategory => "Categoría adulta",
        Key::GameDetailRegion => "Región",
        Key::GameDetailDescription => "Descripción",
        Key::GameDetailShowMore => "Ver más",
        Key::GameDetailShowLess => "Ver menos",
        Key::GameDetailScreenshots => "Capturas de pantalla",
        Key::GameDetailTitleScreen => "Pantalla de título",
        Key::GameDetailInGame => "En juego",
        Key::GameDetailVideos => "Vídeos",
        Key::GameDetailRemoveVideo => "Eliminar",
        Key::GameDetailFindTrailers => "Buscar tráilers",
        Key::GameDetailFindGameplay => "Buscar gameplay",
        Key::GameDetailFind1cc => "Buscar 1CC",
        Key::GameDetailFindOnlineVideos => "Buscar en línea",
        Key::GameDetailNoResults => "No se encontraron vídeos",
        Key::GameDetailSearchError => {
            "Búsqueda de vídeo no disponible. Pega las URLs directamente."
        }
        Key::GameDetailPinVideo => "Fijar",
        Key::GameDetailResources => "Recursos",
        Key::GameDetailManualsGuidesAndLinks => "Manuales, guías y enlaces",
        Key::GameDetailSuggestedResources => "Recursos sugeridos",
        Key::GameDetailAddResource => "Añadir o subir un recurso",
        Key::GameDetailAddResourceSubmit => "Añadir",
        Key::GameDetailNoSavedResources => "Aún no hay recursos guardados",
        Key::GameDetailShowAllResources => "Ver todos los recursos",
        Key::GameDetailResourceUrlPlaceholder => "Pega una URL de vídeo, guía, manual o sitio web",
        Key::GameDetailUserCaptures => "Mis capturas",
        Key::GameDetailNoCaptures => {
            "Haz capturas de pantalla durante el juego en tu RePlayOS \u{2014} ¡aparecerán aquí!"
        }
        Key::GameDetailViewAllCaptures => "Ver todas",
        Key::GameDetailDeleteCapture => "Eliminar captura",
        Key::GameDetailDeleteCaptureConfirm => "¿Eliminar esta captura?",
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
        Key::GameDetailManualUploadBrowserOnly => "La subida de manuales no está disponible aquí.",
        Key::GameDetailViewManual => "Ver",
        Key::GameDetailNoManualResults => "No se encontraron manuales",
        Key::GameDetailManualSaved => "Manual guardado",
        Key::GameDetailResourceSaved => "Recurso guardado",
        Key::GameDetailResourceAlreadySaved => "Este recurso ya está guardado.",
        Key::GameDetailRemoveResource => "Quitar recurso",
        Key::GameDetailRemoveResourceConfirm => "¿Quitar este recurso guardado?",
        Key::ManualConfirmDelete => "¿Eliminar?",
        Key::GameDetailLaunch => "Lanzar en TV",
        Key::GameDetailAlreadyPlaying => "Jugando ahora",
        Key::GameDetailLaunching => "Lanzando...",
        Key::GameDetailLaunched => "¡Lanzado!",
        Key::GameDetailLaunchError => "Error al lanzar",
        Key::GameDetailLaunchNotReplayos => "No se está ejecutando en RePlayOS",
        Key::LaunchReplaceConfirm => "¿Iniciar {0} y reemplazar {1}?",
        Key::GameDetailFavorite => "Favorito",
        Key::GameDetailUnfavorite => "Quitar de favoritos",
        Key::GameDetailMoreActions => "Más acciones",
        Key::GameDetailConfirmDeleteSingle => "Esto eliminará permanentemente este juego ({0}).",
        Key::GameDetailConfirmDeleteMultiple => {
            "Esto eliminará permanentemente {0} archivos ({1} en total):\n{2}"
        }
        Key::GameDetailDeleteDirFiles => "{0} archivos",
        Key::GameDetailRegionalVariants => "Variantes regionales",
        Key::GameDetailArcadeVersions => "Versiones arcade",
        Key::GameDetailTranslations => "Traducciones",
        Key::GameDetailHacks => "Hacks",
        Key::GameDetailSpecialVersions => "Versiones especiales",
        Key::GameDetailAlternateVersions => "Versiones alternativas",
        Key::GameDetailAlsoAvailableOn => "También disponible en",
        Key::GameDetailRecommendations => "Recomendaciones",
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
        Key::MoreMetadata => "Biblioteca de juegos",
        Key::MetadataTitle => "Biblioteca de juegos",
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
        Key::MetadataExportCsv => "Exportar metadatos (CSV)",
        Key::MetadataExportCsvAllSystems => "Todos los sistemas",
        Key::MetadataExportCsvDownload => "Descargar",
        Key::MetadataExportCsvPreparing => "Preparando...",
        Key::MetadataExportCsvHint => {
            "Una fila por juego con sus metadatos y lo que falta — para mantener packs de ROMs o fuentes de origen."
        }

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
        Key::MetadataSummaryPlaytime => "Tiempo de juego",
        Key::MetadataSummarySessionPlaytime => "Esta sesión",
        Key::MetadataSummaryWeekPlaytime => "Esta semana",
        Key::MetadataSummaryGamesPlayed => "Juegos jugados",
        Key::MetadataSummaryLongestSession => "Sesión más larga",
        Key::PlaytimeUnavailable => "No disponible",

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
        Key::MetadataRowPlaytime => "Tiempo de juego",
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
        Key::MetadataRowVerified => "Juegos verificados",
        Key::MetadataRowRetroAchievements => "RetroAchievements",
        Key::MetadataRowNoRetroAchievements => "RetroAchievements no compatible",
        Key::MetadataRowRetroAchievementsNoCore => {
            "RetroAchievements no disponibles en el emulador de RePlay"
        }
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
        Key::LogsLevelError => "Error",
        Key::LogsLevelWarn => "Advertencia",
        Key::LogsLevelDisabled => "Desactivado",
        Key::LogsLevelUnknown => "Desconocido",
        Key::LogsLevelRestartHint => {
            "Al guardar se reinicia Replay Control para aplicar el cambio y la página se reconectará. No afecta a un juego en ejecución en la TV."
        }
        Key::LogsLevelSaveRestart => "Guardar y reiniciar",
        Key::LogsLevelRestarting => "Guardado: reiniciando, reconectando…",
        Key::LogsReplayLevelTitle => "Nivel de registro de RePlayOS UI",
        Key::LogsReplayLevelPrefix => "Nivel",
        Key::LogsReplayLevelHint => {
            "Cámbialo en la TV: SYSTEM \u{2192} LOG LEVEL. Puede que necesites reiniciar para que surta efecto."
        }
        Key::LogsReplayLevelUnavailable => "No disponible",

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
        Key::FilterHasAchievements => "Con logros",
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
        Key::CommonYes => "Sí",
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
        Key::UpdateWhatsNew => "Novedades",
        Key::UpdateShowBetas => "Mostrar versiones beta",
        Key::UpdateBetaTag => "Beta",
        Key::UpdateChangelogError => "No se pudo cargar el registro de cambios.",
        Key::UpdateSkipConfirmTitle => "¿Omitir esta versión?",
        Key::UpdateSkipConfirmMessage => {
            "No volverás a recibir avisos sobre la versión {0} hasta que se publique una más nueva."
        }
        Key::UpdateSkip => "Omitir esta versión",
        Key::UpdateNow => "Actualizar ahora",
        Key::UpdateCheckButton => "Buscar actualizaciones",
        Key::UpdateChecking => "Comprobando...",
        Key::UpdateChannelStable => "Estable",
        Key::UpdateChannelBeta => "Beta",
        Key::UpdateCurrentVersion => "Versión actual: v{0} ({1})",
        Key::UpdateUpToDate => "En la última versión",
        Key::UpdateDownloading => "Descargando actualización...",
        Key::UpdateRestarting => "Reiniciando Replay Control...",
        Key::UpdateFailed => "Error en la actualización",
        Key::UpdateDoNotNavigate => "No cierres ni navegues fuera de esta página",
        Key::UpdateReloadingIn => "Recargando en {0} segundos...",
        Key::UpdateWaitingForServer => "Esperando al servidor...",
        Key::UpdateBackToSettings => "Volver a ajustes",
        Key::UpdateComplete => "Actualización completada",
        Key::UpdateContinue => "Continuar",
        Key::UpdatePageTitle => "Actualizando Replay Control",

        // Setup checklist (first-run)
        Key::SetupWelcome => "\u{00a1}Bienvenido a Replay Control!",
        Key::SetupKicker => "CONFIGURACIÓN",
        Key::SetupProgress => "Progreso de la configuración",
        Key::SetupFailed => "Error",
        Key::SetupCancelled => "Cancelado",
        Key::SetupNetNotConfigured => "RePlayOS Net Control no está configurado",
        Key::SetupNetPendingRestart => "RePlayOS todavía se está reiniciando",
        Key::SetupNetUnauthorized => "RePlayOS rechazó el código de Net Control",
        Key::SetupNetUnsupported => "Esta versión de RePlayOS no admite el control remoto",
        Key::SetupIntro => {
            "Saca el m\u{00e1}ximo partido a tu biblioteca y conecta Replay Control con RePlayOS."
        }
        Key::SetupStandaloneIntro => "Termina de configurar tu biblioteca de juegos.",
        Key::SetupStorageTitle => "Elegir la carpeta de juegos",
        Key::SetupStorageHint => "Las ROM y las car\u{00e1}tulas permanecen en la carpeta elegida",
        Key::SetupMetadataTitle => "Descargar fuentes de metadatos",
        Key::SetupMetadataHint => {
            "A\u{00f1}ade descripciones, valoraciones, fechas y car\u{00e1}tulas autom\u{00e1}ticas"
        }
        Key::SetupReplayosTitle => "Activar integraci\u{00f3}n con RePlayOS",
        Key::SetupReplayosHint => {
            "Activa Net Control de RePlayOS, reinicia RePlayOS y lee el c\u{00f3}digo de Net Control para que no tengas que hacer nada."
        }
        Key::SetupPasswordTitle => "Cambiar la contraseña predeterminada del dispositivo",
        Key::SetupPasswordHint => {
            "Mantiene privado el acceso de administrador. Recomendado si la contraseña root de RePlayOS sigue siendo la predeterminada."
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
        Key::ReplayOsDurationSaveFailed => {
            "No se pudo guardar la duración del mensaje. Se ha restaurado el valor anterior."
        }
        Key::ReplayOsRestartGame => "Reiniciar juego actual",
        Key::ReplayOsRestartGameConfirm => "¿Reiniciar el juego actual?",
        Key::ReplayOsDeviceTitle => "Dispositivo",
        Key::ReplayOsDeviceHint => "Estas acciones afectan a todo el dispositivo RePlayOS.",
        Key::ReplayOsPowerOff => "Apagar",
        Key::ReplayOsPowerOffConfirm => "¿Apagar el dispositivo RePlayOS?",
        Key::ReplayOsRebootConfirm => "¿Reiniciar el dispositivo RePlayOS?",
        Key::ReplayOsModeTitle => "Modo",
        Key::ReplayOsDataLocationTitle => "Ubicación de datos",
        Key::ReplayOsDataLocationStorage => "Almacenamiento de ROMs",
        Key::ReplayOsDataLocationLocalSd => "Tarjeta SD local",
        Key::ReplayOsDataLocationHint => {
            "RePlayOS guarda aquí las partidas, los estados y las capturas. Cambia este ajuste en la TV."
        }
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
        Key::SaveStatesNoScreenshot => "Sin captura",
        Key::SaveStatesPreviousSlot => "Ranura anterior",
        Key::SaveStatesNextSlot => "Ranura siguiente",
        Key::SaveStatesSave => "Guardar estado",
        Key::SaveStatesLoad => "Cargar estado",
        Key::SaveStatesOverwriteTitle => "¿Sobrescribir estado?",
        Key::SaveStatesOverwriteBody => "{0} ya tiene un estado. Guardar ahora lo reemplazará.",
        Key::SaveStatesLoadTitle => "¿Cargar estado?",
        Key::SaveStatesLoadBody => "Cargar {0} reemplazará el estado actual del juego.",

        // Status banners
        Key::CorruptionLibraryCorrupt => "La base de datos de la biblioteca está dañada.",
        Key::CorruptionRebuild => "Reconstruir",
        Key::CorruptionUserDataCorrupt => {
            "Los datos del usuario están dañados. Es posible que se pierdan algunos datos."
        }
        Key::CorruptionRestoreBackup => "Restaurar desde copia de seguridad",
        Key::CorruptionReset => "Restablecer",
        Key::CorruptionResetLoseData => "Restablecer (perder datos)",
        Key::AssetHealthCatalogOutOfDate => {
            "Catálogo desactualizado. Reinstala Replay Control para actualizar los metadatos de los juegos."
        }
        Key::RomWatcherStopped => {
            "La detección automática de ROMs no está activa. Usa el reescaneo manual o reinicia Replay Control para detectar ROMs nuevas."
        }
        Key::StorageUnavailable => {
            "El almacenamiento configurado {0} no está disponible.{1} Inserta el dispositivo o cambia la selección de almacenamiento en los ajustes de RePlayOS."
        }
        Key::StorageFallback => " Usando {0} como alternativa.",
        Key::StorageProblem => {
            "Problema de almacenamiento: {0}. Replay Control sigue usando el último almacenamiento activo si hay alguno disponible."
        }
    }
}
