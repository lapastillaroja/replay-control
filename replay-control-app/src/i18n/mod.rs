mod en;
mod es;
mod ja;
mod keys;

pub use keys::Key;
pub use replay_control_core::locale::Locale;

use leptos::prelude::*;

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
    let ctx = I18nContext { locale, set_locale };
    provide_context(ctx);
    // The client has exactly one i18n instance for the whole session. Cache it so
    // `use_i18n()` still works in components created after an async boundary (a
    // `Suspense`/`Suspend` on a client-side navigation), where the reactive owner
    // chain no longer reaches the context. Not used on the server, where each
    // request has its own locale and the context is always in scope.
    #[cfg(target_arch = "wasm32")]
    CLIENT_I18N.with(|cell| cell.set(Some(ctx)));
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLIENT_I18N: std::cell::Cell<Option<I18nContext>> = const { std::cell::Cell::new(None) };
}

/// Retrieves the current i18n context.
pub fn use_i18n() -> I18nContext {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(ctx) = use_context::<I18nContext>() {
            return ctx;
        }
        CLIENT_I18N
            .with(|cell| cell.get())
            .expect("i18n not initialized: provide_i18n() must run at the App root")
    }
    #[cfg(not(target_arch = "wasm32"))]
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
        Locale::Es => es::translate(key),
        Locale::Ja => ja::translate(key),
        // Auto and En both fall back to English
        _ => en::translate(key),
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
        "PillCoOp" => Some(Key::PillCoOp),
        "PillBoard" => Some(Key::PillBoard),
        "SpotlightCoOp" => Some(Key::SpotlightCoOp),
        "SpotlightBoard" => Some(Key::SpotlightBoard),
        _ => None,
    }
}

/// Translation with numbered placeholder interpolation. Returns an owned String.
/// Placeholders are `{0}`, `{1}`, etc., allowing reordering per language.
///
/// Example: `tf(locale, Key::GameDetailNOfM, &["3", "10"])` → `"3 of 10"` (en)
/// i18n key for a now-playing play-state label. Shared by every surface that
/// renders the state (home hero today; player bar and game detail next).
pub fn play_state_label_key(play_state: replay_control_core::replay_api::PlayState) -> Key {
    use replay_control_core::replay_api::PlayState;
    match play_state {
        PlayState::Playing => Key::NowPlayingLabelPlaying,
        PlayState::Paused => Key::NowPlayingLabelPaused,
        PlayState::Halted => Key::NowPlayingLabelHalted,
        PlayState::InMenu => Key::NowPlayingLabelInMenu,
    }
}

/// "Disc 2/4" label for a multi-disc game, shared by the now-playing surfaces.
pub fn disc_label(locale: Locale, disc: replay_control_core::replay_api::DiscInfo) -> String {
    tf(
        locale,
        Key::NowPlayingDisc,
        &[&disc.number.to_string(), &disc.count.to_string()],
    )
}

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
            "PillCoOp",
            "PillBoard",
            "SpotlightCoOp",
            "SpotlightBoard",
        ];
        for name in server_keys {
            assert!(
                key_from_str(name).is_some(),
                "key_from_str missing entry for server key: {name}"
            );
        }
    }
}
