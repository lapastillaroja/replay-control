//! A libretro core written in Rust that serves as a ROM file viewer / inspector.
//!
//! When launched without a game (no-game mode), it shows the original hello world demo
//! with a bouncing box and movable cursor. When a ROM file is loaded, it provides three
//! interactive view modes:
//!
//!   - **Info screen**: filename, file size, CRC32 checksum, byte histogram
//!   - **Hex dump**: scrollable hex + ASCII view of the ROM data
//!   - **Visual mode**: raw bytes rendered as pixels (byte values mapped to colors)
//!
//! Controls:
//!   - D-pad Up/Down: scroll through data
//!   - D-pad Left/Right: scroll horizontally (hex mode) or page through data (visual mode)
//!   - A button: next view mode
//!   - B button: previous view mode
//!   - Select: toggle fast scroll (8x speed)
//!   - Start: reset view offset to beginning
//!
//! Target: `cdylib` (.so) for use with RetroArch on RePlayOS (aarch64) and x86_64.

#![allow(clippy::missing_safety_doc)]

use std::cell::UnsafeCell;
use std::ffi::CStr;
use std::os::raw::{c_char, c_uint, c_void};

// ─── Libretro constants ────────────────────────────────────────────────────

const RETRO_API_VERSION: c_uint = 1;

// Pixel formats
const RETRO_PIXEL_FORMAT_XRGB8888: c_uint = 2;

// Input device
const RETRO_DEVICE_JOYPAD: c_uint = 1;

// Joypad buttons
const RETRO_DEVICE_ID_JOYPAD_B: c_uint = 0;
const RETRO_DEVICE_ID_JOYPAD_Y: c_uint = 1;
const RETRO_DEVICE_ID_JOYPAD_SELECT: c_uint = 2;
const RETRO_DEVICE_ID_JOYPAD_START: c_uint = 3;
const RETRO_DEVICE_ID_JOYPAD_UP: c_uint = 4;
const RETRO_DEVICE_ID_JOYPAD_DOWN: c_uint = 5;
const RETRO_DEVICE_ID_JOYPAD_LEFT: c_uint = 6;
const RETRO_DEVICE_ID_JOYPAD_RIGHT: c_uint = 7;
const RETRO_DEVICE_ID_JOYPAD_A: c_uint = 8;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_X: c_uint = 9;
const RETRO_DEVICE_ID_JOYPAD_L: c_uint = 10;
const RETRO_DEVICE_ID_JOYPAD_R: c_uint = 11;

// Environment callback commands
const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;

// Region
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

// ─── View modes ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum ViewMode {
    Info = 0,
    HexDump = 1,
    Visual = 2,
}

impl ViewMode {
    fn next(self) -> Self {
        match self {
            ViewMode::Info => ViewMode::HexDump,
            ViewMode::HexDump => ViewMode::Visual,
            ViewMode::Visual => ViewMode::Info,
        }
    }

    fn prev(self) -> Self {
        match self {
            ViewMode::Info => ViewMode::Visual,
            ViewMode::HexDump => ViewMode::Info,
            ViewMode::Visual => ViewMode::HexDump,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ViewMode::Info => "INFO",
            ViewMode::HexDump => "HEX",
            ViewMode::Visual => "VISUAL",
        }
    }
}

// ─── CRC32 (no external crate) ────────────────────────────────────────────

/// Compute CRC32 using the standard polynomial (same as zlib/gzip).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ─── Core state (single-threaded — libretro guarantees this) ───────────────

const WIDTH: usize = 320;
const HEIGHT: usize = 240;

/// ROM data loaded from retro_load_game, stored on the heap.
struct RomData {
    /// The ROM bytes (copied from the frontend's buffer).
    bytes: Vec<u8>,
    /// Original filename (extracted from the path).
    filename: String,
    /// Precomputed CRC32 of the ROM data.
    crc32: u32,
    /// Byte frequency histogram (256 buckets).
    histogram: [u32; 256],
}

/// All mutable core state in one struct, wrapped in UnsafeCell for
/// interior mutability without `static mut` warnings.
/// Safety: libretro API is single-threaded by contract.
struct CoreState {
    // Libretro callbacks
    environment_cb: Option<RetroEnvironmentFn>,
    video_cb: Option<RetroVideoRefreshFn>,
    audio_sample_cb: Option<RetroAudioSampleFn>,
    audio_batch_cb: Option<RetroAudioSampleBatchFn>,
    input_poll_cb: Option<RetroInputPollFn>,
    input_state_cb: Option<RetroInputStateFn>,

    // Display
    framebuffer: [u32; WIDTH * HEIGHT],
    frame_count: u64,

    // No-game mode state (hello world demo)
    cursor_x: i32,
    cursor_y: i32,

    // ROM viewer state
    rom: Option<Box<RomData>>,
    view_mode: ViewMode,
    scroll_offset: usize, // byte offset for hex/visual views
    fast_scroll: bool,

    // Input debounce: track previous frame's button state for edge detection
    prev_a: bool,
    prev_b: bool,
    prev_select: bool,
    prev_start: bool,
    prev_y: bool,
    prev_l: bool,
    prev_r: bool,
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
    framebuffer: [0u32; WIDTH * HEIGHT],
    frame_count: 0,
    cursor_x: (WIDTH / 2) as i32,
    cursor_y: (HEIGHT / 2) as i32,
    rom: None,
    view_mode: ViewMode::Info,
    scroll_offset: 0,
    fast_scroll: false,
    prev_a: false,
    prev_b: false,
    prev_select: false,
    prev_start: false,
    prev_y: false,
    prev_l: false,
    prev_r: false,
}));

