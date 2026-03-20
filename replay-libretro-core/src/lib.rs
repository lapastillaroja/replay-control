//! Recently Played Game Detail Viewer -- a libretro core for RePlayOS.
//!
//! Fetches recently played games from the Replay Control REST API (localhost:8080)
//! and displays rich game metadata on the TV screen, navigable with a gamepad.
//!
//! Navigation flow:
//!   Boot -> Home Screen -> A: enter selected list -> Game Detail -> B: back to Home -> B: exit
//!
//! Home screen controls:
//!   - D-pad Up/Down: select menu item (Recently Played / Favorites)
//!   - A button: open selected list
//!   - B button: exit core (RETRO_ENVIRONMENT_SHUTDOWN)
//!
//! Game detail controls:
//!   - D-pad Left/Right: navigate between games
//!   - D-pad Up/Down: scroll description text (Page 2 only)
//!   - L1/R1: cycle between pages (Game Info / Description)
//!   - B button: back to Home screen
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

mod draw;
mod font;
mod http;
mod json;
mod layout;
mod palette;
mod util;

use std::cell::UnsafeCell;
use std::os::raw::{c_char, c_uint, c_void};

use draw::{
    blit_image, draw_hline, draw_rating, draw_rect, draw_string, draw_string_truncated, word_wrap,
};
use http::{
    fetch_box_art_for_layout, fetch_favorites, fetch_recents, fetch_rom_detail,
    MAX_BOX_ART_PREFETCH,
};
use layout::LayoutConfig;
use palette::CorePalette;
use util::debug_log;

// ---- Libretro constants ----

const RETRO_API_VERSION: c_uint = 1;
#[allow(dead_code)]
const RETRO_PIXEL_FORMAT_RGB565: c_uint = 2;
const RETRO_DEVICE_JOYPAD: c_uint = 1;

const RETRO_DEVICE_ID_JOYPAD_B: c_uint = 0;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_Y: c_uint = 1;
#[allow(dead_code)]
const RETRO_DEVICE_ID_JOYPAD_SELECT: c_uint = 2;
#[allow(dead_code)]
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

const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 16;

const RETRO_REGION_NTSC: c_uint = 0;

// ---- Libretro structs ----

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

// ---- Callback function pointer types ----

type RetroEnvironmentFn = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
type RetroVideoRefreshFn =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
type RetroAudioSampleFn = unsafe extern "C" fn(left: i16, right: i16);
type RetroAudioSampleBatchFn = unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
type RetroInputPollFn = unsafe extern "C" fn();
type RetroInputStateFn =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;

// ---- Game data structures ----

/// Top-level view in the core's state machine.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CoreView {
    Home,       // Home screen with menu options
    GameDetail, // Game detail view (recents or favorites)
}

/// Which list we're currently viewing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMode {
    Recents,
    Favorites,
}

/// Which page of the game detail view is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageKind {
    GameInfo,    // Page 1: box art + metadata, no description
    Description, // Page 2: full-width scrollable description
}

const PAGE_COUNT: usize = 2;
const PAGES: [PageKind; PAGE_COUNT] = [PageKind::GameInfo, PageKind::Description];

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
    /// Word-wrapped description lines for the narrow column (next to box art)
    desc_lines: Vec<String>,
    /// Word-wrapped description lines for full-width Page 2 (wider wrap)
    desc_lines_full: Vec<String>,
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

// ---- Pixel format tracking ----

/// Which pixel format to use for video output.
///
/// RePlayOS accepts SET_PIXEL_FORMAT(XRGB8888). Using 32-bit direct output
/// avoids the per-pixel BGR565 conversion and gives correct colors natively.
///
/// Internal rendering uses u32 (0x00RRGGBB) which IS XRGB8888 -- no conversion needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelFormat {
    /// BGR565 (16-bit): BBBBBGGGGGGRRRRR -- fallback format
    #[allow(dead_code)]
    Bgr565,
    /// XRGB8888 (32-bit) -- direct output, no conversion needed
    Xrgb8888,
}

// ---- Core state (grouped into sub-structs) ----

/// Libretro frontend callbacks.
struct CallbackState {
    environment_cb: Option<RetroEnvironmentFn>,
    video_cb: Option<RetroVideoRefreshFn>,
    #[allow(dead_code)]
    audio_sample_cb: Option<RetroAudioSampleFn>,
    audio_batch_cb: Option<RetroAudioSampleBatchFn>,
    input_poll_cb: Option<RetroInputPollFn>,
    input_state_cb: Option<RetroInputStateFn>,
}

/// Display / video state.
///
/// Uses double-buffering to prevent the host frontend from reading a
/// partially-rendered frame. The core renders into `framebuffer` (back),
/// then copies the completed frame into `present_buffer` (front) before
/// handing the pointer to the video callback. The host's DRM/display
/// thread can safely read `present_buffer` at any time.
struct DisplayState {
    framebuffer: Vec<u32>,
    /// Front buffer: completed frame handed to the host video callback.
    /// The host (and its DRM thread) may read this at any time, so it
    /// must never be resized or reallocated after retro_load_game.
    present_buffer: Vec<u32>,
    /// Conversion buffer for 16-bit pixel formats (0RGB1555 / RGB565)
    framebuffer_16: Vec<u16>,
    /// Conversion front buffer for 16-bit output
    present_buffer_16: Vec<u16>,
    pixel_format: PixelFormat,
    frame_count: u64,
    layout: LayoutConfig,
    /// Active skin color palette, loaded once in retro_load_game.
    palette: CorePalette,
}

/// Game data and navigation state.
struct GameData {
    /// Current top-level view (Home or GameDetail)
    view: CoreView,
    /// Home screen menu cursor (0 = Recently Played, 1 = Favorites)
    home_cursor: usize,
    list_mode: ListMode,
    entries: Vec<GameEntry>,
    favorites: Vec<GameEntry>,
    current_index: usize,
    detail_cache: Vec<Option<GameDetail>>,
    fav_detail_cache: Vec<Option<GameDetail>>,
    desc_scroll: usize,
    /// Current page index into PAGES array (0 = GameInfo)
    current_page: usize,
    /// Saved cursor position in the recents list (remembered across list switches)
    recents_index: usize,
    /// Saved cursor position in the favorites list (remembered across list switches)
    favorites_index: usize,
}

/// Network / API state.
struct NetworkState {
    api_available: bool,
    status_message: String,
    loading: bool,
}

/// Input debounce state.
struct InputState {
    prev_left: bool,
    prev_right: bool,
    prev_b: bool,
    #[allow(dead_code)]
    prev_start: bool,
    prev_l1: bool,
    prev_r1: bool,
    prev_a: bool,
    prev_up: bool,
    prev_down: bool,
    /// Up/down are held (continuous scroll), but with frame delay
    scroll_cooldown: u32,
}

/// Pre-allocated scratch buffers for allocation-free rendering.
/// These are written once per navigation event, then read every frame.
struct ScratchBuffers {
    /// "RECENTLY PLAYED  (1/93)" or "FAVORITES  (2/10)"
    header_text: String,
    /// "[1/5]" scroll position indicator
    scroll_indicator: String,
    /// Pre-computed count text for recents menu item (e.g., "93")
    recents_count_text: String,
    /// Pre-computed count text for favorites menu item (e.g., "57")
    favorites_count_text: String,
}

