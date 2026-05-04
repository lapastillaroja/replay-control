use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};

/// Theme identifiers.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Default,
    Light,
    Amber,
    Green,
    Midnight,
    Rose,
    Nord,
    HighContrast,
}

impl Theme {
    const ALL: &'static [Self] = &[
        Self::Default,
        Self::Light,
        Self::Amber,
        Self::Green,
        Self::Midnight,
        Self::Rose,
        Self::Nord,
        Self::HighContrast,
    ];

    fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "",
            Self::Light => "theme-light",
            Self::Amber => "theme-amber",
            Self::Green => "theme-green",
            Self::Midnight => "theme-midnight",
            Self::Rose => "theme-rose",
            Self::Nord => "theme-nord",
            Self::HighContrast => "theme-high-contrast",
        }
    }

    fn name_key(&self) -> Key {
        match self {
            Self::Default => Key::ThemeDefault,
            Self::Light => Key::ThemeLight,
            Self::Amber => Key::ThemeAmber,
            Self::Green => Key::ThemeGreen,
            Self::Midnight => Key::ThemeMidnight,
            Self::Rose => Key::ThemeRose,
            Self::Nord => Key::ThemeNord,
            Self::HighContrast => Key::ThemeHighContrast,
        }
    }

    /// Swatch colors for the preview card: [bg, surface, accent].
    fn swatches(&self) -> [&'static str; 3] {
        match self {
            Self::Default => ["#0f1115", "#1a1d23", "#6366f1"],
            Self::Light => ["#f3f4f6", "#ffffff", "#6366f1"],
            Self::Amber => ["#1a1500", "#2a2000", "#ffb000"],
            Self::Green => ["#0a1a0a", "#0f200f", "#44ff44"],
            Self::Midnight => ["#0c1222", "#151f32", "#3b82f6"],
            Self::Rose => ["#1a0f14", "#261520", "#f472b6"],
            Self::Nord => ["#2e3440", "#3b4252", "#88c0d0"],
            Self::HighContrast => ["#000000", "#111111", "#ffff00"],
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "theme-light" => Self::Light,
            "theme-amber" => Self::Amber,
            "theme-green" => Self::Green,
            "theme-midnight" => Self::Midnight,
            "theme-rose" => Self::Rose,
            "theme-nord" => Self::Nord,
            "theme-high-contrast" => Self::HighContrast,
            _ => Self::Default,
        }
    }
}

const THEME_STORAGE_KEY: &str = "replay-control-theme";

#[cfg(feature = "hydrate")]
fn get_local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(feature = "hydrate")]
fn get_saved_theme() -> Theme {
    let storage = match get_local_storage() {
        Some(s) => s,
        None => return Theme::Default,
    };
    match storage.get_item(THEME_STORAGE_KEY).ok().flatten() {
        Some(s) => Theme::from_str(&s),
        None => Theme::Default,
    }
}

#[cfg(feature = "hydrate")]
fn apply_theme(theme: Theme) {
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            let html = doc.document_element().unwrap();
            for t in Theme::ALL {
                let cls = t.as_str();
                if !cls.is_empty() {
                    let _ = html.class_list().remove_1(cls);
                }
            }
            if theme != Theme::Default {
                let _ = html.class_list().add_1(theme.as_str());
            }
        }
    }
}

#[cfg(feature = "hydrate")]
fn save_theme(theme: Theme) {
    let storage = match get_local_storage() {
        Some(s) => s,
        None => return,
    };
    if theme == Theme::Default {
        let _ = storage.remove_item(THEME_STORAGE_KEY);
    } else {
        let _ = storage.set_item(THEME_STORAGE_KEY, theme.as_str());
    }
}

/// Apply the saved theme on initial load (hydrate-only).
/// Call this once during app initialization.
#[cfg(feature = "hydrate")]
pub fn init_theme() {
    let theme = get_saved_theme();
    apply_theme(theme);
}

#[cfg(not(feature = "hydrate"))]
pub fn init_theme() {}

/// Theme selector grid shown in Settings > Appearance.
#[component]
pub fn ThemeSelector() -> impl IntoView {
    let i18n = use_i18n();

    #[cfg(feature = "hydrate")]
    let current = RwSignal::new(get_saved_theme());

    #[cfg(not(feature = "hydrate"))]
    let current = RwSignal::new(Theme::Default);

    let on_select = move |theme: Theme| {
        current.set(theme);
        #[cfg(feature = "hydrate")]
        {
            apply_theme(theme);
            save_theme(theme);
        }
    };

    view! {
        <div class="theme-grid">
            {Theme::ALL.iter().map(|&theme| {
                let is_active = move || current.get() == theme;
                let [bg, surface, accent] = theme.swatches();
                let name = move || t(i18n.locale.get(), theme.name_key());

                view! {
                    <button
                        class="theme-card"
                        class:theme-card-active=is_active
                        on:click=move |_| on_select(theme)
                    >
                        <div class="theme-preview" style=("--theme-bg", bg) style=("--theme-surface", surface) style=("--theme-accent", accent)>
                            <div class="theme-preview-bar" style:background=accent></div>
                            <div class="theme-preview-row">
                                <div class="theme-preview-dot" style:background=accent></div>
                                <div class="theme-preview-line" style:background=surface></div>
                            </div>
                            <div class="theme-preview-row">
                                <div class="theme-preview-dot" style:background=accent></div>
                                <div class="theme-preview-line" style:background=surface></div>
                            </div>
                        </div>
                        <span class="theme-name">{name}</span>
                        <Show when=is_active>
                            <span class="theme-badge">{move || t(i18n.locale.get(), Key::ThemeActive)}</span>
                        </Show>
                    </button>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}