/// Get a mutable reference to core state.
/// Safety: only call from libretro callbacks (single-threaded by API contract).
#[inline(always)]
unsafe fn state() -> &'static mut CoreState {
    &mut *STATE.0.get()
}

// ─── Embedded 5x7 bitmap font ─────────────────────────────────────────────

/// Simple 5x7 pixel font for ASCII 32-127.
/// Each character is 5 columns wide, stored as 7 bytes (one per row),
/// where each byte encodes 5 pixels in the lower 5 bits.
fn get_char_bitmap(ch: u8) -> [u8; 7] {
    match ch {
        b' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        b'!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        b'"' => [0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        b'#' => [0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000],
        b'%' => [0b11001, 0b11010, 0b00100, 0b00100, 0b01011, 0b10011, 0b00000],
        b'*' => [0b00000, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0b00000],
        b'+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        b'=' => [0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000],
        b'<' => [0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010],
        b'>' => [0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000],
        b'[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        b']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        b'_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        b'|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        b'.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100],
        b',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b01000],
        b':' => [0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000],
        b';' => [0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000],
        b'-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        b'(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        b')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        b'/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        b'\\' => [0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001],
        b'@' => [0b01110, 0b10001, 0b10111, 0b10101, 0b10110, 0b10000, 0b01110],
        b'~' => [0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000],
        b'?' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00100, 0b00000, 0b00100],
        b'\'' => [0b00100, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],

        // Digits
        b'0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        b'1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        b'3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        b'4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        b'5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        b'6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        b'7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        b'8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        b'9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],

        // Uppercase letters
        b'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        b'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        b'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        b'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        b'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        b'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        b'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        b'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        b'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        b'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        b'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        b'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        b'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        b'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        b'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        b'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        b'S' => [0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110],
        b'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        b'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        b'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        b'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        b'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        b'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],

        // Lowercase letters
        b'a' => [0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
        b'b' => [0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110],
        b'c' => [0b00000, 0b00000, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110],
        b'd' => [0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b10001, 0b01111],
        b'e' => [0b00000, 0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110],
        b'f' => [0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000],
        b'g' => [0b00000, 0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b01110],
        b'h' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        b'i' => [0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'j' => [0b00010, 0b00000, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100],
        b'k' => [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010],
        b'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'm' => [0b00000, 0b00000, 0b11010, 0b10101, 0b10101, 0b10001, 0b10001],
        b'n' => [0b00000, 0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        b'o' => [0b00000, 0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
        b'p' => [0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000],
        b'q' => [0b00000, 0b00000, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001],
        b'r' => [0b00000, 0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000],
        b's' => [0b00000, 0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
        b't' => [0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110],
        b'u' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        b'v' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        b'w' => [0b00000, 0b00000, 0b10001, 0b10001, 0b10101, 0b10101, 0b01010],
        b'x' => [0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
        b'y' => [0b00000, 0b00000, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        b'z' => [0b00000, 0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111],

        // Default: filled block for unknown characters
        _ => [0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111],
    }
}

// ─── Drawing helpers ───────────────────────────────────────────────────────

/// Draw a single character at (x, y) with the given XRGB8888 color.
/// Scale multiplies both width and height.
fn draw_char(fb: &mut [u32], ch: u8, x: usize, y: usize, color: u32, scale: usize) {
    let bitmap = get_char_bitmap(ch);
    for (row, &bits) in bitmap.iter().enumerate() {
        for col in 0..5 {
            if bits & (1 << (4 - col)) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + col * scale + sx;
                        let py = y + row * scale + sy;
                        if px < WIDTH && py < HEIGHT {
                            fb[py * WIDTH + px] = color;
                        }
                    }
                }
            }
        }
    }
}

/// Draw a string at (x, y). Each character is 5*scale wide + 1*scale gap.
fn draw_string(fb: &mut [u32], text: &str, x: usize, y: usize, color: u32, scale: usize) {
    let char_width = 6 * scale; // 5 pixels + 1 gap, scaled
    for (i, ch) in text.bytes().enumerate() {
        let cx = x + i * char_width;
        if cx >= WIDTH {
            break; // clip at right edge
        }
        draw_char(fb, ch, cx, y, color, scale);
    }
}

/// Draw a filled rectangle.
fn draw_rect(fb: &mut [u32], x: usize, y: usize, w: usize, h: usize, color: u32) {
    for row in y..y.saturating_add(h).min(HEIGHT) {
        for col in x..x.saturating_add(w).min(WIDTH) {
            fb[row * WIDTH + col] = color;
        }
    }
}

/// Draw a small crosshair cursor.
fn draw_cursor(fb: &mut [u32], cx: i32, cy: i32, color: u32) {
    let size = 5i32;
    for d in -size..=size {
        let px = (cx + d) as usize;
        let py = cy as usize;
        if px < WIDTH && py < HEIGHT {
            fb[py * WIDTH + px] = color;
        }
        let px2 = cx as usize;
        let py2 = (cy + d) as usize;
        if px2 < WIDTH && py2 < HEIGHT {
            fb[py2 * WIDTH + px2] = color;
        }
    }
}

/// Draw a horizontal line.
fn draw_hline(fb: &mut [u32], x: usize, y: usize, w: usize, color: u32) {
    if y >= HEIGHT {
        return;
    }
    for col in x..x.saturating_add(w).min(WIDTH) {
        fb[y * WIDTH + col] = color;
    }
}

// ─── HSV to RGB helper ────────────────────────────────────────────────────

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r + m) * 255.0) as u32;
    let g = ((g + m) * 255.0) as u32;
    let b = ((b + m) * 255.0) as u32;

    (r << 16) | (g << 8) | b
}

