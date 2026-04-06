/// Supported UI locales for the application.
///
/// `Auto` means "detect from browser Accept-Language header".
/// `En` is the default when no preference is set or an unknown code is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Locale {
    Auto,
    #[default]
    En,
    Es,
    Ja,
}

impl Locale {
    /// The short code used in settings files, HTML `lang` attributes, etc.
    pub fn code(&self) -> &'static str {
        match self {
            Locale::Auto => "auto",
            Locale::En => "en",
            Locale::Es => "es",
            Locale::Ja => "ja",
        }
    }

    /// Parse a locale code string. Unknown values map to `En`.
    pub fn from_code(code: &str) -> Self {
        match code {
            "auto" => Locale::Auto,
            "es" => Locale::Es,
            "ja" => Locale::Ja,
            _ => Locale::En,
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Locale::Auto)
    }

    /// Returns `None` if `Auto`, otherwise `Some(*self)`.
    /// Useful for determining if there's an explicit locale to use.
    pub fn effective(&self) -> Option<Locale> {
        if self.is_auto() { None } else { Some(*self) }
    }

    /// All supported locale codes including "auto".
    pub fn all_codes() -> &'static [&'static str] {
        &["auto", "en", "es", "ja"]
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_en() {
        assert_eq!(Locale::default(), Locale::En);
    }

    #[test]
    fn from_code_roundtrip() {
        for code in Locale::all_codes() {
            assert_eq!(Locale::from_code(code).code(), *code);
        }
    }

    #[test]
    fn unknown_code_maps_to_en() {
        assert_eq!(Locale::from_code("xx"), Locale::En);
        assert_eq!(Locale::from_code(""), Locale::En);
    }

    #[test]
    fn effective_returns_none_for_auto() {
        assert_eq!(Locale::Auto.effective(), None);
        assert_eq!(Locale::En.effective(), Some(Locale::En));
        assert_eq!(Locale::Es.effective(), Some(Locale::Es));
    }

    #[test]
    fn display_matches_code() {
        assert_eq!(Locale::Ja.to_string(), "ja");
        assert_eq!(Locale::Auto.to_string(), "auto");
    }
}
