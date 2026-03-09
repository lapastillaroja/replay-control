/// Color palette for a ReplayOS skin, mapping to CSS custom properties.
#[derive(Debug, Clone)]
pub struct SkinPalette {
    pub bg: &'static str,
    pub surface: &'static str,
    pub surface_hover: &'static str,
    pub border: &'static str,
    pub text: &'static str,
    pub text_secondary: &'static str,
    pub accent: &'static str,
    pub accent_hover: &'static str,
}

/// Names of the 11 built-in skins (indices 0-10).
pub const SKIN_NAMES: [&str; 11] = [
    "REPLAY",
    "MEGA TECH",
    "PLAY CHOICE",
    "ASTRO",
    "SUPER VIDEO",
    "MVS",
    "RPG",
    "FANTASY",
    "SIMPLE PURPLE",
    "METAL",
    "UNICOLORS",
];

/// Built-in skin palettes, indexed 0-10.
///
/// Skin 0 (REPLAY) matches the default CSS `:root` values exactly.
/// Other palettes are derived from the ReplayOS skin images and the
/// aesthetic each skin represents.
const PALETTES: [SkinPalette; 11] = [
    // 0: REPLAY — default blue/indigo (matches current app CSS exactly)
    SkinPalette {
        bg: "#0f1115",
        surface: "#1a1d23",
        surface_hover: "#22262e",
        border: "#2a2e36",
        text: "#e4e6ea",
        text_secondary: "#8b8f96",
        accent: "#6366f1",
        accent_hover: "#818cf8",
    },
    // 1: MEGA TECH — Sega Mega Tech dark carbon with hot-pink selector
    SkinPalette {
        bg: "#1a1c1a",
        surface: "#252825",
        surface_hover: "#2e312e",
        border: "#3a3d3a",
        text: "#e2e4e2",
        text_secondary: "#848684",
        accent: "#ff004a",
        accent_hover: "#ff3370",
    },
    // 2: PLAY CHOICE — Nintendo green/teal with orange selector
    SkinPalette {
        bg: "#0a200a",
        surface: "#142e14",
        surface_hover: "#1c381c",
        border: "#1e421e",
        text: "#d8ecd8",
        text_secondary: "#7aaa7a",
        accent: "#ff4300",
        accent_hover: "#ff6b33",
    },
    // 3: ASTRO — Sega Astro City black with green accent
    SkinPalette {
        bg: "#080808",
        surface: "#141414",
        surface_hover: "#1c1c1c",
        border: "#262626",
        text: "#e0e8e0",
        text_secondary: "#7a8a7a",
        accent: "#00b543",
        accent_hover: "#33cc66",
    },
    // 4: SUPER VIDEO — black with blue accent, red selector
    SkinPalette {
        bg: "#08080c",
        surface: "#141420",
        surface_hover: "#1c1c2a",
        border: "#262638",
        text: "#e0e2ea",
        text_secondary: "#7a7e8e",
        accent: "#2f54a4",
        accent_hover: "#4a72c4",
    },
    // 5: MVS — SNK Neo Geo dark with red accent
    SkinPalette {
        bg: "#0f0f0f",
        surface: "#1a1a1a",
        surface_hover: "#242424",
        border: "#2e2e2e",
        text: "#e4e4e4",
        text_secondary: "#8a8a8a",
        accent: "#e00000",
        accent_hover: "#ff2222",
    },
    // 6: RPG — warm grey with brown/green tones
    SkinPalette {
        bg: "#1e1c1e",
        surface: "#2a282a",
        surface_hover: "#343234",
        border: "#3e3c3e",
        text: "#e4dcd4",
        text_secondary: "#9a8e82",
        accent: "#6daa2c",
        accent_hover: "#84c43e",
    },
    // 7: FANTASY — deep indigo/blue with pink accent
    SkinPalette {
        bg: "#06043a",
        surface: "#0e0c4e",
        surface_hover: "#161460",
        border: "#1e1a6e",
        text: "#e4eaf5",
        text_secondary: "#9a9ec8",
        accent: "#be1250",
        accent_hover: "#d83070",
    },
    // 8: SIMPLE PURPLE — minimal dark with purple accent
    SkinPalette {
        bg: "#111111",
        surface: "#1c1c1c",
        surface_hover: "#262626",
        border: "#303030",
        text: "#e8e8e8",
        text_secondary: "#909090",
        accent: "#7c3aed",
        accent_hover: "#9b5bff",
    },
    // 9: METAL — noir chrome, dark grey/silver
    SkinPalette {
        bg: "#0a0a0a",
        surface: "#161616",
        surface_hover: "#202020",
        border: "#2a2a2a",
        text: "#d0d0d0",
        text_secondary: "#777777",
        accent: "#7e2553",
        accent_hover: "#9e3a70",
    },
    // 10: UNICOLORS — black with gold accent
    SkinPalette {
        bg: "#0a0a08",
        surface: "#161614",
        surface_hover: "#201e1c",
        border: "#2c2a26",
        text: "#e4e2dc",
        text_secondary: "#a09460",
        accent: "#c8a848",
        accent_hover: "#dcc060",
    },
];