/// Map a byte value to an XRGB8888 color for visual mode.
/// Uses a perceptually distinct palette: 0x00 = black, 0xFF = white,
/// with hue-mapped colors in between.
fn byte_to_color(byte: u8) -> u32 {
    if byte == 0 {
        return 0x00000000;
    }
    if byte == 0xFF {
        return 0x00FFFFFF;
    }
    let hue = (byte as f32 / 255.0) * 300.0; // 0-300 degrees (skip magenta wrap)
    let value = 0.4 + (byte as f32 / 255.0) * 0.6; // brighter for higher values
    hsv_to_rgb(hue, 0.85, value)
}

// ─── Hex formatting helpers ────────────────────────────────────────────────

/// Format a nibble (0-15) as a hex character.
fn hex_nibble(n: u8) -> u8 {
    if n < 10 {
        b'0' + n
    } else {
        b'A' + (n - 10)
    }
}

/// Format a byte as two hex characters into a buffer.
fn format_hex_byte(buf: &mut [u8], byte: u8) {
    buf[0] = hex_nibble(byte >> 4);
    buf[1] = hex_nibble(byte & 0x0F);
}

/// Format a u32 as 8 hex characters.
fn format_hex_u32(value: u32) -> [u8; 8] {
    let mut buf = [0u8; 8];
    for (i, slot) in buf.iter_mut().enumerate() {
        let nibble = ((value >> (28 - i * 4)) & 0xF) as u8;
        *slot = hex_nibble(nibble);
    }
    buf
}

/// Format file size as a human-readable string.
fn format_size(size: usize) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}

// ─── No-game mode: hello world demo ───────────────────────────────────────

fn render_hello_world(s: &mut CoreState) {
    let frame = s.frame_count;
    let fb = &mut s.framebuffer;

    // Clear to dark background with animated gradient
    let base_hue = (frame as f32 * 0.5) % 360.0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let hue = (base_hue + (y as f32 * 0.3)) % 360.0;
            fb[y * WIDTH + x] = hsv_to_rgb(hue, 0.3, 0.15);
        }
    }

    // Draw a decorative border
    let border_color = hsv_to_rgb((base_hue + 180.0) % 360.0, 0.8, 0.6);
    for x in 0..WIDTH {
        fb[x] = border_color;
        fb[(HEIGHT - 1) * WIDTH + x] = border_color;
    }
    for y in 0..HEIGHT {
        fb[y * WIDTH] = border_color;
        fb[y * WIDTH + WIDTH - 1] = border_color;
    }

    // Title text (large)
    let title = "HELLO REPLAYOS!";
    let title_x = (WIDTH - title.len() * 6 * 3) / 2;
    draw_string(fb, title, title_x + 2, 22, 0x00000000, 3);
    draw_string(fb, title, title_x, 20, 0x00FFFFFF, 3);

    // Subtitle
    let subtitle = "Rust libretro core";
    let sub_x = (WIDTH - subtitle.len() * 6 * 2) / 2;
    draw_string(fb, subtitle, sub_x, 50, 0x00AAAAFF, 2);

    // Info text (small)
    let info_lines = [
        "ROM File Viewer / Inspector",
        "Launch with a ROM to explore it!",
        "",
        "Supports: .bin .rom .ch8 .nes .sfc",
        "          .smc .gb .gbc .gba .md",
        "",
        "Use D-pad to move cursor.",
        "Press B to reset position.",
    ];

    for (i, line) in info_lines.iter().enumerate() {
        let ly = 78 + i * 12;
        draw_string(fb, line, 10, ly, 0x00CCCCCC, 1);
    }

    // Animated bouncing box
    let box_size = 20;
    let t = frame as f32 * 0.03;
    let bx = ((t.sin() * 0.5 + 0.5) * (WIDTH - box_size) as f32) as usize;
    let by = ((t.cos() * 0.7 + 0.5).clamp(0.0, 1.0) * (HEIGHT - box_size - 30) as f32) as usize
        + 195;
    let box_color = hsv_to_rgb((frame as f32 * 2.0) % 360.0, 0.9, 1.0);
    draw_rect(fb, bx, by, box_size, box_size, box_color);

    // Draw cursor
    let cursor_color = hsv_to_rgb(((frame as f32 * 3.0) + 120.0) % 360.0, 1.0, 1.0);
    draw_cursor(fb, s.cursor_x, s.cursor_y, cursor_color);

    // Frame counter at bottom
    let fc_text = format!("Frame: {}", frame);
    draw_string(fb, &fc_text, 10, HEIGHT - 16, 0x00888888, 1);

    // Version info at bottom right
    let version = "v0.2.0";
    let vx = WIDTH - version.len() * 6 - 10;
    draw_string(fb, version, vx, HEIGHT - 16, 0x00888888, 1);
}

