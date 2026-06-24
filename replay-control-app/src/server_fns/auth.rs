use super::*;

#[cfg(feature = "ssr")]
use crate::util::is_valid_net_control_code;
use replay_control_core::auth::AuthStatus;
#[cfg(feature = "ssr")]
use replay_control_core::auth::{AuthRole, PasswordSubject, valid_session_cookie_value};

#[cfg(feature = "ssr")]
use axum::http::header::{COOKIE, SET_COOKIE};
#[cfg(feature = "ssr")]
use axum::http::{HeaderMap, HeaderValue};
#[cfg(feature = "ssr")]
use leptos_axum::ResponseOptions;
#[cfg(feature = "ssr")]
use replay_control_core_server::auth::{
    AuthSession, ResolvedAuthSession, verify_os_password, verify_replay_code_user_login,
};
#[cfg(feature = "ssr")]
use replay_control_core_server::config::AdminSessionTimeout;
#[cfg(feature = "ssr")]
use replay_control_core_server::settings::{
    read_admin_session_timeout, write_admin_session_timeout, write_first_setup_done,
};
#[cfg(feature = "ssr")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "ssr")]
const SESSION_COOKIE: &str = "ReplayControlSession";
#[cfg(feature = "ssr")]
const USER_MAX_AGE_SECONDS: u64 = 720 * 60 * 60;

#[server(prefix = "/sfn")]
pub async fn get_auth_status() -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Ok(auth_status_from_session(&state, None));
    }
    let session = current_session(&state).await?;
    Ok(auth_status_from_session(&state, session))
}

#[server(prefix = "/sfn")]
pub async fn login_with_replay_code(code: String) -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Ok(auth_status_from_session(&state, None));
    }
    let code = normalize_login_code(code)?;
    state
        .auth
        .login_rate_limiter
        .check()
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let valid = verify_replay_code_user_login(&state.settings, &code)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !valid {
        state
            .auth
            .login_rate_limiter
            .record_failure()
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        return Err(ServerFnError::new(
            "Access code is incorrect or not configured",
        ));
    }
    state
        .auth
        .login_rate_limiter
        .record_success()
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let session = state
        .auth
        .store
        .create_user_session(&state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    set_session_cookie(&session.token, USER_MAX_AGE_SECONDS)?;
    Ok(auth_status_from_created_session(&state, &session, false))
}

#[server(prefix = "/sfn")]
pub async fn login_admin(password: String) -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Err(ServerFnError::new(
            "Admin login with the device password is available only on RePlayOS",
        ));
    }
    if password.is_empty() {
        return Err(ServerFnError::new("Device password is required"));
    }
    verify_admin_password_with_rate_limit(&state, &password)?;

    let previous_token = session_token_from_request().await?;
    let previous_session = match previous_token.as_deref() {
        Some(token) => state
            .auth
            .store
            .resolve_session_details(token, &state.settings)
            .map_err(|e| ServerFnError::new(e.to_string()))?,
        None => None,
    };
    let base_role = admin_base_role(previous_session);

    create_admin_session_response(&state, base_role)
}

#[server(prefix = "/sfn")]
pub async fn complete_first_setup(password: String) -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Ok(auth_status_from_session(&state, None));
    }
    if state.first_setup_done() {
        return Err(ServerFnError::new("First setup is already complete"));
    }
    if password.is_empty() {
        return Err(ServerFnError::new("Device password is required"));
    }

    verify_admin_password_with_rate_limit(&state, &password)?;
    write_first_setup_done(&state.settings, true).map_err(|e| ServerFnError::new(e.to_string()))?;
    state
        .prefs
        .write()
        .expect("prefs lock poisoned")
        .first_setup_done = true;

    create_admin_session_response(&state, None)
}

#[cfg(feature = "ssr")]
fn verify_admin_password_with_rate_limit(
    state: &crate::api::AppState,
    password: &str,
) -> Result<(), ServerFnError> {
    state
        .auth
        .login_rate_limiter
        .check()
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let valid = verify_os_password(PasswordSubject::Root, password)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !valid {
        state
            .auth
            .login_rate_limiter
            .record_failure()
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        return Err(ServerFnError::new("Device password is incorrect"));
    }
    state
        .auth
        .login_rate_limiter
        .record_success()
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(())
}

