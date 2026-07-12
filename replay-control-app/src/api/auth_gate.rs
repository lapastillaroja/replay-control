//! Session-auth gating for the HTTP surface: the `with_auth_guard`
//! middleware, CSRF origin checks, login/access redirects, and the
//! role classification for routes and server functions.
//!
//! Classification is centralized here (allowlists per role) — see the
//! exhaustive-inventory tests at the bottom, which fail whenever a new
//! server function is added without an explicit role decision.

use replay_control_core::auth::{AuthRole, valid_session_cookie_value};

use super::AppState;

pub fn is_public_without_auth(path: &str) -> bool {
    path == "/login"
        || path == "/waiting"
        || path == "/api/version"
        // Read-only library-state signal for the libretro core + health checks +
        // the E2E harness.
        || path == "/api/core/status"
        || path.starts_with("/static/")
}

pub fn with_auth_guard(app: axum::Router, app_state: AppState) -> axum::Router {
    use axum::http::{Request, StatusCode};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect};

    app.layer(axum::middleware::from_fn(
        move |request: Request<axum::body::Body>, next: Next| {
            let state = app_state.clone();
            async move {
                if !state.mode.is_device() {
                    return next.run(request).await;
                }
                let path = request.uri().path().to_string();
                // Static assets and health/version need no session. Short-circuit
                // them BEFORE resolving the session, because resolving an admin
                // cookie reads /etc/shadow (admin credential fingerprint) + the
                // signing key from disk — doing that per static asset on every
                // page load is needless blocking I/O on the hot path. These are
                // all GET, so the CSRF check below doesn't apply to them.
                if path.starts_with("/static/") || path == "/api/version" || path == "/waiting" {
                    return next.run(request).await;
                }
                let role = request_auth_role(&state, &request);
                if is_unsafe_method(request.method())
                    && role != AuthRole::Anonymous
                    && !passes_csrf_origin_check(request.headers())
                {
                    return StatusCode::FORBIDDEN.into_response();
                }

                if !state.first_setup_done() {
                    if is_public_during_first_setup(&path) {
                        return next.run(request).await;
                    }
                    if request.method() == axum::http::Method::GET
                        && wants_html_response(request.headers())
                    {
                        return Redirect::temporary("/first-setup").into_response();
                    }
                    return StatusCode::UNAUTHORIZED.into_response();
                }

                if path == "/login"
                    && request.method() == axum::http::Method::GET
                    && role != AuthRole::Anonymous
                {
                    return Redirect::temporary("/").into_response();
                }

                if is_public_without_auth(&path) {
                    return next.run(request).await;
                }

                if let Some(required_role) = server_function_required_role(&path) {
                    if role.allows(required_role) {
                        return next.run(request).await;
                    }
                    return if role == AuthRole::Anonymous {
                        StatusCode::UNAUTHORIZED.into_response()
                    } else {
                        StatusCode::FORBIDDEN.into_response()
                    };
                }

                let required_role = route_required_role(request.method(), &path);
                if role.allows(required_role) {
                    return next.run(request).await;
                }

                let wants_html = wants_html_response(request.headers());
                if request.method() == axum::http::Method::GET && wants_html {
                    let target = if role == AuthRole::Anonymous {
                        login_redirect_target(request.uri())
                    } else {
                        access_redirect_target(request.uri())
                    };
                    return Redirect::temporary(&target).into_response();
                }

                if role != AuthRole::Anonymous {
                    StatusCode::FORBIDDEN.into_response()
                } else {
                    StatusCode::UNAUTHORIZED.into_response()
                }
            }
        },
    ))
}

fn wants_html_response(headers: &axum::http::HeaderMap) -> bool {
    use axum::http::header::ACCEPT;

    headers
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| value.contains("text/html"))
}

fn login_redirect_target(uri: &axum::http::Uri) -> String {
    let next = uri
        .path_and_query()
        .map(|path| path.as_str())
        .unwrap_or("/");
    let next = if next.starts_with("/login") {
        "/"
    } else {
        next
    };
    format!("/login?next={}", urlencoding::encode(next))
}

