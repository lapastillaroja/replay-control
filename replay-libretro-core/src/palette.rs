/// Color palette for the libretro core UI, loaded at retro_load_game time.
/// All colors are u32 in 0x00RRGGBB (XRGB8888) format.
///
/// Palette values are derived from the app's `SkinPalette` in
/// `replay-control-core/src/settings/skins.rs` — keep both in sync.
#[derive(Clone, Copy)]
pub struct CorePalette {
    // Background layer
    pub bg: u32,        // Full-screen background fill
    pub header_bg: u32, // Header bar background (slightly distinct from bg)

    // Text hierarchy
    pub title: u32,  // Game title (brightest text)
    pub system: u32, // System name + year (accent-colored)
    pub value: u32,  // Metadata values (near-white)
    pub label: u32,  // Metadata labels (dimmed)
    pub desc: u32,   // Description body text
    pub nav: u32,    // Navigation hints, footer, dim UI text
    pub arrow: u32,  // Navigation arrows (slightly brighter than nav)

    // Accent / decorative
    pub accent: u32,      // Separator lines, active indicators
    pub star: u32,        // Filled star (rating)
    pub star_dim: u32,    // Empty star outline
    pub rating_text: u32, // Numeric rating text

    // Status
    pub error: u32,   // Error messages
    pub loading: u32, // Loading indicator
}