struct CoreState {
    cb: CallbackState,
    display: DisplayState,
    game: GameData,
    net: NetworkState,
    input: InputState,
    scratch: ScratchBuffers,
}

unsafe impl Sync for CoreStateWrapper {}
struct CoreStateWrapper(UnsafeCell<CoreState>);

static STATE: CoreStateWrapper = CoreStateWrapper(UnsafeCell::new(CoreState {
    cb: CallbackState {
        environment_cb: None,
        video_cb: None,
        audio_sample_cb: None,
        audio_batch_cb: None,
        input_poll_cb: None,
        input_state_cb: None,
    },
    display: DisplayState {
        framebuffer: Vec::new(),
        present_buffer: Vec::new(),
        framebuffer_16: Vec::new(),
        present_buffer_16: Vec::new(),
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
            full_chars_per_line: 33,
            max_desc_lines_full: 8,
            show_extra_metadata: false,
        },
        palette: palette::load_palette(0),
    },
    game: GameData {
        view: CoreView::Home,
        home_cursor: 0,
        list_mode: ListMode::Recents,
        entries: Vec::new(),
        favorites: Vec::new(),
        current_index: 0,
        detail_cache: Vec::new(),
        fav_detail_cache: Vec::new(),
        desc_scroll: 0,
        current_page: 0,
        recents_index: 0,
        favorites_index: 0,
    },
    net: NetworkState {
        api_available: false,
        status_message: String::new(),
        loading: false,
    },
    input: InputState {
        prev_left: false,
        prev_right: false,
        prev_b: false,
        prev_start: false,
        prev_l1: false,
        prev_r1: false,
        prev_a: false,
        prev_up: false,
        prev_down: false,
        scroll_cooldown: 0,
    },
    scratch: ScratchBuffers {
        header_text: String::new(),
        scroll_indicator: String::new(),
        recents_count_text: String::new(),
        favorites_count_text: String::new(),
    },
}));

#[inline(always)]
unsafe fn state() -> &'static mut CoreState {
    &mut *STATE.0.get()
}

// Colors are now loaded from the active skin palette at retro_load_game time.
// See palette.rs for the CorePalette struct and the 11 built-in skin palettes.

// ---- Pre-computation helpers ----

/// Build the pre-computed display strings for a GameDetail.
/// Called once during prefetch so rendering never allocates.
fn precompute_display(
    detail: &GameDetail,
    chars_per_line: usize,
    full_chars_per_line: usize,
) -> PrecomputedDisplay {
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

    let desc_lines_full = match detail.description.as_ref() {
        Some(desc) => word_wrap(desc, full_chars_per_line),
        None => Vec::new(),
    };

    PrecomputedDisplay {
        sys_year_line,
        players_text,
        rating_text,
        desc_lines,
        desc_lines_full,
    }
}

/// Rewrite the header_text scratch buffer in-place.
/// Called when the user navigates (left/right/enter list), not every frame.
fn update_header_text(s: &mut CoreState) {
    let (entries, _) = active_list(&s.game);
    let total = entries.len();
    let mode_label = match s.game.list_mode {
        ListMode::Recents => "RECENTLY PLAYED",
        ListMode::Favorites => "FAVORITES",
    };
    s.scratch.header_text.clear();
    if total > 0 {
        use std::fmt::Write;
        let _ = write!(
            s.scratch.header_text,
            "{}  ({}/{})",
            mode_label,
            s.game.current_index + 1,
            total
        );
    }
}

/// Rewrite the scroll_indicator scratch buffer in-place.
/// Called when the user scrolls or navigates to a new game.
/// Uses full-width lines on Page 2, narrow lines otherwise.
fn update_scroll_indicator(s: &mut CoreState) {
    s.scratch.scroll_indicator.clear();

    let detail_cache = match s.game.list_mode {
        ListMode::Recents => &s.game.detail_cache,
        ListMode::Favorites => &s.game.fav_detail_cache,
    };

    if let Some(Some(detail)) = detail_cache.get(s.game.current_index) {
        // On Page 2 (Description), use full-width lines and max_desc_lines_full
        let (total_lines, max_lines) = if PAGES.get(s.game.current_page) == Some(&PageKind::Description) {
            (
                detail.display.desc_lines_full.len(),
                s.display.layout.max_desc_lines_full as usize,
            )
        } else {
            (
                detail.display.desc_lines.len(),
                s.display.layout.max_desc_lines as usize,
            )
        };
        if total_lines > max_lines {
            let scroll = s.game.desc_scroll.min(total_lines.saturating_sub(max_lines));
            use std::fmt::Write;
            let _ = write!(
                s.scratch.scroll_indicator,
                "[{}/{}]",
                scroll + 1,
                total_lines.saturating_sub(max_lines) + 1
            );
        }
    }
}

/// Rewrite the home screen count text scratch buffers in-place.
/// Called once after data loading (retro_load_game) and on reset.
fn update_home_counts(s: &mut CoreState) {
    use std::fmt::Write;
    s.scratch.recents_count_text.clear();
    let _ = write!(s.scratch.recents_count_text, "{}", s.game.entries.len());
    s.scratch.favorites_count_text.clear();
    let _ = write!(s.scratch.favorites_count_text, "{}", s.game.favorites.len());
}

/// Get the active entries list and detail cache based on current list mode.
fn active_list(game: &GameData) -> (&Vec<GameEntry>, &Vec<Option<GameDetail>>) {
    match game.list_mode {
        ListMode::Recents => (&game.entries, &game.detail_cache),
        ListMode::Favorites => (&game.favorites, &game.fav_detail_cache),
    }
}

// ---- Data loading (unified) ----

/// Load a game list (recents or favorites) and pre-fetch all details.
fn load_list(
    s: &mut CoreState,
    mode: ListMode,
) {
    let label = match mode {
        ListMode::Recents => "recents",
        ListMode::Favorites => "favorites",
    };

    let fetch_msg = match mode {
        ListMode::Recents => "Fetching recently played...",
        ListMode::Favorites => "Fetching favorites...",
    };

    s.net.status_message = fetch_msg.to_string();
    s.net.loading = true;

    let result = match mode {
        ListMode::Recents => fetch_recents(),
        ListMode::Favorites => fetch_favorites(),
    };

    match result {
        Ok(entries) => {
            let count = entries.len();
            let cache = vec![None; count];

            match mode {
                ListMode::Recents => {
                    s.game.entries = entries;
                    s.game.detail_cache = cache;
                }
                ListMode::Favorites => {
                    s.game.favorites = entries;
                    s.game.fav_detail_cache = cache;
                }
            }

            s.game.current_index = 0;
            s.game.desc_scroll = 0;
            // Reset saved index for the reloaded list
            match mode {
                ListMode::Recents => s.game.recents_index = 0,
                ListMode::Favorites => s.game.favorites_index = 0,
            }
            s.net.api_available = true;
            s.net.loading = false;

            let empty_msg = match mode {
                ListMode::Recents => "No recently played games found.",
                ListMode::Favorites => "No favorites found.",
            };

            if count == 0 {
                s.net.status_message = empty_msg.to_string();
            } else {
                s.net.status_message.clear();
                debug_log(&format!("[{}] pre-fetching details for {} entries", label, count));
                for i in 0..count {
                    prefetch_detail(s, mode, i);
                }
                debug_log(&format!("[{}] pre-fetch complete", label));
            }
        }
        Err(e) => {
            s.net.api_available = false;
            s.net.loading = false;
            let err_msg = match mode {
                ListMode::Recents => format!("Replay Control not available: {}", e),
                ListMode::Favorites => format!("Could not load favorites: {}", e),
            };
            s.net.status_message = err_msg;
            debug_log(&format!("[{}] fetch failed: {}", label, e));
        }
    }
}