fn access_redirect_target(uri: &axum::http::Uri) -> String {
    let next = uri
        .path_and_query()
        .map(|path| path.as_str())
        .unwrap_or("/");
    let next = if next.starts_with("/settings/access") {
        "/settings"
    } else {
        next
    };
    format!("/settings/access?next={}", urlencoding::encode(next))
}

fn is_unsafe_method(method: &axum::http::Method) -> bool {
    use axum::http::Method;

    !matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

fn passes_csrf_origin_check(headers: &axum::http::HeaderMap) -> bool {
    use axum::http::header::{HOST, ORIGIN, REFERER};

    if let Some(fetch_site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        match fetch_site {
            "same-origin" => return true,
            "same-site" | "cross-site" => return false,
            _ => {}
        }
    }

    let Some(host) = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_authority)
    else {
        return false;
    };

    if let Some(origin_matches) = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(|value| origin_authority(value).is_some_and(|origin| origin == host))
    {
        return origin_matches;
    }

    if let Some(referer_matches) = headers
        .get(REFERER)
        .and_then(|value| value.to_str().ok())
        .map(|value| origin_authority(value).is_some_and(|referer| referer == host))
    {
        return referer_matches;
    }

    false
}

fn origin_authority(value: &str) -> Option<String> {
    value
        .parse::<axum::http::Uri>()
        .ok()?
        .authority()
        .and_then(|authority| normalize_authority(authority.as_str()))
}

fn normalize_authority(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('.');
    if value.is_empty()
        || value.contains('@')
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

fn route_required_role(method: &axum::http::Method, path: &str) -> AuthRole {
    use axum::http::Method;

    if (method == Method::POST && path.starts_with("/api/upload/"))
        || (method == Method::GET && path == "/api/upload/targets")
    {
        return AuthRole::Admin;
    }
    if method == Method::PUT && path == "/api/roms/rename" {
        return AuthRole::Admin;
    }
    // The library CSV export is surfaced only in the admin Metadata page's
    // Advanced section, so the download route is admin-gated to match.
    if method == Method::GET && path == "/api/export/library.csv" {
        return AuthRole::Admin;
    }
    if method == Method::GET && is_admin_page_route(path) {
        return AuthRole::Admin;
    }
    if is_user_route(method, path) {
        return AuthRole::User;
    }
    if is_unsafe_method(method) {
        return AuthRole::Admin;
    }
    AuthRole::User
}

fn request_auth_role(
    state: &AppState,
    request: &axum::http::Request<axum::body::Body>,
) -> AuthRole {
    use axum::http::header::COOKIE;

    session_token_from_cookie(request.headers().get(COOKIE))
        .and_then(|token| {
            state
                .auth
                .store
                .resolve_session(&token, &state.settings)
                .ok()
                .flatten()
        })
        .unwrap_or(AuthRole::Anonymous)
}

// pub(super): the storage guard (api::waiting::is_allowed_without_storage)
// consults the same classification to let anonymous server fns through.
pub(super) fn server_function_required_role(path: &str) -> Option<AuthRole> {
    let function = normalized_server_function_path(path)?;
    let function = function.as_str();
    if is_public_auth_server_function(function) {
        return Some(AuthRole::Anonymous);
    }
    if is_admin_server_function(function) {
        Some(AuthRole::Admin)
    } else if is_user_server_function(function) {
        Some(AuthRole::User)
    } else {
        Some(AuthRole::Admin)
    }
}

fn normalized_server_function_path(path: &str) -> Option<String> {
    let function = path.strip_prefix("/sfn/")?.trim_matches('/');
    let function = function.split('/').next().unwrap_or(function);
    let function = function.trim_end_matches(|ch: char| ch.is_ascii_digit());
    Some(normalize_server_function_name(function))
}

fn is_public_auth_server_function(function: &str) -> bool {
    matches!(
        function,
        "get_auth_status"
            | "login_with_replay_code"
            | "login_admin"
            | "complete_first_setup"
            | "logout"
    )
}

fn is_public_during_first_setup(path: &str) -> bool {
    if path == "/first-setup"
        || path == "/waiting"
        || path == "/api/version"
        || path.starts_with("/static/")
    {
        return true;
    }
    normalized_server_function_path(path).is_some_and(|function| {
        matches!(
            function.as_str(),
            "get_auth_status" | "complete_first_setup" | "logout"
        )
    })
}

#[cfg(test)]
fn is_explicitly_classified_server_function(function: &str) -> bool {
    let function = normalize_server_function_name(function);
    let function = function.as_str();
    is_public_auth_server_function(function)
        || is_admin_server_function(function)
        || is_user_read_server_function(function)
        || is_user_server_function(function)
}

fn is_admin_page_route(path: &str) -> bool {
    matches!(
        path,
        "/settings/wifi"
            | "/settings/nfs"
            | "/settings/hostname"
            | "/settings/retroachievements"
            | "/settings/replayos"
            | "/settings/replay-net-control"
            | "/settings/game-library"
            | "/settings/metadata"
            | "/settings/logs"
            | "/settings/github"
            | "/updating"
    )
}

fn is_user_route(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;

    matches!(
        (method, path),
        (&Method::POST, "/api/favorites")
            | (&Method::DELETE, "/api/favorites")
            | (&Method::PUT, "/api/favorites/group")
            | (&Method::PUT, "/api/favorites/flatten")
    ) || (method == Method::POST && path.starts_with("/api/manuals/upload/"))
}

fn is_user_read_server_function(function: &str) -> bool {
    matches!(
        function,
        "get_systems"
            | "get_recents"
            | "get_roms_page"
            | "get_rom_detail"
            | "get_rom_file_group"
            | "global_search"
            | "get_all_genres"
            | "get_system_genres"
            | "search_by_developer"
            | "get_developer_genres"
            | "get_developer_games"
            | "search_by_board"
            | "get_board_genres"
            | "get_board_games"
            | "random_game"
            | "random_game_for_system"
            | "get_related_games"
            | "get_recommendations"
            | "get_game_documents"
            | "get_local_manuals"
            | "get_game_manual_suggestions"
            | "get_game_resource_links"
            | "get_game_videos"
            | "get_provider_game_videos"
            | "search_game_videos"
            | "get_boxart_variants"
            // Read-only: the update banner (shown to every user, not just
            // admins) renders the "what's new" changelog from this.
            | "get_update_changelog"
    )
}

fn normalize_server_function_name(function: &str) -> String {
    if function.contains('_') {
        return function.to_ascii_lowercase();
    }

    let chars = function.chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(function.len() + 8);
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_ascii_uppercase() {
            let prev = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(index + 1).copied();
            if index > 0
                && (prev.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    || next.is_some_and(|c| c.is_ascii_lowercase()))
            {
                normalized.push('_');
            }
            normalized.push(ch.to_ascii_lowercase());
        } else if ch == '-' {
            normalized.push('_');
        } else {
            normalized.push(ch);
        }
    }
    normalized
}

