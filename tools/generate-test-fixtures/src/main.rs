//! Test fixture generator for replay-control integration tests.
//!
//! Parses No-Intro DAT files and arcade data to create a realistic storage
//! directory structure with empty ROM files using real filenames.
//!
//! Usage:
//!     cargo run -p generate-test-fixtures
//!     cargo run -p generate-test-fixtures -- --output /custom/path

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

// ── Configuration ────────────────────────────────────────────────────────────

/// Systems to include in the test fixtures, with their No-Intro DAT filenames
/// and the ROM file extension used in the DAT.
const NOINTRO_SYSTEMS: &[SystemDef] = &[
    SystemDef {
        folder: "nintendo_snes",
        dat_file: "Nintendo - Super Nintendo Entertainment System.dat",
        max_roms: 1500,
    },
    SystemDef {
        folder: "sega_smd",
        dat_file: "Sega - Mega Drive - Genesis.dat",
        max_roms: 1500,
    },
    SystemDef {
        folder: "nintendo_n64",
        dat_file: "Nintendo - Nintendo 64.dat",
        max_roms: 1500,
    },
    SystemDef {
        folder: "nintendo_nes",
        dat_file: "Nintendo - Nintendo Entertainment System.dat",
        max_roms: 1500,
    },
    SystemDef {
        folder: "nintendo_gba",
        dat_file: "Nintendo - Game Boy Advance.dat",
        max_roms: 1000,
    },
];

/// Arcade systems with ROM names from FBNeo DAT.
const ARCADE_MAX_ROMS: usize = 500;

/// Number of dummy PSX disc games to generate (M3U + bin/cue pairs).
const PSX_DISC_GAMES: usize = 30;

/// Number of dummy Sega CD disc games to generate (M3U + bin/cue pairs).
const SEGA_CD_DISC_GAMES: usize = 15;

struct SystemDef {
    folder: &'static str,
    dat_file: &'static str,
    max_roms: usize,
}

// ── 1x1 PNG constant ────────────────────────────────────────────────────────