/// Pre-fetch detail for a single game entry (unified for both lists).
fn prefetch_detail(s: &mut CoreState, mode: ListMode, index: usize) {
    let (entries, cache) = match mode {
        ListMode::Recents => (&s.game.entries, &mut s.game.detail_cache),
        ListMode::Favorites => (&s.game.favorites, &mut s.game.fav_detail_cache),
    };

    if index >= entries.len() {
        return;
    }
    if cache.get(index).and_then(|d| d.as_ref()).is_some() {
        return; // already cached
    }

    let chars_per_line = s.display.layout.chars_per_line as usize;
    let full_chars_per_line = s.display.layout.full_chars_per_line as usize;
    let entry = &entries[index];
    let box_art_url = entry.box_art_url.clone();

    match fetch_rom_detail(&entry.system, &entry.rom_filename) {
        Ok(mut detail) => {
            // Fetch box art if available and within the limit
            if index < MAX_BOX_ART_PREFETCH {
                if let Some(ref url) = box_art_url {
                    detail.box_art = fetch_box_art_for_layout(url, &s.display.layout);
                }
            }
            // Pre-compute display strings so rendering is allocation-free
            detail.display = precompute_display(&detail, chars_per_line, full_chars_per_line);
            if index < cache.len() {
                cache[index] = Some(detail);
            }
        }
        Err(_) => {
            // Create a minimal detail from the entry itself
            let (entries, cache) = match mode {
                ListMode::Recents => (&s.game.entries, &mut s.game.detail_cache),
                ListMode::Favorites => (&s.game.favorites, &mut s.game.fav_detail_cache),
            };
            let mut fallback = GameDetail {
                display_name: entries[index].display_name.clone(),
                system_display: entries[index].system_display.clone(),
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
                    desc_lines_full: Vec::new(),
                },
            };
            fallback.display = precompute_display(&fallback, chars_per_line, full_chars_per_line);
            if index < cache.len() {
                cache[index] = Some(fallback);
            }
        }
    }
}

// ---- Rendering ----

// ALLOCATION-FREE: rendering functions must not allocate on the heap.
// All display strings are pre-computed during prefetch; scratch buffers
// (header_text, scroll_indicator) are updated on navigation events only.

/// Main rendering entry point. Dispatches by top-level view.
fn render_frame(s: &mut CoreState) {
    match s.game.view {
        CoreView::Home => render_home(s),
        CoreView::GameDetail => render_detail(s),
    }
}

/// Home screen: header bar, app title, menu items, controls reference.
fn render_home(s: &mut CoreState) {
    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let label_scale = s.display.layout.label_scale;
    let title_scale = s.display.layout.title_scale;
    let font_scale = s.display.layout.font_scale;
    let mx = s.display.layout.margin_x as i32;
    let my = s.display.layout.margin_y as i32;
    let pal = s.display.palette;
    let fb = &mut s.display.framebuffer;

    // Clear background
    for px in fb.iter_mut() {
        *px = pal.bg;
    }

    // -- Header bar: "REPLAY" left, version right --
    let header_h = (label_scale * 16 + 8) as i32;
    draw_rect(fb, w, h, 0, 0, w, header_h as u32, pal.header_bg);
    draw_hline(fb, w, h, 0, header_h, w, pal.accent);

    let header_text_y = my / 2 + 2;
    draw_string(fb, w, h, "REPLAY", mx, header_text_y, pal.accent, label_scale);

    // Version right-aligned
    let version = "v0.6.0";
    let version_w = (version.len() as i32) * (label_scale * 9) as i32;
    let version_x = (w as i32) - mx - version_w;
    draw_string(fb, w, h, version, version_x, header_text_y, pal.nav, label_scale);

    // -- App title "RePlayOS" centered --
    let title_text = "RePlayOS";
    let title_char_w = (title_scale * 9) as i32;
    let title_w = (title_text.len() as i32) * title_char_w;
    let title_x = ((w as i32) - title_w) / 2;
    let title_y = header_h + my + 12;
    draw_string(fb, w, h, title_text, title_x, title_y, pal.title, title_scale);

    // -- Menu items --
    let menu_start_y = title_y + (title_scale * 18) as i32 + my + 8;
    let item_h = (font_scale * 18 + 8) as i32;

    // Menu item 0: Recently Played
    let y0 = menu_start_y;
    render_home_menu_item(s, 0, "Recently Played", &s.scratch.recents_count_text.clone(), y0);

    // Menu item 1: Favorites
    let y1 = y0 + item_h + 4;
    render_home_menu_item(s, 1, "Favorites", &s.scratch.favorites_count_text.clone(), y1);

    // -- Controls reference at bottom --
    let fb = &mut s.display.framebuffer;
    let label_h = (label_scale * 16) as i32;

    // Two lines of hints
    let (line1, line2) = if w <= 640 {
        ("^v:select  A:open  B:exit", "In game: <->:browse L1/R1:page")
    } else {
        (
            "Up/Down: select  |  A: open  |  B: exit",
            "In game:  Left/Right: browse  |  L1/R1: page  |  Up/Down: scroll",
        )
    };

    let footer_line2_y = (h as i32) - my - label_h;
    let footer_line1_y = footer_line2_y - label_h - 2;

    // Accent line above controls
    draw_hline(fb, w, h, mx, footer_line1_y - 4, (w as i32 - 2 * mx) as u32, pal.accent);

    draw_string(fb, w, h, line1, mx, footer_line1_y, pal.nav, label_scale);
    draw_string(fb, w, h, line2, mx, footer_line2_y, pal.nav, label_scale);
}

/// Render a single home screen menu item.
fn render_home_menu_item(s: &mut CoreState, index: usize, label: &str, count: &str, y: i32) {
    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let font_scale = s.display.layout.font_scale;
    let label_scale = s.display.layout.label_scale;
    let mx = s.display.layout.margin_x as i32;
    let pal = s.display.palette;
    let fb = &mut s.display.framebuffer;
    let is_selected = s.game.home_cursor == index;

    let marker = if is_selected { "[>]" } else { "[ ]" };
    let marker_color = if is_selected { pal.accent } else { pal.nav };
    let text_color = if is_selected { pal.title } else { pal.label };
    let count_color = pal.nav;

    // Draw marker
    let marker_x = mx + (font_scale * 4) as i32;
    let end_x = draw_string(fb, w, h, marker, marker_x, y, marker_color, label_scale);

    // Draw label
    let label_x = end_x + (font_scale * 6) as i32;
    draw_string(fb, w, h, label, label_x, y, text_color, font_scale);

    // Draw count right-aligned
    let count_w = (count.len() as i32) * (font_scale * 9) as i32;
    let count_x = (w as i32) - mx - count_w;
    draw_string(fb, w, h, count, count_x, y, count_color, font_scale);
}