// ─── ROM viewer: shared header/footer ──────────────────────────────────────

/// Vertical space for the status bar at top and bottom.
const HEADER_HEIGHT: usize = 12;
const FOOTER_HEIGHT: usize = 12;
/// Content area starts after the header.
const CONTENT_Y: usize = HEADER_HEIGHT + 2;
/// Usable content height.
const CONTENT_HEIGHT: usize = HEIGHT - HEADER_HEIGHT - FOOTER_HEIGHT - 4;

fn render_rom_header(fb: &mut [u32], rom: &RomData, mode: ViewMode, frame: u64) {
    // Dark header background
    draw_rect(fb, 0, 0, WIDTH, HEADER_HEIGHT, 0x00202040);
    draw_hline(fb, 0, HEADER_HEIGHT, WIDTH, 0x00404080);

    // Filename (truncated if needed)
    let max_name_chars = 30;
    let name = if rom.filename.len() > max_name_chars {
        &rom.filename[..max_name_chars]
    } else {
        &rom.filename
    };
    draw_string(fb, name, 4, 2, 0x00FFFFFF, 1);

    // Mode indicator on the right, with cycling color
    let mode_color = hsv_to_rgb((frame as f32 * 1.5) % 360.0, 0.7, 1.0);
    let mode_label = format!("[{}]", mode.label());
    let mode_x = WIDTH - mode_label.len() * 6 - 4;
    draw_string(fb, &mode_label, mode_x, 2, mode_color, 1);
}

fn render_rom_footer(fb: &mut [u32], rom: &RomData, scroll_offset: usize, fast: bool) {
    let footer_y = HEIGHT - FOOTER_HEIGHT;
    draw_rect(fb, 0, footer_y, WIDTH, FOOTER_HEIGHT, 0x00202040);
    draw_hline(fb, 0, footer_y, WIDTH, 0x00404080);

    // Offset and size info
    let offset_text = format!(
        "{:06X}/{:06X}",
        scroll_offset,
        rom.bytes.len().saturating_sub(1)
    );
    draw_string(fb, &offset_text, 4, footer_y + 2, 0x00AAAAAA, 1);

    // Control hints
    let hints = if fast { "A/B:mode SEL:fast*" } else { "A/B:mode SEL:fast" };
    let hints_x = WIDTH - hints.len() * 6 - 4;
    draw_string(fb, hints, hints_x, footer_y + 2, 0x00888888, 1);
}

// ─── ROM viewer: info screen ───────────────────────────────────────────────

fn render_info_screen(s: &mut CoreState) {
    let fb = &mut s.framebuffer;

    // Clear background
    for px in fb.iter_mut() {
        *px = 0x00101020;
    }

    let rom = s.rom.as_ref().unwrap();
    render_rom_header(fb, rom, ViewMode::Info, s.frame_count);
    render_rom_footer(fb, rom, s.scroll_offset, s.fast_scroll);

    let mut y = CONTENT_Y + 4;
    let line_height = 12;

    // Title
    draw_string(fb, "ROM FILE INFO", 10, y, 0x0066AAFF, 2);
    y += 22;
    draw_hline(fb, 10, y, 200, 0x00334466);
    y += 6;

    // Filename
    draw_string(fb, "File:", 10, y, 0x00888888, 1);
    let display_name = if rom.filename.len() > 40 {
        format!("{}...", &rom.filename[..37])
    } else {
        rom.filename.clone()
    };
    draw_string(fb, &display_name, 46, y, 0x00FFFFFF, 1);
    y += line_height;

    // Size
    draw_string(fb, "Size:", 10, y, 0x00888888, 1);
    let size_text = format!("{} ({} bytes)", format_size(rom.bytes.len()), rom.bytes.len());
    draw_string(fb, &size_text, 46, y, 0x00FFFFFF, 1);
    y += line_height;

    // CRC32
    draw_string(fb, "CRC:", 10, y, 0x00888888, 1);
    let crc_hex = format_hex_u32(rom.crc32);
    let crc_str = std::str::from_utf8(&crc_hex).unwrap_or("????????");
    draw_string(fb, crc_str, 46, y, 0x0066FF66, 1);
    y += line_height;

    // First bytes preview
    draw_string(fb, "Head:", 10, y, 0x00888888, 1);
    let preview_len = rom.bytes.len().min(12);
    let mut hex_buf = String::with_capacity(preview_len * 3);
    for (i, &byte) in rom.bytes[..preview_len].iter().enumerate() {
        if i > 0 {
            hex_buf.push(' ');
        }
        let mut hb = [0u8; 2];
        format_hex_byte(&mut hb, byte);
        hex_buf.push(hb[0] as char);
        hex_buf.push(hb[1] as char);
    }
    if rom.bytes.len() > preview_len {
        hex_buf.push_str("..");
    }
    draw_string(fb, &hex_buf, 46, y, 0x00FFAA44, 1);
    y += line_height + 6;

    // Byte histogram
    draw_string(fb, "BYTE DISTRIBUTION", 10, y, 0x0066AAFF, 1);
    y += 14;

    // Find max histogram value for scaling
    let max_count = *rom.histogram.iter().max().unwrap_or(&1);
    let hist_height = 60usize;
    let hist_width = 256usize; // 1 pixel per byte value
    let hist_x = (WIDTH - hist_width) / 2;

    // Draw histogram bars
    for i in 0..256 {
        let count = rom.histogram[i];
        if count == 0 {
            continue;
        }
        let bar_height = ((count as u64 * hist_height as u64) / max_count as u64) as usize;
        let bar_height = bar_height.max(1);
        let color = byte_to_color(i as u8);
        let bar_x = hist_x + i;
        for row in 0..bar_height {
            let py = y + hist_height - 1 - row;
            if bar_x < WIDTH && py < HEIGHT {
                fb[py * WIDTH + bar_x] = color;
            }
        }
    }

    // Axis labels
    let axis_y = y + hist_height + 2;
    draw_string(fb, "00", hist_x, axis_y, 0x00666666, 1);
    draw_string(fb, "80", hist_x + 128 - 6, axis_y, 0x00666666, 1);
    draw_string(fb, "FF", hist_x + 254 - 6, axis_y, 0x00666666, 1);

    // Entropy estimate
    let total = rom.bytes.len() as f64;
    if total > 0.0 {
        let mut entropy = 0.0f64;
        for &count in &rom.histogram {
            if count > 0 {
                let p = count as f64 / total;
                entropy -= p * p.log2();
            }
        }
        let entropy_text = format!("Entropy: {:.2} bits/byte (max 8.00)", entropy);
        draw_string(fb, &entropy_text, 10, axis_y + 12, 0x00AAAAAA, 1);
    }
}

