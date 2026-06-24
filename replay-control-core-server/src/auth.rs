use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use replay_control_core::error::{Error, Result};
use ring::hmac;
use serde::{Deserialize, Serialize};

pub use replay_control_core::auth::*;

use crate::data_dir::DataDir;
use crate::settings::{SettingsStore, read_admin_session_timeout, read_replay_api_token};

const ROOT_SHADOW_PREFIX: &str = "root:";
const SIGNING_KEY_BYTES: usize = 32;
const USER_SESSION_TTL_SECONDS: u64 = 720 * 60 * 60;
const MAX_ADMIN_SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;
const LOGIN_RATE_LIMIT_MAX_FAILURES: u32 = 8;
const LOGIN_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOGIN_RATE_LIMIT_LOCKOUT: Duration = Duration::from_secs(5 * 60);

const COOKIE_VERSION: &str = "1";
const NONE_VALUE: &str = "-";

pub fn verify_os_password(subject: PasswordSubject, password: &str) -> Result<bool> {
    match subject {
        PasswordSubject::Root => verify_root_password(password),
    }
}

pub fn verify_replay_code_user_login(store: &SettingsStore, code: &str) -> Result<bool> {
    let code = code.trim();
    if !is_valid_replay_login_code(code) {
        return Ok(false);
    }
    let Some(token) = read_replay_api_token(store) else {
        return Ok(false);
    };
    let token = token.trim();
    if !is_valid_replay_login_code(token) {
        return Ok(false);
    }
    Ok(constant_time_eq(token.as_bytes(), code.as_bytes()))
}

impl LoginRateLimiter {
    pub fn check(&self) -> Result<()> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Other("Login rate limiter is unavailable".to_string()))?;
        if state.locked_until.is_some_and(|until| until > now) {
            return Err(Error::Other(
                "Too many failed login attempts. Try again in a few minutes.".to_string(),
            ));
        }
        if now.duration_since(state.window_started) > LOGIN_RATE_LIMIT_WINDOW {
            *state = LoginAttemptState::new(now);
        }
        Ok(())
    }

    pub fn record_success(&self) -> Result<()> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Other("Login rate limiter is unavailable".to_string()))?;
        *state = LoginAttemptState::new(now);
        Ok(())
    }

    pub fn record_failure(&self) -> Result<()> {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Other("Login rate limiter is unavailable".to_string()))?;
        if now.duration_since(state.window_started) > LOGIN_RATE_LIMIT_WINDOW {
            state.failures = 0;
            state.window_started = now;
            state.locked_until = None;
        }
        state.failures += 1;
        if state.failures >= LOGIN_RATE_LIMIT_MAX_FAILURES {
            state.locked_until = Some(now + LOGIN_RATE_LIMIT_LOCKOUT);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AuthStore {
    key_path: PathBuf,
    root_shadow_path: PathBuf,
    key_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Default)]
pub struct LoginRateLimiter {
    state: Arc<Mutex<LoginAttemptState>>,
}

#[derive(Debug, Clone)]
struct LoginAttemptState {
    failures: u32,
    window_started: Instant,
    locked_until: Option<Instant>,
}