/// Game detail rendering (the current full render_frame logic).
fn render_detail(s: &mut CoreState) {
    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let label_scale = s.display.layout.label_scale;
    let mx = s.display.layout.margin_x as i32;
    let my = s.display.layout.margin_y as i32;
    let font_scale = s.display.layout.font_scale;

    // Safety clamp: ensure current_page is within bounds
    if s.game.current_page >= PAGE_COUNT {
        s.game.current_page = 0;
    }

    let pal = s.display.palette;
    let fb = &mut s.display.framebuffer;

    // Clear background
    for px in fb.iter_mut() {
        *px = pal.bg;
    }

    // Get the current list and detail cache
    let (entries, _detail_cache) = match s.game.list_mode {
        ListMode::Recents => (&s.game.entries, &s.game.detail_cache),
        ListMode::Favorites => (&s.game.favorites, &s.game.fav_detail_cache),
    };

    let total = entries.len();

    // Safety clamp: ensure current_index is within bounds for the active list.
    // This guards against stale indices after list switches or re-loads.
    if total > 0 && s.game.current_index >= total {
        s.game.current_index = 0;
    }

    // -- Header: mode label + position indicator + page dots --
    let header_h = (label_scale * 16 + 8) as i32;
    draw_rect(fb, w, h, 0, 0, w, header_h as u32, pal.header_bg);
    draw_hline(fb, w, h, 0, header_h, w, pal.accent);

    if total == 0 {
        let mode_label = match s.game.list_mode {
            ListMode::Recents => "RECENTLY PLAYED",
            ListMode::Favorites => "FAVORITES",
        };
        draw_string(fb, w, h, mode_label, mx, my / 2 + 2, pal.nav, label_scale);
    } else {
        // Left arrow
        let arrow_x = mx;
        draw_string(fb, w, h, "<", arrow_x, my / 2 + 2, pal.arrow, label_scale);

        // Mode + position -- read from pre-computed scratch buffer
        let text_x = arrow_x + (label_scale * 9 + 4) as i32;
        draw_string(
            fb,
            w,
            h,
            &s.scratch.header_text,
            text_x,
            my / 2 + 2,
            pal.nav,
            label_scale,
        );

        // Right arrow on the far right
        let arrow_r = (w as i32) - mx - (label_scale * 9) as i32;
        draw_string(fb, w, h, ">", arrow_r, my / 2 + 2, pal.arrow, label_scale);
    }

    // Page indicator dots (top-right, inside header)
    render_page_indicator(s, header_h);

    // -- Handle empty list / loading / error --
    if !s.net.status_message.is_empty() {
        let color = if s.net.api_available {
            pal.loading
        } else {
            pal.error
        };
        let msg_y = (h as i32) / 2 - 8;
        let fb = &mut s.display.framebuffer;
        draw_string(fb, w, h, &s.net.status_message, mx, msg_y, color, font_scale);
        render_back_hint(s);
        return;
    }

    if total == 0 {
        render_back_hint(s);
        return;
    }

    // Dispatch to page-specific renderer
    let current_page = PAGES.get(s.game.current_page).copied().unwrap_or(PageKind::GameInfo);
    match current_page {
        PageKind::GameInfo => render_page_game_info(s, header_h),
        PageKind::Description => render_page_description(s, header_h),
    }

    // Minimal back hint in bottom-right corner
    render_back_hint(s);
}

/// Draw page indicator dots in the top-right corner of the header.
fn render_page_indicator(s: &mut CoreState, _header_h: i32) {
    let w = s.display.layout.width;
    let mx = s.display.layout.margin_x as i32;
    let my = s.display.layout.margin_y as i32;
    let label_scale = s.display.layout.label_scale;

    let fb = &mut s.display.framebuffer;
    let h = s.display.layout.height;

    let dot_size = if w <= 640 { 3u32 } else { 5 };
    let dot_spacing = if w <= 640 { 8i32 } else { 12 };
    let total_width = (PAGE_COUNT as i32) * dot_spacing;
    // Position: to the left of the right arrow, with some margin
    let start_x = (w as i32) - mx - (label_scale * 9) as i32 - 6 - total_width;
    let cy = my / 2 + (label_scale * 8) as i32; // vertically center in header

    let pal = s.display.palette;
    for i in 0..PAGE_COUNT {
        let cx = start_x + (i as i32) * dot_spacing;
        let color = if i == s.game.current_page {
            pal.title // filled: bright
        } else {
            pal.nav // dim
        };
        draw_rect(fb, w, h, cx, cy - (dot_size as i32 / 2), dot_size, dot_size, color);
    }
}

