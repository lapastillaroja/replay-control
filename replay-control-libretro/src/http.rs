use crate::json::{
    extract_json_float, extract_json_number, extract_json_string, extract_result_array,
    split_json_array,
};
use crate::layout::LayoutConfig;
use crate::util::debug_log;
use crate::{BoxArtImage, GameDetail, GameEntry, PrecomputedDisplay};

/// Base URL for the Replay Control app API.
/// Always port 8080 on the Pi (the only deployment target for this core).
pub fn api_base() -> &'static str {
    "http://localhost:8080"
}

/// Fetch the list of recently played games from Replay Control.
pub fn fetch_recents() -> Result<Vec<GameEntry>, String> {
    let resp = minreq::get(format!("{}/api/core/recents", api_base()))
        .with_header("Accept", "application/json")
        .with_timeout(5)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status_code != 200 {
        return Err(format!(
            "HTTP {}: {}",
            resp.status_code,
            resp.as_str().unwrap_or("")
        ));
    }

    let body = resp.as_str().map_err(|e| format!("UTF-8 error: {}", e))?;
    parse_game_list_json(body)
}

/// Fetch the list of favorites from Replay Control.
pub fn fetch_favorites() -> Result<Vec<GameEntry>, String> {
    let resp = minreq::get(format!("{}/api/core/favorites", api_base()))
        .with_header("Accept", "application/json")
        .with_timeout(5)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status_code != 200 {
        return Err(format!("HTTP {}", resp.status_code));
    }

    let body = resp.as_str().map_err(|e| format!("UTF-8 error: {}", e))?;
    parse_game_list_json(body)
}

/// Fetch detailed metadata for a specific ROM.
pub fn fetch_rom_detail(system: &str, filename: &str) -> Result<GameDetail, String> {
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
pub const MAX_BOX_ART_PREFETCH: usize = 20;

/// Fetch a PNG image from a URL and decode it to XRGB8888 pixels, scaled to fit `max_w x max_h`.
pub fn fetch_and_decode_box_art(url: &str, max_w: u32, max_h: u32) -> Result<BoxArtImage, String> {
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
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    // Expand indexed/palette and grayscale images to full RGB(A)
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG header: {}", e))?;

    let buf_size = reader
        .output_buffer_size()
        .ok_or("PNG: unknown output buffer size")?;
    let mut buf = vec![0u8; buf_size];
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
        _ => 3, // EXPAND converts indexed -> RGB
    };

    // Convert to XRGB8888 (0x00RRGGBB)
    let mut src_pixels = vec![0u32; (src_w * src_h) as usize];
    for y in 0..src_h as usize {
        for x in 0..src_w as usize {
            let offset = y * row_bytes + x * bytes_per_pixel;
            let (r, g, b) = if bytes_per_pixel >= 3 {
                (buf[offset], buf[offset + 1], buf[offset + 2])
            } else {
                let v = buf[offset];
                (v, v, v)
            };

            src_pixels[y * src_w as usize + x] =
                ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
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

/// Fetch and decode box art, sized appropriately for the current layout.
/// Returns None on any failure (network, decode, etc.) -- failures are non-fatal.
pub fn fetch_box_art_for_layout(url: &str, layout: &LayoutConfig) -> Option<BoxArtImage> {
    // The API returns relative URLs like "/media/snes/Named_Boxarts/Game.png".
    // Prepend the API base to make a full HTTP URL.
    let full_url = if url.starts_with("http") {
        url.to_string()
    } else {
        // URL-encode each path segment (preserving '/' separators).
        let encoded_path: String = url
            .split('/')
            .map(urlencoding::encode)
            .collect::<Vec<_>>()
            .join("/");
        format!("{}{}", api_base(), encoded_path)
    };

    let (max_w, max_h) = layout.box_art_dimensions();
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

// ---- JSON response parsers ----

/// Parse a game list JSON response (used for both recents and favorites).
/// Each entry has:
/// { "system": "...", "system_display": "...", "rom_filename": "...",
///   "display_name": "...", "box_art_url": "..." or null }
pub fn parse_game_list_json(json: &str) -> Result<Vec<GameEntry>, String> {
    let mut entries = Vec::new();
    let json = json.trim();
    let array_str = extract_result_array(json)?;
    let objects = split_json_array(array_str);

    for obj in objects {
        let system = extract_json_string(obj, "system").unwrap_or_default();
        let system_display = extract_json_string(obj, "system_display").unwrap_or_default();
        let rom_filename = extract_json_string(obj, "rom_filename").unwrap_or_default();
        let display_name =
            extract_json_string(obj, "display_name").unwrap_or_else(|| rom_filename.clone());
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
            desc_lines_full: Vec::new(),
        }, // populated by precompute_display() during pre-fetch
    })
}

// ---- URL encoding ----

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