/// Minimal valid 1x1 pixel red PNG (67 bytes).
/// Generated from the PNG specification: signature + IHDR + IDAT + IEND.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
    0x00, 0x00, 0x00, 0x0D, // IHDR length
    0x49, 0x48, 0x44, 0x52, // "IHDR"
    0x00, 0x00, 0x00, 0x01, // width: 1
    0x00, 0x00, 0x00, 0x01, // height: 1
    0x08, 0x02, // bit depth 8, color type 2 (RGB)
    0x00, 0x00, 0x00, // compression, filter, interlace
    0x90, 0x77, 0x53, 0xDE, // IHDR CRC
    0x00, 0x00, 0x00, 0x0C, // IDAT length
    0x49, 0x44, 0x41, 0x54, // "IDAT"
    0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, // zlib compressed 1 red pixel
    0x00, 0x02, 0x00, 0x01, // adler32
    0xE2, 0x21, 0xBC, 0x33, // IDAT CRC
    0x00, 0x00, 0x00, 0x00, // IEND length
    0x49, 0x45, 0x4E, 0x44, // "IEND"
    0xAE, 0x42, 0x60, 0x82, // IEND CRC
];

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let output_dir = if let Some(idx) = args.iter().position(|a| a == "--output") {
        PathBuf::from(args.get(idx + 1).expect("--output requires a path argument"))
    } else {
        // Default: tests/fixtures/storage relative to workspace root
        workspace_root().join("tests/fixtures/storage")
    };

    eprintln!("Generating test fixtures in: {}", output_dir.display());

    // Clean output directory if it exists
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir).expect("failed to clean output directory");
    }

    let data_dir = workspace_root().join("data");

    // Track all generated ROM files per system for favorites/recents
    let mut all_roms: HashMap<String, Vec<String>> = HashMap::new();

    // ── No-Intro cartridge systems ───────────────────────────────────────
    for sys in NOINTRO_SYSTEMS {
        let dat_path = data_dir.join("no-intro").join(sys.dat_file);
        if !dat_path.exists() {
            eprintln!("  SKIP {} (DAT not found: {})", sys.folder, dat_path.display());
            continue;
        }

        let filenames = parse_nointro_dat(&dat_path, sys.max_roms);
        let rom_dir = output_dir.join("roms").join(sys.folder);
        fs::create_dir_all(&rom_dir).expect("failed to create ROM directory");

        for filename in &filenames {
            File::create(rom_dir.join(filename)).expect("failed to create ROM file");
        }

        eprintln!("  {} — {} ROM files", sys.folder, filenames.len());
        all_roms.insert(sys.folder.to_string(), filenames);
    }

    // ── Arcade (FBNeo) ──────────────────────────────────────────────────
    let fbneo_dat = data_dir.join("fbneo-arcade.dat");
    if fbneo_dat.exists() {
        let filenames = parse_fbneo_dat(&fbneo_dat, ARCADE_MAX_ROMS);
        let rom_dir = output_dir.join("roms").join("arcade_fbneo");
        fs::create_dir_all(&rom_dir).expect("failed to create arcade ROM directory");

        for filename in &filenames {
            File::create(rom_dir.join(filename)).expect("failed to create arcade ROM file");
        }

        eprintln!("  arcade_fbneo — {} ROM files", filenames.len());
        all_roms.insert("arcade_fbneo".to_string(), filenames);
    } else {
        eprintln!("  SKIP arcade_fbneo (DAT not found)");
    }

    // ── Disc-based systems (PSX, Sega CD) — synthetic M3U games ─────────
    generate_disc_system(
        &output_dir,
        "sony_psx",
        PSX_DISC_GAMES,
        &PSX_GAME_NAMES,
        "bin",
        &mut all_roms,
    );
    generate_disc_system(
        &output_dir,
        "sega_cd",
        SEGA_CD_DISC_GAMES,
        &SEGA_CD_GAME_NAMES,
        "bin",
        &mut all_roms,
    );

    // ── Favorites ───────────────────────────────────────────────────────
    let favs_dir = output_dir.join("roms").join("_favorites");
    fs::create_dir_all(&favs_dir).expect("failed to create favorites directory");
    let fav_count = generate_favorites(&favs_dir, &all_roms, 25);
    eprintln!("  _favorites — {} marker files", fav_count);

    // ── Recents ─────────────────────────────────────────────────────────
    let recents_dir = output_dir.join("roms").join("_recent");
    fs::create_dir_all(&recents_dir).expect("failed to create recents directory");
    let rec_count = generate_recents(&recents_dir, &all_roms, 12);
    eprintln!("  _recent — {} marker files", rec_count);

    // ── .replay-control app data ────────────────────────────────────────
    let app_dir = output_dir.join(".replay-control");
    fs::create_dir_all(&app_dir).expect("failed to create app data directory");

    // config.cfg
    let config_path = app_dir.join("config.cfg");
    fs::write(&config_path, generate_config()).expect("failed to write config.cfg");
    eprintln!("  .replay-control/config.cfg");

    // Boxart thumbnails (dummy PNGs for a subset of systems)
    let boxart_systems = ["nintendo_snes", "sega_smd", "nintendo_nes"];
    for sys in &boxart_systems {
        if let Some(roms) = all_roms.get(*sys) {
            let boxart_dir = app_dir.join("media").join(sys).join("boxart");
            fs::create_dir_all(&boxart_dir).expect("failed to create boxart directory");

            // Create boxart for ~10% of ROMs (capped at 50)
            let count = std::cmp::min(roms.len() / 10, 50);
            for rom_filename in roms.iter().take(count) {
                let stem = Path::new(rom_filename)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let png_path = boxart_dir.join(format!("{stem}.png"));
                fs::write(&png_path, TINY_PNG).expect("failed to write boxart PNG");
            }
            eprintln!("  .replay-control/media/{sys}/boxart/ — {count} PNG files");
        }
    }

    // Summary
    let total_roms: usize = all_roms.values().map(|v| v.len()).sum();
    eprintln!();
    eprintln!("Done. Total: {} ROM files across {} systems", total_roms, all_roms.len());
}

// ── DAT Parsing ──────────────────────────────────────────────────────────────