fn is_admin_server_function(function: &str) -> bool {
    matches!(
        function,
        "get_wifi_config"
            | "save_wifi_config"
            | "get_nfs_config"
            | "save_nfs_config"
            | "get_retroachievements_config"
            | "save_retroachievements_config_and_restart"
            | "reboot_system"
            | "power_off_replayos_device"
            | "downgrade_admin_to_user"
            | "logout_all_browsers"
            | "get_admin_session_timeout"
            | "set_admin_session_timeout"
            | "get_hostname"
            | "save_hostname"
            | "change_root_password"
            | "clear_metadata"
            | "regenerate_metadata"
            | "download_metadata"
            | "clear_images"
            | "cleanup_orphaned_images"
            | "get_metadata_library_overview"
            | "get_metadata_page_snapshot"
            | "get_system_logs"
            | "get_log_level_config"
            | "save_log_level_config"
            | "get_replayos_log_level"
            | "get_github_api_key"
            | "save_github_api_key"
            | "save_region_preference"
            | "save_region_preference_secondary"
            | "save_language_preference"
            | "refresh_storage"
            | "regenerate_tls_certificate_info"
            | "get_tls_certificate_info"
            | "get_analytics_preference"
            | "delete_rom"
            | "rename_rom"
            | "get_replayos_settings"
            | "enable_replay_api_assisted"
            | "verify_replay_api_token"
            | "save_replayos_kiosk_mode"
            | "start_setup_metadata_downloads"
            | "update_thumbnails"
            | "cancel_thumbnail_update"
            | "clear_thumbnail_index"
            | "rescan_game_library"
            | "rebuild_game_library"
            | "rebuild_corrupt_library"
            | "repair_corrupt_user_data"
            | "restore_user_data_backup"
            | "check_for_updates"
            | "get_update_channel"
            | "save_update_channel"
            | "skip_version"
            | "start_update"
            | "save_analytics_preference"
    )
}

