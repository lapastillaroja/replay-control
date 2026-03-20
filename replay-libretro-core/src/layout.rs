/// Adaptive layout configuration for CRT vs HDMI displays.
#[derive(Clone)]
pub struct LayoutConfig {
    pub width: u32,
    pub height: u32,
    /// Font scale for body text (multiplier on base 8x16 font)
    pub font_scale: u32,
    /// Font scale for the game title (larger)
    pub title_scale: u32,
    /// Font scale for labels / small metadata
    pub label_scale: u32,
    /// Horizontal safe area margin in pixels
    pub margin_x: u32,
    /// Vertical safe area margin in pixels
    pub margin_y: u32,
    /// Maximum description lines visible at once
    pub max_desc_lines: u32,
    /// Characters per line for word wrapping description (narrow column, next to box art)
    pub chars_per_line: u32,
    /// Characters per line for full-width description (Page 2, no box art column)
    pub full_chars_per_line: u32,
    /// Maximum description lines for full-width Page 2 (more vertical space)
    pub max_desc_lines_full: u32,
    /// Show extra metadata (publisher, region)
    pub show_extra_metadata: bool,
}

impl LayoutConfig {
    pub fn crt_320x240() -> Self {
        // ~3% margins: 10px horizontal, 8px vertical -- tighter for more content
        // full_chars_per_line: (320 - 2*10) / (1*9) = 33
        Self {
            width: 320,
            height: 240,
            font_scale: 1,
            title_scale: 1,
            label_scale: 1,
            margin_x: 10,
            margin_y: 8,
            max_desc_lines: 6,
            chars_per_line: 33,
            full_chars_per_line: 33,
            max_desc_lines_full: 8,
            show_extra_metadata: false,
        }
    }

    pub fn hdmi_720p() -> Self {
        // 3% margins: ~38px horizontal, ~22px vertical
        // full_chars_per_line: (1280 - 2*38) / (2*9) = 66
        Self {
            width: 1280,
            height: 720,
            font_scale: 2,
            title_scale: 3,
            label_scale: 1,
            margin_x: 38,
            margin_y: 22,
            max_desc_lines: 8,
            chars_per_line: 60,
            full_chars_per_line: 66,
            max_desc_lines_full: 12,
            show_extra_metadata: true,
        }
    }

    pub fn hdmi_1080p() -> Self {
        // full_chars_per_line: (1920 - 2*58) / (3*9) = 66
        Self {
            width: 1920,
            height: 1080,
            font_scale: 3,
            title_scale: 4,
            label_scale: 2,
            margin_x: 58,
            margin_y: 32,
            max_desc_lines: 10,
            chars_per_line: 70,
            full_chars_per_line: 66,
            max_desc_lines_full: 14,
            show_extra_metadata: true,
        }
    }

    pub fn crt_640x480() -> Self {
        // full_chars_per_line: (640 - 2*22) / (1*9) = 66
        Self {
            width: 640,
            height: 480,
            font_scale: 1,
            title_scale: 2,
            label_scale: 1,
            margin_x: 22,
            margin_y: 16,
            max_desc_lines: 6,
            chars_per_line: 50,
            full_chars_per_line: 66,
            max_desc_lines_full: 10,
            show_extra_metadata: false,
        }
    }

    /// Detect the best layout from /media/sd/config/replay.cfg.
    pub fn detect() -> Self {
        let config_path = "/media/sd/config/replay.cfg";
        let config_text = match std::fs::read_to_string(config_path) {
            Ok(text) => text,
            Err(_) => return Self::crt_320x240(), // safe fallback
        };

        let connector = parse_cfg_value(&config_text, "video_connector").unwrap_or("1");

        if connector == "0" {
            // HDMI -- video_mode=0 means DynaRes (adapts to core output).
            // Output 320x240 so RePlayOS upscales cleanly via DynaRes.
            let mode = parse_cfg_value(&config_text, "video_mode").unwrap_or("5");
            match mode {
                "0" => Self::crt_320x240(),        // DynaRes: core decides, use 320x240
                "1" => Self::crt_640x480(),         // low-res HDMI
                "2" | "3" | "4" | "5" => Self::hdmi_720p(),
                _ => Self::hdmi_1080p(),
            }
        } else {
            // DPI/GPIO = CRT
            let crt_type = parse_cfg_value(&config_text, "video_crt_type").unwrap_or("generic_15");
            if crt_type.contains("31") {
                Self::crt_640x480()
            } else {
                Self::crt_320x240()
            }
        }
    }

    /// Calculate box art display dimensions based on layout.
    pub fn box_art_dimensions(&self) -> (u32, u32) {
        match self.width {
            0..=320 => (100, 140),    // CRT 320x240: prominent box art
            321..=640 => (120, 165),   // CRT 640x480
            641..=1280 => (200, 275),  // 720p
            _ => (280, 385),           // 1080p+
        }
    }
}

/// The default layout, used for static initialization.
impl Default for LayoutConfig {
    fn default() -> Self {
        Self::crt_320x240()
    }
}

/// Parse a key=value from replay.cfg text.
/// Strips surrounding double quotes if present (e.g. `video_mode = "0"` -> `0`).
pub fn parse_cfg_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=') {
                let value = value.trim();
                // Strip surrounding double quotes if present
                let value = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(value);
                return Some(value);
            }
        }
    }
    None
}