#[cfg(feature = "ssr")]
fn create_admin_session_response(
    state: &crate::api::AppState,
    base_role: Option<AuthRole>,
) -> Result<AuthStatus, ServerFnError> {
    let session = state
        .auth
        .store
        .create_admin_session(base_role, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let max_age_seconds = remaining_seconds(session.expires_at).unwrap_or(0);
    set_session_cookie(&session.token, max_age_seconds)?;
    Ok(auth_status_from_created_session(
        state,
        &session,
        base_role == Some(AuthRole::User),
    ))
}

#[server(prefix = "/sfn")]
pub async fn get_admin_session_timeout() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Ok(AdminSessionTimeout::default().as_str().to_string());
    }
    let session = current_session(&state).await?;
    if !session.is_some_and(|session| session.role == AuthRole::Admin) {
        return Err(ServerFnError::new("Admin session is required"));
    }
    Ok(read_admin_session_timeout(&state.settings)
        .as_str()
        .to_string())
}

#[server(prefix = "/sfn")]
pub async fn set_admin_session_timeout(value: String) -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Err(ServerFnError::new(
            "Admin sessions are available only on RePlayOS",
        ));
    }
    let timeout = AdminSessionTimeout::from_str_value(value.trim())
        .ok_or_else(|| ServerFnError::new("Unsupported admin session timeout"))?;
    let token = session_token_from_request()
        .await?
        .ok_or_else(|| ServerFnError::new("Admin session is required"))?;
    let session = state
        .auth
        .store
        .resolve_session_details(&token, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !session.is_some_and(|session| session.role == AuthRole::Admin) {
        return Err(ServerFnError::new("Admin session is required"));
    }

    write_admin_session_timeout(&state.settings, timeout)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let session = state
        .auth
        .store
        .refresh_admin_session_timeout(&token, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    set_session_cookie(
        &session.token,
        remaining_seconds(session.expires_at).unwrap_or(0),
    )?;
    Ok(auth_status_from_created_session(
        &state,
        &session,
        session.base_role == Some(AuthRole::User),
    ))
}

#[server(prefix = "/sfn")]
pub async fn downgrade_admin_to_user() -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Err(ServerFnError::new(
            "Admin sessions are available only on RePlayOS",
        ));
    }
    let token = session_token_from_request()
        .await?
        .ok_or_else(|| ServerFnError::new("Admin session is required"))?;
    let session = state
        .auth
        .store
        .resolve_session_details(&token, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !session.is_some_and(|session| session.can_downgrade) {
        return Err(ServerFnError::new("Admin session is required"));
    }

    let session = state
        .auth
        .store
        .downgrade_session(&token, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    set_session_cookie(
        &session.token,
        remaining_seconds(session.expires_at).unwrap_or(0),
    )?;
    Ok(auth_status_from_created_session(&state, &session, false))
}

#[server(prefix = "/sfn")]
pub async fn logout() -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    clear_session_cookie()?;
    Ok(auth_status_from_session(&state, None))
}

#[server(prefix = "/sfn")]
pub async fn logout_all_browsers() -> Result<AuthStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        clear_session_cookie()?;
        return Err(ServerFnError::new(
            "Admin sessions are available only on RePlayOS",
        ));
    }
    let token = session_token_from_request()
        .await?
        .ok_or_else(|| ServerFnError::new("Admin session is required"))?;
    let session = state
        .auth
        .store
        .resolve_session_details(&token, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    if !session.is_some_and(|session| session.role == AuthRole::Admin) {
        return Err(ServerFnError::new("Admin session is required"));
    }
    state
        .auth
        .store
        .rotate_signing_key()
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    clear_session_cookie()?;
    Ok(auth_status_from_session(&state, None))
}

#[cfg(feature = "ssr")]
fn auth_status_from_session(
    state: &crate::api::AppState,
    session: Option<ResolvedAuthSession>,
) -> AuthStatus {
    let role = session
        .as_ref()
        .map(|session| session.role)
        .unwrap_or(AuthRole::Anonymous);
    let can_downgrade = session.is_some_and(|session| session.can_downgrade);
    let session_seconds_remaining = session
        .as_ref()
        .and_then(|session| remaining_seconds(session.expires_at));
    let admin_seconds_remaining = session
        .as_ref()
        .and_then(|session| session.elevated_until)
        .and_then(remaining_seconds);
    auth_status_for_role(
        state,
        role,
        can_downgrade,
        session_seconds_remaining,
        admin_seconds_remaining,
    )
}

