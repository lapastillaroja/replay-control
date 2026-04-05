mod en;
mod es;
mod ja;
mod keys;

pub use keys::Key;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Supported UI locales. English is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    #[default]
    En,
    Es,
    Ja,
}

impl Locale {
    pub fn code(&self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Es => "es",
            Locale::Ja => "ja",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "es" => Locale::Es,
            "ja" => Locale::Ja,
            _ => Locale::En,
        }
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

/// Wrapper so SSR can inject the starting locale before `App` renders.
#[derive(Clone, Copy)]
pub struct InitialLocale(pub Locale);

/// Provides the i18n context to the component tree.
/// Call this once at the App root level.
///
/// On SSR, the initial locale is read from `InitialLocale` context (injected
/// by the SSR handler from `settings.cfg` / Accept-Language header).
/// On hydration, the client reads the `<html lang>` attribute that SSR set,
/// ensuring both sides agree on the initial locale — no mismatch.
pub fn provide_i18n() {
    let initial = use_context::<InitialLocale>()
        .map(|il| il.0)
        .unwrap_or_else(|| {
            // Client-side hydration: read the lang attribute that SSR set on <html>.
            #[cfg(target_arch = "wasm32")]
            {
                web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.document_element())
                    .and_then(|el| {
                        let lang = el.get_attribute("lang").unwrap_or_default();
                        if lang.is_empty() {
                            None
                        } else {
                            Some(Locale::from_code(&lang))
                        }
                    })
                    .unwrap_or_default()
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                Locale::default()
            }
        });
    let (locale, set_locale) = signal(initial);
    provide_context(I18nContext { locale, set_locale });
}

/// Retrieves the current i18n context.
pub fn use_i18n() -> I18nContext {
    expect_context::<I18nContext>()
}

#[derive(Clone, Copy)]
pub struct I18nContext {
    pub locale: ReadSignal<Locale>,
    pub set_locale: WriteSignal<Locale>,
}

/// Translation function. Returns the localized string for the given key.
/// All locales are exhaustive — every key must be handled in every locale file.
pub fn t(locale: Locale, key: Key) -> &'static str {
    match locale {
        Locale::En => en::translate(key),
        Locale::Es => es::translate(key),
        Locale::Ja => ja::translate(key),
    }
}

/// Resolves a key name string to a `Key` variant.
/// Only covers keys used in server-returned structured data (recommendation titles).
pub fn key_from_str(s: &str) -> Option<Key> {
    match s {
        "SpotlightBestGenre" => Some(Key::SpotlightBestGenre),
        "SpotlightBestOf" => Some(Key::SpotlightBestOf),
        "SpotlightGamesBy" => Some(Key::SpotlightGamesBy),
        "SpotlightHiddenGems" => Some(Key::SpotlightHiddenGems),
        "SpotlightTopRated" => Some(Key::SpotlightTopRated),
        "SpotlightRediscover" => Some(Key::SpotlightRediscover),
        "SpotlightBecauseYouLove" => Some(Key::SpotlightBecauseYouLove),
        "SpotlightMoreFrom" => Some(Key::SpotlightMoreFrom),
        "PillClassics" => Some(Key::PillClassics),
        "PillBestOf" => Some(Key::PillBestOf),
        "PillGamesBy" => Some(Key::PillGamesBy),
        "PillMultiplayer" => Some(Key::PillMultiplayer),
        _ => None,
    }
}

/// Translation with numbered placeholder interpolation. Returns an owned String.
/// Placeholders are `{0}`, `{1}`, etc., allowing reordering per language.
///
/// Example: `tf(locale, Key::GameDetailNOfM, &["3", "10"])` → `"3 of 10"` (en)
pub fn tf(locale: Locale, key: Key, args: &[&str]) -> String {
    let template = t(locale, key);
    let mut result = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{i}}}"), arg);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_str_covers_all_server_keys() {
        let server_keys = [
            "SpotlightBestGenre",
            "SpotlightBestOf",
            "SpotlightGamesBy",
            "SpotlightHiddenGems",
            "SpotlightTopRated",
            "SpotlightRediscover",
            "SpotlightBecauseYouLove",
            "SpotlightMoreFrom",
            "PillClassics",
            "PillBestOf",
            "PillGamesBy",
            "PillMultiplayer",
        ];
        for name in server_keys {
            assert!(
                key_from_str(name).is_some(),
                "key_from_str missing entry for server key: {name}"
            );
        }
    }
}