/// Page 1: Game Info -- two-column layout (box art left, metadata right).
fn render_page_game_info(s: &mut CoreState, header_h: i32) {
    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let font_scale = s.display.layout.font_scale;
    let title_scale = s.display.layout.title_scale;
    let label_scale = s.display.layout.label_scale;
    let mx = s.display.layout.margin_x as i32;
    let my = s.display.layout.margin_y as i32;
    let show_extra_metadata = s.display.layout.show_extra_metadata;
    let pal = s.display.palette;

    let fb = &mut s.display.framebuffer;

    let (entries, detail_cache) = match s.game.list_mode {
        ListMode::Recents => (&s.game.entries, &s.game.detail_cache),
        ListMode::Favorites => (&s.game.favorites, &s.game.fav_detail_cache),
    };

    // Bounds guard: current_index must be valid for the active list
    if s.game.current_index >= entries.len() {
        return;
    }

    let detail = detail_cache
        .get(s.game.current_index)
        .and_then(|d| d.as_ref());
    let entry = &entries[s.game.current_index];

    let line_h = (font_scale * 18) as i32;
    let title_h = (title_scale * 18) as i32;

    let mut y = header_h + (my / 2) + 4;

    // Title (large) -- always full width, above the two-column area
    let title = detail.map(|d| d.display_name.as_str()).unwrap_or(&entry.display_name);
    let title_max = ((w as i32 - 2 * mx) / (title_scale * 9) as i32) as usize;
    draw_string_truncated(fb, w, h, title, mx, y, pal.title, title_scale, title_max);
    y += title_h + 4;

    // System + Year
    let sys_year = detail.map(|d| d.display.sys_year_line.as_str()).unwrap_or(&entry.system_display);
    draw_string(fb, w, h, sys_year, mx, y, pal.system, font_scale);
    y += line_h;

    // Separator line
    draw_hline(fb, w, h, mx, y, (w as i32 - 2 * mx) as u32, pal.accent);
    y += 6;

    // Two-column layout
    let has_box_art = detail.and_then(|d| d.box_art.as_ref()).is_some();
    let art_x = mx;
    let art_y = y;

    let (text_x, text_w) = if has_box_art {
        let art = detail.unwrap().box_art.as_ref().unwrap();
        let gap = if w <= 320 { 6 } else { 12 };
        let tx = art_x + art.width as i32 + gap;
        let tw = (w as i32 - tx - mx).max(40) as u32;
        (tx, tw)
    } else {
        (mx, (w as i32 - 2 * mx) as u32)
    };

    // Blit box art
    if let Some(d) = detail {
        if let Some(ref art) = d.box_art {
            blit_image(fb, w, h, &art.pixels, art.width, art.height, art_x as u32, art_y as u32);
        }
    }

    // Metadata column
    let meta_chars_per_line = (text_w as i32 / (font_scale * 9) as i32) as usize;

    // Developer
    if let Some(d) = detail {
        if !d.developer.is_empty() {
            let end_x = draw_string(fb, w, h, "Dev: ", text_x, y, pal.label, label_scale);
            draw_string_truncated(fb, w, h, &d.developer, end_x, y, pal.value, font_scale, meta_chars_per_line.saturating_sub(5));
            y += line_h;
        }
    }

    // Rating stars
    if let Some(detail) = detail {
        if let Some(rating) = detail.rating {
            let end_x = draw_string(fb, w, h, "Rating: ", text_x, y, pal.label, label_scale);
            let star_size = (font_scale * 4).max(3) as i32;
            draw_rating(fb, w, h, rating, end_x, y, star_size, &detail.display.rating_text, pal.star, pal.star_dim, pal.rating_text);
            y += line_h;
        }
    }

    // Genre
    if let Some(d) = detail {
        if !d.genre.is_empty() {
            let end_x = draw_string(fb, w, h, "Genre: ", text_x, y, pal.label, label_scale);
            draw_string_truncated(fb, w, h, &d.genre, end_x, y, pal.value, font_scale, meta_chars_per_line.saturating_sub(7));
            y += line_h;
        }
    }

    // Players
    if let Some(d) = detail {
        if !d.display.players_text.is_empty() {
            let end_x = draw_string(fb, w, h, "Players: ", text_x, y, pal.label, label_scale);
            draw_string(fb, w, h, &d.display.players_text, end_x, y, pal.value, font_scale);
            y += line_h;
        }
    }

    // Extra metadata (publisher, region) for HD layouts
    if show_extra_metadata {
        if let Some(d) = detail {
            if let Some(ref publisher) = d.publisher {
                if !publisher.is_empty() {
                    let end_x = draw_string(fb, w, h, "Publisher: ", text_x, y, pal.label, label_scale);
                    draw_string(fb, w, h, publisher, end_x, y, pal.value, font_scale);
                    y += line_h;
                }
            }
            if let Some(ref region) = d.region {
                if !region.is_empty() {
                    let end_x = draw_string(fb, w, h, "Region: ", text_x, y, pal.label, label_scale);
                    draw_string(fb, w, h, region, end_x, y, pal.value, font_scale);
                    // y += line_h; // last field, no need to advance
                }
            }
        }
    }
}

/// Page 2: Description -- title header + full-width scrollable description text.
fn render_page_description(s: &mut CoreState, header_h: i32) {
    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let font_scale = s.display.layout.font_scale;
    let title_scale = s.display.layout.title_scale;
    let mx = s.display.layout.margin_x as i32;
    let my = s.display.layout.margin_y as i32;
    let max_desc_lines = s.display.layout.max_desc_lines_full as usize;
    let pal = s.display.palette;

    let fb = &mut s.display.framebuffer;

    let (entries, detail_cache) = match s.game.list_mode {
        ListMode::Recents => (&s.game.entries, &s.game.detail_cache),
        ListMode::Favorites => (&s.game.favorites, &s.game.fav_detail_cache),
    };

    // Bounds guard: current_index must be valid for the active list
    if s.game.current_index >= entries.len() {
        return;
    }

    let detail = detail_cache
        .get(s.game.current_index)
        .and_then(|d| d.as_ref());
    let entry = &entries[s.game.current_index];

    let line_h = (font_scale * 18) as i32;
    let title_h = (title_scale * 18) as i32;

    let mut y = header_h + (my / 2) + 4;

    // Compact title header: title + system/year
    let title = detail.map(|d| d.display_name.as_str()).unwrap_or(&entry.display_name);
    let title_max = ((w as i32 - 2 * mx) / (title_scale * 9) as i32) as usize;
    draw_string_truncated(fb, w, h, title, mx, y, pal.title, title_scale, title_max);
    y += title_h + 2;

    let sys_year = detail.map(|d| d.display.sys_year_line.as_str()).unwrap_or(&entry.system_display);
    draw_string(fb, w, h, sys_year, mx, y, pal.system, font_scale);
    y += line_h;

    // Separator
    draw_hline(fb, w, h, mx, y, (w as i32 - 2 * mx) as u32, pal.accent);
    y += 8;

    // Description text (full-width, scrollable)
    if let Some(d) = detail {
        if d.display.desc_lines_full.is_empty() {
            // No description available
            let center_y = (h as i32) / 2 + 10;
            draw_string(fb, w, h, "No description available.", mx, center_y, pal.nav, font_scale);
        } else {
            let total_lines = d.display.desc_lines_full.len();
            let scroll = s.game.desc_scroll.min(total_lines.saturating_sub(max_desc_lines));

            let end = d.display.desc_lines_full.len().min(scroll + max_desc_lines);
            // Compute the bottom boundary -- just leave room for the minimal "B:back" hint
            let footer_top = (h as i32) - my - 16 - 2; // scale=1 hint height + margin

            for line in &d.display.desc_lines_full[scroll..end] {
                if y + line_h >= footer_top {
                    break;
                }
                draw_string(fb, w, h, line, mx, y, pal.desc, font_scale);
                y += line_h;
            }

            // Scroll indicator -- from pre-computed scratch buffer
            if !s.scratch.scroll_indicator.is_empty() {
                let ix = (w as i32) - mx - (s.scratch.scroll_indicator.len() as i32 * 9);
                // Position at the end of visible text area
                let indicator_y = y.min(footer_top - line_h) + 2;
                draw_string(fb, w, h, &s.scratch.scroll_indicator, ix, indicator_y, pal.nav, 1);
            }
        }
    } else {
        // No detail loaded yet
        let center_y = (h as i32) / 2 + 10;
        draw_string(fb, w, h, "No description available.", mx, center_y, pal.nav, font_scale);
    }
}

/// Render a minimal "B:back" hint in the bottom-right corner of the game detail view.
fn render_back_hint(s: &mut CoreState) {
    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let mx = s.display.layout.margin_x as i32;
    let my = s.display.layout.margin_y as i32;
    let pal = s.display.palette;
    let fb = &mut s.display.framebuffer;

    // Use the smallest scale (1) for the back hint regardless of layout
    let hint = "B:back";
    let hint_w = (hint.len() as i32) * 9; // scale=1: 9px per char
    let hint_x = (w as i32) - mx - hint_w;
    let hint_y = (h as i32) - my - 16;
    draw_string(fb, w, h, hint, hint_x, hint_y, pal.nav, 1);
}

// ---- Input handling ----