// ─── ROM viewer: hex dump ──────────────────────────────────────────────────

/// Number of bytes per hex dump row.
const HEX_BYTES_PER_ROW: usize = 16;
/// Rows that fit in the content area (8px per row at scale 1).
const HEX_ROWS: usize = CONTENT_HEIGHT / 10;

fn render_hex_dump(s: &mut CoreState) {
    let fb = &mut s.framebuffer;

    // Clear background
    for px in fb.iter_mut() {
        *px = 0x00101020;
    }

    let rom = s.rom.as_ref().unwrap();
    render_rom_header(fb, rom, ViewMode::HexDump, s.frame_count);
    render_rom_footer(fb, rom, s.scroll_offset, s.fast_scroll);

    // Align scroll offset to row boundary
    let aligned_offset = (s.scroll_offset / HEX_BYTES_PER_ROW) * HEX_BYTES_PER_ROW;

    let mut y = CONTENT_Y;
    let line_height = 10;

    // Column header
    //   "OFFSET   00 01 02 03 ... 0F  ASCII"
    draw_string(fb, "OFFSET", 4, y, 0x00666688, 1);
    for col in 0..HEX_BYTES_PER_ROW {
        let col_x = 52 + col * 12;
        if col_x + 12 > WIDTH {
            break;
        }
        let mut hb = [0u8; 2];
        format_hex_byte(&mut hb, col as u8);
        let s = std::str::from_utf8(&hb).unwrap_or("??");
        draw_string(fb, s, col_x, y, 0x00666688, 1);
    }
    y += line_height;
    draw_hline(fb, 4, y, WIDTH - 8, 0x00333355);
    y += 2;

    // Data rows
    for row in 0..HEX_ROWS {
        let row_offset = aligned_offset + row * HEX_BYTES_PER_ROW;
        if row_offset >= rom.bytes.len() {
            break;
        }

        if y + 8 >= HEIGHT - FOOTER_HEIGHT {
            break;
        }

        // Row offset (6 hex digits)
        let offset_hex = format_hex_u32(row_offset as u32);
        let offset_str = std::str::from_utf8(&offset_hex[2..]).unwrap_or("??????");
        draw_string(fb, offset_str, 4, y, 0x00446688, 1);

        // Hex bytes
        let row_end = (row_offset + HEX_BYTES_PER_ROW).min(rom.bytes.len());
        for col in 0..(row_end - row_offset) {
            let byte = rom.bytes[row_offset + col];
            let col_x = 52 + col * 12;
            if col_x + 12 > WIDTH {
                break;
            }
            let mut hb = [0u8; 2];
            format_hex_byte(&mut hb, byte);
            let hex_str = std::str::from_utf8(&hb).unwrap_or("??");
            // Color: 00 bytes are dim, printable ASCII is bright, others are medium
            let color = if byte == 0 {
                0x00444444
            } else if (0x20..=0x7E).contains(&byte) {
                0x00FFFFFF
            } else {
                0x00AABB88
            };
            draw_string(fb, hex_str, col_x, y, color, 1);
        }

        // ASCII column (only if it fits)
        let ascii_x = 52 + HEX_BYTES_PER_ROW * 12 + 4;
        if ascii_x < WIDTH {
            // Separator
            if y < HEIGHT {
                let sep_x = ascii_x - 3;
                if sep_x < WIDTH {
                    for dy in 0..8usize {
                        if y + dy < HEIGHT {
                            fb[(y + dy) * WIDTH + sep_x] = 0x00333355;
                        }
                    }
                }
            }

            for col in 0..(row_end - row_offset) {
                let byte = rom.bytes[row_offset + col];
                let ch = if (0x20..=0x7E).contains(&byte) {
                    byte
                } else {
                    b'.'
                };
                let char_x = ascii_x + col * 6;
                if char_x + 6 > WIDTH {
                    break;
                }
                let color = if byte == 0 {
                    0x00444444
                } else if (0x20..=0x7E).contains(&byte) {
                    0x0088CCFF
                } else {
                    0x00666666
                };
                draw_char(fb, ch, char_x, y, color, 1);
            }
        }

        y += line_height;
    }
}

