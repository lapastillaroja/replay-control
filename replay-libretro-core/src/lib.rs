//! Recently Played Game Detail Viewer — a libretro core for RePlayOS.
//!
//! Fetches recently played games from the Replay Control REST API (localhost:8080)
//! and displays rich game metadata on the TV screen, navigable with a gamepad.
//!
//! Controls:
//!   - D-pad Left/Right: navigate between recently played games
//!   - D-pad Up/Down: scroll description text
//!   - B button: exit core (RETRO_ENVIRONMENT_SHUTDOWN)
//!   - Start: toggle between recently played and favorites
//!
//! Display: adapts layout based on CRT (320x240) vs HDMI (720p) via replay.cfg.
//!
//! Target: `cdylib` (.so) for use with RePlayOS custom frontend on aarch64.

#![allow(clippy::missing_safety_doc)]

// Force the system allocator (libc malloc/free) instead of Rust's default.
// When this .so is dlopen'd by the RePlayOS replay binary (which uses a C
// allocator), mixed Rust-allocator/C-allocator usage across the dlopen
// boundary corrupts the heap. Using the system allocator ensures all
// allocations go through the same malloc/free as the host process.
#[global_allocator]
static ALLOC: std::alloc::System = std::alloc::System;

use std::cell::UnsafeCell;
use std::io::Write;
use std::os::raw::{c_char, c_uint, c_void};

// ─── Libretro constants ────────────────────────────────────────────────────

const RETRO_API_VERSION: c_uint = 1;
#[allow(dead_code)]
const RETRO_PIXEL_FORMAT_RGB565: c_uint = 2;
const RETRO_DEVICE_JOYPAD: c_uint = 1;

const RETRO_DEVICE_ID_JOYPAD_B: c_uint = 0;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_Y: c_uint = 1;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_SELECT: c_uint = 2;
const RETRO_DEVICE_ID_JOYPAD_START: c_uint = 3;
const RETRO_DEVICE_ID_JOYPAD_UP: c_uint = 4;
const RETRO_DEVICE_ID_JOYPAD_DOWN: c_uint = 5;
const RETRO_DEVICE_ID_JOYPAD_LEFT: c_uint = 6;
const RETRO_DEVICE_ID_JOYPAD_RIGHT: c_uint = 7;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_A: c_uint = 8;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_X: c_uint = 9;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_L: c_uint = 10;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_R: c_uint = 11;

const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 16;

const RETRO_REGION_NTSC: c_uint = 0;

// ─── Libretro structs ──────────────────────────────────────────────────────

#[repr(C)]
pub struct RetroSystemInfo {
    library_name: *const c_char,
    library_version: *const c_char,
    valid_extensions: *const c_char,
    need_fullpath: bool,
    block_extract: bool,
}

#[repr(C)]
pub struct RetroGameGeometry {
    base_width: c_uint,
    base_height: c_uint,
    max_width: c_uint,
    max_height: c_uint,
    aspect_ratio: f32,
}

#[repr(C)]
pub struct RetroSystemTiming {
    fps: f64,
    sample_rate: f64,
}

#[repr(C)]
pub struct RetroSystemAvInfo {
    geometry: RetroGameGeometry,
    timing: RetroSystemTiming,
}

#[repr(C)]
pub struct RetroGameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}

// ─── Callback function pointer types ───────────────────────────────────────

type RetroEnvironmentFn = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
type RetroVideoRefreshFn =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
type RetroAudioSampleFn = unsafe extern "C" fn(left: i16, right: i16);
type RetroAudioSampleBatchFn = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
type RetroInputPollFn = unsafe extern "C" fn();
type RetroInputStateFn =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;

// ─── Layout configuration ──────────────────────────────────────────────────

/// Adaptive layout configuration for CRT vs HDMI displays.
#[derive(Clone)]
struct LayoutConfig {
    width: u32,
    height: u32,
    /// Font scale for body text (multiplier on base 8x16 font)
    font_scale: u32,
    /// Font scale for the game title (larger)
    title_scale: u32,
    /// Font scale for labels / small metadata
    label_scale: u32,
    /// Horizontal safe area margin in pixels
    margin_x: u32,
    /// Vertical safe area margin in pixels
    margin_y: u32,
    /// Maximum description lines visible at once
    max_desc_lines: u32,
    /// Characters per line for word wrapping description
    chars_per_line: u32,
    /// Show extra metadata (publisher, region)
    show_extra_metadata: bool,
}

impl LayoutConfig {
    fn crt_320x240() -> Self {
        // ~3% margins: 10px horizontal, 8px vertical — tighter for more content
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
            show_extra_metadata: false,
        }
    }

    fn hdmi_720p() -> Self {
        // 3% margins: ~38px horizontal, ~22px vertical
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
            show_extra_metadata: true,
        }
    }

    fn hdmi_1080p() -> Self {
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
            show_extra_metadata: true,
        }
    }

    fn crt_640x480() -> Self {
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
            show_extra_metadata: false,
        }
    }

    /// Detect the best layout from /media/sd/config/replay.cfg.
    fn detect() -> Self {
        let config_path = "/media/sd/config/replay.cfg";
        let config_text = match std::fs::read_to_string(config_path) {
            Ok(text) => text,
            Err(_) => return Self::crt_320x240(), // safe fallback
        };

        let connector = parse_cfg_value(&config_text, "video_connector").unwrap_or("1");

        if connector == "0" {
            // HDMI — video_mode=0 means DynaRes (adapts to core output).
            // Output 320x240 so RePlayOS upscales cleanly via DynaRes.
            let mode = parse_cfg_value(&config_text, "video_mode").unwrap_or("5");
            match mode {
                "0" => Self::crt_320x240(),       // DynaRes: core decides, use 320x240
                "1" => Self::crt_640x480(),        // low-res HDMI
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
}

/// Parse a key=value from replay.cfg text.
/// Strips surrounding double quotes if present (e.g. `video_mode = "0"` → `0`).
fn parse_cfg_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
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

// ─── Game data structures ──────────────────────────────────────────────────

/// Which list we're currently viewing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMode {
    Recents,
    Favorites,
}

/// A game entry in our list (from recents or favorites).
#[derive(Clone)]
struct GameEntry {
    system: String,
    system_display: String,
    rom_filename: String,
    display_name: String,
    box_art_url: Option<String>,
}

/// Decoded box art image (XRGB8888 pixels, already scaled to display size).
#[derive(Clone)]
struct BoxArtImage {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

/// Pre-computed display strings for a game, formatted once during prefetch.
/// This avoids all heap allocations during rendering (retro_run).
#[derive(Clone)]
struct PrecomputedDisplay {
    /// "System Display  -  Year" or just "System Display"
    sys_year_line: String,
    /// "1 Player" or "1-N Players" or "" if unknown
    players_text: String,
    /// Pre-formatted rating text (e.g., "3.5") or empty if no rating
    rating_text: String,
    /// Word-wrapped description lines (empty vec if no description)
    desc_lines: Vec<String>,
}

/// Detailed metadata for the currently focused game.
#[derive(Clone)]
struct GameDetail {
    display_name: String,
    system_display: String,
    year: String,
    developer: String,
    genre: String,
    players: u8,
    rating: Option<f32>,
    description: Option<String>,
    publisher: Option<String>,
    region: Option<String>,
    box_art: Option<BoxArtImage>,
    /// Pre-formatted strings for allocation-free rendering.
    display: PrecomputedDisplay,
}

// ─── Pixel format tracking ────────────────────────────────────────────────

/// Which pixel format to use for video output.
///
/// RePlayOS accepts SET_PIXEL_FORMAT(XRGB8888). Using 32-bit direct output
/// avoids the per-pixel BGR565 conversion and gives correct colors natively.
///
/// Internal rendering uses u32 (0x00RRGGBB) which IS XRGB8888 — no conversion needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelFormat {
    /// BGR565 (16-bit): BBBBBGGGGGGRRRRR — fallback format
    #[allow(dead_code)]
    Bgr565,
    /// XRGB8888 (32-bit) — direct output, no conversion needed
    Xrgb8888,
}

// ─── Core state ────────────────────────────────────────────────────────────

struct CoreState {
    // Libretro callbacks
    environment_cb: Option<RetroEnvironmentFn>,
    video_cb: Option<RetroVideoRefreshFn>,
    #[allow(dead_code)]
    audio_sample_cb: Option<RetroAudioSampleFn>,
    audio_batch_cb: Option<RetroAudioSampleBatchFn>,
    input_poll_cb: Option<RetroInputPollFn>,
    input_state_cb: Option<RetroInputStateFn>,

    // Display
    framebuffer: Vec<u32>,
    /// Conversion buffer for 16-bit pixel formats (0RGB1555 / RGB565)
    framebuffer_16: Vec<u16>,
    pixel_format: PixelFormat,
    frame_count: u64,
    layout: LayoutConfig,

    // Game data
    list_mode: ListMode,
    entries: Vec<GameEntry>,
    favorites: Vec<GameEntry>,
    current_index: usize,
    detail_cache: Vec<Option<GameDetail>>,
    fav_detail_cache: Vec<Option<GameDetail>>,
    desc_scroll: usize,

    // Network state
    api_available: bool,
    status_message: String,
    loading: bool,

    // Input debounce
    prev_left: bool,
    prev_right: bool,
    prev_b: bool,
    prev_start: bool,
    // Up/down are held (continuous scroll), but with frame delay
    scroll_cooldown: u32,

    // Pre-allocated scratch buffers for allocation-free rendering.
    // These are written once per navigation event, then read every frame.
    /// "RECENTLY PLAYED  (1/93)" or "FAVORITES  (2/10)"
    header_text: String,
    /// "[1/5]" scroll position indicator
    scroll_indicator: String,
}

unsafe impl Sync for CoreStateWrapper {}
struct CoreStateWrapper(UnsafeCell<CoreState>);

static STATE: CoreStateWrapper = CoreStateWrapper(UnsafeCell::new(CoreState {
    environment_cb: None,
    video_cb: None,
    audio_sample_cb: None,
    audio_batch_cb: None,
    input_poll_cb: None,
    input_state_cb: None,
    framebuffer: Vec::new(),
    framebuffer_16: Vec::new(),
    pixel_format: PixelFormat::Xrgb8888,
    frame_count: 0,
    layout: LayoutConfig {
        width: 320,
        height: 240,
        font_scale: 1,
        title_scale: 2,
        label_scale: 1,
        margin_x: 10,
        margin_y: 8,
        max_desc_lines: 6,
        chars_per_line: 33,
        show_extra_metadata: false,
    },
    list_mode: ListMode::Recents,
    entries: Vec::new(),
    favorites: Vec::new(),
    current_index: 0,
    detail_cache: Vec::new(),
    fav_detail_cache: Vec::new(),
    desc_scroll: 0,
    api_available: false,
    status_message: String::new(),
    loading: false,
    prev_left: false,
    prev_right: false,
    prev_b: false,
    prev_start: false,
    scroll_cooldown: 0,
    header_text: String::new(),
    scroll_indicator: String::new(),
}));

#[inline(always)]
unsafe fn state() -> &'static mut CoreState {
    &mut *STATE.0.get()
}