// ALLOCATION-FREE: this function must not allocate (except on edge-triggered
// navigation events, which rewrite pre-allocated scratch buffers in-place).
fn handle_input(s: &mut CoreState) {
    let input_state = match s.cb.input_state_cb {
        Some(cb) => cb,
        None => return,
    };

    let btn = |id: c_uint| -> bool { unsafe { input_state(0, RETRO_DEVICE_JOYPAD, 0, id) != 0 } };

    let left = btn(RETRO_DEVICE_ID_JOYPAD_LEFT);
    let right = btn(RETRO_DEVICE_ID_JOYPAD_RIGHT);
    let up = btn(RETRO_DEVICE_ID_JOYPAD_UP);
    let down = btn(RETRO_DEVICE_ID_JOYPAD_DOWN);
    let b_pressed = btn(RETRO_DEVICE_ID_JOYPAD_B);
    let a_pressed = btn(RETRO_DEVICE_ID_JOYPAD_A);
    let l1 = btn(RETRO_DEVICE_ID_JOYPAD_L);
    let r1 = btn(RETRO_DEVICE_ID_JOYPAD_R);

    match s.game.view {
        CoreView::Home => handle_input_home(s, up, down, a_pressed, b_pressed),
        CoreView::GameDetail => handle_input_detail(s, left, right, up, down, b_pressed, l1, r1),
    }

    // Update debounce state
    s.input.prev_left = left;
    s.input.prev_right = right;
    s.input.prev_b = b_pressed;
    s.input.prev_a = a_pressed;
    s.input.prev_up = up;
    s.input.prev_down = down;
    s.input.prev_l1 = l1;
    s.input.prev_r1 = r1;
}

/// Input handling for the Home screen view.
fn handle_input_home(s: &mut CoreState, up: bool, down: bool, a_pressed: bool, b_pressed: bool) {
    const HOME_MENU_COUNT: usize = 2;

    // Up: move cursor up (edge-triggered, wraps)
    if up && !s.input.prev_up {
        if s.game.home_cursor > 0 {
            s.game.home_cursor -= 1;
        } else {
            s.game.home_cursor = HOME_MENU_COUNT - 1;
        }
    }

    // Down: move cursor down (edge-triggered, wraps)
    if down && !s.input.prev_down {
        if s.game.home_cursor < HOME_MENU_COUNT - 1 {
            s.game.home_cursor += 1;
        } else {
            s.game.home_cursor = 0;
        }
    }

    // A: select current menu item -> enter GameDetail (edge-triggered)
    if a_pressed && !s.input.prev_a {
        // Set list_mode based on cursor
        match s.game.home_cursor {
            0 => {
                s.game.list_mode = ListMode::Recents;
                let rec_len = s.game.entries.len();
                s.game.current_index = if rec_len > 0 {
                    s.game.recents_index.min(rec_len - 1)
                } else {
                    0
                };
                s.net.status_message.clear();
                if s.game.entries.is_empty() {
                    s.net.status_message.push_str("No recently played games found.");
                }
            }
            _ => {
                s.game.list_mode = ListMode::Favorites;
                let fav_len = s.game.favorites.len();
                s.game.current_index = if fav_len > 0 {
                    s.game.favorites_index.min(fav_len - 1)
                } else {
                    0
                };
                s.net.status_message.clear();
                if s.game.favorites.is_empty() {
                    s.net.status_message.push_str("No favorites found.");
                }
            }
        }
        s.game.desc_scroll = 0;
        s.game.current_page = 0;
        s.game.view = CoreView::GameDetail;
        update_header_text(s);
        update_scroll_indicator(s);
    }

    // B: exit core (edge-triggered)
    if b_pressed && !s.input.prev_b {
        if let Some(env_cb) = s.cb.environment_cb {
            unsafe {
                env_cb(RETRO_ENVIRONMENT_SHUTDOWN, std::ptr::null_mut());
            }
        }
    }
}

/// Input handling for the GameDetail view.
fn handle_input_detail(
    s: &mut CoreState,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    b_pressed: bool,
    l1: bool,
    r1: bool,
) {
    // Read list length without holding a borrow on s
    let entries_len = match s.game.list_mode {
        ListMode::Recents => s.game.entries.len(),
        ListMode::Favorites => s.game.favorites.len(),
    };

    // Safety clamp: current_index must be valid for the active list
    if entries_len > 0 && s.game.current_index >= entries_len {
        s.game.current_index = 0;
    }

    // B: back to Home (edge-triggered) -- NOT exit
    if b_pressed && !s.input.prev_b {
        // Save current position for the active list
        match s.game.list_mode {
            ListMode::Recents => s.game.recents_index = s.game.current_index,
            ListMode::Favorites => s.game.favorites_index = s.game.current_index,
        }
        s.game.view = CoreView::Home;
        return;
    }

    // Track whether navigation happened (need to update scratch buffers)
    let mut navigated = false;

    // L1: previous page (edge-triggered, wraps around)
    if l1 && !s.input.prev_l1 {
        if s.game.current_page > 0 {
            s.game.current_page -= 1;
        } else {
            s.game.current_page = PAGE_COUNT - 1;
        }
        s.game.desc_scroll = 0;
        navigated = true;
    }

    // R1: next page (edge-triggered, wraps around)
    if r1 && !s.input.prev_r1 {
        if s.game.current_page < PAGE_COUNT - 1 {
            s.game.current_page += 1;
        } else {
            s.game.current_page = 0;
        }
        s.game.desc_scroll = 0;
        navigated = true;
    }

    // Left/Right: navigate between games (edge-triggered)
    // All details were pre-fetched in retro_load_game -- just update index, no HTTP.
    if left && !s.input.prev_left && entries_len > 0 {
        if s.game.current_index > 0 {
            s.game.current_index -= 1;
        } else {
            s.game.current_index = entries_len - 1; // wrap around
        }
        // Keep saved index in sync with navigation
        match s.game.list_mode {
            ListMode::Recents => s.game.recents_index = s.game.current_index,
            ListMode::Favorites => s.game.favorites_index = s.game.current_index,
        }
        s.game.desc_scroll = 0;
        s.game.current_page = 0; // reset to Game Info on game change
        navigated = true;
    }

    if right && !s.input.prev_right && entries_len > 0 {
        if s.game.current_index < entries_len - 1 {
            s.game.current_index += 1;
        } else {
            s.game.current_index = 0; // wrap around
        }
        // Keep saved index in sync with navigation
        match s.game.list_mode {
            ListMode::Recents => s.game.recents_index = s.game.current_index,
            ListMode::Favorites => s.game.favorites_index = s.game.current_index,
        }
        s.game.desc_scroll = 0;
        s.game.current_page = 0; // reset to Game Info on game change
        navigated = true;
    }

    // Up/Down: scroll (with cooldown for held buttons)
    // Only scroll on pages that support it (Description)
    if s.input.scroll_cooldown > 0 {
        s.input.scroll_cooldown -= 1;
    }

    let mut scrolled = false;

    let page_scrollable = matches!(
        PAGES.get(s.game.current_page),
        Some(PageKind::Description)
    );

    if (up || down) && s.input.scroll_cooldown == 0 && page_scrollable {
        // Use full-width desc_lines on Page 2
        let scroll_info = {
            let detail_cache = match s.game.list_mode {
                ListMode::Recents => &s.game.detail_cache,
                ListMode::Favorites => &s.game.fav_detail_cache,
            };
            detail_cache
                .get(s.game.current_index)
                .and_then(|d| d.as_ref())
                .map(|detail| {
                    let total_lines = detail.display.desc_lines_full.len();
                    let max_lines = s.display.layout.max_desc_lines_full as usize;
                    total_lines.saturating_sub(max_lines)
                })
        };

        if let Some(max_scroll) = scroll_info {
            if up && s.game.desc_scroll > 0 {
                s.game.desc_scroll -= 1;
                s.input.scroll_cooldown = 8; // ~133ms at 60fps
                scrolled = true;
            }
            if down && s.game.desc_scroll < max_scroll {
                s.game.desc_scroll += 1;
                s.input.scroll_cooldown = 8;
                scrolled = true;
            }
        }
    }

    if !up && !down {
        s.input.scroll_cooldown = 0; // reset when released
    }

    // Update scratch buffers only when state actually changed (edge-triggered)
    if navigated {
        update_header_text(s);
        update_scroll_indicator(s);
    } else if scrolled {
        update_scroll_indicator(s);
    }
}

