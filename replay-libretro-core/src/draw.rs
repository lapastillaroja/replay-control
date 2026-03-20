use crate::font;

/// Draw a single 8x16 character at (x, y) with the given color and scale.
pub fn draw_char(fb: &mut [u32], w: u32, h: u32, ch: u8, x: i32, y: i32, color: u32, scale: u32) {
    let bitmap = font::get_char_bitmap(ch);
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
pub fn draw_string(
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
pub fn draw_string_truncated(
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
pub fn draw_rect(fb: &mut [u32], w: u32, h: u32, rx: i32, ry: i32, rw: u32, rh: u32, color: u32) {
    let w = w as i32;
    let h = h as i32;
    for row in ry.max(0)..(ry + rh as i32).min(h) {
        for col in rx.max(0)..(rx + rw as i32).min(w) {
            fb[(row * w + col) as usize] = color;
        }
    }
}

/// Draw a horizontal line.
pub fn draw_hline(fb: &mut [u32], w: u32, h: u32, x: i32, y: i32, len: u32, color: u32) {
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
pub fn blit_image(
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
pub fn word_wrap(text: &str, max_chars: usize) -> Vec<String> {
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
pub fn draw_star_filled(fb: &mut [u32], w: u32, h: u32, cx: i32, cy: i32, size: i32, color: u32) {
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
pub fn draw_star_empty(fb: &mut [u32], w: u32, h: u32, cx: i32, cy: i32, size: i32, color: u32) {
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
///
/// Colors are passed from the active skin palette:
///   - `star_color`: filled star color (typically gold)
///   - `star_dim_color`: empty star outline color
///   - `rating_text_color`: numeric rating text color
pub fn draw_rating(
    fb: &mut [u32],
    w: u32,
    h: u32,
    rating: f32,
    x: i32,
    y: i32,
    star_size: i32,
    rating_text: &str,
    star_color: u32,
    star_dim_color: u32,
    rating_text_color: u32,
) -> i32 {
    let full_stars = rating.floor() as i32;
    let half = (rating - rating.floor()) >= 0.25;
    let spacing = star_size * 3;

    for i in 0..5 {
        let sx = x + i * spacing + star_size;
        let sy = y + star_size;
        if i < full_stars {
            draw_star_filled(fb, w, h, sx, sy, star_size, star_color);
        } else if i == full_stars && half {
            // Half star: filled left, empty right (approximate with filled)
            draw_star_filled(fb, w, h, sx, sy, star_size, star_color);
        } else {
            draw_star_empty(fb, w, h, sx, sy, star_size, star_dim_color);
        }
    }

    // Also draw numeric rating next to stars (pre-computed text, no allocation)
    let text_x = x + 5 * spacing + star_size * 2;
    draw_string(fb, w, h, rating_text, text_x, y, rating_text_color, 1);

    text_x + (rating_text.len() as i32) * 9
}