impl Default for LoginAttemptState {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl LoginAttemptState {
    fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            window_started: now,
            locked_until: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub token: String,
    pub role: AuthRole,
    pub base_role: Option<AuthRole>,
    pub expires_at: u64,
    pub elevated_until: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedAuthSession {
    pub role: AuthRole,
    pub can_downgrade: bool,
    pub expires_at: u64,
    pub elevated_until: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignedSessionClaims {
    role: AuthRole,
    base_role: Option<AuthRole>,
    issued_at: u64,
    expires_at: u64,
    elevated_until: Option<u64>,
    user_fingerprint: Option<String>,
    admin_fingerprint: Option<String>,
}

impl AuthStore {
    pub fn open(data_dir: &DataDir) -> Result<Self> {
        let path = data_dir.auth_cookie_key_path();
        Self::open_at(path)
    }

    pub fn open_at(path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            key_path: path.into(),
            root_shadow_path: PathBuf::from("/etc/shadow"),
            key_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn open_at_with_shadow(
        path: impl Into<PathBuf>,
        root_shadow_path: impl Into<PathBuf>,
    ) -> Result<Self> {
        Ok(Self {
            key_path: path.into(),
            root_shadow_path: root_shadow_path.into(),
            key_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn create_user_session(&self, settings: &SettingsStore) -> Result<AuthSession> {
        let key = self.load_or_create_signing_key()?;
        let now = now_unix();
        let expires_at = now + USER_SESSION_TTL_SECONDS;
        let user_fingerprint = self.user_credential_fingerprint_with_key(settings, &key)?;
        let claims = SignedSessionClaims {
            role: AuthRole::User,
            base_role: None,
            issued_at: now,
            expires_at,
            elevated_until: None,
            user_fingerprint: Some(user_fingerprint),
            admin_fingerprint: None,
        };
        let token = sign_claims_with_key(&claims, &key);
        Ok(AuthSession {
            token,
            role: AuthRole::User,
            base_role: None,
            expires_at,
            elevated_until: None,
        })
    }

    pub fn create_admin_session(
        &self,
        base_role: Option<AuthRole>,
        settings: &SettingsStore,
    ) -> Result<AuthSession> {
        if !valid_admin_base_role(base_role) {
            return Err(Error::Other("Invalid admin session base role".to_string()));
        }
        let key = self.load_or_create_signing_key()?;
        let now = now_unix();
        let admin_ttl = read_admin_session_timeout(settings).seconds();
        let expires_at = session_expires_at(now, base_role, admin_ttl);
        let elevated_until = Some((now + admin_ttl).min(expires_at));
        let user_fingerprint = if base_role == Some(AuthRole::User) {
            Some(self.user_credential_fingerprint_with_key(settings, &key)?)
        } else {
            None
        };
        let claims = SignedSessionClaims {
            role: AuthRole::Admin,
            base_role,
            issued_at: now,
            expires_at,
            elevated_until,
            user_fingerprint,
            admin_fingerprint: Some(self.admin_credential_fingerprint_with_key(&key)?),
        };
        let token = sign_claims_with_key(&claims, &key);
        Ok(AuthSession {
            token,
            role: AuthRole::Admin,
            base_role,
            expires_at,
            elevated_until,
        })
    }

    pub fn refresh_admin_session_timeout(
        &self,
        token: &str,
        settings: &SettingsStore,
    ) -> Result<AuthSession> {
        let now = now_unix();
        let key = self
            .load_signing_key()?
            .ok_or_else(|| Error::Other("Admin session is required".to_string()))?;
        let claims = verify_claims_with_key(token, &key)?
            .ok_or_else(|| Error::Other("Admin session is required".to_string()))?;
        let resolved = self.effective_session(&claims, settings, now)?;
        if !resolved.is_some_and(|session| session.role == AuthRole::Admin) {
            return Err(Error::Other("Admin session is required".to_string()));
        }

        let admin_ttl = read_admin_session_timeout(settings).seconds();
        let expires_at = if claims.base_role == Some(AuthRole::User) {
            claims.expires_at
        } else {
            now + admin_ttl
        };
        let elevated_until = Some((now + admin_ttl).min(expires_at));
        let user_fingerprint = if claims.base_role == Some(AuthRole::User) {
            Some(self.user_credential_fingerprint_with_key(settings, &key)?)
        } else {
            None
        };
        let refreshed_claims = SignedSessionClaims {
            role: AuthRole::Admin,
            base_role: claims.base_role,
            issued_at: now,
            expires_at,
            elevated_until,
            user_fingerprint,
            admin_fingerprint: Some(self.admin_credential_fingerprint_with_key(&key)?),
        };
        let new_token = sign_claims_with_key(&refreshed_claims, &key);
        Ok(AuthSession {
            token: new_token,
            role: AuthRole::Admin,
            base_role: claims.base_role,
            expires_at,
            elevated_until,
        })
    }

    pub fn resolve_session(
        &self,
        token: &str,
        settings: &SettingsStore,
    ) -> Result<Option<AuthRole>> {
        Ok(self
            .resolve_session_details(token, settings)?
            .map(|session| session.role))
    }

    pub fn resolve_session_details(
        &self,
        token: &str,
        settings: &SettingsStore,
    ) -> Result<Option<ResolvedAuthSession>> {
        let now = now_unix();
        self.resolve_session_details_at(token, settings, now)
    }

    pub fn downgrade_session(&self, token: &str, settings: &SettingsStore) -> Result<AuthSession> {
        let now = now_unix();
        let key = self
            .load_signing_key()?
            .ok_or_else(|| Error::Other("Admin session is required".to_string()))?;
        let claims = verify_claims_with_key(token, &key)?
            .ok_or_else(|| Error::Other("Admin session is required".to_string()))?;
        let resolved = self.effective_session(&claims, settings, now)?;
        if !resolved.is_some_and(|session| session.can_downgrade) {
            return Err(Error::Other(
                "Only elevated admin sessions can switch to normal user".to_string(),
            ));
        }
        let user_fingerprint = self.user_credential_fingerprint_with_key(settings, &key)?;
        let downgraded_claims = SignedSessionClaims {
            role: AuthRole::User,
            base_role: None,
            issued_at: now,
            expires_at: claims.expires_at,
            elevated_until: None,
            user_fingerprint: Some(user_fingerprint),
            admin_fingerprint: None,
        };
        let new_token = sign_claims_with_key(&downgraded_claims, &key);
        Ok(AuthSession {
            token: new_token,
            role: AuthRole::User,
            base_role: None,
            expires_at: claims.expires_at,
            elevated_until: None,
        })
    }

    pub fn rotate_signing_key(&self) -> Result<()> {
        let _guard = self
            .key_lock
            .lock()
            .map_err(|_| Error::Other("Auth signing key lock is unavailable".to_string()))?;
        let key = generate_signing_key()?;
        write_signing_key(&self.key_path, &key)
    }

    fn resolve_session_details_at(
        &self,
        token: &str,
        settings: &SettingsStore,
        now: u64,
    ) -> Result<Option<ResolvedAuthSession>> {
        let Some(claims) = self.verify_claims(token)? else {
            return Ok(None);
        };
        self.effective_session(&claims, settings, now)
    }

    fn effective_session(
        &self,
        claims: &SignedSessionClaims,
        settings: &SettingsStore,
        now: u64,
    ) -> Result<Option<ResolvedAuthSession>> {
        if claims.expires_at <= now {
            return Ok(None);
        }
        match claims.role {
            AuthRole::User => {
                if !self.user_fingerprint_matches(settings, claims.user_fingerprint.as_deref())? {
                    return Ok(None);
                }
                Ok(Some(ResolvedAuthSession {
                    role: AuthRole::User,
                    can_downgrade: false,
                    expires_at: claims.expires_at,
                    elevated_until: None,
                }))
            }
            AuthRole::Admin => {
                let user_valid = claims.base_role == Some(AuthRole::User)
                    && self
                        .user_fingerprint_matches(settings, claims.user_fingerprint.as_deref())?;
                if claims.elevated_until.is_some_and(|expires| expires <= now) {
                    if user_valid {
                        return Ok(Some(ResolvedAuthSession {
                            role: AuthRole::User,
                            can_downgrade: false,
                            expires_at: claims.expires_at,
                            elevated_until: None,
                        }));
                    }
                    return Ok(None);
                }
                let admin_valid =
                    match self.admin_fingerprint_matches(claims.admin_fingerprint.as_deref()) {
                        Ok(valid) => valid,
                        Err(error) => {
                            tracing::warn!("admin session fingerprint check failed: {error}");
                            false
                        }
                    };
                if !admin_valid {
                    if user_valid {
                        return Ok(Some(ResolvedAuthSession {
                            role: AuthRole::User,
                            can_downgrade: false,
                            expires_at: claims.expires_at,
                            elevated_until: None,
                        }));
                    }
                    return Ok(None);
                }
                Ok(Some(ResolvedAuthSession {
                    role: AuthRole::Admin,
                    can_downgrade: user_valid,
                    expires_at: claims.expires_at,
                    elevated_until: claims.elevated_until,
                }))
            }
            AuthRole::Anonymous => Ok(None),
        }
    }

    #[cfg(test)]
    fn sign_claims(&self, claims: &SignedSessionClaims) -> Result<String> {
        let key = self.load_or_create_signing_key()?;
        Ok(sign_claims_with_key(claims, &key))
    }

    fn verify_claims(&self, token: &str) -> Result<Option<SignedSessionClaims>> {
        let key = match self.load_signing_key()? {
            Some(key) => key,
            None => return Ok(None),
        };
        verify_claims_with_key(token, &key)
    }

    fn load_or_create_signing_key(&self) -> Result<Vec<u8>> {
        let _guard = self
            .key_lock
            .lock()
            .map_err(|_| Error::Other("Auth signing key lock is unavailable".to_string()))?;
        if let Some(key) = self.load_signing_key()? {
            return Ok(key);
        }
        let key = generate_signing_key()?;
        write_signing_key(&self.key_path, &key)?;
        Ok(key)
    }

    fn load_signing_key(&self) -> Result<Option<Vec<u8>>> {
        match std::fs::read(&self.key_path) {
            Ok(key) if key.len() == SIGNING_KEY_BYTES => Ok(Some(key)),
            Ok(key) => {
                tracing::warn!(
                    "ignoring invalid auth cookie signing key at {}: expected {SIGNING_KEY_BYTES} bytes, found {}",
                    self.key_path.display(),
                    key.len()
                );
                Ok(None)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::io(&self.key_path, error)),
        }
    }

    fn user_credential_fingerprint(&self, settings: &SettingsStore) -> Result<String> {
        let key = self.load_or_create_signing_key()?;
        self.user_credential_fingerprint_with_key(settings, &key)
    }

    fn user_credential_fingerprint_with_key(
        &self,
        settings: &SettingsStore,
        key: &[u8],
    ) -> Result<String> {
        let Some(token) = read_replay_api_token(settings) else {
            return Err(Error::Other(
                "RePlayOS Net Control code is not configured".to_string(),
            ));
        };
        let token = token.trim();
        if !is_valid_replay_login_code(token) {
            return Err(Error::Other(
                "Stored RePlayOS Net Control code is invalid".to_string(),
            ));
        }
        Ok(fingerprint_with_key("user", token.as_bytes(), key))
    }

    fn admin_credential_fingerprint(&self) -> Result<String> {
        let key = self.load_or_create_signing_key()?;
        self.admin_credential_fingerprint_with_key(&key)
    }

    fn admin_credential_fingerprint_with_key(&self, key: &[u8]) -> Result<String> {
        let shadow = std::fs::read_to_string(&self.root_shadow_path)
            .map_err(|e| Error::Other(format!("Cannot read shadow file: {e}")))?;
        let stored_hash = root_password_hash(&shadow)?;
        Ok(fingerprint_with_key("admin", stored_hash.as_bytes(), key))
    }

    fn user_fingerprint_matches(
        &self,
        settings: &SettingsStore,
        claim: Option<&str>,
    ) -> Result<bool> {
        let Some(claim) = claim else {
            return Ok(false);
        };
        let Ok(current) = self.user_credential_fingerprint(settings) else {
            return Ok(false);
        };
        Ok(constant_time_eq(current.as_bytes(), claim.as_bytes()))
    }

    fn admin_fingerprint_matches(&self, claim: Option<&str>) -> Result<bool> {
        let Some(claim) = claim else {
            return Ok(false);
        };
        let current = self.admin_credential_fingerprint()?;
        Ok(constant_time_eq(current.as_bytes(), claim.as_bytes()))
    }
}

fn session_expires_at(now: u64, base_role: Option<AuthRole>, admin_ttl: u64) -> u64 {
    match base_role {
        Some(AuthRole::User) => now + USER_SESSION_TTL_SECONDS,
        _ => now + admin_ttl,
    }
}

fn valid_admin_base_role(base_role: Option<AuthRole>) -> bool {
    matches!(base_role, None | Some(AuthRole::User))
}

fn is_valid_replay_login_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|ch| ch.is_ascii_digit())
}

fn verify_root_password(password: &str) -> Result<bool> {
    if !valid_password_input(password) {
        return Ok(false);
    }
    let shadow = std::fs::read_to_string("/etc/shadow")
        .map_err(|e| Error::Other(format!("Cannot read shadow file: {e}")))?;
    let stored_hash = root_password_hash(&shadow)?;
    let computed_hash = crypt_password(password, stored_hash)?;

    Ok(constant_time_eq(
        computed_hash.as_bytes(),
        stored_hash.as_bytes(),
    ))
}

fn valid_password_input(password: &str) -> bool {
    !password.contains('\n') && !password.contains('\0')
}

fn root_password_hash(shadow: &str) -> Result<&str> {
    let stored_hash = shadow
        .lines()
        .find(|line| line.starts_with(ROOT_SHADOW_PREFIX))
        .and_then(|line| line.split(':').nth(1))
        .ok_or_else(|| Error::Other("Cannot find root password hash".to_string()))?;

    if stored_hash == "*" || stored_hash == "!" || stored_hash.is_empty() {
        return Err(Error::Other("Root account has no password set".to_string()));
    }

    Ok(stored_hash)
}

fn crypt_password(password: &str, stored_hash: &str) -> Result<String> {
    // Both `su` and `unix_chkpwd` skip authentication when called by root,
    // so Replay Control verifies the hash directly. Python ctypes avoids
    // cross-compilation issues with libcrypt soname mismatches and supports
    // yescrypt (`$y$`) hashes used by current Linux distributions.
    // The submitted password goes over stdin, never argv, so it is not exposed
    // in `/proc/<pid>/cmdline`.
    let mut child = Command::new("python3")
        .args([
            "-c",
            "import sys,ctypes; d=sys.stdin.read().split('\\n',1); \
             l=ctypes.CDLL('libcrypt.so.1'); l.crypt.restype=ctypes.c_char_p; \
             r=l.crypt(d[0].encode(),d[1].encode()); print(r.decode() if r else '')",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("Failed to verify password: {e}")))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Other("Failed to open stdin".to_string()))?;
        stdin
            .write_all(format!("{password}\n{stored_hash}").as_bytes())
            .map_err(|e| Error::Other(format!("Failed to verify password: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| Error::Other(format!("Failed to verify password: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "Password verification failed: {stderr}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn generate_signing_key() -> Result<Vec<u8>> {
    let mut bytes = [0u8; SIGNING_KEY_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|e| Error::Other(format!("Failed to generate auth signing key: {e}")))?;
    Ok(bytes.to_vec())
}

fn write_signing_key(path: &std::path::Path, key: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let tmp = signing_key_temp_path(path);
    let stale = std::fs::remove_file(&tmp);
    if let Err(error) = stale
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(Error::io(&tmp, error));
    }
    write_private_file(&tmp, key)?;
    std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))?;
    set_private_file_permissions(path);
    if let Some(parent) = path.parent() {
        sync_directory(parent);
    }
    Ok(())
}

fn signing_key_temp_path(path: &std::path::Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("auth-cookie.key");
    path.with_file_name(format!("{file_name}.tmp-{}", std::process::id()))
}

fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut file = private_create_new_file(path)?;
    file.write_all(bytes).map_err(|e| Error::io(path, e))?;
    file.sync_all().map_err(|e| Error::io(path, e))?;
    Ok(())
}

#[cfg(unix)]
fn private_create_new_file(path: &std::path::Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

#[cfg(not(unix))]
fn private_create_new_file(path: &std::path::Path) -> Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

fn sync_directory(path: &std::path::Path) {
    if let Ok(dir) = std::fs::File::open(path)
        && let Err(error) = dir.sync_all()
    {
        tracing::warn!(
            "failed to sync auth signing key directory {}: {error}",
            path.display()
        );
    }
}

#[cfg(unix)]
fn set_private_file_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!("failed to set auth signing key permissions: {error}");
    }
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &std::path::Path) {}

fn sign_payload(key: &[u8], payload: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, payload).as_ref().to_vec()
}

fn verify_payload_signature(key: &[u8], payload: &[u8], signature: &[u8]) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::verify(&key, payload, signature).is_ok()
}