// ---- Libretro API implementation ----

#[no_mangle]
pub extern "C" fn retro_api_version() -> c_uint {
    RETRO_API_VERSION
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_environment(cb: RetroEnvironmentFn) {
    let s = state();
    s.cb.environment_cb = Some(cb);

    let mut no_game: bool = true;
    cb(
        RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME,
        &mut no_game as *mut bool as *mut c_void,
    );

    // Request XRGB8888 (32-bit) pixel format -- confirmed accepted by RePlayOS.
    // This lets us output the u32 framebuffer directly with no conversion.
    let mut fmt: c_uint = 1; // RETRO_PIXEL_FORMAT_XRGB8888
    let accepted = cb(
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
        &mut fmt as *mut c_uint as *mut c_void,
    );
    if accepted {
        s.display.pixel_format = PixelFormat::Xrgb8888;
        debug_log("[replay-core] SET_PIXEL_FORMAT(XRGB8888) accepted");
    } else {
        s.display.pixel_format = PixelFormat::Bgr565;
        debug_log("[replay-core] SET_PIXEL_FORMAT(XRGB8888) rejected, falling back to BGR565");
    }
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_video_refresh(cb: RetroVideoRefreshFn) {
    state().cb.video_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_audio_sample(cb: RetroAudioSampleFn) {
    state().cb.audio_sample_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_audio_sample_batch(cb: RetroAudioSampleBatchFn) {
    state().cb.audio_batch_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_input_poll(cb: RetroInputPollFn) {
    state().cb.input_poll_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_set_input_state(cb: RetroInputStateFn) {
    state().cb.input_state_cb = Some(cb);
}

#[no_mangle]
pub unsafe extern "C" fn retro_init() {
    // Layout detection happens in retro_load_game (after environment is set up)
}

#[no_mangle]
pub unsafe extern "C" fn retro_deinit() {
    debug_log(&format!(
        "[deinit] shutting down after {} frames",
        state().display.frame_count
    ));
    let s = state();
    // Clear callbacks first so no stale pointers remain
    s.cb.environment_cb = None;
    s.cb.video_cb = None;
    s.cb.audio_sample_cb = None;
    s.cb.audio_batch_cb = None;
    s.cb.input_poll_cb = None;
    s.cb.input_state_cb = None;
    // Drop game data
    s.game.entries.clear();
    s.game.favorites.clear();
    s.game.detail_cache.clear();
    s.game.fav_detail_cache.clear();
    // Drop all framebuffers (both back and front)
    s.display.framebuffer.clear();
    s.display.present_buffer.clear();
    s.display.framebuffer_16.clear();
    s.display.present_buffer_16.clear();
    s.scratch.header_text.clear();
    s.scratch.scroll_indicator.clear();
    s.scratch.recents_count_text.clear();
    s.scratch.favorites_count_text.clear();
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
    let w = s.display.layout.width;
    let h = s.display.layout.height;

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
    s.display.frame_count = 0;
    s.game.view = CoreView::Home;
    s.game.home_cursor = 0;
    s.game.current_index = 0;
    s.game.desc_scroll = 0;
    s.game.current_page = 0;
    s.game.recents_index = 0;
    s.game.favorites_index = 0;
    s.game.list_mode = ListMode::Recents;
    debug_log("[reset] re-fetching all data");
    load_list(s, ListMode::Recents);
    load_list(s, ListMode::Favorites);
    s.game.list_mode = ListMode::Recents;
    s.game.current_index = 0;
    s.game.desc_scroll = 0;
    s.game.recents_index = 0;
    s.game.favorites_index = 0;
    if !s.game.entries.is_empty() {
        s.net.status_message.clear();
    }
    update_header_text(s);
    update_scroll_indicator(s);
    update_home_counts(s);
}

// ALLOCATION-FREE: retro_run must not allocate on the heap.
// All rendering uses pre-computed strings; framebuffers are pre-allocated.
// The only writes to scratch buffers happen in handle_input on edge-triggered
// navigation events (clear + push_str into existing capacity).
//
// DOUBLE-BUFFERED: We render into `framebuffer` (back buffer), then copy
// the completed frame into `present_buffer` (front buffer) before handing
// the pointer to the video callback. This prevents the host's DRM/display
// thread from reading a partially-rendered frame and corrupting its own
// buffer management state.

/// Static audio silence buffer -- lives for the entire process lifetime,
/// so the host frontend can safely hold onto the pointer indefinitely.
static AUDIO_SILENCE: [i16; 735 * 2] = [0i16; 735 * 2];

#[no_mangle]
pub unsafe extern "C" fn retro_run() {
    let s = state();

    // Guard: framebuffer must be allocated (retro_load_game must have been called)
    if s.display.framebuffer.is_empty() {
        return;
    }

    // Poll input
    if let Some(poll) = s.cb.input_poll_cb {
        poll();
    }

    handle_input(s);
    render_frame(s);

    s.display.frame_count += 1;

    // Periodic diagnostic logging (every 6000 frames = ~100 seconds at 60fps).
    // Uses format! which allocates, but only once every ~100s -- negligible.
    if s.display.frame_count % 6000 == 0 {
        debug_log(&format!(
            "[frame {}] alive, view={}, idx={}, mode={}",
            s.display.frame_count,
            match s.game.view {
                CoreView::Home => "Home",
                CoreView::GameDetail => "Detail",
            },
            s.game.current_index,
            match s.game.list_mode {
                ListMode::Recents => "R",
                ListMode::Favorites => "F",
            }
        ));
    }

    // Send frame to frontend via double-buffering:
    // Copy completed back-buffer into the stable present-buffer, then
    // hand the present-buffer pointer to the host.
    if let Some(video) = s.cb.video_cb {
        let w = s.display.layout.width;
        let h = s.display.layout.height;

        match s.display.pixel_format {
            PixelFormat::Xrgb8888 => {
                // Copy back -> front
                s.display.present_buffer.copy_from_slice(&s.display.framebuffer);
                video(
                    s.display.present_buffer.as_ptr() as *const c_void,
                    w as c_uint,
                    h as c_uint,
                    (w as usize) * std::mem::size_of::<u32>(),
                );
            }
            PixelFormat::Bgr565 => {
                // Convert u32 0x00RRGGBB -> u16 BBBBBGGGGGGRRRRR into back 16-bit buffer
                let pixel_count = (w * h) as usize;
                for i in 0..pixel_count {
                    let c = s.display.framebuffer[i];
                    let r = ((c >> 16) & 0xFF) as u16;
                    let g = ((c >> 8) & 0xFF) as u16;
                    let b = (c & 0xFF) as u16;
                    s.display.framebuffer_16[i] = ((b >> 3) << 11) | ((g >> 2) << 5) | (r >> 3);
                }
                // Copy back -> front
                s.display.present_buffer_16.copy_from_slice(&s.display.framebuffer_16);
                video(
                    s.display.present_buffer_16.as_ptr() as *const c_void,
                    w as c_uint,
                    h as c_uint,
                    (w as usize) * std::mem::size_of::<u16>(),
                );
            }
        }
    }

    // Send silence for audio -- uses a static buffer so the pointer
    // is always valid even if the host holds onto it across frames.
    if let Some(audio_batch) = s.cb.audio_batch_cb {
        audio_batch(AUDIO_SILENCE.as_ptr(), 735);
    }
}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(_game: *const RetroGameInfo) -> bool {
    let s = state();

    // Detect display mode from replay.cfg
    s.display.layout = LayoutConfig::detect();

    // Detect and load skin palette
    let skin_index = palette::detect_skin_index();
    s.display.palette = palette::load_palette(skin_index);
    debug_log(&format!("[replay-game-info] using skin palette {}", skin_index));

    let w = s.display.layout.width;
    let h = s.display.layout.height;
    let pixel_count = (w * h) as usize;
    s.display.framebuffer = vec![0u32; pixel_count];
    s.display.present_buffer = vec![0u32; pixel_count];
    s.display.framebuffer_16 = vec![0u16; pixel_count];
    s.display.present_buffer_16 = vec![0u16; pixel_count];

    debug_log(&format!(
        "[replay-game-info] load_game: detected {}x{} layout",
        w, h
    ));

    // Pre-fetch ALL data (both lists + all details) now, so retro_run never
    // makes HTTP calls. HTTP inside a dlopen-ed .so causes heap corruption.
    load_list(s, ListMode::Recents);
    load_list(s, ListMode::Favorites);

    // Start on the Home screen
    s.game.view = CoreView::Home;
    s.game.home_cursor = 0;
    s.game.list_mode = ListMode::Recents;
    s.game.current_index = 0;
    s.game.desc_scroll = 0;
    s.game.current_page = 0;
    s.game.recents_index = 0;
    s.game.favorites_index = 0;
    if !s.game.entries.is_empty() {
        s.net.status_message.clear();
    }

    // Pre-allocate scratch buffers with enough capacity so retro_run never
    // needs to grow them. This ensures the allocation-free invariant holds.
    // "RECENTLY PLAYED  (999/999)" = 27 chars; "[999/999]" = 9 chars;
    // longest status message = 31 chars ("No recently played games found.").
    //
    // We ensure capacity >= 64 by clearing then reserving. For status_message,
    // we preserve any existing content (e.g., "No recently played games found.")
    // and just ensure sufficient capacity for future in-place writes.
    s.scratch.header_text.clear();
    s.scratch.header_text.reserve(64);
    s.scratch.scroll_indicator.clear();
    s.scratch.scroll_indicator.reserve(64);
    s.scratch.recents_count_text.clear();
    s.scratch.recents_count_text.reserve(16);
    s.scratch.favorites_count_text.clear();
    s.scratch.favorites_count_text.reserve(16);
    // status_message may contain a valid message (e.g., error or empty-list text).
    // Ensure it has enough capacity for the longest message retro_run might write.
    let status_len = s.net.status_message.len();
    s.net.status_message.reserve(64_usize.saturating_sub(status_len));

    // Pre-compute header, scroll indicator, and home screen counts
    update_header_text(s);
    update_scroll_indicator(s);
    update_home_counts(s);

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
    debug_log("[unload_game] clearing game data");
    let s = state();
    s.game.entries.clear();
    s.game.favorites.clear();
    s.game.detail_cache.clear();
    s.game.fav_detail_cache.clear();
    // Zero-fill present buffers so the host never reads stale frame data
    // after the game is unloaded. Don't deallocate -- the host's DRM thread
    // may still be reading the pointer we handed out.
    for px in s.display.present_buffer.iter_mut() {
        *px = 0;
    }
    for px in s.display.present_buffer_16.iter_mut() {
        *px = 0;
    }
}

// ---- Save state serialization ----
//
// Layout (24 bytes):
//   [0..4)   current_index: u32
//   [4..8)   desc_scroll: u32
//   [8..9)   list_mode: u8 (0=Recents, 1=Favorites)
//   [9..10)  current_page: u8 (0=GameInfo, 1=Description)
//   [10..11) view: u8 (0=Home, 1=GameDetail)
//   [11..12) home_cursor: u8 (0=RecentlyPlayed, 1=Favorites)
//   [12..16) recents_index: u32
//   [16..20) favorites_index: u32
//   [20..24) reserved (zeroed)

const SAVE_STATE_SIZE: usize = 24;

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
    buf[0..4].copy_from_slice(&(s.game.current_index as u32).to_le_bytes());
    buf[4..8].copy_from_slice(&(s.game.desc_scroll as u32).to_le_bytes());
    buf[8] = match s.game.list_mode {
        ListMode::Recents => 0,
        ListMode::Favorites => 1,
    };
    buf[9] = s.game.current_page as u8;
    buf[10] = match s.game.view {
        CoreView::Home => 0,
        CoreView::GameDetail => 1,
    };
    buf[11] = s.game.home_cursor as u8;
    buf[12..16].copy_from_slice(&(s.game.recents_index as u32).to_le_bytes());
    buf[16..20].copy_from_slice(&(s.game.favorites_index as u32).to_le_bytes());
    true
}

#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if size < SAVE_STATE_SIZE {
        return false;
    }
    let s = state();
    let buf = std::slice::from_raw_parts(data as *const u8, SAVE_STATE_SIZE);
    // Restore view and home_cursor (backward compatible: 0 = Home, which is the correct default)
    s.game.view = match buf[10] {
        1 => CoreView::GameDetail,
        _ => CoreView::Home,
    };
    s.game.home_cursor = (buf[11] as usize).min(1); // clamp to valid range
    s.game.list_mode = match buf[8] {
        1 => ListMode::Favorites,
        _ => ListMode::Recents,
    };
    // Restore and clamp current_index to the active list's bounds
    let restored_index = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
    let list_len = match s.game.list_mode {
        ListMode::Recents => s.game.entries.len(),
        ListMode::Favorites => s.game.favorites.len(),
    };
    s.game.current_index = if list_len > 0 { restored_index.min(list_len - 1) } else { 0 };
    s.game.desc_scroll = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let page = buf[9] as usize;
    s.game.current_page = if page < PAGE_COUNT { page } else { 0 };
    // Restore saved list indexes, clamped to list bounds
    let rec_idx = u32::from_le_bytes(buf[12..16].try_into().unwrap()) as usize;
    let fav_idx = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize;
    let rec_len = s.game.entries.len();
    let fav_len = s.game.favorites.len();
    s.game.recents_index = if rec_len > 0 { rec_idx.min(rec_len - 1) } else { 0 };
    s.game.favorites_index = if fav_len > 0 { fav_idx.min(fav_len - 1) } else { 0 };
    // Update scratch buffers to reflect restored state
    update_header_text(s);
    update_scroll_indicator(s);
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