/// Parse a No-Intro ClrMamePro DAT file and extract ROM filenames.
///
/// Format:
/// ```text
/// game (
///     name "Super Mario World (USA)"
///     rom ( name "Super Mario World (USA).sfc" size 524288 crc B19ED489 ... )
/// )
/// ```
///
/// Returns up to `max_count` filenames, prioritizing USA/World/Europe regions.
fn parse_nointro_dat(path: &Path, max_count: usize) -> Vec<String> {
    let file = File::open(path).expect("failed to open DAT file");
    let reader = BufReader::new(file);

    let mut all_entries: Vec<(String, String)> = Vec::new(); // (filename, region)
    let mut in_game = false;
    let mut current_rom_name = String::new();
    let mut current_name = String::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();

        if trimmed.starts_with("game (") || trimmed == "game (" {
            in_game = true;
            current_rom_name.clear();
            current_name.clear();
            continue;
        }

        if trimmed == ")" && in_game {
            in_game = false;
            if !current_rom_name.is_empty() {
                let region = extract_region(&current_name);
                all_entries.push((current_rom_name.clone(), region));
            }
            continue;
        }

        if !in_game {
            continue;
        }

        if let Some(val) = extract_quoted(trimmed, "name ") {
            if current_name.is_empty() {
                // First name field = game name
                current_name = val;
            } else if trimmed.contains("rom (") || current_rom_name.is_empty() {
                // ROM name inside rom ( ... ) block
                current_rom_name = val;
            }
        }
    }

    // Sort: USA/World first, then Europe, then rest. This ensures the most
    // useful test entries survive the truncation to max_count.
    all_entries.sort_by(|a, b| region_priority(&a.1).cmp(&region_priority(&b.1)));

    all_entries
        .into_iter()
        .take(max_count)
        .map(|(filename, _)| filename)
        .collect()
}

/// Parse an FBNeo XML DAT file and extract ROM zip filenames.
///
/// The FBNeo DAT uses MAME-style XML where each `<game name="romname">` element
/// represents an arcade game. The `name` attribute becomes the `.zip` filename.
fn parse_fbneo_dat(path: &Path, max_count: usize) -> Vec<String> {
    let content = fs::read_to_string(path).expect("failed to read FBNeo DAT file");
    let mut filenames: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        // Match lines like: <game name="88games" sourcefile="...">
        if trimmed.starts_with("<game ")
            && let Some(name) = extract_xml_attr(trimmed, "name") {
                filenames.push(format!("{name}.zip"));
                if filenames.len() >= max_count {
                    break;
                }
            }
    }

    filenames
}