fn is_user_server_function(function: &str) -> bool {
    is_user_read_server_function(function)
        || matches!(
            function,
            "get_info"
                | "get_live_stats"
                | "get_mode"
                | "get_favorites"
                | "get_system_favorites"
                | "add_favorite"
                | "remove_favorite"
                | "organize_favorites"
                | "group_favorites"
                | "flatten_favorites"
                | "get_favorites_recommendations"
                | "delete_recent"
                | "get_user_captures"
                | "delete_user_capture"
                | "launch_game"
                | "get_replay_api_status"
                | "get_library_playtime"
                | "get_game_playtime"
                | "reprobe_replay_api"
                | "send_replay_player_command"
                | "send_replayos_message"
                | "restart_replayos_game"
                | "get_save_state_slots"
                | "add_game_resource_link"
                | "remove_game_resource_link"
                | "add_game_video"
                | "remove_game_video"
                | "download_manual"
                | "delete_manual"
                | "set_boxart_override"
                | "reset_boxart_override"
                | "get_setup_status"
                | "dismiss_setup"
                | "get_skins"
                | "set_skin"
                | "set_skin_sync"
                | "get_font_size"
                | "save_font_size"
                | "get_region_preference"
                | "get_region_preference_secondary"
                | "get_language_preference"
                | "get_locale"
                | "save_locale"
                | "get_preferred_languages"
        )
}