// ─── Embedded 8x16 bitmap font (CP437 style) ──────────────────────────────
//
// Each character is 8 pixels wide, 16 pixels tall.
// Stored as 16 bytes per character, one byte per row, MSB = leftmost pixel.
// We only define printable ASCII (32-127). Unknown chars get a filled block.

/// Get the 8x16 bitmap for a character. Returns 16 bytes, one per row.
fn get_char_bitmap_8x16(ch: u8) -> [u8; 16] {
    match ch {
        b' ' => [0x00; 16],
        b'!' => [
            0x00, 0x00, 0x18, 0x3C, 0x3C, 0x3C, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'"' => [
            0x00, 0x66, 0x66, 0x66, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'#' => [
            0x00, 0x00, 0x00, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'$' => [
            0x18, 0x18, 0x7C, 0xC6, 0xC2, 0xC0, 0x7C, 0x06, 0x06, 0x86, 0xC6, 0x7C, 0x18, 0x18,
            0x00, 0x00,
        ],
        b'%' => [
            0x00, 0x00, 0x00, 0x00, 0xC2, 0xC6, 0x0C, 0x18, 0x30, 0x60, 0xC6, 0x86, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'&' => [
            0x00, 0x00, 0x38, 0x6C, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'\'' => [
            0x00, 0x30, 0x30, 0x30, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'(' => [
            0x00, 0x00, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b')' => [
            0x00, 0x00, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'*' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'+' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b',' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x18, 0x30, 0x00,
            0x00, 0x00,
        ],
        b'-' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'.' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'/' => [
            0x00, 0x00, 0x00, 0x00, 0x02, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00, 0x00,
            0x00, 0x00,
        ],
        // Digits 0-9
        b'0' => [
            0x00, 0x00, 0x38, 0x6C, 0xC6, 0xC6, 0xD6, 0xD6, 0xC6, 0xC6, 0x6C, 0x38, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'1' => [
            0x00, 0x00, 0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'2' => [
            0x00, 0x00, 0x7C, 0xC6, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xC6, 0xFE, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'3' => [
            0x00, 0x00, 0x7C, 0xC6, 0x06, 0x06, 0x3C, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'4' => [
            0x00, 0x00, 0x0C, 0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x0C, 0x0C, 0x1E, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'5' => [
            0x00, 0x00, 0xFE, 0xC0, 0xC0, 0xC0, 0xFC, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'6' => [
            0x00, 0x00, 0x38, 0x60, 0xC0, 0xC0, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'7' => [
            0x00, 0x00, 0xFE, 0xC6, 0x06, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'8' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'9' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0x06, 0x06, 0x0C, 0x78, 0x00, 0x00,
            0x00, 0x00,
        ],
        b':' => [
            0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b';' => [
            0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'<' => [
            0x00, 0x00, 0x00, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'=' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'>' => [
            0x00, 0x00, 0x00, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'?' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0x0C, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'@' => [
            0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xDE, 0xDE, 0xDE, 0xDC, 0xC0, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        // Uppercase A-Z
        b'A' => [
            0x00, 0x00, 0x10, 0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'B' => [
            0x00, 0x00, 0xFC, 0x66, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x66, 0x66, 0xFC, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'C' => [
            0x00, 0x00, 0x3C, 0x66, 0xC2, 0xC0, 0xC0, 0xC0, 0xC0, 0xC2, 0x66, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'D' => [
            0x00, 0x00, 0xF8, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'E' => [
            0x00, 0x00, 0xFE, 0x66, 0x62, 0x68, 0x78, 0x68, 0x60, 0x62, 0x66, 0xFE, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'F' => [
            0x00, 0x00, 0xFE, 0x66, 0x62, 0x68, 0x78, 0x68, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'G' => [
            0x00, 0x00, 0x3C, 0x66, 0xC2, 0xC0, 0xC0, 0xDE, 0xC6, 0xC6, 0x66, 0x3A, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'H' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'I' => [
            0x00, 0x00, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'J' => [
            0x00, 0x00, 0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0xCC, 0x78, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'K' => [
            0x00, 0x00, 0xE6, 0x66, 0x66, 0x6C, 0x78, 0x78, 0x6C, 0x66, 0x66, 0xE6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'L' => [
            0x00, 0x00, 0xF0, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x62, 0x66, 0xFE, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'M' => [
            0x00, 0x00, 0xC6, 0xEE, 0xFE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'N' => [
            0x00, 0x00, 0xC6, 0xE6, 0xF6, 0xFE, 0xDE, 0xCE, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'O' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'P' => [
            0x00, 0x00, 0xFC, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'Q' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x0C, 0x0E,
            0x00, 0x00,
        ],
        b'R' => [
            0x00, 0x00, 0xFC, 0x66, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x66, 0xE6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'S' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0x60, 0x38, 0x0C, 0x06, 0xC6, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'T' => [
            0x00, 0x00, 0xFF, 0xDB, 0x99, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'U' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'V' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'W' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xD6, 0xD6, 0xFE, 0xEE, 0x6C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'X' => [
            0x00, 0x00, 0xC6, 0xC6, 0x6C, 0x7C, 0x38, 0x38, 0x7C, 0x6C, 0xC6, 0xC6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'Y' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'Z' => [
            0x00, 0x00, 0xFE, 0xC6, 0x86, 0x0C, 0x18, 0x30, 0x60, 0xC2, 0xC6, 0xFE, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'[' => [
            0x00, 0x00, 0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'\\' => [
            0x00, 0x00, 0x00, 0x80, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b']' => [
            0x00, 0x00, 0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'^' => [
            0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'_' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x00,
            0x00, 0x00,
        ],
        b'`' => [
            0x30, 0x30, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        // Lowercase a-z
        b'a' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'b' => [
            0x00, 0x00, 0xE0, 0x60, 0x60, 0x78, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'c' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'd' => [
            0x00, 0x00, 0x1C, 0x0C, 0x0C, 0x3C, 0x6C, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'e' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'f' => [
            0x00, 0x00, 0x1C, 0x36, 0x32, 0x30, 0x78, 0x30, 0x30, 0x30, 0x30, 0x78, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'g' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x76, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x7C, 0x0C, 0xCC,
            0x78, 0x00,
        ],
        b'h' => [
            0x00, 0x00, 0xE0, 0x60, 0x60, 0x6C, 0x76, 0x66, 0x66, 0x66, 0x66, 0xE6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'i' => [
            0x00, 0x00, 0x18, 0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'j' => [
            0x00, 0x00, 0x06, 0x06, 0x00, 0x0E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x66, 0x66,
            0x3C, 0x00,
        ],
        b'k' => [
            0x00, 0x00, 0xE0, 0x60, 0x60, 0x66, 0x6C, 0x78, 0x78, 0x6C, 0x66, 0xE6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'l' => [
            0x00, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'm' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xE6, 0xFF, 0xDB, 0xDB, 0xDB, 0xDB, 0xDB, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'n' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'o' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'p' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60,
            0xF0, 0x00,
        ],
        b'q' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x76, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x7C, 0x0C, 0x0C,
            0x1E, 0x00,
        ],
        b'r' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xDC, 0x76, 0x66, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00,
            0x00, 0x00,
        ],
        b's' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0x60, 0x38, 0x0C, 0xC6, 0x7C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b't' => [
            0x00, 0x00, 0x10, 0x30, 0x30, 0xFC, 0x30, 0x30, 0x30, 0x30, 0x36, 0x1C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'u' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'v' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'w' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xD6, 0xD6, 0xFE, 0x6C, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'x' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0x6C, 0x38, 0x38, 0x38, 0x6C, 0xC6, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'y' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0x0C,
            0xF8, 0x00,
        ],
        b'z' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xCC, 0x18, 0x30, 0x60, 0xC6, 0xFE, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'{' => [
            0x00, 0x00, 0x0E, 0x18, 0x18, 0x18, 0x70, 0x18, 0x18, 0x18, 0x18, 0x0E, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'|' => [
            0x00, 0x00, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'}' => [
            0x00, 0x00, 0x70, 0x18, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x18, 0x18, 0x70, 0x00, 0x00,
            0x00, 0x00,
        ],
        b'~' => [
            0x00, 0x00, 0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ],
        // Default: filled block for unknown characters
        _ => [0xFF; 16],
    }
}

// ─── Drawing helpers (resolution-independent) ──────────────────────────────

/// Draw a single 8x16 character at (x, y) with the given color and scale.
fn draw_char(fb: &mut [u32], w: u32, h: u32, ch: u8, x: i32, y: i32, color: u32, scale: u32) {
    let bitmap = get_char_bitmap_8x16(ch);
    let w = w as i32;
    let h = h as i32;
    let scale = scale as i32;
    for (row, &bits) in bitmap.iter().enumerate() {
        let row = row as i32;
        for col in 0..8i32 {
            if bits & (1 << (7 - col)) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row * scale + sy;
                        if px >= 0 && px < w && py >= 0 && py < h {
                            fb[(py * w + px) as usize] = color;
                        }
                    }
                }
            }
        }
    }
}

/// Draw a string at (x, y). Returns the x position after the last character.
fn draw_string(
    fb: &mut [u32],
    w: u32,
    h: u32,
    text: &str,
    x: i32,
    y: i32,
    color: u32,
    scale: u32,
) -> i32 {
    let char_width = 9 * scale as i32; // 8 pixels + 1 gap, scaled
    let mut cx = x;
    for ch in text.bytes() {
        if cx >= w as i32 {
            break;
        }
        draw_char(fb, w, h, ch, cx, y, color, scale);
        cx += char_width;
    }
    cx
}

/// Draw a string truncated to max_chars, adding ".." if truncated.
fn draw_string_truncated(
    fb: &mut [u32],
    w: u32,
    h: u32,
    text: &str,
    x: i32,
    y: i32,
    color: u32,
    scale: u32,
    max_chars: usize,
) {
    if text.len() <= max_chars {
        draw_string(fb, w, h, text, x, y, color, scale);
    } else {
        let truncated = &text[..max_chars.saturating_sub(2)];
        let end_x = draw_string(fb, w, h, truncated, x, y, color, scale);
        draw_string(fb, w, h, "..", end_x, y, color, scale);
    }
}

/// Fill a rectangle.
fn draw_rect(fb: &mut [u32], w: u32, h: u32, rx: i32, ry: i32, rw: u32, rh: u32, color: u32) {
    let w = w as i32;
    let h = h as i32;
    for row in ry.max(0)..(ry + rh as i32).min(h) {
        for col in rx.max(0)..(rx + rw as i32).min(w) {
            fb[(row * w + col) as usize] = color;
        }
    }
}

/// Draw a horizontal line.
fn draw_hline(fb: &mut [u32], w: u32, h: u32, x: i32, y: i32, len: u32, color: u32) {
    if y < 0 || y >= h as i32 {
        return;
    }
    let w_i = w as i32;
    for col in x.max(0)..(x + len as i32).min(w_i) {
        fb[(y * w_i + col) as usize] = color;
    }
}

/// Blit an image onto the framebuffer at position (dx, dy).
/// Clips to framebuffer bounds.
fn blit_image(
    fb: &mut [u32],
    fb_w: u32,
    fb_h: u32,
    pixels: &[u32],
    img_w: u32,
    img_h: u32,
    dx: u32,
    dy: u32,
) {
    for row in 0..img_h {
        let fb_y = dy + row;
        if fb_y >= fb_h {
            break;
        }
        let copy_w = img_w.min(fb_w.saturating_sub(dx));
        if copy_w == 0 {
            continue;
        }
        let src_offset = (row * img_w) as usize;
        let dst_offset = (fb_y * fb_w + dx) as usize;
        fb[dst_offset..dst_offset + copy_w as usize]
            .copy_from_slice(&pixels[src_offset..src_offset + copy_w as usize]);
    }
}

/// Word-wrap text into lines of at most `max_chars` characters.
fn word_wrap(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current_line = String::new();
        for word in words {
            if current_line.is_empty() {
                if word.len() > max_chars {
                    // Break long word
                    let mut start = 0;
                    while start < word.len() {
                        let end = (start + max_chars).min(word.len());
                        lines.push(word[start..end].to_string());
                        start = end;
                    }
                } else {
                    current_line = word.to_string();
                }
            } else if current_line.len() + 1 + word.len() <= max_chars {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(current_line);
                current_line = word.to_string();
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }
    lines
}

/// Draw a filled star (rating indicator).
fn draw_star_filled(fb: &mut [u32], w: u32, h: u32, cx: i32, cy: i32, size: i32, color: u32) {
    // Simple diamond-ish star approximation that works at small sizes
    let w_i = w as i32;
    let h_i = h as i32;
    for dy in -size..=size {
        let spread = size - dy.abs();
        for dx in -spread..=spread {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && px < w_i && py >= 0 && py < h_i {
                fb[(py * w_i + px) as usize] = color;
            }
        }
    }
}

/// Draw a hollow star outline.
fn draw_star_empty(fb: &mut [u32], w: u32, h: u32, cx: i32, cy: i32, size: i32, color: u32) {
    let w_i = w as i32;
    let h_i = h as i32;
    for dy in -size..=size {
        let spread = size - dy.abs();
        for dx in [-spread, spread] {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && px < w_i && py >= 0 && py < h_i {
                fb[(py * w_i + px) as usize] = color;
            }
        }
    }
    // Top and bottom points
    for dx in -1..=1 {
        let px = cx + dx;
        let py_top = cy - size;
        let py_bot = cy + size;
        if px >= 0 && px < w_i {
            if py_top >= 0 && py_top < h_i {
                fb[(py_top * w_i + px) as usize] = color;
            }
            if py_bot >= 0 && py_bot < h_i {
                fb[(py_bot * w_i + px) as usize] = color;
            }
        }
    }
}

/// Draw rating stars. Returns the width used.
fn draw_rating(
    fb: &mut [u32],
    w: u32,
    h: u32,
    rating: f32,
    x: i32,
    y: i32,
    star_size: i32,
    rating_text: &str,
) -> i32 {
    let full_stars = rating.floor() as i32;
    let half = (rating - rating.floor()) >= 0.25;
    let spacing = star_size * 3;
    let gold = 0x00FFD700;
    let dim = 0x00555555;

    for i in 0..5 {
        let sx = x + i * spacing + star_size;
        let sy = y + star_size;
        if i < full_stars {
            draw_star_filled(fb, w, h, sx, sy, star_size, gold);
        } else if i == full_stars && half {
            // Half star: filled left, empty right (approximate with filled)
            draw_star_filled(fb, w, h, sx, sy, star_size, gold);
        } else {
            draw_star_empty(fb, w, h, sx, sy, star_size, dim);
        }
    }

    // Also draw numeric rating next to stars (pre-computed text, no allocation)
    let text_x = x + 5 * spacing + star_size * 2;
    draw_string(fb, w, h, rating_text, text_x, y, 0x00CCCCCC, 1);

    text_x + (rating_text.len() as i32) * 9
}

// ─── Colors ────────────────────────────────────────────────────────────────

const COLOR_BG: u32 = 0x001A1A2E; // Dark navy background
const COLOR_HEADER_BG: u32 = 0x0016213E; // Slightly different header
const COLOR_TITLE: u32 = 0x00FFFFFF; // White title
const COLOR_SYSTEM: u32 = 0x00E0A030; // Gold/amber system name
const COLOR_LABEL: u32 = 0x00888899; // Dim label color
const COLOR_VALUE: u32 = 0x00DDDDEE; // Light value color
const COLOR_DESC: u32 = 0x00BBBBCC; // Description text
const COLOR_NAV: u32 = 0x00667788; // Navigation hints
const COLOR_ARROW: u32 = 0x00AABBCC; // Navigation arrows
const COLOR_ACCENT: u32 = 0x004488CC; // Accent/separator lines
const COLOR_ERROR: u32 = 0x00FF6644; // Error messages
const COLOR_LOADING: u32 = 0x0088AACC; // Loading indicator

// ─── Debug logging ─────────────────────────────────────────────────────────

/// Append a debug message to /tmp/replay-core-debug.log.
/// Silently ignores errors (logging must never crash the core).
fn debug_log(msg: &str) {
    let _ = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/replay-core-debug.log")?;
        writeln!(f, "{}", msg)?;
        Ok(())
    })();
}

// ─── HTTP client (Phase 2) ─────────────────────────────────────────────────

/// Base URL for the Replay Control app API.
/// Always port 8080 on the Pi (the only deployment target for this core).
fn api_base() -> &'static str {
    "http://localhost:8080"
}

/// Fetch the list of recently played games from Replay Control.
fn fetch_recents() -> Result<Vec<GameEntry>, String> {
    let resp = minreq::get(&format!("{}/api/core/recents", api_base()))
        .with_header("Accept", "application/json")
        .with_timeout(5)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status_code != 200 {
        return Err(format!("HTTP {}: {}", resp.status_code, resp.as_str().unwrap_or("")));
    }

    let body = resp.as_str().map_err(|e| format!("UTF-8 error: {}", e))?;
    parse_recents_json(body)
}

/// Fetch the list of favorites from Replay Control.
fn fetch_favorites() -> Result<Vec<GameEntry>, String> {
    let resp = minreq::get(&format!("{}/api/core/favorites", api_base()))
        .with_header("Accept", "application/json")
        .with_timeout(5)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status_code != 200 {
        return Err(format!("HTTP {}", resp.status_code));
    }

    let body = resp.as_str().map_err(|e| format!("UTF-8 error: {}", e))?;
    parse_favorites_json(body)
}

/// Fetch detailed metadata for a specific ROM.
fn fetch_rom_detail(system: &str, filename: &str) -> Result<GameDetail, String> {
    let url = format!(
        "{}/api/core/game/{}/{}",
        api_base(),
        urlencoding::encode(system),
        urlencoding::encode(filename),
    );

    let resp = minreq::get(&url)
        .with_header("Accept", "application/json")
        .with_timeout(5)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status_code != 200 {
        return Err(format!("HTTP {}", resp.status_code));
    }

    let body = resp.as_str().map_err(|e| format!("UTF-8 error: {}", e))?;
    parse_rom_detail_json(body)
}

/// Maximum number of games to pre-fetch box art for (avoid downloading too many PNGs at startup).
const MAX_BOX_ART_PREFETCH: usize = 20;

/// Fetch a PNG image from a URL and decode it to XRGB8888 pixels, scaled to fit `max_w x max_h`.
fn fetch_and_decode_box_art(url: &str, max_w: u32, max_h: u32) -> Result<BoxArtImage, String> {
    let resp = minreq::get(url)
        .with_timeout(10)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status_code != 200 {
        return Err(format!("HTTP {}", resp.status_code));
    }

    let png_bytes = resp.as_bytes();
    decode_png_to_xrgb8888(png_bytes, max_w, max_h)
}

/// Decode PNG bytes to XRGB8888 pixel data, scaled to fit within `max_w x max_h`.
fn decode_png_to_xrgb8888(data: &[u8], max_w: u32, max_h: u32) -> Result<BoxArtImage, String> {
    let mut decoder = png::Decoder::new(data);
    // Expand indexed/palette and grayscale images to full RGB(A)
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG header: {}", e))?;

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG decode: {}", e))?;

    let src_w = info.width;
    let src_h = info.height;
    let row_bytes = info.line_size;

    // After EXPAND transformation, output is RGB (3 bytes) or RGBA (4 bytes)
    let bytes_per_pixel = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2, // shouldn't happen after EXPAND, but handle it
        png::ColorType::Grayscale => 1,       // shouldn't happen after EXPAND, but handle it
        _ => 3, // EXPAND converts indexed → RGB
    };

    // Convert to XRGB8888 (0x00RRGGBB)
    let mut src_pixels = vec![0u32; (src_w * src_h) as usize];
    for y in 0..src_h as usize {
        for x in 0..src_w as usize {
            let offset = y * row_bytes + x * bytes_per_pixel;
            let (r, g, b) = if bytes_per_pixel >= 3 {
                (buf[offset], buf[offset + 1], buf[offset + 2])
            } else if bytes_per_pixel == 2 {
                let v = buf[offset];
                (v, v, v)
            } else {
                let v = buf[offset];
                (v, v, v)
            };

            src_pixels[y * src_w as usize + x] = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }

    // Scale to fit within max_w x max_h, preserving aspect ratio
    let (dst_w, dst_h) = fit_dimensions(src_w, src_h, max_w, max_h);

    if dst_w == 0 || dst_h == 0 {
        return Err("Image too small to display".to_string());
    }

    let scaled = scale_image_nearest(&src_pixels, src_w, src_h, dst_w, dst_h);
    Ok(BoxArtImage {
        pixels: scaled,
        width: dst_w,
        height: dst_h,
    })
}

/// Calculate dimensions that fit within max_w x max_h while preserving aspect ratio.
fn fit_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (0, 0);
    }
    let scale_w = max_w as f32 / src_w as f32;
    let scale_h = max_h as f32 / src_h as f32;
    let scale = scale_w.min(scale_h);
    let dst_w = (src_w as f32 * scale).round() as u32;
    let dst_h = (src_h as f32 * scale).round() as u32;
    (dst_w.max(1), dst_h.max(1))
}

/// Nearest-neighbor image scaling.
fn scale_image_nearest(src: &[u32], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u32> {
    let mut dst = vec![0u32; (dst_w * dst_h) as usize];
    for y in 0..dst_h {
        for x in 0..dst_w {
            let sx = (x * src_w / dst_w) as usize;
            let sy = (y * src_h / dst_h) as usize;
            dst[(y * dst_w + x) as usize] = src[sy * src_w as usize + sx];
        }
    }
    dst
}

// ─── Minimal URL encoding ──────────────────────────────────────────────────

mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push('%');
                    result.push(hex_char(byte >> 4));
                    result.push(hex_char(byte & 0x0F));
                }
            }
        }
        result
    }

    fn hex_char(nibble: u8) -> char {
        match nibble {
            0..=9 => (b'0' + nibble) as char,
            _ => (b'A' + nibble - 10) as char,
        }
    }
}

// ─── Minimal JSON parsing ──────────────────────────────────────────────────
//
// We parse the JSON responses from Replay Control manually to avoid
// pulling in serde_json (~300KB). The response shapes are known and stable.

/// Parse recents JSON response from the Replay Control REST API.
/// Each entry has:
/// { "system": "...", "system_display": "...", "rom_filename": "...",
///   "display_name": "...", "box_art_url": "..." or null }
fn parse_recents_json(json: &str) -> Result<Vec<GameEntry>, String> {
    let mut entries = Vec::new();

    // Find the array
    let json = json.trim();

    let array_str = extract_result_array(json)?;

    // Split array into objects
    let objects = split_json_array(array_str);

    for obj in objects {
        let system = extract_json_string(obj, "system").unwrap_or_default();
        let system_display = extract_json_string(obj, "system_display").unwrap_or_default();
        let rom_filename = extract_json_string(obj, "rom_filename").unwrap_or_default();
        let display_name = extract_json_string(obj, "display_name")
            .unwrap_or_else(|| rom_filename.clone());
        let box_art_url = extract_json_string(obj, "box_art_url");

        if !system.is_empty() && !rom_filename.is_empty() {
            entries.push(GameEntry {
                system,
                system_display,
                rom_filename,
                display_name,
                box_art_url,
            });
        }
    }

    Ok(entries)
}

/// Parse favorites JSON response. Same shape as recents.
fn parse_favorites_json(json: &str) -> Result<Vec<GameEntry>, String> {
    let mut entries = Vec::new();
    let json = json.trim();
    let array_str = extract_result_array(json)?;
    let objects = split_json_array(array_str);

    for obj in objects {
        let system = extract_json_string(obj, "system").unwrap_or_default();
        let system_display = extract_json_string(obj, "system_display").unwrap_or_default();
        let rom_filename = extract_json_string(obj, "rom_filename").unwrap_or_default();
        let display_name = extract_json_string(obj, "display_name")
            .unwrap_or_else(|| rom_filename.clone());
        let box_art_url = extract_json_string(obj, "box_art_url");

        if !system.is_empty() && !rom_filename.is_empty() {
            entries.push(GameEntry {
                system,
                system_display,
                rom_filename,
                display_name,
                box_art_url,
            });
        }
    }

    Ok(entries)
}

/// Parse ROM detail JSON response from the Replay Control REST API.
/// Shape is a flat object:
/// { "display_name": "...", "system_display": "...", "year": "...",
///   "developer": "...", "genre": "...", "players": N,
///   "description": "..." or null, "rating": N.N or null,
///   "publisher": "..." or null, "region": "..." or null }
fn parse_rom_detail_json(json: &str) -> Result<GameDetail, String> {
    let json = json.trim();

    let obj = if json.starts_with('{') {
        json
    } else {
        return Err("Expected JSON object".to_string());
    };

    let display_name = extract_json_string(obj, "display_name").unwrap_or_default();
    let system_display = extract_json_string(obj, "system_display").unwrap_or_default();
    let year = extract_json_string(obj, "year").unwrap_or_default();
    let developer = extract_json_string(obj, "developer").unwrap_or_default();
    let genre = extract_json_string(obj, "genre").unwrap_or_default();
    let players = extract_json_number(obj, "players").unwrap_or(0) as u8;
    let description = extract_json_string(obj, "description");
    let rating = extract_json_float(obj, "rating");
    let publisher = extract_json_string(obj, "publisher");
    let region = extract_json_string(obj, "region");

    Ok(GameDetail {
        display_name,
        system_display,
        year,
        developer,
        genre,
        players,
        rating,
        description,
        publisher,
        region,
        box_art: None, // populated separately during pre-fetch
        display: PrecomputedDisplay {
            sys_year_line: String::new(),
            players_text: String::new(),
            rating_text: String::new(),
            desc_lines: Vec::new(),
        }, // populated by precompute_display() during pre-fetch
    })
}

/// Extract the JSON array from a response, handling potential error wrapping.
fn extract_result_array(json: &str) -> Result<&str, String> {
    // If the response starts with '[', it's a direct array
    if json.starts_with('[') {
        return Ok(json);
    }
    // If it starts with '{', it might be an error or wrapped result
    if json.starts_with('{') {
        // Check for error message
        if let Some(err) = extract_json_string(json, "Err") {
            return Err(err);
        }
        return Err(format!("Unexpected JSON object: {}", &json[..json.len().min(100)]));
    }
    Err(format!("Unexpected response format: {}", &json[..json.len().min(100)]))
}

/// Split a JSON array string into its top-level elements.
fn split_json_array(json: &str) -> Vec<&str> {
    let json = json.trim();
    let json = json.strip_prefix('[').unwrap_or(json);
    let json = json.strip_suffix(']').unwrap_or(json);

    let mut objects = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut start = 0;
    let bytes = json.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if b == b'\\' && in_string {
            escape = true;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match b {
            b'{' | b'[' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    objects.push(&json[start..=i]);
                }
            }
            _ => {}
        }
    }

    objects
}

/// Extract a string value for a given key from a JSON object.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let idx = json.find(&search)?;
    let rest = &json[idx + search.len()..];
    // Skip whitespace and colon
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();

    if rest.starts_with("null") {
        return None;
    }

    if !rest.starts_with('"') {
        return None;
    }

    // Find end of string, handling escapes
    let content = &rest[1..];
    let mut result = String::new();
    let mut chars = content.chars();
    loop {
        match chars.next() {
            None => break,
            Some('"') => break,
            Some('\\') => {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('t') => result.push('\t'),
                    Some('r') => result.push('\r'),
                    Some('"') => result.push('"'),
                    Some('\\') => result.push('\\'),
                    Some('/') => result.push('/'),
                    Some('u') => {
                        // Unicode escape \uXXXX
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(cp) {
                                result.push(ch);
                            }
                        }
                    }
                    Some(c) => {
                        result.push('\\');
                        result.push(c);
                    }
                    None => break,
                }
            }
            Some(c) => result.push(c),
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Extract a numeric value for a given key.
fn extract_json_number(json: &str, key: &str) -> Option<i64> {
    let search = format!("\"{}\"", key);
    let idx = json.find(&search)?;
    let rest = &json[idx + search.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();

    if rest.starts_with("null") {
        return None;
    }

    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-' && c != '+')?;
    let num_str = &rest[..end];
    num_str.parse().ok()
}

/// Extract a float value for a given key.
fn extract_json_float(json: &str, key: &str) -> Option<f32> {
    let search = format!("\"{}\"", key);
    let idx = json.find(&search)?;
    let rest = &json[idx + search.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();

    if rest.starts_with("null") {
        return None;
    }

    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-' && c != '+' && c != '.')
        .unwrap_or(rest.len());
    let num_str = &rest[..end];
    num_str.parse().ok()
}

// ─── Pre-computation helpers ───────────────────────────────────────────

/// Build the pre-computed display strings for a GameDetail.
/// Called once during prefetch so rendering never allocates.
fn precompute_display(detail: &GameDetail, chars_per_line: usize) -> PrecomputedDisplay {
    let sys_year_line = if !detail.year.is_empty() {
        format!("{}  -  {}", detail.system_display, detail.year)
    } else {
        detail.system_display.clone()
    };

    let players_text = match detail.players {
        0 => String::new(),
        1 => "1 Player".to_string(),
        n => format!("1-{} Players", n),
    };

    let rating_text = match detail.rating {
        Some(r) => format!("{:.1}", r),
        None => String::new(),
    };

    let desc_lines = match detail.description.as_ref() {
        Some(desc) => word_wrap(desc, chars_per_line),
        None => Vec::new(),
    };

    PrecomputedDisplay {
        sys_year_line,
        players_text,
        rating_text,
        desc_lines,
    }
}

/// Rewrite the header_text scratch buffer in-place.
/// Called when the user navigates (left/right/start), not every frame.
fn update_header_text(s: &mut CoreState) {
    let (entries, _) = match s.list_mode {
        ListMode::Recents => (&s.entries, &s.detail_cache),
        ListMode::Favorites => (&s.favorites, &s.fav_detail_cache),
    };
    let total = entries.len();
    let mode_label = match s.list_mode {
        ListMode::Recents => "RECENTLY PLAYED",
        ListMode::Favorites => "FAVORITES",
    };
    s.header_text.clear();
    if total > 0 {
        use std::fmt::Write;
        let _ = write!(s.header_text, "{}  ({}/{})", mode_label, s.current_index + 1, total);
    }
}

/// Rewrite the scroll_indicator scratch buffer in-place.
/// Called when the user scrolls or navigates to a new game.
fn update_scroll_indicator(s: &mut CoreState) {
    s.scroll_indicator.clear();

    let detail_cache = match s.list_mode {
        ListMode::Recents => &s.detail_cache,
        ListMode::Favorites => &s.fav_detail_cache,
    };

    if let Some(Some(detail)) = detail_cache.get(s.current_index) {
        let total_lines = detail.display.desc_lines.len();
        let max_lines = s.layout.max_desc_lines as usize;
        if total_lines > max_lines {
            let scroll = s.desc_scroll.min(total_lines.saturating_sub(max_lines));
            use std::fmt::Write;
            let _ = write!(
                s.scroll_indicator,
                "[{}/{}]",
                scroll + 1,
                total_lines.saturating_sub(max_lines) + 1
            );
        }
    }
}

// ─── Data loading ──────────────────────────────────────────────────────────

fn load_recents(s: &mut CoreState) {
    s.status_message = "Fetching recently played...".to_string();
    s.loading = true;

    match fetch_recents() {
        Ok(entries) => {
            let count = entries.len();
            s.detail_cache = vec![None; count];
            s.entries = entries;
            s.current_index = 0;
            s.desc_scroll = 0;
            s.api_available = true;
            s.loading = false;

            if count == 0 {
                s.status_message = "No recently played games found.".to_string();
            } else {
                s.status_message.clear();
                // Pre-fetch ALL details at load time so retro_run never touches the network
                debug_log(&format!("[recents] pre-fetching details for {} entries", count));
                for i in 0..count {
                    prefetch_detail(s, i);
                }
                debug_log("[recents] pre-fetch complete");
            }
        }
        Err(e) => {
            s.api_available = false;
            s.loading = false;
            s.status_message = format!("Replay Control not available: {}", e);
            debug_log(&format!("[recents] fetch failed: {}", e));
        }
    }
}

fn load_favorites(s: &mut CoreState) {
    s.status_message = "Fetching favorites...".to_string();
    s.loading = true;

    match fetch_favorites() {
        Ok(entries) => {
            let count = entries.len();
            s.fav_detail_cache = vec![None; count];
            s.favorites = entries;
            s.current_index = 0;
            s.desc_scroll = 0;
            s.api_available = true;
            s.loading = false;

            if count == 0 {
                s.status_message = "No favorites found.".to_string();
            } else {
                s.status_message.clear();
                // Pre-fetch ALL details at load time so retro_run never touches the network
                debug_log(&format!("[favorites] pre-fetching details for {} entries", count));
                for i in 0..count {
                    prefetch_detail_for_favorites(s, i);
                }
                debug_log("[favorites] pre-fetch complete");
            }
        }
        Err(e) => {
            s.api_available = false;
            s.loading = false;
            s.status_message = format!("Could not load favorites: {}", e);
            debug_log(&format!("[favorites] fetch failed: {}", e));
        }
    }
}

fn prefetch_detail(s: &mut CoreState, index: usize) {
    if index >= s.entries.len() {
        return;
    }
    if s.detail_cache.get(index).and_then(|d| d.as_ref()).is_some() {
        return; // already cached
    }

    let chars_per_line = s.layout.chars_per_line as usize;
    let entry = &s.entries[index];
    let box_art_url = entry.box_art_url.clone();
    match fetch_rom_detail(&entry.system, &entry.rom_filename) {
        Ok(mut detail) => {
            // Fetch box art if available and within the limit
            if index < MAX_BOX_ART_PREFETCH {
                if let Some(ref url) = box_art_url {
                    detail.box_art = fetch_box_art_for_layout(url, &s.layout);
                }
            }
            // Pre-compute display strings so rendering is allocation-free
            detail.display = precompute_display(&detail, chars_per_line);
            if index < s.detail_cache.len() {
                s.detail_cache[index] = Some(detail);
            }
        }
        Err(_) => {
            // Create a minimal detail from the entry itself
            let mut fallback = GameDetail {
                display_name: s.entries[index].display_name.clone(),
                system_display: s.entries[index].system_display.clone(),
                year: String::new(),
                developer: String::new(),
                genre: String::new(),
                players: 0,
                rating: None,
                description: None,
                publisher: None,
                region: None,
                box_art: None,
                display: PrecomputedDisplay {
                    sys_year_line: String::new(),
                    players_text: String::new(),
                    rating_text: String::new(),
                    desc_lines: Vec::new(),
                },
            };
            fallback.display = precompute_display(&fallback, chars_per_line);
            if index < s.detail_cache.len() {
                s.detail_cache[index] = Some(fallback);
            }
        }
    }
}

fn prefetch_detail_for_favorites(s: &mut CoreState, index: usize) {
    if index >= s.favorites.len() {
        return;
    }
    if s.fav_detail_cache
        .get(index)
        .and_then(|d| d.as_ref())
        .is_some()
    {
        return;
    }

    let chars_per_line = s.layout.chars_per_line as usize;
    let entry = &s.favorites[index];
    let box_art_url = entry.box_art_url.clone();
    match fetch_rom_detail(&entry.system, &entry.rom_filename) {
        Ok(mut detail) => {
            // Fetch box art if available and within the limit
            if index < MAX_BOX_ART_PREFETCH {
                if let Some(ref url) = box_art_url {
                    detail.box_art = fetch_box_art_for_layout(url, &s.layout);
                }
            }
            // Pre-compute display strings so rendering is allocation-free
            detail.display = precompute_display(&detail, chars_per_line);
            if index < s.fav_detail_cache.len() {
                s.fav_detail_cache[index] = Some(detail);
            }
        }
        Err(_) => {
            let mut fallback = GameDetail {
                display_name: s.favorites[index].display_name.clone(),
                system_display: s.favorites[index].system_display.clone(),
                year: String::new(),
                developer: String::new(),
                genre: String::new(),
                players: 0,
                rating: None,
                description: None,
                publisher: None,
                region: None,
                box_art: None,
                display: PrecomputedDisplay {
                    sys_year_line: String::new(),
                    players_text: String::new(),
                    rating_text: String::new(),
                    desc_lines: Vec::new(),
                },
            };
            fallback.display = precompute_display(&fallback, chars_per_line);
            if index < s.fav_detail_cache.len() {
                s.fav_detail_cache[index] = Some(fallback);
            }
        }
    }
}

/// Fetch and decode box art, sized appropriately for the current layout.
/// Returns None on any failure (network, decode, etc.) — failures are non-fatal.
fn fetch_box_art_for_layout(url: &str, layout: &LayoutConfig) -> Option<BoxArtImage> {
    // The API returns relative URLs like "/media/snes/Named_Boxarts/Game.png".
    // Prepend the API base to make a full HTTP URL.
    let full_url = if url.starts_with("http") {
        url.to_string()
    } else {
        // URL-encode each path segment (preserving '/' separators).
        // e.g., "/media/arcade/boxart/Alien vs. Predator (Europe).png"
        // -> "/media/arcade/boxart/Alien%20vs.%20Predator%20%28Europe%29.png"
        let encoded_path: String = url
            .split('/')
            .map(|seg| urlencoding::encode(seg))
            .collect::<Vec<_>>()
            .join("/");
        format!("{}{}", api_base(), encoded_path)
    };

    let (max_w, max_h) = box_art_dimensions(layout);
    match fetch_and_decode_box_art(&full_url, max_w, max_h) {
        Ok(img) => {
            debug_log(&format!(
                "[box-art] decoded {}x{} from {}",
                img.width, img.height, url
            ));
            Some(img)
        }
        Err(e) => {
            debug_log(&format!("[box-art] failed for {}: {}", url, e));
            None
        }
    }
}

/// Calculate box art display dimensions based on layout.
fn box_art_dimensions(layout: &LayoutConfig) -> (u32, u32) {
    match layout.width {
        0..=320 => (100, 140),   // CRT 320x240: prominent box art
        321..=640 => (120, 165),  // CRT 640x480
        641..=1280 => (200, 275), // 720p
        _ => (280, 385),          // 1080p+
    }
}

// ─── Rendering ─────────────────────────────────────────────────────────────

// ALLOCATION-FREE: this function must not allocate.
// All display strings are pre-computed during prefetch; scratch buffers
// (header_text, scroll_indicator) are updated on navigation events only.
fn render_game_detail(s: &mut CoreState) {
    let w = s.layout.width;
    let h = s.layout.height;
    let font_scale = s.layout.font_scale;
    let title_scale = s.layout.title_scale;
    let label_scale = s.layout.label_scale;
    let mx = s.layout.margin_x as i32;
    let my = s.layout.margin_y as i32;
    let max_desc_lines = s.layout.max_desc_lines as usize;
    let show_extra_metadata = s.layout.show_extra_metadata;

    let fb = &mut s.framebuffer;

    // Clear background
    for px in fb.iter_mut() {
        *px = COLOR_BG;
    }

    // Get the current list and detail cache
    let (entries, detail_cache) = match s.list_mode {
        ListMode::Recents => (&s.entries, &s.detail_cache),
        ListMode::Favorites => (&s.favorites, &s.fav_detail_cache),
    };

    let total = entries.len();

    // ── Header: mode label + position indicator ──
    let header_h = (label_scale * 16 + 8) as i32;
    draw_rect(fb, w, h, 0, 0, w, header_h as u32, COLOR_HEADER_BG);
    draw_hline(fb, w, h, 0, header_h, w, COLOR_ACCENT);

    let mode_label = match s.list_mode {
        ListMode::Recents => "RECENTLY PLAYED",
        ListMode::Favorites => "FAVORITES",
    };

    if total == 0 {
        draw_string(fb, w, h, mode_label, mx, my / 2 + 2, COLOR_NAV, label_scale);
    } else {
        // Left arrow
        let arrow_x = mx;
        draw_string(fb, w, h, "<", arrow_x, my / 2 + 2, COLOR_ARROW, label_scale);

        // Mode + position — read from pre-computed scratch buffer
        let text_x = arrow_x + (label_scale * 9 + 4) as i32;
        draw_string(
            fb, w, h, &s.header_text, text_x, my / 2 + 2, COLOR_NAV, label_scale,
        );

        // Right arrow on the far right
        let arrow_r = (w as i32) - mx - (label_scale * 9) as i32;
        draw_string(fb, w, h, ">", arrow_r, my / 2 + 2, COLOR_ARROW, label_scale);
    }

    // ── Handle empty list / loading / error ──
    // Status messages are short and only shown in error/loading states, so
    // we render them character-by-character without word_wrap. The message
    // is pre-set and rarely changes, so this is fine for display.
    if !s.status_message.is_empty() {
        let color = if s.api_available {
            COLOR_LOADING
        } else {
            COLOR_ERROR
        };
        let msg_y = (h as i32) / 2 - 8;

        // Status messages are short enough to render without word-wrapping.
        // They fit within a single line on all layouts (max ~40 chars).
        draw_string(fb, w, h, &s.status_message, mx, msg_y, color, font_scale);

        render_footer_hints(fb, w, h, mx, my, label_scale, w);
        return;
    }

    if total == 0 {
        render_footer_hints(fb, w, h, mx, my, label_scale, w);
        return;
    }

    // ── Game detail content ──
    let detail = detail_cache.get(s.current_index).and_then(|d| d.as_ref());
    let entry = &entries[s.current_index];

    let mut y = header_h + (my / 2) + 4;
    let line_h = (font_scale * 18) as i32;
    let title_h = (title_scale * 18) as i32;

    // Title (large) — always full width
    let title = detail
        .map(|d| d.display_name.as_str())
        .unwrap_or(&entry.display_name);
    let title_max = ((w as i32 - 2 * mx) / (title_scale * 9) as i32) as usize;
    draw_string_truncated(fb, w, h, title, mx, y, COLOR_TITLE, title_scale, title_max);
    y += title_h + 4;

    // System + Year — from pre-computed display string
    let sys_year = detail
        .map(|d| d.display.sys_year_line.as_str())
        .unwrap_or(&entry.system_display);
    draw_string(fb, w, h, sys_year, mx, y, COLOR_SYSTEM, font_scale);
    y += line_h;

    // Separator line
    draw_hline(fb, w, h, mx, y, (w as i32 - 2 * mx) as u32, COLOR_ACCENT);
    y += 6;

    // ── Two-column layout: box art (left) + metadata (right) ──
    let has_box_art = detail.and_then(|d| d.box_art.as_ref()).is_some();
    let art_x = mx;
    let art_y = y;

    // Calculate text column start: if box art exists, shift text right of it
    let (text_x, text_w) = if has_box_art {
        let art = detail.unwrap().box_art.as_ref().unwrap();
        let gap = if w <= 320 { 6 } else { 12 }; // gap between art and text
        let tx = art_x + art.width as i32 + gap;
        let tw = (w as i32 - tx - mx).max(40) as u32; // ensure at least 40px for text
        (tx, tw)
    } else {
        (mx, (w as i32 - 2 * mx) as u32)
    };

    // Blit box art (or placeholder) on the left
    let mut art_bottom = y; // track where box art ends vertically
    if let Some(d) = detail {
        if let Some(ref art) = d.box_art {
            blit_image(fb, w, h, &art.pixels, art.width, art.height, art_x as u32, art_y as u32);
            art_bottom = art_y + art.height as i32 + 4;
        }
    }

    // ── Metadata column (to the right of box art, or full width if no art) ──
    let meta_chars_per_line = (text_w as i32 / (font_scale * 9) as i32) as usize;

    // Developer
    if let Some(d) = detail {
        if !d.developer.is_empty() {
            let end_x = draw_string(fb, w, h, "Dev: ", text_x, y, COLOR_LABEL, label_scale);
            draw_string_truncated(
                fb, w, h, &d.developer, end_x, y, COLOR_VALUE, font_scale,
                meta_chars_per_line.saturating_sub(5),
            );
            y += line_h;
        }
    }

    // Rating stars (using pre-computed rating_text)
    if let Some(detail) = detail {
        if let Some(rating) = detail.rating {
            let end_x = draw_string(fb, w, h, "Rating: ", text_x, y, COLOR_LABEL, label_scale);
            let star_size = (font_scale * 4).max(3) as i32;
            draw_rating(fb, w, h, rating, end_x, y, star_size, &detail.display.rating_text);
            y += line_h;
        }
    }

    // Genre
    if let Some(d) = detail {
        if !d.genre.is_empty() {
            let end_x = draw_string(fb, w, h, "Genre: ", text_x, y, COLOR_LABEL, label_scale);
            draw_string_truncated(
                fb, w, h, &d.genre, end_x, y, COLOR_VALUE, font_scale,
                meta_chars_per_line.saturating_sub(7),
            );
            y += line_h;
        }
    }

    // Players — from pre-computed display string
    if let Some(d) = detail {
        if !d.display.players_text.is_empty() {
            let end_x = draw_string(fb, w, h, "Players: ", text_x, y, COLOR_LABEL, label_scale);
            draw_string(fb, w, h, &d.display.players_text, end_x, y, COLOR_VALUE, font_scale);
            y += line_h;
        }
    }

    // Extra metadata (publisher, region) for HD layouts
    if show_extra_metadata {
        if let Some(d) = detail {
            if let Some(ref publisher) = d.publisher {
                if !publisher.is_empty() {
                    let end_x =
                        draw_string(fb, w, h, "Publisher: ", text_x, y, COLOR_LABEL, label_scale);
                    draw_string(fb, w, h, publisher, end_x, y, COLOR_VALUE, font_scale);
                    y += line_h;
                }
            }
            if let Some(ref region) = d.region {
                if !region.is_empty() {
                    let end_x =
                        draw_string(fb, w, h, "Region: ", text_x, y, COLOR_LABEL, label_scale);
                    draw_string(fb, w, h, region, end_x, y, COLOR_VALUE, font_scale);
                    y += line_h;
                }
            }
        }
    }

    // Ensure description starts below box art if it extends further down
    if has_box_art && y < art_bottom {
        y = art_bottom;
    }

    // Description (with scroll support) — from pre-computed wrapped lines
    if let Some(d) = detail {
        if !d.display.desc_lines.is_empty() {
            y += 4;
            draw_hline(fb, w, h, mx, y, (w as i32 - 2 * mx) as u32, COLOR_ACCENT);
            y += 8;

            let total_lines = d.display.desc_lines.len();
            let scroll = s.desc_scroll.min(total_lines.saturating_sub(max_desc_lines));

            let end = d.display.desc_lines.len().min(scroll + max_desc_lines);
            for line in &d.display.desc_lines[scroll..end] {
                if y + line_h >= (h as i32 - my - line_h) {
                    break;
                }
                draw_string(fb, w, h, line, mx, y, COLOR_DESC, font_scale);
                y += line_h;
            }

            // Scroll indicator — from pre-computed scratch buffer
            if !s.scroll_indicator.is_empty() {
                let ix = (w as i32) - mx - (s.scroll_indicator.len() as i32 * 9);
                draw_string(fb, w, h, &s.scroll_indicator, ix, y + 2, COLOR_NAV, 1);
            }
        }
    }

    // Footer hints
    render_footer_hints(fb, w, h, mx, my, label_scale, w);
}

fn render_footer_hints(
    fb: &mut [u32],
    w: u32,
    h: u32,
    mx: i32,
    my: i32,
    label_scale: u32,
    layout_width: u32,
) {
    let label_h = (label_scale * 16) as i32;

    if layout_width <= 640 {
        // CRT: two lines of hints at the bottom
        let line2_y = (h as i32) - my - label_h;
        let line1_y = line2_y - label_h - 2;
        draw_hline(fb, w, h, mx, line1_y - 4, (w as i32 - 2 * mx) as u32, COLOR_ACCENT);

        // Line 1: browsing and scrolling
        let line1 = "<->: browse  ^v: scroll desc";
        draw_string(fb, w, h, line1, mx, line1_y, COLOR_NAV, label_scale);
        // Line 2: mode toggle and exit
        let line2 = "Start: recents/favs  B: exit";
        draw_string(fb, w, h, line2, mx, line2_y, COLOR_NAV, label_scale);
    } else {
        // HD: single line
        let footer_y = (h as i32) - my - label_h;
        draw_hline(fb, w, h, mx, footer_y - 4, (w as i32 - 2 * mx) as u32, COLOR_ACCENT);

        let hints =
            "Left/Right: browse games  |  Up/Down: scroll desc  |  Start: recents/favorites  |  B: exit";
        draw_string(fb, w, h, hints, mx, footer_y, COLOR_NAV, label_scale);
    }
}

// ─── Input handling ────────────────────────────────────────────────────────

// ALLOCATION-FREE: this function must not allocate (except on edge-triggered
// navigation events, which rewrite pre-allocated scratch buffers in-place).
fn handle_input(s: &mut CoreState) {
    let input_state = match s.input_state_cb {
        Some(cb) => cb,
        None => return,
    };

    let btn = |id: c_uint| -> bool { unsafe { input_state(0, RETRO_DEVICE_JOYPAD, 0, id) != 0 } };

    let left = btn(RETRO_DEVICE_ID_JOYPAD_LEFT);
    let right = btn(RETRO_DEVICE_ID_JOYPAD_RIGHT);
    let up = btn(RETRO_DEVICE_ID_JOYPAD_UP);
    let down = btn(RETRO_DEVICE_ID_JOYPAD_DOWN);
    let b_pressed = btn(RETRO_DEVICE_ID_JOYPAD_B);
    let start = btn(RETRO_DEVICE_ID_JOYPAD_START);

    // Read list length without holding a borrow on s
    let entries_len = match s.list_mode {
        ListMode::Recents => s.entries.len(),
        ListMode::Favorites => s.favorites.len(),
    };

    // B: exit (edge-triggered)
    if b_pressed && !s.prev_b {
        if let Some(env_cb) = s.environment_cb {
            unsafe {
                env_cb(RETRO_ENVIRONMENT_SHUTDOWN, std::ptr::null_mut());
            }
        }
    }

    // Track whether navigation happened (need to update scratch buffers)
    let mut navigated = false;

    // Start: toggle list mode (edge-triggered)
    // All data was pre-fetched in retro_load_game — just switch views, no HTTP.
    if start && !s.prev_start {
        match s.list_mode {
            ListMode::Recents => {
                s.list_mode = ListMode::Favorites;
                s.current_index = 0;
                s.desc_scroll = 0;
                // Rewrite status_message in-place (no new allocation if capacity suffices)
                s.status_message.clear();
                if s.favorites.is_empty() {
                    s.status_message.push_str("No favorites found.");
                }
            }
            ListMode::Favorites => {
                s.list_mode = ListMode::Recents;
                s.current_index = 0;
                s.desc_scroll = 0;
                s.status_message.clear();
                if s.entries.is_empty() {
                    s.status_message.push_str("No recently played games found.");
                }
            }
        }
        navigated = true;
    }

    // Left/Right: navigate between games (edge-triggered)
    // All details were pre-fetched in retro_load_game — just update index, no HTTP.
    if left && !s.prev_left && entries_len > 0 {
        if s.current_index > 0 {
            s.current_index -= 1;
        } else {
            s.current_index = entries_len - 1; // wrap around
        }
        s.desc_scroll = 0;
        navigated = true;
    }

    if right && !s.prev_right && entries_len > 0 {
        if s.current_index < entries_len - 1 {
            s.current_index += 1;
        } else {
            s.current_index = 0; // wrap around
        }
        s.desc_scroll = 0;
        navigated = true;
    }

    // Up/Down: scroll description (with cooldown for held buttons)
    if s.scroll_cooldown > 0 {
        s.scroll_cooldown -= 1;
    }

    let mut scrolled = false;

    if (up || down) && s.scroll_cooldown == 0 {
        // Use pre-computed desc_lines instead of calling word_wrap every frame
        let scroll_info = {
            let detail_cache = match s.list_mode {
                ListMode::Recents => &s.detail_cache,
                ListMode::Favorites => &s.fav_detail_cache,
            };
            detail_cache
                .get(s.current_index)
                .and_then(|d| d.as_ref())
                .map(|detail| {
                    let total_lines = detail.display.desc_lines.len();
                    let max_lines = s.layout.max_desc_lines as usize;
                    total_lines.saturating_sub(max_lines)
                })
        };

        if let Some(max_scroll) = scroll_info {
            if up && s.desc_scroll > 0 {
                s.desc_scroll -= 1;
                s.scroll_cooldown = 8; // ~133ms at 60fps
                scrolled = true;
            }
            if down && s.desc_scroll < max_scroll {
                s.desc_scroll += 1;
                s.scroll_cooldown = 8;
                scrolled = true;
            }
        }
    }

    if !up && !down {
        s.scroll_cooldown = 0; // reset when released
    }

    // Update scratch buffers only when state actually changed (edge-triggered)
    if navigated {
        update_header_text(s);
        update_scroll_indicator(s);
    } else if scrolled {
        update_scroll_indicator(s);
    }

    // Update debounce state
    s.prev_left = left;
    s.prev_right = right;
    s.prev_b = b_pressed;
    s.prev_start = start;
}

// ─── Libretro API implementation ───────────────────────────────────────────

#[no_mangle]
pub extern "C" fn retro_api_version() -> c_uint {
    RETRO_API_VERSION
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_environment(cb: RetroEnvironmentFn) {
    let s = state();
    s.environment_cb = Some(cb);

    let mut no_game: bool = true;
    cb(
        RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
        &mut no_game as *mut bool as *mut c_void,
    );

    // Request XRGB8888 (32-bit) pixel format — confirmed accepted by RePlayOS.
    // This lets us output the u32 framebuffer directly with no conversion.
    let mut fmt: c_uint = 1; // RETRO_PIXEL_FORMAT_XRGB8888
    let accepted = cb(
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
        &mut fmt as *mut c_uint as *mut c_void,
    );
    if accepted {
        s.pixel_format = PixelFormat::Xrgb8888;
        debug_log("[replay-core] SET_PIXEL_FORMAT(XRGB8888) accepted");
    } else {
        s.pixel_format = PixelFormat::Bgr565;
        debug_log("[replay-core] SET_PIXEL_FORMAT(XRGB8888) rejected, falling back to BGR565");
    }
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_video_refresh(cb: RetroVideoRefreshFn) {
    state().video_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_audio_sample(cb: RetroAudioSampleFn) {
    state().audio_sample_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_audio_sample_batch(cb: RetroAudioSampleBatchFn) {
    state().audio_batch_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_input_poll(cb: RetroInputPollFn) {
    state().input_poll_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_input_state(cb: RetroInputStateFn) {
    state().input_state_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_init() {
    // Layout detection happens in retro_load_game (after environment is set up)
}

#[no_mangle]
pub unsafe extern "C" fn retro_deinit() {
    let s = state();
    s.environment_cb = None;
    s.video_cb = None;
    s.audio_sample_cb = None;
    s.audio_batch_cb = None;
    s.input_poll_cb = None;
    s.input_state_cb = None;
    s.entries.clear();
    s.favorites.clear();
    s.detail_cache.clear();
    s.fav_detail_cache.clear();
    s.framebuffer.clear();
    s.framebuffer_16.clear();
    s.header_text.clear();
    s.scroll_indicator.clear();
}

static LIBRARY_NAME: &[u8] = b"A/V Test\0";
static LIBRARY_VERSION: &[u8] = b"0.6.0\0";
static VALID_EXTENSIONS: &[u8] = b"\0"; // no-game mode only

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    (*info).library_name = LIBRARY_NAME.as_ptr() as *const c_char;
    (*info).library_version = LIBRARY_VERSION.as_ptr() as *const c_char;
    (*info).valid_extensions = VALID_EXTENSIONS.as_ptr() as *const c_char;
    (*info).need_fullpath = true;
    (*info).block_extract = false;
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    let s = state();
    let w = s.layout.width;
    let h = s.layout.height;

    (*info).geometry.base_width = w as c_uint;
    (*info).geometry.base_height = h as c_uint;
    (*info).geometry.max_width = w as c_uint;
    (*info).geometry.max_height = h as c_uint;
    (*info).geometry.aspect_ratio = 0.0; // let frontend decide
    (*info).timing.fps = 60.0;
    (*info).timing.sample_rate = 44100.0;
}

#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: c_uint, _device: c_uint) {}

#[no_mangle]
pub unsafe extern "C" fn retro_reset() {
    let s = state();
    s.frame_count = 0;
    s.current_index = 0;
    s.desc_scroll = 0;
    s.list_mode = ListMode::Recents;
    debug_log("[reset] re-fetching all data");
    load_recents(s);
    load_favorites(s);
    s.list_mode = ListMode::Recents;
    s.current_index = 0;
    s.desc_scroll = 0;
    if !s.entries.is_empty() {
        s.status_message.clear();
    }
    update_header_text(s);
    update_scroll_indicator(s);
}

// ALLOCATION-FREE: retro_run must not allocate on the heap.
// All rendering uses pre-computed strings; framebuffers are pre-allocated.
// The only writes to scratch buffers happen in handle_input on edge-triggered
// navigation events (clear + push_str into existing capacity).
#[no_mangle]
pub unsafe extern "C" fn retro_run() {
    let s = state();

    // Guard: framebuffer must be allocated (retro_load_game must have been called)
    if s.framebuffer.is_empty() {
        return;
    }

    // Poll input
    if let Some(poll) = s.input_poll_cb {
        poll();
    }

    handle_input(s);
    render_game_detail(s);

    s.frame_count += 1;

    // Send frame to frontend
    if let Some(video) = s.video_cb {
        let w = s.layout.width;
        let h = s.layout.height;

        match s.pixel_format {
            PixelFormat::Xrgb8888 => {
                // Direct output — internal u32 0x00RRGGBB is already XRGB8888
                video(
                    s.framebuffer.as_ptr() as *const c_void,
                    w as c_uint,
                    h as c_uint,
                    (w as usize) * std::mem::size_of::<u32>(),
                );
            }
            PixelFormat::Bgr565 => {
                // Fallback: convert u32 0x00RRGGBB → u16 BBBBBGGGGGGRRRRR
                // framebuffer_16 is pre-allocated in retro_load_game — no resize needed
                let pixel_count = (w * h) as usize;
                for i in 0..pixel_count {
                    let c = s.framebuffer[i];
                    let r = ((c >> 16) & 0xFF) as u16;
                    let g = ((c >> 8) & 0xFF) as u16;
                    let b = (c & 0xFF) as u16;
                    s.framebuffer_16[i] = ((b >> 3) << 11) | ((g >> 2) << 5) | (r >> 3);
                }
                video(
                    s.framebuffer_16.as_ptr() as *const c_void,
                    w as c_uint,
                    h as c_uint,
                    (w as usize) * std::mem::size_of::<u16>(),
                );
            }
        }
    }

    // Send silence for audio
    if let Some(audio_batch) = s.audio_batch_cb {
        let silence = [0i16; 735 * 2];
        audio_batch(silence.as_ptr(), 735);
    }
}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(_game: *const RetroGameInfo) -> bool {
    let s = state();

    // Phase 3: detect display mode from replay.cfg
    s.layout = LayoutConfig::detect();

    let w = s.layout.width;
    let h = s.layout.height;
    s.framebuffer = vec![0u32; (w * h) as usize];
    s.framebuffer_16 = vec![0u16; (w * h) as usize];

    debug_log(&format!(
        "[replay-game-info] load_game: detected {}x{} layout",
        w, h
    ));

    // Phase 2: fetch real data from Replay Control
    // Pre-fetch ALL data (both lists + all details) now, so retro_run never
    // makes HTTP calls. HTTP inside a dlopen-ed .so causes heap corruption.
    load_recents(s);
    load_favorites(s);

    // Switch back to recents as the default view
    s.list_mode = ListMode::Recents;
    s.current_index = 0;
    s.desc_scroll = 0;
    if !s.entries.is_empty() {
        s.status_message.clear();
    }

    // Pre-compute header and scroll indicator so retro_run is allocation-free
    update_header_text(s);
    update_scroll_indicator(s);

    debug_log("[replay-game-info] load_game: all pre-fetching complete");

    true
}

#[no_mangle]
pub extern "C" fn retro_load_game_special(
    _type: c_uint,
    _info: *const RetroGameInfo,
    _num: usize,
) -> bool {
    false
}

#[no_mangle]
pub unsafe extern "C" fn retro_unload_game() {
    let s = state();
    s.entries.clear();
    s.favorites.clear();
    s.detail_cache.clear();
    s.fav_detail_cache.clear();
}

// ─── Save state serialization ──────────────────────────────────────────────
//
// Layout (16 bytes):
//   [0..4)  current_index: u32
//   [4..8)  desc_scroll: u32
//   [8..9)  list_mode: u8 (0=Recents, 1=Favorites)
//   [9..16) reserved (zeroed)

const SAVE_STATE_SIZE: usize = 16;

#[no_mangle]
pub unsafe extern "C" fn retro_serialize_size() -> usize {
    SAVE_STATE_SIZE
}

#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    if size < SAVE_STATE_SIZE {
        return false;
    }
    let s = state();
    let buf = std::slice::from_raw_parts_mut(data as *mut u8, SAVE_STATE_SIZE);
    buf.fill(0);
    buf[0..4].copy_from_slice(&(s.current_index as u32).to_le_bytes());
    buf[4..8].copy_from_slice(&(s.desc_scroll as u32).to_le_bytes());
    buf[8] = match s.list_mode {
        ListMode::Recents => 0,
        ListMode::Favorites => 1,
    };
    true
}

#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if size < SAVE_STATE_SIZE {
        return false;
    }
    let s = state();
    let buf = std::slice::from_raw_parts(data as *const u8, SAVE_STATE_SIZE);
    s.current_index = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
    s.desc_scroll = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    s.list_mode = match buf[8] {
        1 => ListMode::Favorites,
        _ => ListMode::Recents,
    };
    true
}

#[no_mangle]
pub extern "C" fn retro_cheat_reset() {}

#[no_mangle]
pub extern "C" fn retro_cheat_set(_index: c_uint, _enabled: bool, _code: *const c_char) {}

#[no_mangle]
pub extern "C" fn retro_get_region() -> c_uint {
    RETRO_REGION_NTSC
}

#[no_mangle]
pub extern "C" fn retro_get_memory_data(_id: c_uint) -> *mut c_void {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn retro_get_memory_size(_id: c_uint) -> usize {
    0
}