#[cfg(feature = "ssr")]
fn auth_status_from_created_session(
    state: &crate::api::AppState,
    session: &AuthSession,
    can_downgrade: bool,
) -> AuthStatus {
    auth_status_for_role(
        state,
        session.role,
        can_downgrade,
        remaining_seconds(session.expires_at),
        session.elevated_until.and_then(remaining_seconds),
    )
}

#[cfg(feature = "ssr")]
fn auth_status_for_role(
    state: &crate::api::AppState,
    role: AuthRole,
    can_downgrade: bool,
    session_seconds_remaining: Option<u64>,
    admin_seconds_remaining: Option<u64>,
) -> AuthStatus {
    AuthStatus {
        role,
        auth_required: state.mode.is_device(),
        can_downgrade: state.mode.is_device() && can_downgrade,
        session_seconds_remaining,
        admin_seconds_remaining,
    }
}

#[cfg(feature = "ssr")]
fn remaining_seconds(deadline: u64) -> Option<u64> {
    remaining_seconds_at(deadline, current_unix_seconds())
}

#[cfg(feature = "ssr")]
fn remaining_seconds_at(deadline: u64, now: u64) -> Option<u64> {
    deadline.checked_sub(now).filter(|seconds| *seconds > 0)
}

#[cfg(feature = "ssr")]
fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(feature = "ssr")]
async fn current_session(
    state: &crate::api::AppState,
) -> Result<Option<ResolvedAuthSession>, ServerFnError> {
    if !state.mode.is_device() {
        return Ok(None);
    }
    let Some(token) = session_token_from_request().await? else {
        return Ok(None);
    };
    state
        .auth
        .store
        .resolve_session_details(&token, &state.settings)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[cfg(feature = "ssr")]
pub(crate) async fn current_auth_role(
    state: &crate::api::AppState,
) -> Result<AuthRole, ServerFnError> {
    Ok(current_session(state)
        .await?
        .map(|session| session.role)
        .unwrap_or(AuthRole::Anonymous))
}

#[cfg(feature = "ssr")]
fn normalize_login_code(code: String) -> Result<String, ServerFnError> {
    let code = code.trim().to_string();
    if is_valid_net_control_code(&code) {
        Ok(code)
    } else {
        Err(ServerFnError::new("Enter the 6-digit Net Control code"))
    }
}

#[cfg(feature = "ssr")]
fn admin_base_role(previous_session: Option<ResolvedAuthSession>) -> Option<AuthRole> {
    match previous_session {
        Some(ResolvedAuthSession {
            role: AuthRole::User,
            ..
        }) => Some(AuthRole::User),
        Some(ResolvedAuthSession {
            role: AuthRole::Admin,
            can_downgrade: true,
            ..
        }) => Some(AuthRole::User),
        _ => None,
    }
}

#[cfg(feature = "ssr")]
async fn session_token_from_request() -> Result<Option<String>, ServerFnError> {
    let headers: HeaderMap = leptos_axum::extract().await?;
    Ok(session_token_from_headers(&headers))
}

#[cfg(feature = "ssr")]
fn session_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| {
            let value = value.trim();
            (name == SESSION_COOKIE && valid_session_cookie_value(value)).then(|| value.to_string())
        })
}

#[cfg(feature = "ssr")]
fn set_session_cookie(token: &str, max_age_seconds: u64) -> Result<(), ServerFnError> {
    let cookie = build_session_cookie(token, max_age_seconds, session_cookie_secure());
    append_cookie_header(cookie)
}

#[cfg(feature = "ssr")]
fn clear_session_cookie() -> Result<(), ServerFnError> {
    append_cookie_header(build_session_cookie("", 0, true))?;
    append_cookie_header(build_session_cookie("", 0, false))
}

#[cfg(feature = "ssr")]
fn append_cookie_header(cookie: String) -> Result<(), ServerFnError> {
    let response = expect_context::<ResponseOptions>();
    let value = HeaderValue::from_str(&cookie)
        .map_err(|e| ServerFnError::new(format!("invalid session cookie: {e}")))?;
    response.append_header(SET_COOKIE, value);
    Ok(())
}

#[cfg(feature = "ssr")]
fn session_cookie_secure() -> bool {
    expect_context::<crate::api::AppState>()
        .auth
        .cookie_policy
        .secure_attribute()
}