fn session_token_from_cookie(value: Option<&axum::http::HeaderValue>) -> Option<String> {
    value?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| {
            let value = value.trim();
            (name == "ReplayControlSession" && valid_session_cookie_value(value))
                .then(|| value.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_guard_keeps_bootstrap_server_functions_public() {
        assert_eq!(
            server_function_required_role("/sfn/get_auth_status"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_auth_status10507523110576576594"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/GetAuthStatus"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/login_with_replay_code"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/LoginWithReplayCode"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/complete_first_setup"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/CompleteFirstSetup"),
            Some(AuthRole::Anonymous)
        );
    }

    #[test]
    fn first_setup_public_paths_are_narrow() {
        assert!(is_public_during_first_setup("/first-setup"));
        assert!(is_public_during_first_setup("/static/style.css"));
        assert!(is_public_during_first_setup("/api/version"));
        assert!(is_public_during_first_setup("/sfn/get_auth_status"));
        assert!(is_public_during_first_setup("/sfn/complete_first_setup"));

        assert!(!is_public_during_first_setup("/login"));
        assert!(!is_public_during_first_setup("/settings"));
        assert!(!is_public_during_first_setup("/games/nintendo_nes"));
        assert!(!is_public_during_first_setup("/sfn/login_admin"));
        assert!(!is_public_during_first_setup("/sfn/login_with_replay_code"));
    }

    #[test]
    fn auth_guard_classifies_admin_and_user_server_functions() {
        assert_eq!(
            server_function_required_role("/sfn/save_wifi_config"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/SaveWifiConfig"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_wifi_config"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_hostname"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/delete_rom"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/DeleteRom"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/rename_rom"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/refresh_storage"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_region_preference"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_region_preference_secondary"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_language_preference"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/downgrade_admin_to_user"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/logout_all_browsers"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_admin_session_timeout"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/set_admin_session_timeout"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_tls_certificate_info"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/regenerate_tls_certificate_info"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_metadata_page_snapshot"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_metadata_library_overview"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/completely_new_server_function"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/add_favorite"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/launch_game"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/send_replay_player_command"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_locale"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_font_size"),
            Some(AuthRole::User)
        );
    }

    #[test]
    fn auth_guard_classifies_every_server_function_intentionally() {
        let names = discovered_server_function_names();
        assert!(
            names.len() > 100,
            "server-function inventory unexpectedly small: {}",
            names.len()
        );

        let missing = names
            .iter()
            .filter(|name| !is_explicitly_classified_server_function(name))
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "server functions need explicit auth classification: {missing:?}"
        );
    }

    /// Every discovered `#[server]` function must be registered via
    /// `register_explicit` in `main.rs`. An unregistered function resolves on
    /// the initial SSR render (direct call) but 404s when a client-side
    /// navigation re-runs its resource as an HTTP POST — a silent break on the
    /// SPA path only. This closes that drift class (two functions had slipped
    /// through: `get_update_changelog`, `get_replayos_log_level`).
    #[test]
    fn every_server_function_is_registered_in_main() {
        let main_src = include_str!("../main.rs");
        // Collapse whitespace so registrations split across lines
        // (`register_explicit::<...,\n>();`) match the same way single-line
        // ones do, then pull the last `::`-segment from each type argument.
        let joined: String = main_src.split_whitespace().collect();
        let needle = "register_explicit::<";
        let mut registered = std::collections::HashSet::new();
        let mut rest = joined.as_str();
        while let Some(pos) = rest.find(needle) {
            rest = &rest[pos + needle.len()..];
            if let Some(end) = rest.find('>') {
                let ty = rest[..end].trim_end_matches(',');
                let name = ty.rsplit("::").next().unwrap_or(ty);
                registered.insert(name.to_string());
            }
        }

        let to_pascal = |snake: &str| -> String {
            snake
                .split('_')
                .map(|seg| {
                    let mut chars = seg.chars();
                    match chars.next() {
                        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect()
        };

        let missing = discovered_server_function_names()
            .iter()
            .map(|name| to_pascal(name))
            .filter(|struct_name| !registered.contains(struct_name))
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "server functions defined but not registered in main.rs (they 404 on client-side nav): {missing:?}"
        );
    }

    #[test]
    fn auth_guard_inventory_covers_every_server_function_file() {
        let server_fns_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server_fns");
        let mut files = std::fs::read_dir(&server_fns_dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                (name.ends_with(".rs") && name != "mod.rs").then_some(name)
            })
            .collect::<Vec<_>>();
        files.sort();

        let mut inventoried = SERVER_FUNCTION_SOURCES
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect::<Vec<_>>();
        inventoried.sort();

        assert_eq!(
            inventoried, files,
            "server function source files must be added to the auth inventory"
        );
    }

    #[test]
    fn auth_guard_classifies_rest_mutations() {
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/upload/snes"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/upload/targets"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/manuals/upload/snes"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::PUT, "/api/roms/rename"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::DELETE, "/api/roms"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/favorites"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::DELETE, "/api/favorites"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/new-mutation"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/settings/wifi"),
            AuthRole::Admin
        );
    }

    #[test]
    fn unauthenticated_browse_routes_require_user_access() {
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/games/nes"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/systems"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/media/nes/Mario.png"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/favorites"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/info"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/settings"),
            AuthRole::User
        );
        assert_eq!(
            server_function_required_role("/sfn/get_systems"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/add_favorite"),
            Some(AuthRole::User)
        );
    }

    #[test]
    fn login_redirect_preserves_local_next_path() {
        assert_eq!(
            login_redirect_target(&"/settings/wifi?mode=manual".parse().unwrap()),
            "/login?next=%2Fsettings%2Fwifi%3Fmode%3Dmanual"
        );
        assert_eq!(
            login_redirect_target(&"/login".parse().unwrap()),
            "/login?next=%2F"
        );
    }

    #[test]
    fn access_redirect_preserves_local_next_path_without_looping() {
        assert_eq!(
            access_redirect_target(&"/settings/wifi?mode=manual".parse().unwrap()),
            "/settings/access?next=%2Fsettings%2Fwifi%3Fmode%3Dmanual"
        );
        assert_eq!(
            access_redirect_target(&"/settings/access".parse().unwrap()),
            "/settings/access?next=%2Fsettings"
        );
    }

    #[test]
    fn route_auth_cookie_parser_trims_and_rejects_malformed_values() {
        let valid = axum::http::HeaderValue::from_static(
            "theme=dark; ReplayControlSession= abc.def ; other=value",
        );
        assert_eq!(
            session_token_from_cookie(Some(&valid)).as_deref(),
            Some("abc.def")
        );

        let empty = axum::http::HeaderValue::from_static("ReplayControlSession= ");
        assert_eq!(session_token_from_cookie(Some(&empty)), None);

        let unsigned = axum::http::HeaderValue::from_static("ReplayControlSession=abc");
        assert_eq!(session_token_from_cookie(Some(&unsigned)), None);

        let too_many_segments =
            axum::http::HeaderValue::from_static("ReplayControlSession=abc.def.ghi");
        assert_eq!(session_token_from_cookie(Some(&too_many_segments)), None);

        let control = axum::http::HeaderValue::from_static("ReplayControlSession=abc\tdef");
        assert_eq!(session_token_from_cookie(Some(&control)), None);
    }

    #[test]
    fn csrf_check_accepts_same_origin() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("replay.local:8443"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("https://replay.local:8443"),
        );

        assert!(passes_csrf_origin_check(&headers));
    }

    #[test]
    fn csrf_check_accepts_same_origin_referer() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("192.168.1.20:8443"),
        );
        headers.insert(
            axum::http::header::REFERER,
            axum::http::HeaderValue::from_static("https://192.168.1.20:8443/settings"),
        );

        assert!(passes_csrf_origin_check(&headers));
    }

    #[test]
    fn csrf_check_accepts_same_origin_fetch_metadata() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "sec-fetch-site",
            axum::http::HeaderValue::from_static("same-origin"),
        );
        assert!(passes_csrf_origin_check(&headers));

        headers.insert(
            "sec-fetch-site",
            axum::http::HeaderValue::from_static("none"),
        );
        assert!(!passes_csrf_origin_check(&headers));

        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("replay.local:8443"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("https://replay.local:8443"),
        );
        assert!(passes_csrf_origin_check(&headers));

        headers.insert(
            "sec-fetch-site",
            axum::http::HeaderValue::from_static("cross-site"),
        );
        assert!(!passes_csrf_origin_check(&headers));
    }

    #[test]
    fn csrf_check_rejects_cross_origin_or_missing_headers() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("replay.local:8443"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("https://example.com"),
        );
        assert!(!passes_csrf_origin_check(&headers));

        headers.remove(axum::http::header::ORIGIN);
        assert!(!passes_csrf_origin_check(&headers));
    }

    const SERVER_FUNCTION_SOURCES: &[(&str, &str)] = &[
        ("auth.rs", include_str!("../server_fns/auth.rs")),
        ("boxart.rs", include_str!("../server_fns/boxart.rs")),
        ("favorites.rs", include_str!("../server_fns/favorites.rs")),
        ("images.rs", include_str!("../server_fns/images.rs")),
        ("manuals.rs", include_str!("../server_fns/manuals.rs")),
        ("metadata.rs", include_str!("../server_fns/metadata.rs")),
        (
            "recommendations.rs",
            include_str!("../server_fns/recommendations.rs"),
        ),
        ("related.rs", include_str!("../server_fns/related.rs")),
        ("replay_api.rs", include_str!("../server_fns/replay_api.rs")),
        ("resources.rs", include_str!("../server_fns/resources.rs")),
        ("roms.rs", include_str!("../server_fns/roms.rs")),
        (
            "save_states.rs",
            include_str!("../server_fns/save_states.rs"),
        ),
        ("search.rs", include_str!("../server_fns/search.rs")),
        ("settings.rs", include_str!("../server_fns/settings.rs")),
        ("system.rs", include_str!("../server_fns/system.rs")),
        ("thumbnails.rs", include_str!("../server_fns/thumbnails.rs")),
        ("videos.rs", include_str!("../server_fns/videos.rs")),
    ];

    fn discovered_server_function_names() -> Vec<String> {
        let mut names = SERVER_FUNCTION_SOURCES
            .iter()
            .flat_map(|(_, source)| source.lines())
            .filter_map(server_function_name_from_line)
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    fn server_function_name_from_line(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("pub async fn ")?;
        let name = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect::<String>();
        (!name.is_empty()).then_some(name)
    }
}
