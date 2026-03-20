// Minimal JSON parsing helpers.
//
// We parse the JSON responses from Replay Control manually to avoid
// pulling in serde_json (~300KB). The response shapes are known and stable.

/// Extract the JSON array from a response, handling potential error wrapping.
pub fn extract_result_array(json: &str) -> Result<&str, String> {
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
        return Err(format!(
            "Unexpected JSON object: {}",
            &json[..json.len().min(100)]
        ));
    }
    Err(format!(
        "Unexpected response format: {}",
        &json[..json.len().min(100)]
    ))
}

/// Split a JSON array string into its top-level elements.
pub fn split_json_array(json: &str) -> Vec<&str> {
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

/// Find the byte offset of a JSON key that appears at the top level of an object
/// (i.e., not inside a string value). Returns the index of the opening `"` of the key.
fn find_json_key(json: &str, key: &str) -> Option<usize> {
    let search = format!("\"{}\"", key);
    let bytes = json.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let idx = json[pos..].find(&search)?;
        let abs = pos + idx;
        // Check we are not inside a string value by counting unescaped quotes before this position
        let in_string = is_inside_string(json, abs);
        if !in_string {
            return Some(abs);
        }
        pos = abs + 1;
    }
    None
}

/// Check whether a given byte offset falls inside a JSON string value.
/// Counts unescaped double-quotes from the start of `json` up to `pos`.
fn is_inside_string(json: &str, pos: usize) -> bool {
    let bytes = &json.as_bytes()[..pos];
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && in_str {
            i += 2; // skip escaped character
            continue;
        }
        if bytes[i] == b'"' {
            in_str = !in_str;
        }
        i += 1;
    }
    in_str
}

/// Extract a string value for a given key from a JSON object.
pub fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let idx = find_json_key(json, key)?;
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
pub fn extract_json_number(json: &str, key: &str) -> Option<i64> {
    let search = format!("\"{}\"", key);
    let idx = find_json_key(json, key)?;
    let rest = &json[idx + search.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();

    if rest.starts_with("null") {
        return None;
    }

    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-' && c != '+')
        .unwrap_or(rest.len());
    let num_str = &rest[..end];
    num_str.parse().ok()
}

/// Extract a float value for a given key.
pub fn extract_json_float(json: &str, key: &str) -> Option<f32> {
    let search = format!("\"{}\"", key);
    let idx = find_json_key(json, key)?;
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