// ─── ROM viewer: visual mode ───────────────────────────────────────────────

fn render_visual(s: &mut CoreState) {
    let fb = &mut s.framebuffer;

    // Clear background
    for px in fb.iter_mut() {
        *px = 0x00080810;
    }

    let rom = s.rom.as_ref().unwrap();
    render_rom_header(fb, rom, ViewMode::Visual, s.frame_count);
    render_rom_footer(fb, rom, s.scroll_offset, s.fast_scroll);

    // Pixel area: fill the content region with colored pixels from ROM data.
    // Each byte becomes one pixel. We use a 2x2 scale for visibility.
    let pixel_scale = 2usize;
    let cols = (WIDTH - 8) / pixel_scale; // pixels per row
    let rows = CONTENT_HEIGHT / pixel_scale;

    let area_x = (WIDTH - cols * pixel_scale) / 2;
    let area_y = CONTENT_Y + 1;

    for row in 0..rows {
        for col in 0..cols {
            let byte_idx = s.scroll_offset + row * cols + col;
            let color = if byte_idx < rom.bytes.len() {
                byte_to_color(rom.bytes[byte_idx])
            } else {
                0x00080810 // past end of file
            };

            // Draw a pixel_scale x pixel_scale block
            let px = area_x + col * pixel_scale;
            let py = area_y + row * pixel_scale;
            for dy in 0..pixel_scale {
                for dx in 0..pixel_scale {
                    let fx = px + dx;
                    let fy = py + dy;
                    if fx < WIDTH && fy < HEIGHT {
                        fb[fy * WIDTH + fx] = color;
                    }
                }
            }
        }
    }

    // Show how many bytes are visible per screen
    let bytes_per_screen = cols * rows;
    let pct = if !rom.bytes.is_empty() {
        (s.scroll_offset as f64 / rom.bytes.len() as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let info = format!("{}x{} ({} B/screen) {:.0}%", cols, rows, bytes_per_screen, pct);
    let info_x = (WIDTH - info.len() * 6) / 2;
    // Draw on top of footer area
    let footer_y = HEIGHT - FOOTER_HEIGHT;
    draw_string(fb, &info, info_x, footer_y + 2, 0x00AAAAAA, 1);
}

// ─── Input handling for ROM viewer ─────────────────────────────────────────

/// Process input for ROM viewer modes. Returns true if a button was freshly pressed.
fn handle_rom_input(s: &mut CoreState) {
    let input_state = match s.input_state_cb {
        Some(cb) => cb,
        None => return,
    };

    // Read current button states
    let btn = |id: c_uint| -> bool {
        unsafe { input_state(0, RETRO_DEVICE_JOYPAD, 0, id) != 0 }
    };

    let up = btn(RETRO_DEVICE_ID_JOYPAD_UP);
    let down = btn(RETRO_DEVICE_ID_JOYPAD_DOWN);
    let left = btn(RETRO_DEVICE_ID_JOYPAD_LEFT);
    let right = btn(RETRO_DEVICE_ID_JOYPAD_RIGHT);
    let a_pressed = btn(RETRO_DEVICE_ID_JOYPAD_A);
    let b_pressed = btn(RETRO_DEVICE_ID_JOYPAD_B);
    let select_pressed = btn(RETRO_DEVICE_ID_JOYPAD_SELECT);
    let start_pressed = btn(RETRO_DEVICE_ID_JOYPAD_START);
    let y_pressed = btn(RETRO_DEVICE_ID_JOYPAD_Y);
    let l_pressed = btn(RETRO_DEVICE_ID_JOYPAD_L);
    let r_pressed = btn(RETRO_DEVICE_ID_JOYPAD_R);

    let rom_len = s.rom.as_ref().map(|r| r.bytes.len()).unwrap_or(0);

    // A: next view mode (edge-triggered)
    if a_pressed && !s.prev_a {
        s.view_mode = s.view_mode.next();
        s.scroll_offset = 0;
    }

    // B: previous view mode (edge-triggered)
    if b_pressed && !s.prev_b {
        s.view_mode = s.view_mode.prev();
        s.scroll_offset = 0;
    }

    // Select: toggle fast scroll (edge-triggered)
    if select_pressed && !s.prev_select {
        s.fast_scroll = !s.fast_scroll;
    }

    // Start: reset to beginning (edge-triggered)
    if start_pressed && !s.prev_start {
        s.scroll_offset = 0;
    }

    // Y: jump to end (edge-triggered)
    if y_pressed && !s.prev_y && rom_len > 0 {
        s.scroll_offset = rom_len.saturating_sub(HEX_BYTES_PER_ROW);
    }

    // L/R: page up/down (edge-triggered)
    let page_size = match s.view_mode {
        ViewMode::HexDump => HEX_BYTES_PER_ROW * HEX_ROWS,
        ViewMode::Visual => {
            let pixel_scale = 2usize;
            let cols = (WIDTH - 8) / pixel_scale;
            let rows = CONTENT_HEIGHT / pixel_scale;
            cols * rows
        }
        ViewMode::Info => 0,
    };

    if l_pressed && !s.prev_l {
        s.scroll_offset = s.scroll_offset.saturating_sub(page_size);
    }
    if r_pressed && !s.prev_r {
        s.scroll_offset = s.scroll_offset.saturating_add(page_size).min(rom_len.saturating_sub(1));
    }

    // D-pad: continuous scrolling (held buttons)
    let scroll_step = if s.fast_scroll { 8 } else { 1 };

    match s.view_mode {
        ViewMode::Info => {
            // No scrolling on info screen
        }
        ViewMode::HexDump => {
            if up {
                s.scroll_offset = s
                    .scroll_offset
                    .saturating_sub(HEX_BYTES_PER_ROW * scroll_step);
            }
            if down {
                s.scroll_offset = s
                    .scroll_offset
                    .saturating_add(HEX_BYTES_PER_ROW * scroll_step)
                    .min(rom_len.saturating_sub(1));
            }
            // Left/right scroll by 1 byte (allows fine-grained offset)
            if left {
                s.scroll_offset = s.scroll_offset.saturating_sub(scroll_step);
            }
            if right {
                s.scroll_offset = s
                    .scroll_offset
                    .saturating_add(scroll_step)
                    .min(rom_len.saturating_sub(1));
            }
        }
        ViewMode::Visual => {
            let pixel_scale = 2usize;
            let cols = (WIDTH - 8) / pixel_scale;
            let row_bytes = cols; // 1 byte per pixel column

            if up {
                s.scroll_offset = s
                    .scroll_offset
                    .saturating_sub(row_bytes * scroll_step);
            }
            if down {
                s.scroll_offset = s
                    .scroll_offset
                    .saturating_add(row_bytes * scroll_step)
                    .min(rom_len.saturating_sub(1));
            }
            if left {
                // Page backward
                let page = row_bytes * (CONTENT_HEIGHT / pixel_scale);
                s.scroll_offset = s.scroll_offset.saturating_sub(page);
            }
            if right {
                // Page forward
                let page = row_bytes * (CONTENT_HEIGHT / pixel_scale);
                s.scroll_offset = s
                    .scroll_offset
                    .saturating_add(page)
                    .min(rom_len.saturating_sub(1));
            }
        }
    }

    // Update previous button states
    s.prev_a = a_pressed;
    s.prev_b = b_pressed;
    s.prev_select = select_pressed;
    s.prev_start = start_pressed;
    s.prev_y = y_pressed;
    s.prev_l = l_pressed;
    s.prev_r = r_pressed;
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

    // Tell the frontend we support running without game content
    let mut no_game: bool = true;
    cb(
        RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
        &mut no_game as *mut bool as *mut c_void,
    );

    // Set pixel format to XRGB8888
    let mut pixel_format: c_uint = RETRO_PIXEL_FORMAT_XRGB8888;
    cb(
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
        &mut pixel_format as *mut c_uint as *mut c_void,
    );
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
    let s = state();
    s.frame_count = 0;
    s.cursor_x = WIDTH as i32 / 2;
    s.cursor_y = HEIGHT as i32 / 2;
    s.view_mode = ViewMode::Info;
    s.scroll_offset = 0;
    s.fast_scroll = false;
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
    s.rom = None;
}

// Static C strings for system info (must live for the entire program lifetime)
static LIBRARY_NAME: &[u8] = b"RePlay ROM Viewer\0";
static LIBRARY_VERSION: &[u8] = b"0.2.0\0";
static VALID_EXTENSIONS: &[u8] = b"bin|rom|ch8|nes|sfc|smc|gb|gbc|gba|md|sms|gg|pce|a26|col|int|sg|ngp|ngc|ws|wsc|vb|vec\0";

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    (*info).library_name = LIBRARY_NAME.as_ptr() as *const c_char;
    (*info).library_version = LIBRARY_VERSION.as_ptr() as *const c_char;
    (*info).valid_extensions = VALID_EXTENSIONS.as_ptr() as *const c_char;
    (*info).need_fullpath = false; // we want the data in memory
    (*info).block_extract = false;
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    (*info).geometry.base_width = WIDTH as c_uint;
    (*info).geometry.base_height = HEIGHT as c_uint;
    (*info).geometry.max_width = WIDTH as c_uint;
    (*info).geometry.max_height = HEIGHT as c_uint;
    (*info).geometry.aspect_ratio = WIDTH as f32 / HEIGHT as f32;
    (*info).timing.fps = 60.0;
    (*info).timing.sample_rate = 44100.0;
}

#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: c_uint, _device: c_uint) {
    // No-op for this simple core
}