fn sign_claims_with_key(claims: &SignedSessionClaims, key: &[u8]) -> String {
    let payload = encode_claims(claims);
    let signature = sign_payload(key, payload.as_bytes());
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(payload.as_bytes()),
        URL_SAFE_NO_PAD.encode(signature)
    )
}

fn verify_claims_with_key(token: &str, key: &[u8]) -> Result<Option<SignedSessionClaims>> {
    let Some((payload_b64, signature_b64)) = token.split_once('.') else {
        return Ok(None);
    };
    let payload = match URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(payload) => payload,
        Err(_) => return Ok(None),
    };
    let signature = match URL_SAFE_NO_PAD.decode(signature_b64) {
        Ok(signature) => signature,
        Err(_) => return Ok(None),
    };
    if !verify_payload_signature(key, &payload, &signature) {
        return Ok(None);
    }
    let payload = match String::from_utf8(payload) {
        Ok(payload) => payload,
        Err(_) => return Ok(None),
    };
    decode_claims(&payload)
}

fn fingerprint_with_key(kind: &str, credential: &[u8], key: &[u8]) -> String {
    let mut payload = Vec::with_capacity(kind.len() + credential.len() + 1);
    payload.extend_from_slice(kind.as_bytes());
    payload.push(b':');
    payload.extend_from_slice(credential);
    hex_encode(&sign_payload(key, &payload))
}