/// Look up the palette for a skin index.
///
/// Returns `None` for out-of-range indices (custom skin slots 11+ are not
/// supported yet).
pub fn palette(skin_index: u32) -> Option<&'static SkinPalette> {
    PALETTES.get(skin_index as usize)
}

/// Generate a CSS `<style>` block that overrides `:root` custom properties
/// for the given skin index.
///
/// Returns `None` for skin 0 (the default, which matches the static CSS)
/// or for out-of-range indices.
pub fn theme_css(skin_index: u32) -> Option<String> {
    if skin_index == 0 {
        return None;
    }
    let p = palette(skin_index)?;
    Some(format!(
        ":root{{\
--bg:{bg};\
--surface:{surface};\
--surface-hover:{surface_hover};\
--border:{border};\
--text:{text};\
--text-secondary:{text_secondary};\
--accent:{accent};\
--accent-hover:{accent_hover};\
}}",
        bg = p.bg,
        surface = p.surface,
        surface_hover = p.surface_hover,
        border = p.border,
        text = p.text,
        text_secondary = p.text_secondary,
        accent = p.accent,
        accent_hover = p.accent_hover,
    ))
}

/// Return the `--bg` color for a skin index (used for `<meta name="theme-color">`).
///
/// Falls back to the default skin 0 background for unknown indices.
pub fn theme_color(skin_index: u32) -> &'static str {
    palette(skin_index).map_or(PALETTES[0].bg, |p| p.bg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_skin_returns_no_css() {
        assert!(theme_css(0).is_none());
    }

    #[test]
    fn valid_skin_returns_css() {
        let css = theme_css(1).unwrap();
        assert!(css.contains("--bg:"));
        assert!(css.contains("#ff004a")); // MEGA TECH accent
    }

    #[test]
    fn out_of_range_returns_none() {
        assert!(theme_css(11).is_none());
        assert!(palette(99).is_none());
    }

    #[test]
    fn theme_color_default_fallback() {
        assert_eq!(theme_color(0), "#0f1115");
        assert_eq!(theme_color(99), "#0f1115");
    }

    #[test]
    fn all_palettes_have_valid_hex_colors() {
        for (i, p) in PALETTES.iter().enumerate() {
            for (name, color) in [
                ("bg", p.bg),
                ("surface", p.surface),
                ("surface_hover", p.surface_hover),
                ("border", p.border),
                ("text", p.text),
                ("text_secondary", p.text_secondary),
                ("accent", p.accent),
                ("accent_hover", p.accent_hover),
            ] {
                assert!(
                    color.starts_with('#') && (color.len() == 7 || color.len() == 4),
                    "Skin {i} ({}) has invalid {name} color: {color}",
                    SKIN_NAMES[i],
                );
            }
        }
    }

    #[test]
    fn skin_names_count_matches_palettes() {
        assert_eq!(SKIN_NAMES.len(), PALETTES.len());
    }
}