/// Extract an XML attribute value from a tag line.
/// e.g., `extract_xml_attr(r#"<game name="88games" ...>"#, "name")` -> Some("88games")
fn extract_xml_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = line.find(&pattern)?;
    let rest = &line[start + pattern.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// ── Disc-Based System Generation ─────────────────────────────────────────────

/// Generate disc-based game files: M3U playlist + companion bin/cue files.
fn generate_disc_system(
    output_dir: &Path,
    system_folder: &str,
    game_count: usize,
    game_names: &[&str],
    disc_ext: &str,
    all_roms: &mut HashMap<String, Vec<String>>,
) {
    let rom_dir = output_dir.join("roms").join(system_folder);
    fs::create_dir_all(&rom_dir).expect("failed to create disc ROM directory");

    let mut filenames = Vec::new();
    let count = std::cmp::min(game_count, game_names.len());

    for name in game_names.iter().take(count) {
        // Single-disc game: name.cue + name.bin + name.m3u
        let cue_name = format!("{name}.cue");
        let bin_name = format!("{name}.{disc_ext}");
        let m3u_name = format!("{name}.m3u");

        // Create empty bin file
        File::create(rom_dir.join(&bin_name)).expect("failed to create bin file");

        // Create minimal cue file
        let cue_content = format!(
            "FILE \"{bin_name}\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n"
        );
        fs::write(rom_dir.join(&cue_name), &cue_content).expect("failed to write cue file");

        // Create M3U referencing the cue
        fs::write(rom_dir.join(&m3u_name), format!("{cue_name}\n"))
            .expect("failed to write m3u file");

        filenames.push(m3u_name);
    }

    // Also create a few multi-disc games (last 5 entries use 2 discs)
    let multi_disc_names = &[
        "Final Fantasy VII (USA)",
        "Final Fantasy VIII (USA)",
        "Legend of Dragoon, The (USA)",
        "Chrono Cross (USA)",
        "Xenogears (USA)",
    ];

    if system_folder == "sony_psx" {
        for name in multi_disc_names {
            for disc in 1..=2 {
                let disc_label = format!("{name} (Disc {disc})");
                let cue_name = format!("{disc_label}.cue");
                let bin_name = format!("{disc_label}.{disc_ext}");

                File::create(rom_dir.join(&bin_name)).expect("failed to create multi-disc bin");
                let cue_content = format!(
                    "FILE \"{bin_name}\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n"
                );
                fs::write(rom_dir.join(&cue_name), &cue_content)
                    .expect("failed to write multi-disc cue");
            }

            let m3u_name = format!("{name}.m3u");
            let m3u_content = format!(
                "{name} (Disc 1).cue\n{name} (Disc 2).cue\n"
            );
            fs::write(rom_dir.join(&m3u_name), &m3u_content)
                .expect("failed to write multi-disc m3u");
            filenames.push(m3u_name);
        }
    }

    eprintln!("  {system_folder} — {} disc games", filenames.len());
    all_roms.insert(system_folder.to_string(), filenames);
}

// ── Favorites & Recents ──────────────────────────────────────────────────────

/// Generate .fav marker files referencing actual ROM filenames.
/// Format: `<system>@<rom_filename>.fav` containing the relative ROM path.
fn generate_favorites(
    favs_dir: &Path,
    all_roms: &HashMap<String, Vec<String>>,
    target_count: usize,
) -> usize {
    let mut count = 0;

    // Sort systems for deterministic output
    let mut systems: Vec<&String> = all_roms.keys().collect();
    systems.sort();

    // Round-robin across systems until we hit the target
    let mut system_idx = 0;
    let mut rom_offsets: HashMap<&String, usize> = HashMap::new();

    while count < target_count && !systems.is_empty() {
        let system = systems[system_idx % systems.len()];
        let roms = &all_roms[system];
        if roms.is_empty() {
            system_idx += 1;
            continue;
        }

        let offset = rom_offsets.entry(system).or_insert(0);
        // Pick evenly spaced ROMs
        let step = (roms.len() / 10).max(1);
        let idx = (*offset * step) % roms.len();

        let rom_filename = &roms[idx];
        let fav_filename = format!("{system}@{rom_filename}.fav");
        let rom_path = format!("{system}/{rom_filename}");

        fs::write(favs_dir.join(&fav_filename), &rom_path)
            .expect("failed to write fav file");
        count += 1;
        *offset += 1;
        system_idx += 1;
    }
    count
}

/// Generate .rec marker files referencing actual ROM filenames.
/// Format: `<system>@<rom_filename>.rec` containing the ROM path.
fn generate_recents(
    recents_dir: &Path,
    all_roms: &HashMap<String, Vec<String>>,
    target_count: usize,
) -> usize {
    let mut count = 0;

    // Sort systems for deterministic output
    let mut systems: Vec<&String> = all_roms.keys().collect();
    systems.sort();

    // Round-robin across systems, picking from a different offset than favorites
    let mut system_idx = 0;
    let mut rom_offsets: HashMap<&String, usize> = HashMap::new();

    while count < target_count && !systems.is_empty() {
        let system = systems[system_idx % systems.len()];
        let roms = &all_roms[system];
        if roms.is_empty() {
            system_idx += 1;
            continue;
        }

        let offset = rom_offsets.entry(system).or_insert(0);
        // Pick from middle of list (different from favorites)
        let base = roms.len() / 3;
        let step = (roms.len() / 10).max(1);
        let idx = (base + *offset * step) % roms.len();

        let rom_filename = &roms[idx];
        let rec_filename = format!("{system}@{rom_filename}.rec");
        let rom_path = format!("roms/{system}/{rom_filename}");

        fs::write(recents_dir.join(&rec_filename), format!("{rom_path}\n"))
            .expect("failed to write rec file");
        count += 1;
        *offset += 1;
        system_idx += 1;
    }
    count
}

// ── Config ───────────────────────────────────────────────────────────────────

fn generate_config() -> String {
    r#"# replay-control test fixture config
language = "en"
theme = "dark"
games_per_page = "60"
"#
    .to_string()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a quoted value from a DAT line.
/// e.g., `extract_quoted(r#"name "Sonic (USA).md""#, "name ")` -> Some("Sonic (USA).md")
fn extract_quoted(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix).or_else(|| {
        // Also handle "rom ( name ..." pattern
        line.find(prefix).map(|i| &line[i + prefix.len()..])
    })?;
    let rest = rest.trim();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        // Unquoted value (ends at space or end of line)
        let end = rest.find(' ').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// Extract region from a game name like "Super Mario World (USA)" -> "USA"
fn extract_region(name: &str) -> String {
    // Find the first parenthesized group
    if let Some(start) = name.find('(')
        && let Some(end) = name[start..].find(')') {
            return name[start + 1..start + end].to_string();
        }
    String::new()
}

/// Priority for sorting: lower = better. USA/World first for most useful test data.
fn region_priority(region: &str) -> u8 {
    if region.contains("USA") || region.contains("World") {
        0
    } else if region.contains("Europe") {
        1
    } else if region.contains("Japan") {
        2
    } else {
        3
    }
}

/// Find the workspace root by looking for the top-level Cargo.toml.
fn workspace_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("failed to get current directory");
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists()
            // Check if this is the workspace root (has [workspace] section)
            && let Ok(content) = fs::read_to_string(&cargo_toml)
            && content.contains("[workspace]") {
                return dir;
            }
        if !dir.pop() {
            // Fallback: assume current directory
            return std::env::current_dir().expect("failed to get current directory");
        }
    }
}