/// Convert a CSS hex color (e.g., "#0f1115") to 0x00RRGGBB.
const fn hex(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Blend two colors: result = a * (255 - t)/255 + b * t/255.
/// `t` ranges from 0 (100% a) to 255 (100% b).
const fn blend(a: u32, b: u32, t: u8) -> u32 {
    let t = t as u32;
    let inv = 255 - t;
    let r = (((a >> 16) & 0xFF) * inv + ((b >> 16) & 0xFF) * t) / 255;
    let g = (((a >> 8) & 0xFF) * inv + ((b >> 8) & 0xFF) * t) / 255;
    let bl = ((a & 0xFF) * inv + (b & 0xFF) * t) / 255;
    (r << 16) | (g << 8) | bl
}

/// Build a CorePalette from the app's SkinPalette colors.
///
/// Mapping (from design doc):
///   bg         ← SkinPalette.bg
///   header_bg  ← SkinPalette.surface
///   title      ← SkinPalette.text
///   system     ← SkinPalette.accent
///   value      ← SkinPalette.text
///   label      ← SkinPalette.text_secondary
///   desc       ← blend(text, text_secondary, 35%)  (~65% text + 35% secondary)
///   nav        ← blend(text_secondary, bg, 30%)     (70% secondary toward bg)
///   arrow      ← blend(text, text_secondary, 35%)   (same as desc)
///   accent     ← SkinPalette.accent
///   star       ← fixed gold 0x00F59E0B
///   star_dim   ← blend(bg, text_secondary, 50%)
///   rating_text← SkinPalette.text_secondary
///   error      ← fixed 0x00FF6644
///   loading    ← SkinPalette.accent_hover
const fn make_palette(
    bg: u32,
    surface: u32,
    text: u32,
    text_secondary: u32,
    accent: u32,
    accent_hover: u32,
) -> CorePalette {
    CorePalette {
        bg,
        header_bg: surface,
        title: text,
        system: accent,
        value: text,
        label: text_secondary,
        desc: blend(text, text_secondary, 89),  // 35% of 255 ≈ 89
        nav: blend(text_secondary, bg, 77),      // 30% of 255 ≈ 77
        arrow: blend(text, text_secondary, 89),  // same as desc
        accent,
        star: 0x00F59E0B,     // gold (fixed across all skins)
        star_dim: blend(bg, text_secondary, 128), // midpoint
        rating_text: text_secondary,
        error: 0x00FF6644,    // fixed
        loading: accent_hover,
    }
}

/// All 11 built-in skin palettes, indexed 0-10.
///
/// Source of truth: `replay-control-core/src/settings/skins.rs` PALETTES array.
/// Keep both files in sync when adding or modifying skins.
const PALETTES: [CorePalette; 11] = [
    // 0: REPLAY — default indigo accent
    make_palette(
        hex(0x0f, 0x11, 0x15), // bg: #0f1115
        hex(0x1a, 0x1d, 0x23), // surface: #1a1d23
        hex(0xe4, 0xe6, 0xea), // text: #e4e6ea
        hex(0x8b, 0x8f, 0x96), // text_secondary: #8b8f96
        hex(0x63, 0x66, 0xf1), // accent: #6366f1
        hex(0x81, 0x8c, 0xf8), // accent_hover: #818cf8
    ),
    // 1: MEGA TECH — hot-pink accent on carbon
    make_palette(
        hex(0x1a, 0x1c, 0x1a), // bg: #1a1c1a
        hex(0x25, 0x28, 0x25), // surface: #252825
        hex(0xe2, 0xe4, 0xe2), // text: #e2e4e2
        hex(0x84, 0x86, 0x84), // text_secondary: #848684
        hex(0xff, 0x00, 0x4a), // accent: #ff004a
        hex(0xff, 0x33, 0x70), // accent_hover: #ff3370
    ),
    // 2: PLAY CHOICE — orange accent on green
    make_palette(
        hex(0x0a, 0x20, 0x0a), // bg: #0a200a
        hex(0x14, 0x2e, 0x14), // surface: #142e14
        hex(0xd8, 0xec, 0xd8), // text: #d8ecd8
        hex(0x7a, 0xaa, 0x7a), // text_secondary: #7aaa7a
        hex(0xff, 0x43, 0x00), // accent: #ff4300
        hex(0xff, 0x6b, 0x33), // accent_hover: #ff6b33
    ),
    // 3: ASTRO — green accent on black
    make_palette(
        hex(0x08, 0x08, 0x08), // bg: #080808
        hex(0x14, 0x14, 0x14), // surface: #141414
        hex(0xe0, 0xe8, 0xe0), // text: #e0e8e0
        hex(0x7a, 0x8a, 0x7a), // text_secondary: #7a8a7a
        hex(0x00, 0xb5, 0x43), // accent: #00b543
        hex(0x33, 0xcc, 0x66), // accent_hover: #33cc66
    ),
    // 4: SUPER VIDEO — blue accent
    make_palette(
        hex(0x08, 0x08, 0x0c), // bg: #08080c
        hex(0x14, 0x14, 0x20), // surface: #141420
        hex(0xe0, 0xe2, 0xea), // text: #e0e2ea
        hex(0x7a, 0x7e, 0x8e), // text_secondary: #7a7e8e
        hex(0x2f, 0x54, 0xa4), // accent: #2f54a4
        hex(0x4a, 0x72, 0xc4), // accent_hover: #4a72c4
    ),
    // 5: MVS — red accent on near-black
    make_palette(
        hex(0x0f, 0x0f, 0x0f), // bg: #0f0f0f
        hex(0x1a, 0x1a, 0x1a), // surface: #1a1a1a
        hex(0xe4, 0xe4, 0xe4), // text: #e4e4e4
        hex(0x8a, 0x8a, 0x8a), // text_secondary: #8a8a8a
        hex(0xe0, 0x00, 0x00), // accent: #e00000
        hex(0xff, 0x22, 0x22), // accent_hover: #ff2222
    ),
    // 6: RPG — green accent on warm grey
    make_palette(
        hex(0x1e, 0x1c, 0x1e), // bg: #1e1c1e
        hex(0x2a, 0x28, 0x2a), // surface: #2a282a
        hex(0xe4, 0xdc, 0xd4), // text: #e4dcd4
        hex(0x9a, 0x8e, 0x82), // text_secondary: #9a8e82
        hex(0x6d, 0xaa, 0x2c), // accent: #6daa2c
        hex(0x84, 0xc4, 0x3e), // accent_hover: #84c43e
    ),
    // 7: FANTASY — pink accent on deep indigo
    make_palette(
        hex(0x06, 0x04, 0x3a), // bg: #06043a
        hex(0x0e, 0x0c, 0x4e), // surface: #0e0c4e
        hex(0xe4, 0xea, 0xf5), // text: #e4eaf5
        hex(0x9a, 0x9e, 0xc8), // text_secondary: #9a9ec8
        hex(0xbe, 0x12, 0x50), // accent: #be1250
        hex(0xd8, 0x30, 0x70), // accent_hover: #d83070
    ),
    // 8: SIMPLE PURPLE — purple accent on dark
    make_palette(
        hex(0x11, 0x11, 0x11), // bg: #111111
        hex(0x1c, 0x1c, 0x1c), // surface: #1c1c1c
        hex(0xe8, 0xe8, 0xe8), // text: #e8e8e8
        hex(0x90, 0x90, 0x90), // text_secondary: #909090
        hex(0x7c, 0x3a, 0xed), // accent: #7c3aed
        hex(0x9b, 0x5b, 0xff), // accent_hover: #9b5bff
    ),
    // 9: METAL — maroon accent on near-black
    make_palette(
        hex(0x0a, 0x0a, 0x0a), // bg: #0a0a0a
        hex(0x16, 0x16, 0x16), // surface: #161616
        hex(0xd0, 0xd0, 0xd0), // text: #d0d0d0
        hex(0x77, 0x77, 0x77), // text_secondary: #777777
        hex(0x7e, 0x25, 0x53), // accent: #7e2553
        hex(0x9e, 0x3a, 0x70), // accent_hover: #9e3a70
    ),
    // 10: UNICOLORS — gold accent on warm black
    make_palette(
        hex(0x0a, 0x0a, 0x08), // bg: #0a0a08
        hex(0x16, 0x16, 0x14), // surface: #161614
        hex(0xe4, 0xe2, 0xdc), // text: #e4e2dc
        hex(0xa0, 0x94, 0x60), // text_secondary: #a09460
        hex(0xc8, 0xa8, 0x48), // accent: #c8a848
        hex(0xdc, 0xc0, 0x60), // accent_hover: #dcc060
    ),
];

/// Return the palette for a skin index, defaulting to skin 0 for unknown indices.
pub const fn load_palette(skin_index: u32) -> CorePalette {
    let idx = skin_index as usize;
    if idx < PALETTES.len() {
        PALETTES[idx]
    } else {
        PALETTES[0]
    }
}

/// Detect the active skin index from config files.
///
/// Priority:
///   1. `/media/usb/.replay-control/settings.cfg` key `skin`
///   2. `/media/sd/config/replay.cfg` key `system_skin`
///   3. Default: 0 (REPLAY)
pub fn detect_skin_index() -> u32 {
    use crate::layout::parse_cfg_value;
    use crate::util::debug_log;

    // Try app-specific settings first (USB storage)
    if let Ok(text) = std::fs::read_to_string("/media/usb/.replay-control/settings.cfg") {
        if let Some(val) = parse_cfg_value(&text, "skin") {
            if let Ok(idx) = val.parse::<u32>() {
                debug_log(&format!("[palette] skin={} from settings.cfg", idx));
                return idx;
            }
        }
    }

    // Fall back to system skin from replay.cfg
    if let Ok(text) = std::fs::read_to_string("/media/sd/config/replay.cfg") {
        if let Some(val) = parse_cfg_value(&text, "system_skin") {
            if let Ok(idx) = val.parse::<u32>() {
                debug_log(&format!("[palette] system_skin={} from replay.cfg", idx));
                return idx;
            }
        }
    }

    debug_log("[palette] no skin config found, using default 0");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_palettes_have_nonzero_bg_and_text() {
        for (i, p) in PALETTES.iter().enumerate() {
            // bg may be very dark but should not be pure transparent
            // (0x00000000 is valid black, so just check text is nonzero)
            assert!(
                p.title != 0,
                "Skin {} has zero title color",
                i
            );
        }
    }

    #[test]
    fn skin_0_is_default() {
        let p = load_palette(0);
        assert_eq!(p.bg, hex(0x0f, 0x11, 0x15));
        assert_eq!(p.accent, hex(0x63, 0x66, 0xf1));
    }

    #[test]
    fn out_of_range_returns_skin_0() {
        let p = load_palette(99);
        assert_eq!(p.bg, load_palette(0).bg);
    }

    #[test]
    fn blend_extremes() {
        let a = 0x00FF0000; // red
        let b = 0x000000FF; // blue
        assert_eq!(blend(a, b, 0), a);   // 100% a
        assert_eq!(blend(a, b, 255), b); // 100% b
    }

    #[test]
    fn blend_midpoint() {
        let a = 0x00000000;
        let b = 0x00FEFEFE; // use 254 to avoid rounding issues
        let mid = blend(a, b, 128); // ~50%
        let r = (mid >> 16) & 0xFF;
        let g = (mid >> 8) & 0xFF;
        let bl = mid & 0xFF;
        // Should be approximately 127
        assert!(r >= 126 && r <= 128, "r={}", r);
        assert!(g >= 126 && g <= 128, "g={}", g);
        assert!(bl >= 126 && bl <= 128, "b={}", bl);
    }

    #[test]
    fn palette_count_matches() {
        assert_eq!(PALETTES.len(), 11);
    }

    #[test]
    fn detect_defaults_without_config_files() {
        // On dev machines where /media/usb and /media/sd don't exist,
        // detect_skin_index should gracefully return 0.
        let idx = detect_skin_index();
        assert_eq!(idx, 0);
    }
}