fn encode_claims(claims: &SignedSessionClaims) -> String {
    [
        COOKIE_VERSION.to_string(),
        role_to_cookie(claims.role).to_string(),
        claims
            .base_role
            .map(role_to_cookie)
            .unwrap_or(NONE_VALUE)
            .to_string(),
        claims.issued_at.to_string(),
        claims.expires_at.to_string(),
        claims
            .elevated_until
            .map(|value| value.to_string())
            .unwrap_or_else(|| NONE_VALUE.to_string()),
        claims
            .user_fingerprint
            .as_deref()
            .unwrap_or(NONE_VALUE)
            .to_string(),
        claims
            .admin_fingerprint
            .as_deref()
            .unwrap_or(NONE_VALUE)
            .to_string(),
    ]
    .join("\n")
}

fn decode_claims(payload: &str) -> Result<Option<SignedSessionClaims>> {
    let parts = payload.split('\n').collect::<Vec<_>>();
    if parts.len() != 8 || parts[0] != COOKIE_VERSION {
        return Ok(None);
    }
    let Some(role) = cookie_to_role(parts[1]) else {
        return Ok(None);
    };
    let base_role = if parts[2] == NONE_VALUE {
        None
    } else {
        cookie_to_role(parts[2])
    };
    if base_role == Some(AuthRole::Anonymous) || base_role == Some(AuthRole::Admin) {
        return Ok(None);
    }
    let Some(base_role) = base_role else {
        if parts[2] != NONE_VALUE {
            return Ok(None);
        }
        return parse_claim_numbers(parts, role, None);
    };
    parse_claim_numbers(parts, role, Some(base_role))
}