// ── Disc game name lists ─────────────────────────────────────────────────────

const PSX_GAME_NAMES: [&str; 30] = [
    "Castlevania - Symphony of the Night (USA)",
    "Crash Bandicoot (USA)",
    "Crash Bandicoot 2 - Cortex Strikes Back (USA)",
    "Crash Bandicoot 3 - Warped (USA)",
    "Final Fantasy Tactics (USA)",
    "Final Fantasy IX (USA)",
    "Gran Turismo (USA)",
    "Gran Turismo 2 (USA)",
    "Metal Gear Solid (USA)",
    "Oddworld - Abe's Oddysee (USA)",
    "PaRappa the Rapper (USA)",
    "Rayman (USA)",
    "Resident Evil (USA)",
    "Resident Evil 2 (USA)",
    "Resident Evil 3 - Nemesis (USA)",
    "Ridge Racer (USA)",
    "Silent Hill (USA)",
    "Spyro the Dragon (USA)",
    "Spyro 2 - Ripto's Rage! (USA)",
    "Suikoden II (USA)",
    "Tekken 3 (USA)",
    "Tomb Raider (USA)",
    "Tony Hawk's Pro Skater 2 (USA)",
    "Twisted Metal 2 (USA)",
    "Vagrant Story (USA)",
    "Valkyrie Profile (USA)",
    "Wild Arms (USA)",
    "Wipeout XL (USA)",
    "Mega Man X4 (USA)",
    "Breath of Fire III (USA)",
];

const SEGA_CD_GAME_NAMES: [&str; 15] = [
    "Lunar - The Silver Star (USA)",
    "Lunar 2 - Eternal Blue (USA)",
    "Sonic CD (USA)",
    "Snatcher (USA)",
    "Shining Force CD (USA)",
    "Popful Mail (USA)",
    "Sewer Shark (USA)",
    "Night Trap (USA)",
    "Silpheed (USA)",
    "Vay (USA)",
    "Dark Wizard (USA)",
    "Keio Flying Squadron (USA)",
    "Robo Aleste (USA)",
    "Android Assault (USA)",
    "Sega Classics Arcade Collection (USA)",
];