#[no_mangle]
pub unsafe extern "C" fn retro_reset() {
    let s = state();
    s.frame_count = 0;
    s.cursor_x = WIDTH as i32 / 2;
    s.cursor_y = HEIGHT as i32 / 2;
    s.view_mode = ViewMode::Info;
    s.scroll_offset = 0;
    s.fast_scroll = false;
}

#[no_mangle]
pub unsafe extern "C" fn retro_run() {
    let s = state();

    // Poll input
    if let Some(poll) = s.input_poll_cb {
        poll();
    }

    // Branch: ROM loaded → ROM viewer, no ROM → hello world demo
    if s.rom.is_some() {
        handle_rom_input(s);

        match s.view_mode {
            ViewMode::Info => render_info_screen(s),
            ViewMode::HexDump => render_hex_dump(s),
            ViewMode::Visual => render_visual(s),
        }
    } else {
        // No-game mode: original hello world with cursor
        if let Some(input_state) = s.input_state_cb {
            let speed = 2;

            if input_state(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP) != 0 {
                s.cursor_y -= speed;
            }
            if input_state(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN) != 0 {
                s.cursor_y += speed;
            }
            if input_state(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT) != 0 {
                s.cursor_x -= speed;
            }
            if input_state(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT) != 0 {
                s.cursor_x += speed;
            }
            if input_state(0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B) != 0 {
                s.cursor_x = WIDTH as i32 / 2;
                s.cursor_y = HEIGHT as i32 / 2;
            }
            s.cursor_x = s.cursor_x.clamp(0, WIDTH as i32 - 1);
            s.cursor_y = s.cursor_y.clamp(0, HEIGHT as i32 - 1);
        }

        render_hello_world(s);
    }

    s.frame_count += 1;

    // Send frame to frontend
    if let Some(video) = s.video_cb {
        video(
            s.framebuffer.as_ptr() as *const c_void,
            WIDTH as c_uint,
            HEIGHT as c_uint,
            WIDTH * std::mem::size_of::<u32>(),
        );
    }

    // Send silence for audio (required to keep the frontend happy)
    if let Some(audio_batch) = s.audio_batch_cb {
        // 44100 Hz / 60 fps = ~735 samples per frame
        let silence = [0i16; 735 * 2]; // stereo
        audio_batch(silence.as_ptr(), 735);
    }
}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    let s = state();

    if game.is_null() {
        eprintln!("[replay-rom-viewer] load_game: no-game mode");
        s.rom = None;
        return true;
    }

    let data_ptr = (*game).data;
    let data_size = (*game).size;

    // Extract filename from path
    let filename = if !(*game).path.is_null() {
        let path_cstr = CStr::from_ptr((*game).path);
        let path_str = path_cstr.to_str().unwrap_or("<invalid utf8>");
        eprintln!(
            "[replay-rom-viewer] load_game: path={}, size={}",
            path_str, data_size
        );
        // Extract just the filename from the full path
        path_str
            .rsplit('/')
            .next()
            .unwrap_or(path_str)
            .rsplit('\\')
            .next()
            .unwrap_or(path_str)
            .to_string()
    } else {
        eprintln!(
            "[replay-rom-viewer] load_game: no path, size={}",
            data_size
        );
        "<unknown>".to_string()
    };

    // Copy ROM data from the frontend's buffer
    if data_ptr.is_null() || data_size == 0 {
        eprintln!("[replay-rom-viewer] load_game: no data provided, entering no-game mode");
        s.rom = None;
        return true;
    }

    let bytes = std::slice::from_raw_parts(data_ptr as *const u8, data_size).to_vec();

    // Compute CRC32
    let checksum = crc32(&bytes);

    // Build histogram
    let mut histogram = [0u32; 256];
    for &byte in &bytes {
        histogram[byte as usize] += 1;
    }

    eprintln!(
        "[replay-rom-viewer] ROM loaded: {} ({} bytes, CRC32={:08X})",
        filename, bytes.len(), checksum
    );

    s.rom = Some(Box::new(RomData {
        bytes,
        filename,
        crc32: checksum,
        histogram,
    }));

    // Reset view state
    s.view_mode = ViewMode::Info;
    s.scroll_offset = 0;
    s.fast_scroll = false;

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
    s.rom = None;
    s.view_mode = ViewMode::Info;
    s.scroll_offset = 0;
}