fn parse_claim_numbers(
    parts: Vec<&str>,
    role: AuthRole,
    base_role: Option<AuthRole>,
) -> Result<Option<SignedSessionClaims>> {
    let issued_at = match parts[3].parse::<u64>() {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let expires_at = match parts[4].parse::<u64>() {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let elevated_until = if parts[5] == NONE_VALUE {
        None
    } else {
        match parts[5].parse::<u64>() {
            Ok(value) => Some(value),
            Err(_) => return Ok(None),
        }
    };
    let claims = SignedSessionClaims {
        role,
        base_role,
        issued_at,
        expires_at,
        elevated_until,
        user_fingerprint: optional_claim(parts[6]),
        admin_fingerprint: optional_claim(parts[7]),
    };
    if !valid_claim_shape(&claims) {
        return Ok(None);
    }
    Ok(Some(claims))
}

fn valid_claim_shape(claims: &SignedSessionClaims) -> bool {
    if claims.expires_at <= claims.issued_at {
        return false;
    }

    match claims.role {
        AuthRole::Anonymous => false,
        AuthRole::User => {
            claims.base_role.is_none()
                && claims.elevated_until.is_none()
                && claims.expires_at <= claims.issued_at + USER_SESSION_TTL_SECONDS
                && claims.user_fingerprint.is_some()
                && claims.admin_fingerprint.is_none()
        }
        AuthRole::Admin => {
            let Some(elevated_until) = claims.elevated_until else {
                return false;
            };
            if elevated_until <= claims.issued_at || elevated_until > claims.expires_at {
                return false;
            }
            if elevated_until > claims.issued_at + MAX_ADMIN_SESSION_TTL_SECONDS {
                return false;
            }
            if claims.admin_fingerprint.is_none() {
                return false;
            }
            match claims.base_role {
                None => {
                    claims.expires_at <= claims.issued_at + MAX_ADMIN_SESSION_TTL_SECONDS
                        && claims.user_fingerprint.is_none()
                }
                Some(AuthRole::User) => {
                    claims.expires_at <= claims.issued_at + USER_SESSION_TTL_SECONDS
                        && claims.user_fingerprint.is_some()
                }
                Some(AuthRole::Anonymous | AuthRole::Admin) => false,
            }
        }
    }
}

fn optional_claim(value: &str) -> Option<String> {
    (value != NONE_VALUE).then(|| value.to_string())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn role_to_cookie(role: AuthRole) -> &'static str {
    match role {
        AuthRole::Anonymous => "anonymous",
        AuthRole::User => "user",
        AuthRole::Admin => "admin",
    }
}

fn cookie_to_role(value: &str) -> Option<AuthRole> {
    match value {
        "user" => Some(AuthRole::User),
        "admin" => Some(AuthRole::Admin),
        "anonymous" => Some(AuthRole::Anonymous),
        _ => None,
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::write_replay_api_token;

    #[test]
    fn extracts_root_password_hash() {
        let shadow = "daemon:*:19793:0:99999:7:::\nroot:$y$j9T$abc:def:ghi\n";

        assert_eq!(root_password_hash(shadow).unwrap(), "$y$j9T$abc");
    }

    #[test]
    fn rejects_locked_or_missing_root_hash() {
        assert!(root_password_hash("root:*:19793:0:99999:7:::\n").is_err());
        assert!(root_password_hash("root:!:19793:0:99999:7:::\n").is_err());
        assert!(root_password_hash("daemon:*:19793:0:99999:7:::\n").is_err());
    }

    #[test]
    fn password_verification_rejects_protocol_breaking_input() {
        assert!(valid_password_input("replayos"));
        assert!(!valid_password_input("bad\npassword"));
        assert!(!valid_password_input("bad\0password"));
    }

    #[test]
    fn constant_time_equality_requires_same_bytes_and_length() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
    }

    #[test]
    fn replay_code_verification_uses_app_stored_token() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path());
        write_replay_api_token(&settings, "123456").unwrap();

        assert!(verify_replay_code_user_login(&settings, "123456").unwrap());
        assert!(verify_replay_code_user_login(&settings, " 123456 ").unwrap());
        assert!(!verify_replay_code_user_login(&settings, "654321").unwrap());
        assert!(!verify_replay_code_user_login(&settings, "12345a").unwrap());
        assert!(!verify_replay_code_user_login(&settings, "1234567").unwrap());
        write_replay_api_token(&settings, "abcdef").unwrap();
        assert!(!verify_replay_code_user_login(&settings, "abcdef").unwrap());
    }

    #[test]
    fn user_session_creation_rejects_malformed_stored_replay_code() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        write_replay_api_token(&settings, "abcdef").unwrap();
        assert!(store.create_user_session(&settings).is_err());

        write_replay_api_token(&settings, "１２３４５６").unwrap();
        assert!(store.create_user_session(&settings).is_err());
    }

    #[test]
    fn auth_store_creates_and_resolves_signed_user_cookie() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        let session = store.create_user_session(&settings).unwrap();

        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            Some(AuthRole::User)
        );
    }

    #[test]
    fn changing_net_control_code_invalidates_existing_user_sessions() {
        // FINDING #7: user sessions are fingerprinted from the Net Control code.
        // Re-onboarding / regenerating the code makes every live user session
        // stop resolving -> the whole household is logged out mid-session. The
        // invalidation itself is correct; the bug is that the re-onboarding flow
        // gives no warning and does not re-issue. Confirm the mechanism here.
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        let session = store.create_user_session(&settings).unwrap();
        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            Some(AuthRole::User)
        );

        // Admin regenerates / re-enters the Net Control code.
        write_replay_api_token(&settings, "654321").unwrap();

        // The previously-valid user cookie now resolves to None: logged out.
        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            None
        );
    }

    #[test]
    fn changing_root_password_invalidates_current_admin_session() {
        // FINDING #8: admin sessions are fingerprinted from /etc/shadow. Changing
        // the root password (new shadow hash) makes the current admin cookie stop
        // resolving, so the admin who just changed their own password is silently
        // logged out on the next request (the password page reports success but
        // never re-issues a session). Confirm the invalidation mechanism here.
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$oldhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();

        let session = store.create_admin_session(None, &settings).unwrap();
        assert!(
            store
                .resolve_session(&session.token, &settings)
                .unwrap()
                .is_some()
        );

        // Root password changes -> new hash in shadow.
        std::fs::write(&shadow_path, "root:$y$j9T$newhash:19999:0:99999:7:::\n").unwrap();

        // The admin's current cookie no longer resolves: silently logged out.
        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            None
        );
    }

    #[test]
    fn opening_auth_store_does_not_create_signing_key() {
        let temp = tempfile::tempdir().unwrap();
        let key_path = temp.path().join("auth-cookie.key");

        let _store = AuthStore::open_at(&key_path).unwrap();

        assert!(!key_path.exists());
    }

    #[test]
    fn resolved_direct_admin_session_reports_elevation_deadline() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();

        let session = store.create_admin_session(None, &settings).unwrap();
        let resolved = store
            .resolve_session_details(&session.token, &settings)
            .unwrap()
            .unwrap();

        assert_eq!(resolved.role, AuthRole::Admin);
        assert!(!resolved.can_downgrade);
        assert_eq!(resolved.expires_at, session.expires_at);
        assert_eq!(resolved.elevated_until, session.elevated_until);
        assert_eq!(resolved.elevated_until, Some(resolved.expires_at));
    }

    #[test]
    fn configured_admin_session_timeout_controls_new_admin_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        crate::settings::write_admin_session_timeout(
            &settings,
            crate::config::AdminSessionTimeout::TwelveHours,
        )
        .unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();

        let before = now_unix();
        let session = store.create_admin_session(None, &settings).unwrap();
        let after = now_unix();

        assert_eq!(session.elevated_until, Some(session.expires_at));
        assert!(session.expires_at >= before + 12 * 60 * 60);
        assert!(session.expires_at <= after + 12 * 60 * 60);
    }

    #[test]
    fn refreshed_admin_session_uses_current_timeout_from_now() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();
        let session = store.create_admin_session(None, &settings).unwrap();
        crate::settings::write_admin_session_timeout(
            &settings,
            crate::config::AdminSessionTimeout::TwelveHours,
        )
        .unwrap();

        let before = now_unix();
        let refreshed = store
            .refresh_admin_session_timeout(&session.token, &settings)
            .unwrap();
        let after = now_unix();

        assert_eq!(refreshed.elevated_until, Some(refreshed.expires_at));
        assert!(refreshed.expires_at >= before + 12 * 60 * 60);
        assert!(refreshed.expires_at <= after + 12 * 60 * 60);
        assert!(refreshed.expires_at >= session.expires_at);
    }

    #[test]
    fn resolved_elevated_admin_session_reports_user_and_admin_deadlines() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();

        let session = store
            .create_admin_session(Some(AuthRole::User), &settings)
            .unwrap();
        let resolved = store
            .resolve_session_details(&session.token, &settings)
            .unwrap()
            .unwrap();

        assert_eq!(resolved.role, AuthRole::Admin);
        assert!(resolved.can_downgrade);
        assert_eq!(resolved.expires_at, session.expires_at);
        assert_eq!(resolved.elevated_until, session.elevated_until);
        assert!(resolved.elevated_until.unwrap() < resolved.expires_at);
    }

    #[test]
    fn created_direct_admin_session_expires_to_anonymous_without_user_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();

        let session = store.create_admin_session(None, &settings).unwrap();
        assert!(store.downgrade_session(&session.token, &settings).is_err());

        assert_eq!(
            store
                .resolve_session_details_at(&session.token, &settings, session.expires_at + 1)
                .unwrap(),
            None
        );
    }

    #[test]
    fn created_elevated_admin_session_falls_back_to_user_after_admin_deadline() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();

        let session = store
            .create_admin_session(Some(AuthRole::User), &settings)
            .unwrap();
        let resolved = store
            .resolve_session_details_at(
                &session.token,
                &settings,
                session.elevated_until.unwrap() + 1,
            )
            .unwrap()
            .unwrap();

        assert_eq!(resolved.role, AuthRole::User);
        assert!(!resolved.can_downgrade);
        assert_eq!(resolved.expires_at, session.expires_at);
        assert_eq!(resolved.elevated_until, None);
    }

    #[test]
    fn concurrent_first_user_logins_resolve_with_one_signing_key() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(12));

        let handles = (0..12)
            .map(|_| {
                let store = store.clone();
                let settings = settings.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.create_user_session(&settings).unwrap().token
                })
            })
            .collect::<Vec<_>>();

        let tokens = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        for token in tokens {
            assert_eq!(
                store.resolve_session(&token, &settings).unwrap(),
                Some(AuthRole::User)
            );
        }
    }

    #[test]
    fn admin_session_creation_rejects_impossible_base_roles() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        assert!(
            store
                .create_admin_session(Some(AuthRole::Anonymous), &settings)
                .is_err()
        );
        assert!(
            store
                .create_admin_session(Some(AuthRole::Admin), &settings)
                .is_err()
        );
    }

    #[test]
    fn signed_cookie_tampering_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        let session = store.create_user_session(&settings).unwrap();
        let tampered = session.token.replacen('.', "x.", 1);

        assert_eq!(store.resolve_session(&tampered, &settings).unwrap(), None);
    }

    #[test]
    fn signing_key_rotation_invalidates_existing_cookies() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        let session = store.create_user_session(&settings).unwrap();
        store.rotate_signing_key().unwrap();

        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            None
        );
    }

    #[test]
    fn signing_key_rotation_rejects_downgrade_from_old_admin_cookie() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();
        let session = store
            .create_admin_session(Some(AuthRole::User), &settings)
            .unwrap();

        store.rotate_signing_key().unwrap();

        assert!(store.downgrade_session(&session.token, &settings).is_err());
    }

    #[test]
    fn invalid_signing_key_makes_existing_cookies_anonymous_without_error() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let store = AuthStore::open_at(&key_path).unwrap();
        let session = store.create_user_session(&settings).unwrap();

        std::fs::write(&key_path, b"partial").unwrap();

        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            None
        );
    }

    #[test]
    fn invalid_signing_key_is_recreated_on_next_login() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        std::fs::write(&key_path, b"partial").unwrap();
        let store = AuthStore::open_at(&key_path).unwrap();

        let session = store.create_user_session(&settings).unwrap();

        assert_eq!(std::fs::read(&key_path).unwrap().len(), SIGNING_KEY_BYTES);
        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            Some(AuthRole::User)
        );
    }

    #[cfg(unix)]
    #[test]
    fn signing_key_file_is_created_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let store = AuthStore::open_at(&key_path).unwrap();

        store.create_user_session(&settings).unwrap();

        let mode = std::fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn signed_non_utf8_cookie_payload_is_rejected_without_error() {
        let temp = tempfile::tempdir().unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();
        let key = store.load_or_create_signing_key().unwrap();
        let payload = [0xff, 0xfe, 0xfd];
        let signature = sign_payload(&key, &payload);
        let token = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(signature)
        );

        assert_eq!(store.verify_claims(&token).unwrap(), None);
    }

    #[test]
    fn signed_cookie_with_impossible_claim_shape_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();
        let now = now_unix();
        let cases = [
            SignedSessionClaims {
                role: AuthRole::Anonymous,
                base_role: None,
                issued_at: now,
                expires_at: now + USER_SESSION_TTL_SECONDS,
                elevated_until: None,
                user_fingerprint: None,
                admin_fingerprint: None,
            },
            SignedSessionClaims {
                role: AuthRole::User,
                base_role: Some(AuthRole::User),
                issued_at: now,
                expires_at: now + USER_SESSION_TTL_SECONDS,
                elevated_until: None,
                user_fingerprint: Some("user-fingerprint".to_string()),
                admin_fingerprint: None,
            },
            SignedSessionClaims {
                role: AuthRole::Admin,
                base_role: None,
                issued_at: now,
                expires_at: now + MAX_ADMIN_SESSION_TTL_SECONDS,
                elevated_until: None,
                user_fingerprint: None,
                admin_fingerprint: Some("admin-fingerprint".to_string()),
            },
            SignedSessionClaims {
                role: AuthRole::Admin,
                base_role: Some(AuthRole::User),
                issued_at: now,
                expires_at: now + USER_SESSION_TTL_SECONDS,
                elevated_until: Some(now + MAX_ADMIN_SESSION_TTL_SECONDS),
                user_fingerprint: None,
                admin_fingerprint: Some("admin-fingerprint".to_string()),
            },
            SignedSessionClaims {
                role: AuthRole::Admin,
                base_role: None,
                issued_at: now,
                expires_at: now + MAX_ADMIN_SESSION_TTL_SECONDS,
                elevated_until: Some(now + MAX_ADMIN_SESSION_TTL_SECONDS + 1),
                user_fingerprint: None,
                admin_fingerprint: Some("admin-fingerprint".to_string()),
            },
            SignedSessionClaims {
                role: AuthRole::User,
                base_role: None,
                issued_at: now,
                expires_at: now + USER_SESSION_TTL_SECONDS + 1,
                elevated_until: None,
                user_fingerprint: Some("user-fingerprint".to_string()),
                admin_fingerprint: None,
            },
            SignedSessionClaims {
                role: AuthRole::Admin,
                base_role: None,
                issued_at: now,
                expires_at: now + MAX_ADMIN_SESSION_TTL_SECONDS + 1,
                elevated_until: Some(now + MAX_ADMIN_SESSION_TTL_SECONDS),
                user_fingerprint: None,
                admin_fingerprint: Some("admin-fingerprint".to_string()),
            },
            SignedSessionClaims {
                role: AuthRole::Admin,
                base_role: Some(AuthRole::User),
                issued_at: now,
                expires_at: now + USER_SESSION_TTL_SECONDS,
                elevated_until: Some(now + MAX_ADMIN_SESSION_TTL_SECONDS + 1),
                user_fingerprint: Some("user-fingerprint".to_string()),
                admin_fingerprint: Some("admin-fingerprint".to_string()),
            },
        ];

        for claims in cases {
            let token = store.sign_claims(&claims).unwrap();
            assert_eq!(store.verify_claims(&token).unwrap(), None);
        }
    }

    #[test]
    fn user_cookie_is_invalidated_by_net_control_code_change() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();

        let session = store.create_user_session(&settings).unwrap();
        write_replay_api_token(&settings, "654321").unwrap();

        assert_eq!(
            store.resolve_session(&session.token, &settings).unwrap(),
            None
        );
    }

    #[test]
    fn expired_elevated_admin_cookie_falls_back_to_user() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();
        let now = now_unix();
        let user_fingerprint = store.user_credential_fingerprint(&settings).unwrap();
        let claims = SignedSessionClaims {
            role: AuthRole::Admin,
            base_role: Some(AuthRole::User),
            issued_at: now - 20,
            expires_at: now + USER_SESSION_TTL_SECONDS,
            elevated_until: Some(now - 1),
            user_fingerprint: Some(user_fingerprint),
            admin_fingerprint: Some("test-admin".to_string()),
        };

        let resolved = store
            .effective_session(&claims, &settings, now)
            .unwrap()
            .unwrap();
        assert_eq!(resolved.role, AuthRole::User);
        assert!(!resolved.can_downgrade);
        assert_eq!(resolved.elevated_until, None);
    }

    #[test]
    fn expired_direct_admin_cookie_resolves_to_anonymous() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let store = AuthStore::open_at(temp.path().join("auth-cookie.key")).unwrap();
        let now = now_unix();
        let claims = SignedSessionClaims {
            role: AuthRole::Admin,
            base_role: None,
            issued_at: now - 20,
            expires_at: now + USER_SESSION_TTL_SECONDS,
            elevated_until: Some(now - 1),
            user_fingerprint: None,
            admin_fingerprint: Some("test-admin".to_string()),
        };

        assert_eq!(
            store.effective_session(&claims, &settings, now).unwrap(),
            None
        );
    }

    #[test]
    fn elevated_admin_session_falls_back_to_user_when_shadow_is_unreadable() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        write_replay_api_token(&settings, "123456").unwrap();
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();
        let session = store
            .create_admin_session(Some(AuthRole::User), &settings)
            .unwrap();
        let store_without_shadow =
            AuthStore::open_at_with_shadow(&key_path, temp.path().join("missing-shadow")).unwrap();

        let resolved = store_without_shadow
            .resolve_session_details(&session.token, &settings)
            .unwrap()
            .unwrap();

        assert_eq!(resolved.role, AuthRole::User);
        assert!(!resolved.can_downgrade);
        assert_eq!(resolved.elevated_until, None);
    }

    #[test]
    fn direct_admin_session_with_unreadable_shadow_resolves_to_anonymous() {
        let temp = tempfile::tempdir().unwrap();
        let settings = SettingsStore::new(temp.path().join("settings"));
        let key_path = temp.path().join("auth-cookie.key");
        let shadow_path = temp.path().join("shadow");
        std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
        let store = AuthStore::open_at_with_shadow(&key_path, &shadow_path).unwrap();
        let session = store.create_admin_session(None, &settings).unwrap();
        let store_without_shadow =
            AuthStore::open_at_with_shadow(&key_path, temp.path().join("missing-shadow")).unwrap();

        assert_eq!(
            store_without_shadow
                .resolve_session_details(&session.token, &settings)
                .unwrap(),
            None
        );
    }

    #[test]
    fn login_rate_limiter_locks_after_repeated_failures() {
        let limiter = LoginRateLimiter::default();

        for _ in 0..LOGIN_RATE_LIMIT_MAX_FAILURES {
            limiter.check().unwrap();
            limiter.record_failure().unwrap();
        }

        assert!(limiter.check().is_err());
    }

    #[test]
    fn login_rate_limiter_success_clears_failures() {
        let limiter = LoginRateLimiter::default();

        limiter.record_failure().unwrap();
        limiter.record_success().unwrap();

        assert!(limiter.check().is_ok());
    }
}
