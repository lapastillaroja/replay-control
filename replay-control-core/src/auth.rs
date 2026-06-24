use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthRole {
    Anonymous,
    User,
    Admin,
}

impl AuthRole {
    pub fn allows(self, required: AuthRole) -> bool {
        self.rank() >= required.rank()
    }

    fn rank(self) -> u8 {
        match self {
            AuthRole::Anonymous => 0,
            AuthRole::User => 1,
            AuthRole::Admin => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    pub role: AuthRole,
    pub auth_required: bool,
    pub can_downgrade: bool,
    pub session_seconds_remaining: Option<u64>,
    pub admin_seconds_remaining: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordSubject {
    Root,
}

pub fn valid_session_cookie_value(value: &str) -> bool {
    const MAX_COOKIE_VALUE_LEN: usize = 2048;

    if value.is_empty() || value.len() > MAX_COOKIE_VALUE_LEN {
        return false;
    }

    let mut dot_count = 0;
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => {}
            b'.' => dot_count += 1,
            _ => return false,
        }
    }

    dot_count == 1 && !value.starts_with('.') && !value.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_hierarchy_allows_lower_roles_only() {
        assert!(AuthRole::Admin.allows(AuthRole::User));
        assert!(AuthRole::Admin.allows(AuthRole::Admin));
        assert!(AuthRole::User.allows(AuthRole::Anonymous));
        assert!(!AuthRole::User.allows(AuthRole::Admin));
        assert!(!AuthRole::Anonymous.allows(AuthRole::User));
    }

    #[test]
    fn session_cookie_values_match_signed_token_shape() {
        assert!(valid_session_cookie_value("abc.DEF-123_456"));
        assert!(!valid_session_cookie_value(""));
        assert!(!valid_session_cookie_value("abc123"));
        assert!(!valid_session_cookie_value(".abc123"));
        assert!(!valid_session_cookie_value("abc123."));
        assert!(!valid_session_cookie_value("abc.def.ghi"));
        assert!(!valid_session_cookie_value("abc def"));
        assert!(!valid_session_cookie_value("abc\tdef"));
        assert!(!valid_session_cookie_value("abc=<script>.def"));
        assert!(!valid_session_cookie_value(&format!(
            "{}.def",
            "a".repeat(2048)
        )));
    }
}