#[cfg(feature = "ssr")]
fn build_session_cookie(token: &str, max_age_seconds: u64, secure: bool) -> String {
    let secure = if secure { " Secure;" } else { "" };
    let expires = if max_age_seconds == 0 {
        " Expires=Thu, 01 Jan 1970 00:00:00 GMT;"
    } else {
        ""
    };
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax;{secure}{expires} Path=/; Max-Age={max_age_seconds}"
    )
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn reads_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("foo=bar; ReplayControlSession= abc.def ; theme=dark"),
        );

        assert_eq!(
            session_token_from_headers(&headers).as_deref(),
            Some("abc.def")
        );
    }

    #[test]
    fn rejects_empty_or_control_character_session_cookie_values() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("ReplayControlSession= "));
        assert_eq!(session_token_from_headers(&headers), None);

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("ReplayControlSession=abc\tdef"),
        );
        assert_eq!(session_token_from_headers(&headers), None);

        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("ReplayControlSession=abc"));
        assert_eq!(session_token_from_headers(&headers), None);

        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("ReplayControlSession=abc.def.ghi"),
        );
        assert_eq!(session_token_from_headers(&headers), None);
    }

    #[test]
    fn session_cookie_uses_secure_by_default() {
        let cookie = build_session_cookie("abc123", 60, true);

        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains(" Secure;"));
        assert!(cookie.contains("Max-Age=60"));
    }

    #[test]
    fn session_cookie_can_be_explicitly_insecure_for_dangerous_http_auth() {
        let cookie = build_session_cookie("abc123", 60, false);

        assert!(!cookie.contains(" Secure;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
    }

    #[test]
    fn session_cookie_deletion_includes_expiry() {
        let secure_delete = build_session_cookie("", 0, true);
        let insecure_delete = build_session_cookie("", 0, false);

        assert!(secure_delete.contains(" Secure;"));
        assert!(secure_delete.contains("Max-Age=0"));
        assert!(secure_delete.contains("Expires=Thu, 01 Jan 1970 00:00:00 GMT"));
        assert!(!insecure_delete.contains(" Secure;"));
        assert!(insecure_delete.contains("Max-Age=0"));
        assert!(insecure_delete.contains("Expires=Thu, 01 Jan 1970 00:00:00 GMT"));
    }

    #[test]
    fn normal_user_login_code_must_be_six_ascii_digits() {
        assert_eq!(
            normalize_login_code(" 123456 ".to_string()).unwrap(),
            "123456"
        );
        assert!(normalize_login_code("12345".to_string()).is_err());
        assert!(normalize_login_code("1234567".to_string()).is_err());
        assert!(normalize_login_code("12345a".to_string()).is_err());
        assert!(normalize_login_code("１２３４５６".to_string()).is_err());
    }

    #[test]
    fn admin_login_base_role_requires_existing_user_session() {
        assert_eq!(admin_base_role(None), None);
        assert_eq!(
            admin_base_role(Some(ResolvedAuthSession {
                role: AuthRole::Anonymous,
                can_downgrade: false,
                expires_at: 100,
                elevated_until: None,
            })),
            None
        );
        assert_eq!(
            admin_base_role(Some(ResolvedAuthSession {
                role: AuthRole::Admin,
                can_downgrade: false,
                expires_at: 100,
                elevated_until: Some(100),
            })),
            None
        );
        assert_eq!(
            admin_base_role(Some(ResolvedAuthSession {
                role: AuthRole::Admin,
                can_downgrade: true,
                expires_at: 100,
                elevated_until: Some(100),
            })),
            Some(AuthRole::User)
        );
        assert_eq!(
            admin_base_role(Some(ResolvedAuthSession {
                role: AuthRole::User,
                can_downgrade: false,
                expires_at: 100,
                elevated_until: None,
            })),
            Some(AuthRole::User)
        );
    }

    #[test]
    fn remaining_seconds_excludes_expired_deadlines() {
        assert_eq!(remaining_seconds_at(100, 99), Some(1));
        assert_eq!(remaining_seconds_at(100, 100), None);
        assert_eq!(remaining_seconds_at(100, 101), None);
    }

    #[test]
    fn session_cookie_max_age_can_match_remaining_claim_lifetime() {
        let max_age = remaining_seconds_at(200, 125).unwrap_or(0);
        let cookie = build_session_cookie("abc123", max_age, true);

        assert!(cookie.contains("Max-Age=75"));
        assert!(!cookie.contains("Expires=Thu, 01 Jan 1970"));
    }
}