// ─── Save state serialization ──────────────────────────────────────────────
//
// Save state layout (32 bytes):
//   [0..8)   frame_count: u64
//   [8..12)  cursor_x: i32
//   [12..16) cursor_y: i32
//   [16..20) scroll_offset: u32  (truncated to 32-bit, supports ROMs up to 4GB)
//   [20..21) view_mode: u8
//   [21..22) fast_scroll: u8 (0 or 1)
//   [22..32) reserved (zeroed)

const SAVE_STATE_SIZE: usize = 32;

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
    buf[0..8].copy_from_slice(&s.frame_count.to_le_bytes());
    buf[8..12].copy_from_slice(&s.cursor_x.to_le_bytes());
    buf[12..16].copy_from_slice(&s.cursor_y.to_le_bytes());
    buf[16..20].copy_from_slice(&(s.scroll_offset as u32).to_le_bytes());
    buf[20] = s.view_mode as u8;
    buf[21] = s.fast_scroll as u8;
    true
}

#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if size < SAVE_STATE_SIZE {
        return false;
    }
    let s = state();
    let buf = std::slice::from_raw_parts(data as *const u8, SAVE_STATE_SIZE);
    s.frame_count = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    s.cursor_x = i32::from_le_bytes(buf[8..12].try_into().unwrap());
    s.cursor_y = i32::from_le_bytes(buf[12..16].try_into().unwrap());
    s.scroll_offset = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize;
    s.view_mode = match buf[20] {
        0 => ViewMode::Info,
        1 => ViewMode::HexDump,
        2 => ViewMode::Visual,
        _ => ViewMode::Info,
    };
    s.fast_scroll = buf[21] != 0;
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
