use std::fs;
use std::path::Path;

fn main() {
    // Embed git short hash for version display.
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=GIT_HASH={hash}");

    let style_dir = Path::new("style");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = Path::new(&out_dir).join("style.css");

    // Collect all _*.css partials and sort alphabetically.
    // Numbered prefixes (_01-, _02-, ...) control the load order.
    let mut partials: Vec<_> = fs::read_dir(style_dir)
        .expect("Failed to read style/ directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('_') && name.ends_with(".css") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    partials.sort();

    let mut combined = String::new();
    for path in &partials {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&content);

        // Re-run build script when any partial changes
        println!("cargo:rerun-if-changed={}", path.display());
    }

    // Also re-run if partials are added or removed
    println!("cargo:rerun-if-changed={}", style_dir.display());

    fs::write(&out_path, combined)
        .unwrap_or_else(|e| panic!("Failed to write {}: {e}", out_path.display()));
}
