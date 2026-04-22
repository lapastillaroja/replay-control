// Port of replay-control-core/build.rs: parses game data files and writes
// catalog.sqlite instead of generating PHF Rust code.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use clap::Parser;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use rusqlite::{Connection, params};

#[allow(dead_code)]
#[path = "../../../replay-control-core/src/game/title_utils.rs"]
mod title_utils;

// =============================================================================
// CLI
// =============================================================================

#[derive(Parser)]
#[command(
    name = "build-catalog",
    about = "Build catalog.sqlite from game data files"
)]
struct Args {
    /// Output path for catalog.sqlite
    #[arg(long, default_value = "catalog.sqlite")]
    output: PathBuf,

    /// Directory containing raw data files (no-intro/, tgdb, etc.)
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    /// Use fixture data from replay-control-core/fixtures/ instead of data/
    #[arg(long)]
    stub: bool,
}

// =============================================================================
// Schema
// =============================================================================

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE arcade_games (
            rom_name TEXT PRIMARY KEY,
            display_name TEXT NOT NULL DEFAULT '',
            year TEXT NOT NULL DEFAULT '',
            manufacturer TEXT NOT NULL DEFAULT '',
            players INTEGER NOT NULL DEFAULT 0,
            rotation TEXT NOT NULL DEFAULT 'unknown',
            status TEXT NOT NULL DEFAULT 'unknown',
            is_clone INTEGER NOT NULL DEFAULT 0,
            is_bios INTEGER NOT NULL DEFAULT 0,
            parent TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT '',
            normalized_genre TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE canonical_games (
            id INTEGER PRIMARY KEY,
            system TEXT NOT NULL,
            display_name TEXT NOT NULL,
            year INTEGER NOT NULL DEFAULT 0,
            genre TEXT NOT NULL DEFAULT '',
            developer TEXT NOT NULL DEFAULT '',
            publisher TEXT NOT NULL DEFAULT '',
            players INTEGER NOT NULL DEFAULT 0,
            coop INTEGER,
            rating TEXT NOT NULL DEFAULT '',
            normalized_genre TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX idx_cg_system ON canonical_games(system);

        CREATE TABLE rom_entries (
            id INTEGER PRIMARY KEY,
            system TEXT NOT NULL,
            filename_stem TEXT NOT NULL,
            region TEXT NOT NULL DEFAULT '',
            crc32 INTEGER NOT NULL DEFAULT 0,
            canonical_game_id INTEGER NOT NULL REFERENCES canonical_games(id),
            normalized_title TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX idx_re_stem ON rom_entries(system, filename_stem);
        CREATE INDEX idx_re_crc  ON rom_entries(system, crc32);
        CREATE INDEX idx_re_norm ON rom_entries(system, normalized_title);

        CREATE TABLE rom_alternates (
            canonical_game_id INTEGER NOT NULL,
            system TEXT NOT NULL,
            alternate_name TEXT NOT NULL
        );
        CREATE INDEX idx_ra_game ON rom_alternates(canonical_game_id, system);

        CREATE TABLE series_entries (
            id INTEGER PRIMARY KEY,
            game_title TEXT NOT NULL,
            series_name TEXT NOT NULL DEFAULT '',
            system TEXT NOT NULL,
            series_order INTEGER,
            follows TEXT NOT NULL DEFAULT '',
            followed_by TEXT NOT NULL DEFAULT '',
            normalized_title TEXT NOT NULL
        );
        CREATE INDEX idx_se_system ON series_entries(system, normalized_title);

        CREATE TABLE arcade_release_dates (
            rom_name TEXT NOT NULL,
            year TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'mame'
        );

        CREATE TABLE console_release_dates (
            system TEXT NOT NULL,
            base_title TEXT NOT NULL,
            region TEXT NOT NULL,
            release_date TEXT NOT NULL,
            precision TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'tgdb',
            PRIMARY KEY (system, base_title, region)
        );

        CREATE TABLE db_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    ",
    )?;
    Ok(())
}

// =============================================================================
// Arcade data structures
// =============================================================================

struct ArcadeEntry {
    rom_name: String,
    display_name: String,
    year: String,
    manufacturer: String,
    players: u8,
    rotation: String,
    status: String,
    is_clone: bool,
    is_bios: bool,
    parent: String,
    category: String,
}

// =============================================================================
// Arcade parsers
// =============================================================================

fn parse_csv(path: &Path) -> Vec<ArcadeEntry> {
    let mut entries = Vec::new();
    let mut rdr = match csv::Reader::from_path(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Warning: failed to open CSV at {}: {}", path.display(), e);
            return entries;
        }
    };
    for result in rdr.records() {
        let record = match result {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rom_name = record.get(0).unwrap_or("").to_string();
        if rom_name.is_empty() {
            continue;
        }
        let players: u8 = record.get(4).unwrap_or("0").parse().unwrap_or(0);
        let is_clone = record.get(7).unwrap_or("false") == "true";
        entries.push(ArcadeEntry {
            rom_name,
            display_name: record.get(1).unwrap_or("").to_string(),
            year: record.get(2).unwrap_or("").to_string(),
            manufacturer: record.get(3).unwrap_or("").to_string(),
            players,
            rotation: record.get(5).unwrap_or("0").to_string(),
            status: record.get(6).unwrap_or("unknown").to_string(),
            is_clone,
            is_bios: false,
            parent: record.get(8).unwrap_or("").to_string(),
            category: record.get(9).unwrap_or("").to_string(),
        });
    }
    entries
}

fn parse_fbneo_dat(path: &Path) -> Vec<ArcadeEntry> {
    let mut entries = Vec::new();
    let mut reader = match Reader::from_file(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "Warning: failed to open FBNeo DAT at {}: {}",
                path.display(),
                e
            );
            return entries;
        }
    };
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut in_game = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_element = String::new();
    let mut current_is_bios = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"game" => {
                    in_game = true;
                    current_name.clear();
                    current_cloneof.clear();
                    current_description.clear();
                    current_year.clear();
                    current_manufacturer.clear();
                    current_is_bios = false;
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        match attr.key.local_name().as_ref() {
                            b"name" => {
                                current_name = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"cloneof" => {
                                current_cloneof = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"isbios" => {
                                current_is_bios = String::from_utf8_lossy(&attr.value) == "yes"
                            }
                            _ => {}
                        }
                    }
                }
                b"description" | b"year" | b"manufacturer" if in_game => {
                    current_element = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) if in_game => {
                let text = e.decode().unwrap_or_default();
                match current_element.as_str() {
                    "description" => current_description.push_str(&text),
                    "year" => current_year.push_str(&text),
                    "manufacturer" => current_manufacturer.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"game" if in_game => {
                    if !current_name.is_empty() {
                        entries.push(ArcadeEntry {
                            rom_name: current_name.clone(),
                            display_name: current_description.clone(),
                            year: current_year.clone(),
                            manufacturer: current_manufacturer.clone(),
                            players: 0,
                            rotation: "unknown".to_string(),
                            status: "unknown".to_string(),
                            is_clone: !current_cloneof.is_empty(),
                            is_bios: current_is_bios,
                            parent: current_cloneof.clone(),
                            category: String::new(),
                        });
                    }
                    in_game = false;
                }
                b"description" | b"year" | b"manufacturer" => current_element.clear(),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!(
                    "Error parsing FBNeo DAT at {}: {:?}",
                    reader.error_position(),
                    e
                );
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    entries
}

fn parse_mame2003plus_xml(path: &Path) -> Vec<ArcadeEntry> {
    let mut entries = Vec::new();
    let mut reader = match Reader::from_file(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "Warning: failed to open MAME 2003+ XML at {}: {}",
                path.display(),
                e
            );
            return entries;
        }
    };
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut in_game = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_orientation = "unknown".to_string();
    let mut current_players: u8 = 0;
    let mut current_status = "unknown".to_string();
    let mut current_element = String::new();
    let mut current_is_bios = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"game" => {
                    in_game = true;
                    current_name.clear();
                    current_cloneof.clear();
                    current_description.clear();
                    current_year.clear();
                    current_manufacturer.clear();
                    current_orientation = "unknown".to_string();
                    current_players = 0;
                    current_status = "unknown".to_string();
                    current_is_bios = false;
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        match attr.key.local_name().as_ref() {
                            b"name" => {
                                current_name = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"cloneof" => {
                                current_cloneof = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"runnable" if String::from_utf8_lossy(&attr.value) == "no" => {
                                current_is_bios = true;
                            }
                            _ => {}
                        }
                    }
                }
                b"description" | b"year" | b"manufacturer" if in_game => {
                    current_element = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) if in_game => match e.local_name().as_ref() {
                b"video" => {
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        if attr.key.local_name().as_ref() == b"orientation" {
                            let val = String::from_utf8_lossy(&attr.value).into_owned();
                            current_orientation = match val.as_str() {
                                "horizontal" => "0".to_string(),
                                "vertical" => "90".to_string(),
                                _ => "unknown".to_string(),
                            };
                        }
                    }
                }
                b"input" => {
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        if attr.key.local_name().as_ref() == b"players" {
                            current_players =
                                String::from_utf8_lossy(&attr.value).parse().unwrap_or(0);
                        }
                    }
                }
                b"driver" => {
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        if attr.key.local_name().as_ref() == b"status" {
                            current_status = String::from_utf8_lossy(&attr.value).into_owned();
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) if in_game => {
                let text = e.decode().unwrap_or_default();
                match current_element.as_str() {
                    "description" => current_description.push_str(&text),
                    "year" => current_year.push_str(&text),
                    "manufacturer" => current_manufacturer.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"game" if in_game => {
                    if !current_name.is_empty() {
                        entries.push(ArcadeEntry {
                            rom_name: current_name.clone(),
                            display_name: current_description.clone(),
                            year: current_year.clone(),
                            manufacturer: current_manufacturer.clone(),
                            players: current_players,
                            rotation: current_orientation.clone(),
                            status: current_status.clone(),
                            is_clone: !current_cloneof.is_empty(),
                            is_bios: current_is_bios,
                            parent: current_cloneof.clone(),
                            category: String::new(),
                        });
                    }
                    in_game = false;
                }
                b"description" | b"year" | b"manufacturer" => current_element.clear(),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!(
                    "Error parsing MAME 2003+ XML at {}: {:?}",
                    reader.error_position(),
                    e
                );
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    entries
}

fn parse_mame_current_xml(path: &Path) -> Vec<ArcadeEntry> {
    let mut entries = Vec::new();
    let mut reader = match Reader::from_file(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "Warning: failed to open MAME current XML at {}: {}",
                path.display(),
                e
            );
            return entries;
        }
    };
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut in_machine = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_rotate = "unknown".to_string();
    let mut current_players: u8 = 0;
    let mut current_status = "unknown".to_string();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"m" => {
                    in_machine = true;
                    current_name.clear();
                    current_cloneof.clear();
                    current_rotate = "unknown".to_string();
                    current_players = 0;
                    current_status = "unknown".to_string();
                    current_description.clear();
                    current_year.clear();
                    current_manufacturer.clear();
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        match attr.key.local_name().as_ref() {
                            b"name" => {
                                current_name = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"cloneof" => {
                                current_cloneof = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"rotate" => {
                                current_rotate = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            b"players" => {
                                current_players =
                                    String::from_utf8_lossy(&attr.value).parse().unwrap_or(0)
                            }
                            b"status" => {
                                current_status = String::from_utf8_lossy(&attr.value).into_owned()
                            }
                            _ => {}
                        }
                    }
                }
                b"d" | b"y" | b"f" if in_machine => {
                    current_element = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) if in_machine => {
                let text = e.decode().unwrap_or_default();
                match current_element.as_str() {
                    "d" => current_description.push_str(&text),
                    "y" => current_year.push_str(&text),
                    "f" => current_manufacturer.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"m" if in_machine => {
                    if !current_name.is_empty() {
                        entries.push(ArcadeEntry {
                            rom_name: current_name.clone(),
                            display_name: current_description.clone(),
                            year: current_year.clone(),
                            manufacturer: current_manufacturer.clone(),
                            players: current_players,
                            rotation: current_rotate.clone(),
                            status: current_status.clone(),
                            is_clone: !current_cloneof.is_empty(),
                            is_bios: false,
                            parent: current_cloneof.clone(),
                            category: String::new(),
                        });
                    }
                    in_machine = false;
                }
                b"d" | b"y" | b"f" => current_element.clear(),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                eprintln!(
                    "Error parsing MAME current XML at {}: {:?}",
                    reader.error_position(),
                    e
                );
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    entries
}

fn parse_catver_ini(path: &Path) -> HashMap<String, String> {
    let mut categories = HashMap::new();
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Warning: failed to open catver.ini at {}: {}",
                path.display(),
                e
            );
            return categories;
        }
    };
    let mut in_category_section = false;
    for line in BufReader::new(file).lines() {
        let line = line.unwrap_or_default();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') {
            in_category_section = trimmed == "[Category]";
            continue;
        }
        if in_category_section && let Some((rom_name, category)) = trimmed.split_once('=') {
            let rom_name = rom_name.trim();
            let category = category.trim();
            if !rom_name.is_empty() && !category.is_empty() {
                categories.insert(rom_name.to_string(), category.to_string());
            }
        }
    }
    categories
}

fn parse_nplayers_ini(path: &Path) -> HashMap<String, u8> {
    let mut players_map = HashMap::new();
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Warning: failed to open nplayers.ini at {}: {}",
                path.display(),
                e
            );
            return players_map;
        }
    };
    let mut in_nplayers_section = false;
    for line in BufReader::new(file).lines() {
        let line = line.unwrap_or_default();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') {
            in_nplayers_section = trimmed == "[NPlayers]";
            continue;
        }
        if in_nplayers_section && let Some((rom_name, value)) = trimmed.split_once('=') {
            let rom_name = rom_name.trim();
            let value = value.trim();
            if matches!(value, "???" | "Device" | "Non-arcade" | "BIOS" | "Pinball") {
                continue;
            }
            if let Some(players) = parse_nplayers_value(value) {
                players_map.insert(rom_name.to_string(), players);
            }
        }
    }
    players_map
}

fn parse_nplayers_value(value: &str) -> Option<u8> {
    for part in value.split('/') {
        let part = part.trim();
        if let Some(p_pos) = part.find('P') {
            let prefix = &part[..p_pos];
            if let Ok(n) = prefix.trim().parse::<u8>()
                && n > 0
            {
                return Some(n);
            }
        }
    }
    None
}

// =============================================================================
// Arcade normalization
// =============================================================================

fn rotation_str(rot: &str) -> &'static str {
    match rot {
        "0" => "horizontal",
        "90" => "vertical",
        "180" => "horizontal",
        "270" => "vertical",
        _ => "unknown",
    }
}

fn status_str(status: &str) -> &'static str {
    match status {
        "good" | "working" => "working",
        "imperfect" => "imperfect",
        "preliminary" | "protection" => "preliminary",
        _ => "unknown",
    }
}

fn normalize_arcade_genre(category: &str) -> &'static str {
    let primary = category.split(" / ").next().unwrap_or(category).trim();
    match primary {
        "Fighter" => "Fighting",
        "Platform" | "Climbing" => "Platform",
        "Shooter" => "Shooter",
        "Driving" => "Driving",
        "Sports" => "Sports",
        "Puzzle" => "Puzzle",
        "Maze" => "Maze",
        "Casino" | "Slot Machine" => "Board & Card",
        "Tabletop" => "Board & Card",
        "Quiz" | "Trivia" => "Quiz",
        "Pinball" => "Pinball",
        "Ball & Paddle" | "Breakout" => "Action",
        "Music" | "Rhythm" => "Music",
        "Racing" => "Driving",
        "Beat'em Up" | "BeatEmUp" => "Beat'em Up",
        "Action" => "Action",
        "Adventure" => "Adventure",
        "Simulation" | "Flight" => "Simulation",
        "Strategy" => "Strategy",
        "Board Game" | "Cards" => "Board & Card",
        "Educational" => "Educational",
        "Role-Playing" | "RPG" => "Role-Playing",
        "System" | "BIOS" | "Utilities" | "Electromechanical" | "Device" | "Rewritable"
        | "Not Coverage" | "Mature" => "Other",
        _ if category.is_empty() => "",
        _ => "Other",
    }
}

// =============================================================================
// Arcade DB insertion
// =============================================================================

fn insert_arcade_games(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<()> {
    let arcade_dir = sources_dir.join("arcade");
    let mut entries_map: HashMap<String, ArcadeEntry> = HashMap::new();
    let mut entry_source: HashMap<String, &'static str> = HashMap::new();
    let mut flycast_rom_names: HashSet<String> = HashSet::new();

    // 1. Flycast CSV
    let flycast_path = arcade_dir.join("flycast_games.csv");
    if flycast_path.exists() {
        let flycast_entries = parse_csv(&flycast_path);
        eprintln!(
            "Arcade DB: Flycast CSV loaded {} entries",
            flycast_entries.len()
        );
        for entry in flycast_entries {
            flycast_rom_names.insert(entry.rom_name.clone());
            entry_source.insert(entry.rom_name.clone(), "naomi");
            entries_map.insert(entry.rom_name.clone(), entry);
        }
    }

    // 2. FBNeo DAT
    let fbneo_path = sources_dir.join("fbneo-arcade.dat");
    if fbneo_path.exists() {
        let fbneo_entries = parse_fbneo_dat(&fbneo_path);
        eprintln!(
            "Arcade DB: FBNeo DAT loaded {} entries",
            fbneo_entries.len()
        );
        for entry in fbneo_entries {
            let rom_name = entry.rom_name.clone();
            if let std::collections::hash_map::Entry::Vacant(v) =
                entries_map.entry(rom_name.clone())
            {
                v.insert(entry);
                entry_source.insert(rom_name, "fbneo");
            }
        }
    }

    // 3. MAME 2003+
    let mame_path = sources_dir.join("mame2003plus.xml");
    if mame_path.exists() {
        let mame_entries = parse_mame2003plus_xml(&mame_path);
        eprintln!(
            "Arcade DB: MAME 2003+ loaded {} entries",
            mame_entries.len()
        );
        for entry in mame_entries {
            let rom_name = entry.rom_name.clone();
            match entries_map.entry(rom_name.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occ) => {
                    let existing = occ.get();
                    if existing.players == 0
                        && existing.rotation == "unknown"
                        && existing.status == "unknown"
                    {
                        occ.insert(entry);
                        entry_source.insert(rom_name, "mame");
                    }
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(entry);
                    entry_source.insert(rom_name, "mame");
                }
            }
        }
    }

    // 4. MAME current
    let mame_current_path = sources_dir.join("mame0285-arcade.xml");
    if mame_current_path.exists() {
        let mame_current_entries = parse_mame_current_xml(&mame_current_path);
        eprintln!(
            "Arcade DB: MAME current loaded {} entries",
            mame_current_entries.len()
        );
        let mut new_count = 0u32;
        let mut override_count = 0u32;
        for entry in mame_current_entries {
            let rom_name = entry.rom_name.clone();
            match entries_map.entry(rom_name.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occ) => {
                    if !flycast_rom_names.contains(&rom_name) {
                        occ.insert(entry);
                        entry_source.insert(rom_name, "mame");
                        override_count += 1;
                    }
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(entry);
                    entry_source.insert(rom_name, "mame");
                    new_count += 1;
                }
            }
        }
        eprintln!(
            "Arcade DB: MAME current added {} new, overrode {} existing",
            new_count, override_count
        );
    }

    // 5. catver.ini overlays
    let catver_path = sources_dir.join("catver.ini");
    if catver_path.exists() {
        let categories = parse_catver_ini(&catver_path);
        let mut applied = 0u32;
        for (rom_name, category) in &categories {
            if let Some(entry) = entries_map.get_mut(rom_name)
                && entry.category.is_empty()
            {
                entry.category = category.clone();
                applied += 1;
            }
        }
        eprintln!("Arcade DB: Applied {} catver.ini overlays", applied);
    }

    let catver_current_path = sources_dir.join("catver-mame-current.ini");
    if catver_current_path.exists() {
        let categories = parse_catver_ini(&catver_current_path);
        let mut applied = 0u32;
        for (rom_name, category) in &categories {
            if let Some(entry) = entries_map.get_mut(rom_name)
                && entry.category.is_empty()
            {
                entry.category = category.clone();
                applied += 1;
            }
        }
        eprintln!(
            "Arcade DB: Applied {} catver-mame-current.ini overlays",
            applied
        );
    }

    // 6. nplayers.ini overlay
    let nplayers_path = sources_dir.join("nplayers.ini");
    if nplayers_path.exists() {
        let nplayers = parse_nplayers_ini(&nplayers_path);
        let mut applied = 0u32;
        for (rom_name, players) in &nplayers {
            if let Some(entry) = entries_map.get_mut(rom_name)
                && entry.players == 0
            {
                entry.players = *players;
                applied += 1;
            }
        }
        eprintln!("Arcade DB: Applied {} nplayers.ini overlays", applied);
    }

    // 7. Mark BIOS by category
    for entry in entries_map.values_mut() {
        if entry.category.starts_with("System / BIOS") {
            entry.is_bios = true;
        }
    }

    // 8. Filter non-game machines
    let non_game_prefixes = [
        "Electromechanical",
        "Slot Machine",
        "Gambling",
        "Computer",
        "Handheld",
        "Game Console",
        "Calculator",
        "Printer",
        "Utilities",
        "System",
    ];
    let total_before = entries_map.len();
    entries_map.retain(|_, entry| {
        if entry.is_bios {
            return true;
        }
        if entry.category.is_empty() {
            return true;
        }
        !non_game_prefixes
            .iter()
            .any(|p| entry.category.starts_with(p))
    });
    eprintln!(
        "Arcade DB: Filtered {} non-game machines",
        total_before - entries_map.len()
    );

    let mut entries: Vec<ArcadeEntry> = entries_map.into_values().collect();
    entries.sort_by(|a, b| a.rom_name.cmp(&b.rom_name));

    eprintln!(
        "Arcade DB: Total {} entries ({} playable, {} BIOS)",
        entries.len(),
        entries.iter().filter(|e| !e.is_bios).count(),
        entries.iter().filter(|e| e.is_bios).count()
    );

    // Insert into arcade_games
    let mut stmt = conn.prepare(
        "INSERT INTO arcade_games (rom_name, display_name, year, manufacturer, players, rotation, status, is_clone, is_bios, parent, category, normalized_genre) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
    )?;
    for entry in &entries {
        let norm_genre = normalize_arcade_genre(&entry.category);
        stmt.execute(params![
            entry.rom_name,
            entry.display_name,
            entry.year,
            entry.manufacturer,
            entry.players as i64,
            rotation_str(&entry.rotation),
            status_str(&entry.status),
            entry.is_clone as i64,
            entry.is_bios as i64,
            entry.parent,
            entry.category,
            norm_genre,
        ])?;
    }

    // Insert arcade_release_dates
    let mut stmt_rd = conn
        .prepare("INSERT INTO arcade_release_dates (rom_name, year, source) VALUES (?1, ?2, ?3)")?;
    let mut rd_count = 0u32;
    for entry in &entries {
        if entry.is_bios {
            continue;
        }
        let y = entry.year.trim();
        if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) {
            let src = entry_source.get(&entry.rom_name).copied().unwrap_or("mame");
            stmt_rd.execute(params![entry.rom_name, y, src])?;
            rd_count += 1;
        }
    }
    eprintln!("Arcade DB: Inserted {} release date rows", rd_count);

    Ok(())
}

// =============================================================================
// Console data structures
// =============================================================================

struct SystemConfig {
    folder_name: &'static str,
    nointro_dat: &'static str,
    tgdb_platform_ids: &'static [u32],
}

const GAME_DB_SYSTEMS: &[SystemConfig] = &[
    // Nintendo cartridge/handheld
    SystemConfig {
        folder_name: "nintendo_nes",
        nointro_dat: "Nintendo - Nintendo Entertainment System.dat",
        tgdb_platform_ids: &[7],
    },
    SystemConfig {
        folder_name: "nintendo_snes",
        nointro_dat: "Nintendo - Super Nintendo Entertainment System.dat",
        tgdb_platform_ids: &[6],
    },
    SystemConfig {
        folder_name: "nintendo_gb",
        nointro_dat: "Nintendo - Game Boy.dat",
        tgdb_platform_ids: &[4],
    },
    SystemConfig {
        folder_name: "nintendo_gbc",
        nointro_dat: "Nintendo - Game Boy Color.dat",
        tgdb_platform_ids: &[41],
    },
    SystemConfig {
        folder_name: "nintendo_gba",
        nointro_dat: "Nintendo - Game Boy Advance.dat",
        tgdb_platform_ids: &[5],
    },
    SystemConfig {
        folder_name: "nintendo_n64",
        nointro_dat: "Nintendo - Nintendo 64.dat",
        tgdb_platform_ids: &[3],
    },
    // Sega cartridge/handheld
    SystemConfig {
        folder_name: "sega_sms",
        nointro_dat: "Sega - Master System - Mark III.dat",
        tgdb_platform_ids: &[35],
    },
    SystemConfig {
        folder_name: "sega_smd",
        nointro_dat: "Sega - Mega Drive - Genesis.dat",
        tgdb_platform_ids: &[18, 36],
    },
    SystemConfig {
        folder_name: "sega_gg",
        nointro_dat: "Sega - Game Gear.dat",
        tgdb_platform_ids: &[20],
    },
    SystemConfig {
        folder_name: "sega_sg",
        nointro_dat: "Sega - SG-1000.dat",
        tgdb_platform_ids: &[4949],
    },
    SystemConfig {
        folder_name: "sega_32x",
        nointro_dat: "Sega - 32X.dat",
        tgdb_platform_ids: &[33],
    },
    // Atari
    SystemConfig {
        folder_name: "atari_2600",
        nointro_dat: "",
        tgdb_platform_ids: &[22],
    },
    SystemConfig {
        folder_name: "atari_5200",
        nointro_dat: "",
        tgdb_platform_ids: &[26],
    },
    SystemConfig {
        folder_name: "atari_7800",
        nointro_dat: "",
        tgdb_platform_ids: &[27],
    },
    SystemConfig {
        folder_name: "atari_jaguar",
        nointro_dat: "",
        tgdb_platform_ids: &[28],
    },
    SystemConfig {
        folder_name: "atari_lynx",
        nointro_dat: "",
        tgdb_platform_ids: &[4924],
    },
    // NEC
    SystemConfig {
        folder_name: "nec_pce",
        nointro_dat: "",
        tgdb_platform_ids: &[34],
    },
    SystemConfig {
        folder_name: "nec_pcecd",
        nointro_dat: "",
        tgdb_platform_ids: &[4955],
    },
    // Nintendo (no DAT yet)
    SystemConfig {
        folder_name: "nintendo_ds",
        nointro_dat: "",
        tgdb_platform_ids: &[8],
    },
    // SNK
    SystemConfig {
        folder_name: "snk_ng",
        nointro_dat: "",
        tgdb_platform_ids: &[24],
    },
    SystemConfig {
        folder_name: "snk_ngcd",
        nointro_dat: "",
        tgdb_platform_ids: &[4956],
    },
    SystemConfig {
        folder_name: "snk_ngp",
        nointro_dat: "",
        tgdb_platform_ids: &[4922, 4923],
    },
    // Disc-based consoles
    SystemConfig {
        folder_name: "sony_psx",
        nointro_dat: "",
        tgdb_platform_ids: &[10],
    },
    SystemConfig {
        folder_name: "sega_dc",
        nointro_dat: "",
        tgdb_platform_ids: &[16],
    },
    SystemConfig {
        folder_name: "sega_st",
        nointro_dat: "",
        tgdb_platform_ids: &[17],
    },
    SystemConfig {
        folder_name: "sega_cd",
        nointro_dat: "",
        tgdb_platform_ids: &[21],
    },
    SystemConfig {
        folder_name: "panasonic_3do",
        nointro_dat: "",
        tgdb_platform_ids: &[25],
    },
    SystemConfig {
        folder_name: "philips_cdi",
        nointro_dat: "",
        tgdb_platform_ids: &[4917],
    },
    // Computer systems
    SystemConfig {
        folder_name: "amstrad_cpc",
        nointro_dat: "",
        tgdb_platform_ids: &[4914],
    },
    SystemConfig {
        folder_name: "commodore_ami",
        nointro_dat: "",
        tgdb_platform_ids: &[4911],
    },
    SystemConfig {
        folder_name: "commodore_amicd",
        nointro_dat: "",
        tgdb_platform_ids: &[4947],
    },
    SystemConfig {
        folder_name: "commodore_c64",
        nointro_dat: "",
        tgdb_platform_ids: &[40],
    },
    SystemConfig {
        folder_name: "ibm_pc",
        nointro_dat: "",
        tgdb_platform_ids: &[1],
    },
    SystemConfig {
        folder_name: "microsoft_msx",
        nointro_dat: "",
        tgdb_platform_ids: &[4929],
    },
    SystemConfig {
        folder_name: "sharp_x68k",
        nointro_dat: "",
        tgdb_platform_ids: &[4931],
    },
    SystemConfig {
        folder_name: "sinclair_zx",
        nointro_dat: "",
        tgdb_platform_ids: &[4913],
    },
];

struct NoIntroEntry {
    name: String,
    rom_filename: String,
    region: String,
    crc32: u32,
}

struct TgdbEntry {
    #[allow(dead_code)]
    title: String,
    year: u16,
    players: u8,
    genre_ids: Vec<u32>,
    developer_ids: Vec<u32>,
    publisher_ids: Vec<u32>,
    coop: Option<bool>,
    rating: String,
    alternates: Vec<String>,
}

#[derive(Debug, Clone)]
struct TgdbRegionalDate {
    region_id: u32,
    release_date: String,
}

type TgdbRegionalDatesMap = HashMap<(String, u32), Vec<TgdbRegionalDate>>;

struct CanonicalGameBuild {
    display_name: String,
    year: u16,
    genre: String,
    developer: String,
    publisher: String,
    players: u8,
    coop: Option<bool>,
    rating: String,
    alternates: Vec<String>,
}

struct RomEntryBuild {
    filename_stem: String,
    region: String,
    crc32: u32,
    game_id: usize,
}

// =============================================================================
// Console parsers
// =============================================================================

fn extract_quoted_field(line: &str, field: &str) -> Option<String> {
    let rest = line.strip_prefix(field)?.trim().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_quoted_after(line: &str, keyword: &str) -> Option<String> {
    let idx = line.find(keyword)?;
    let rest = &line[idx + keyword.len()..];
    let rest = rest.trim().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_word_after(line: &str, keyword: &str) -> Option<String> {
    let idx = line.find(keyword)?;
    let rest = &line[idx + keyword.len()..];
    let word: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ')')
        .collect();
    if word.is_empty() { None } else { Some(word) }
}

fn extract_region_from_name(name: &str) -> String {
    if let Some(start) = name.find('(')
        && let Some(end) = name[start..].find(')')
    {
        let tag = &name[start + 1..start + end];
        let regions = [
            "USA",
            "Europe",
            "Japan",
            "World",
            "Australia",
            "Brazil",
            "Canada",
            "China",
            "France",
            "Germany",
            "Hong Kong",
            "Italy",
            "Korea",
            "Netherlands",
            "Russia",
            "Spain",
            "Sweden",
            "Taiwan",
            "UK",
        ];
        for region in &regions {
            if tag.contains(region) {
                return region.to_string();
            }
        }
        return tag.to_string();
    }
    String::new()
}

fn parse_nointro_dat(path: &Path) -> Vec<NoIntroEntry> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: could not open {}: {}", path.display(), e);
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    let mut in_game = false;
    let mut in_rom = false;
    let mut current_name = String::new();
    let mut current_region = String::new();
    let mut current_rom_name = String::new();
    let mut current_crc: u32 = 0;

    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();

        if trimmed == ")" && in_rom {
            in_rom = false;
            continue;
        }
        if trimmed.starts_with("game (") || trimmed == "game (" {
            in_game = true;
            current_name.clear();
            current_region.clear();
            current_rom_name.clear();
            current_crc = 0;
            continue;
        }
        if trimmed == ")" && in_game {
            in_game = false;
            if !current_name.is_empty() && !current_rom_name.is_empty() {
                if current_region.is_empty() {
                    current_region = extract_region_from_name(&current_name);
                }
                entries.push(NoIntroEntry {
                    name: current_name.clone(),
                    rom_filename: current_rom_name.clone(),
                    region: current_region.clone(),
                    crc32: current_crc,
                });
            }
            continue;
        }
        if !in_game {
            continue;
        }

        if let Some(val) = extract_quoted_field(trimmed, "name ")
            && !in_rom
            && !trimmed.starts_with("rom (")
            && !trimmed.contains("rom ( name")
        {
            current_name = val;
        }
        if let Some(val) = extract_quoted_field(trimmed, "region ") {
            current_region = val;
        }
        if trimmed.starts_with("rom (") || trimmed.starts_with("rom(") {
            in_rom = true;
            if let Some(rom_name) = extract_quoted_after(trimmed, "name ") {
                current_rom_name = rom_name;
            }
            if let Some(crc_str) = extract_word_after(trimmed, "crc ") {
                current_crc = u32::from_str_radix(&crc_str, 16).unwrap_or(0);
            }
            if trimmed.ends_with(')') {
                in_rom = false;
            }
        }
    }
    entries
}

fn parse_libretro_meta_dat(path: &Path, field_name: &str) -> HashMap<u32, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return HashMap::new(),
    };
    let mut result = HashMap::new();
    let mut in_game = false;
    let mut current_value = String::new();
    let mut current_crc: u32 = 0;

    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.starts_with("game (") || trimmed == "game (" {
            in_game = true;
            current_value.clear();
            current_crc = 0;
            continue;
        }
        if trimmed == ")" && in_game {
            in_game = false;
            if current_crc != 0 && !current_value.is_empty() {
                result.insert(current_crc, current_value.clone());
            }
            continue;
        }
        if !in_game {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(field_name) {
            let rest = rest.trim();
            if let Some(quoted) = rest.strip_prefix('"') {
                if let Some(end) = quoted.find('"') {
                    current_value = quoted[..end].to_string();
                }
            } else {
                current_value = rest.to_string();
            }
        }
        if (trimmed.starts_with("rom (") || trimmed.starts_with("rom("))
            && let Some(crc_str) = extract_word_after(trimmed, "crc ")
        {
            current_crc = u32::from_str_radix(&crc_str, 16).unwrap_or(0);
        }
    }
    result
}

fn load_tgdb_name_map(path: &Path) -> HashMap<u32, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return HashMap::new(),
    };
    let map: HashMap<String, String> = match serde_json::from_reader(BufReader::new(file)) {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    map.into_iter()
        .filter_map(|(k, v)| k.parse::<u32>().ok().map(|id| (id, v)))
        .collect()
}

// =============================================================================
// Console normalization helpers
// =============================================================================

fn normalize_console_genre(genre: &str) -> &'static str {
    match genre {
        "Action" => "Action",
        "Adventure" => "Adventure",
        "Beat'em Up" | "Beat-'Em-Up" | "Beat 'Em Up" => "Beat'em Up",
        "Board" | "Card" | "Board Game" | "Casino" | "Gambling" => "Board & Card",
        "Racing" | "Driving" => "Driving",
        "Educational" => "Educational",
        "Fighting" => "Fighting",
        "Music" | "Rhythm" => "Music",
        "Pinball" => "Pinball",
        "Platform" => "Platform",
        "Puzzle" => "Puzzle",
        "Quiz" | "Trivia" => "Quiz",
        "Role-Playing" | "Role-playing (RPG)" | "RPG" | "Role-Playing (RPG)" => "Role-Playing",
        "Shooter" | "Shoot-'Em-Up" | "Shoot'em Up" | "Lightgun Shooter" | "Run & Gun"
        | "Shoot 'Em Up" => "Shooter",
        "Simulation" | "Flight Simulator" | "Virtual Life" => "Simulation",
        "Sports" | "Fitness" => "Sports",
        "Strategy" => "Strategy",
        "Maze" => "Maze",
        "Compilation" | "Party" => "Action",
        "Sandbox" | "Stealth" | "Horror" | "MMO" | "Family" | "Comedy" => "Action",
        _ if genre.is_empty() => "",
        _ => "Other",
    }
}

fn normalize_title(name: &str) -> String {
    let base = name.split('(').next().unwrap_or(name).trim();
    let mut result = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}

fn clean_display_name(name: &str) -> String {
    let base = name.split('(').next().unwrap_or(name).trim();
    for article in &[", The", ", An", ", A"] {
        if let Some(idx) = base.find(article) {
            let after = &base[idx + article.len()..];
            if after.is_empty() || after.starts_with(" - ") || after.starts_with(" ~ ") {
                let prefix = &base[..idx];
                let art = &article[2..];
                if after.is_empty() {
                    return format!("{art} {prefix}");
                } else {
                    return format!("{art} {prefix}{after}");
                }
            }
        }
    }
    base.to_string()
}

fn is_beta_or_proto(name: &str) -> bool {
    name.contains("(Beta")
        || name.contains("(Proto")
        || name.contains("(Sample")
        || name.contains("(Demo")
}

// =============================================================================
// Developer normalization
// =============================================================================

fn developer_override(raw: &str) -> Option<&'static str> {
    match raw {
        "Strata/Incredible Technologies" => Some("Incredible Technologies"),
        "Victor / Cave / Capcom" => Some("Cave"),
        "Capcom / Cave / Victor Interactive Software" => Some("Cave"),
        "Sony/Capcom" => Some("Capcom"),
        "SNK Playmore" => Some("SNK"),
        "Sega Toys" => Some("Sega"),
        "Lucasfilm Games" => Some("LucasArts"),
        "Nintendo / Capcom" => Some("Capcom"),
        "Taito Corporation (licensed from Midway)" => Some("Midway"),
        "IGS / Cave (Tong Li Animation license)" => Some("Cave"),
        "IGS / Cave" => Some("Cave"),
        _ => None,
    }
}

const CORPORATE_SUFFIXES: &[&str] = &[
    " Computer Entertainment Osaka",
    " Computer Entertainment Kobe",
    " Computer Entertainment Tokyo",
    " Digital Entertainment",
    " Technical Institute",
    " Interactive Software",
    " Entertainment",
    " Enterprises",
    " Corporation",
    " Industry",
    " of America",
    " of Japan",
    " Co., Ltd.",
    " Co., Ltd",
    " Corp.",
    " Corp",
    " LTD.",
    " Ltd.",
    " Ltd",
    " Inc.",
    " Inc",
    " Co.",
    " Co",
    " USA",
];
const REGIONAL_QUALIFIERS: &[&str] = &[" America", " Japan", " Europe", " do Brasil"];
const DIVISION_SUFFIXES: &[&str] = &[
    " AM1", " AM2", " AM3", " AM4", " AM5", " CS1", " CS2", " CS3", " R&D 1", " R&D 2", " R&D 3",
    " R&D 4", " R&D1", " R&D2", " R&D3", " R&D4", " EAD", " SPD",
];

fn is_noise(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower == "bootleg"
        || lower == "<unknown>"
        || lower == "unknown"
        || lower.starts_with("bootleg ")
        || lower.starts_with("bootleg(")
        || lower.starts_with("hack ")
        || lower.starts_with("hack(")
        || lower == "hack"
}

fn strip_suffixes_ci(s: &mut String, suffixes: &[&str]) {
    loop {
        let before = s.len();
        for suffix in suffixes {
            let s_lower = s.to_ascii_lowercase();
            if s_lower.ends_with(&suffix.to_ascii_lowercase()) {
                let new_len = s.len() - suffix.len();
                s.truncate(new_len);
                *s = s.trim().to_string();
            }
        }
        if s.len() == before {
            break;
        }
    }
}

fn normalize_case(s: &str) -> String {
    if s.len() <= 1 {
        return s.to_string();
    }
    let alpha: Vec<char> = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if alpha.is_empty() {
        return s.to_string();
    }
    if alpha.iter().all(|c| c.is_ascii_uppercase()) && alpha.len() > 3 {
        let mut chars = s.chars();
        let first = chars.next().unwrap();
        let mut result = first.to_uppercase().to_string();
        for c in chars {
            result.extend(c.to_lowercase());
        }
        result
    } else {
        s.to_string()
    }
}

fn normalize_developer(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(canonical) = developer_override(trimmed) {
        return canonical.to_string();
    }
    if is_noise(trimmed) {
        return String::new();
    }
    let mut s = trimmed.to_string();
    if let Some(paren_idx) = s.find('(')
        && s[paren_idx..].to_ascii_lowercase().contains("license")
    {
        s = s[..paren_idx].trim().to_string();
    }
    if s.starts_with('[')
        && let Some(close) = s.find(']')
    {
        let name = s[1..close].trim().to_string();
        if !name.is_empty() {
            s = name;
        }
    }
    strip_suffixes_ci(&mut s, CORPORATE_SUFFIXES);
    strip_suffixes_ci(&mut s, REGIONAL_QUALIFIERS);
    if let Some(idx) = s.find(" / ") {
        s = s[..idx].trim().to_string();
    } else if let Some(idx) = s.find('/') {
        s = s[..idx].trim().to_string();
    } else if let Some(idx) = s.find(" + ") {
        s = s[..idx].trim().to_string();
    }
    strip_suffixes_ci(&mut s, CORPORATE_SUFFIXES);
    strip_suffixes_ci(&mut s, REGIONAL_QUALIFIERS);
    strip_suffixes_ci(&mut s, DIVISION_SUFFIXES);
    let s = s.trim_end_matches(|c: char| c == '/' || c == '?' || c.is_whitespace());
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let result = normalize_case(s);
    if is_noise(&result) {
        String::new()
    } else {
        result
    }
}

fn tgdb_genre_name(id: u32) -> &'static str {
    match id {
        1 => "Action",
        2 => "Adventure",
        3 => "Board",
        4 => "Card",
        5 => "Casino",
        6 => "Comedy",
        7 => "Compilation",
        8 => "Shooter",
        9 => "Educational",
        10 => "Family",
        11 => "Fighting",
        12 => "Horror",
        13 => "MMO",
        14 => "Music",
        15 => "Other",
        16 => "Pinball",
        17 => "Platform",
        18 => "Puzzle",
        19 => "Racing",
        20 => "Role-Playing",
        21 => "Sandbox",
        22 => "Simulation",
        23 => "Sports",
        24 => "Stealth",
        25 => "Strategy",
        26 => "Trivia",
        27 => "Virtual Life",
        28 => "Flight Simulator",
        29 => "Fitness",
        30 => "Party",
        _ => "",
    }
}

// =============================================================================
// TGDB parsing
// =============================================================================

fn parse_tgdb_json(path: &Path) -> (HashMap<(String, u32), TgdbEntry>, TgdbRegionalDatesMap) {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Warning: could not open TheGamesDB JSON at {}: {}",
                path.display(),
                e
            );
            return (HashMap::new(), HashMap::new());
        }
    };
    let json: serde_json::Value = match serde_json::from_reader(BufReader::new(file)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Warning: failed to parse TheGamesDB JSON: {}", e);
            return (HashMap::new(), HashMap::new());
        }
    };

    let mut result: HashMap<(String, u32), TgdbEntry> = HashMap::new();
    let mut regional_dates: TgdbRegionalDatesMap = HashMap::new();

    let games = match json["data"]["games"].as_array() {
        Some(arr) => arr,
        None => return (result, regional_dates),
    };

    for game in games {
        let title = match game["game_title"].as_str() {
            Some(t) => t.to_string(),
            None => continue,
        };
        let platform = match game["platform"].as_u64() {
            Some(p) => p as u32,
            None => continue,
        };
        let release_date_raw = game["release_date"].as_str().unwrap_or("").to_string();
        let year: u16 = release_date_raw
            .get(..4)
            .and_then(|y| y.parse().ok())
            .unwrap_or(0);
        let players: u8 = game["players"]
            .as_u64()
            .map(|p| p.min(255) as u8)
            .unwrap_or(0);
        let genre_ids: Vec<u32> = game["genres"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();
        let developer_ids: Vec<u32> = game["developers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();
        let publisher_ids: Vec<u32> = game["publishers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();
        let coop: Option<bool> = game["coop"].as_str().map(|s| s.eq_ignore_ascii_case("yes"));
        let rating: String = game["rating"].as_str().unwrap_or("").to_string();
        let alternates: Vec<String> = game["alternates"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let normalized = normalize_title_for_tgdb(&title);
        let key = (normalized.clone(), platform);

        if year > 0 && !release_date_raw.is_empty() {
            let region_id = game["region_id"].as_u64().unwrap_or(0) as u32;
            regional_dates
                .entry(key.clone())
                .or_default()
                .push(TgdbRegionalDate {
                    region_id,
                    release_date: release_date_raw,
                });
        }

        result.entry(key).or_insert(TgdbEntry {
            title,
            year,
            players,
            genre_ids,
            developer_ids,
            publisher_ids,
            coop,
            rating,
            alternates,
        });
    }

    (result, regional_dates)
}

fn tgdb_region_id_to_str(region_id: u32) -> &'static str {
    match region_id {
        1..=3 => "usa",
        4 => "japan",
        5 => "korea",
        6..=8 => "europe",
        9 => "world",
        _ => "unknown",
    }
}

fn classify_tgdb_date(raw: &str) -> Option<(String, &'static str)> {
    let year_str = raw.get(..4)?;
    let year: u16 = year_str.parse().ok()?;
    if year == 0 {
        return None;
    }
    if raw.len() >= 10 {
        let rest = &raw[4..10];
        if rest == "-01-01" {
            Some((year_str.to_string(), "year"))
        } else if rest.starts_with('-') && rest.chars().filter(|c| c.is_ascii_digit()).count() == 4
        {
            Some((raw[..10].to_string(), "day"))
        } else {
            Some((year_str.to_string(), "year"))
        }
    } else {
        Some((year_str.to_string(), "year"))
    }
}

fn normalize_title_for_tgdb(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

// =============================================================================
// Console DB insertion
// =============================================================================

fn insert_console_games(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<()> {
    let nointro_dir = sources_dir.join("no-intro");
    let maxusers_dir = sources_dir.join("libretro-meta").join("maxusers");
    let genre_dir = sources_dir.join("libretro-meta").join("genre");
    let tgdb_path = sources_dir.join("thegamesdb-latest.json");

    let (tgdb, tgdb_regional_dates) = if tgdb_path.exists() {
        eprintln!("Game DB: Loading TheGamesDB JSON dump...");
        let parsed = parse_tgdb_json(&tgdb_path);
        eprintln!("Game DB: TheGamesDB loaded {} entries", parsed.0.len());
        parsed
    } else {
        eprintln!("Game DB: TheGamesDB JSON not found, skipping metadata enrichment");
        (HashMap::new(), HashMap::new())
    };

    let tgdb_developers = load_tgdb_name_map(&sources_dir.join("tgdb-developers.json"));
    let tgdb_publishers = load_tgdb_name_map(&sources_dir.join("tgdb-publishers.json"));
    let tgdb_genres = load_tgdb_name_map(&sources_dir.join("tgdb-genres.json"));
    eprintln!(
        "Game DB: TGDB lookups: {} devs, {} pubs, {} genres",
        tgdb_developers.len(),
        tgdb_publishers.len(),
        tgdb_genres.len()
    );

    let mut total_roms = 0usize;
    let mut total_games = 0usize;
    let mut total_tgdb_matches = 0usize;
    let mut console_release_dates: Vec<(String, String, &'static str, String, &'static str)> =
        Vec::new();

    let mut stmt_cg = conn.prepare(
        "INSERT INTO canonical_games (system, display_name, year, genre, developer, publisher, players, coop, rating, normalized_genre) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
    )?;
    let mut stmt_re = conn.prepare(
        "INSERT OR IGNORE INTO rom_entries (system, filename_stem, region, crc32, canonical_game_id, normalized_title) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    )?;
    let mut stmt_alt = conn.prepare(
        "INSERT INTO rom_alternates (canonical_game_id, system, alternate_name) VALUES (?1, ?2, ?3)"
    )?;

    for sys in GAME_DB_SYSTEMS {
        if sys.nointro_dat.is_empty() {
            // No No-Intro DAT for this system — insert TGDB-only canonical games
            // so that supported_systems() returns them and release dates work.
            // We only do this if we have TGDB data for the platform.
            let mut has_tgdb = false;
            for &platform_id in sys.tgdb_platform_ids {
                if tgdb.keys().any(|(_, p)| *p == platform_id) {
                    has_tgdb = true;
                    break;
                }
            }
            if has_tgdb {
                // Insert a sentinel canonical game so the system appears in supported_systems().
                // The app's scan logic will still work — it just won't have No-Intro ROM entries.
            }
            // Still harvest TGDB release dates for the system even without No-Intro DAT.
            // Insert a placeholder game_id = 0 won't work — skip release dates for now.
            // NOTE: Systems without No-Intro DAT have no ROM entries but may still appear
            // via the system folder scanner. We skip them here.
            continue;
        }

        let dat_path = nointro_dir.join(sys.nointro_dat);
        if !dat_path.exists() {
            eprintln!(
                "Game DB: No-Intro DAT not found for {}, skipping",
                sys.folder_name
            );
            continue;
        }

        let nointro_entries = parse_nointro_dat(&dat_path);
        eprintln!(
            "Game DB: {} - parsed {} ROM entries",
            sys.folder_name,
            nointro_entries.len()
        );

        let maxusers_path = maxusers_dir.join(sys.nointro_dat);
        let maxusers: HashMap<u32, String> = if maxusers_path.exists() {
            parse_libretro_meta_dat(&maxusers_path, "users ")
        } else {
            HashMap::new()
        };

        let genre_path = genre_dir.join(sys.nointro_dat);
        let genres: HashMap<u32, String> = if genre_path.exists() {
            parse_libretro_meta_dat(&genre_path, "genre ")
        } else {
            HashMap::new()
        };

        // Group ROM entries into canonical games by normalized title
        let mut game_groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, entry) in nointro_entries.iter().enumerate() {
            let key = normalize_title(&entry.name);
            game_groups.entry(key).or_default().push(idx);
        }

        let mut group_keys: Vec<String> = game_groups.keys().cloned().collect();
        group_keys.sort();

        let mut canonical_games: Vec<CanonicalGameBuild> = Vec::new();
        let mut rom_entries: Vec<RomEntryBuild> = Vec::new();
        let mut tgdb_match_count = 0usize;

        for group_key in &group_keys {
            let indices = &game_groups[group_key];
            let game_id = canonical_games.len();

            let best_idx = indices
                .iter()
                .copied()
                .find(|&i| matches!(nointro_entries[i].region.as_str(), "USA" | "World"))
                .unwrap_or(indices[0]);

            let display_name = clean_display_name(&nointro_entries[best_idx].name);

            let mut year: u16 = 0;
            let mut tgdb_players: u8 = 0;
            let mut tgdb_genre = String::new();
            let mut tgdb_alternates: Vec<String> = Vec::new();
            let mut tgdb_developer = String::new();
            let mut tgdb_publisher = String::new();
            let mut tgdb_coop: Option<bool> = None;
            let mut tgdb_rating = String::new();

            let tgdb_normalized = normalize_title_for_tgdb(&display_name);
            let base_title_lc = title_utils::base_title(&display_name);
            let mut region_dates_seen: HashSet<(&'static str, String)> = HashSet::new();

            for &platform_id in sys.tgdb_platform_ids {
                if let Some(region_rows) =
                    tgdb_regional_dates.get(&(tgdb_normalized.clone(), platform_id))
                {
                    for row in region_rows {
                        if let Some((date_str, precision)) = classify_tgdb_date(&row.release_date) {
                            let region = tgdb_region_id_to_str(row.region_id);
                            if region_dates_seen.insert((region, date_str.clone())) {
                                console_release_dates.push((
                                    sys.folder_name.to_string(),
                                    base_title_lc.clone(),
                                    region,
                                    date_str,
                                    precision,
                                ));
                            }
                        }
                    }
                }
            }

            for &platform_id in sys.tgdb_platform_ids {
                if let Some(tgdb_entry) = tgdb.get(&(tgdb_normalized.clone(), platform_id)) {
                    year = tgdb_entry.year;
                    tgdb_players = tgdb_entry.players;
                    if !tgdb_entry.genre_ids.is_empty() {
                        tgdb_genre = tgdb_genres
                            .get(&tgdb_entry.genre_ids[0])
                            .cloned()
                            .unwrap_or_else(|| {
                                tgdb_genre_name(tgdb_entry.genre_ids[0]).to_string()
                            });
                    }
                    tgdb_alternates = tgdb_entry.alternates.clone();
                    if let Some(&dev_id) = tgdb_entry.developer_ids.first()
                        && let Some(name) = tgdb_developers.get(&dev_id)
                    {
                        tgdb_developer = normalize_developer(name);
                    }
                    if let Some(&pub_id) = tgdb_entry.publisher_ids.first()
                        && let Some(name) = tgdb_publishers.get(&pub_id)
                    {
                        tgdb_publisher = name.clone();
                    }
                    tgdb_coop = tgdb_entry.coop;
                    tgdb_rating = tgdb_entry.rating.clone();
                    tgdb_match_count += 1;
                    break;
                }
            }

            // Players and genre: prefer libretro (CRC-based), fall back to TGDB
            let mut final_players: u8 = 0;
            let mut final_genre = String::new();

            // Pass 1: non-beta/proto ROMs
            for &idx in indices {
                if is_beta_or_proto(&nointro_entries[idx].name) {
                    continue;
                }
                let crc = nointro_entries[idx].crc32;
                if final_players == 0
                    && let Some(s) = maxusers.get(&crc)
                {
                    final_players = s.parse().unwrap_or(0);
                }
                if final_genre.is_empty()
                    && let Some(g) = genres.get(&crc)
                {
                    final_genre = g.clone();
                }
                if final_players > 0 && !final_genre.is_empty() {
                    break;
                }
            }
            // Pass 2: beta/proto ROMs
            if final_players == 0 || final_genre.is_empty() {
                for &idx in indices {
                    if !is_beta_or_proto(&nointro_entries[idx].name) {
                        continue;
                    }
                    let crc = nointro_entries[idx].crc32;
                    if final_players == 0
                        && let Some(s) = maxusers.get(&crc)
                    {
                        final_players = s.parse().unwrap_or(0);
                    }
                    if final_genre.is_empty()
                        && let Some(g) = genres.get(&crc)
                    {
                        final_genre = g.clone();
                    }
                    if final_players > 0 && !final_genre.is_empty() {
                        break;
                    }
                }
            }
            // Pass 3: TGDB fallback
            if final_players == 0 {
                final_players = tgdb_players;
            }
            if final_genre.is_empty() {
                final_genre = tgdb_genre;
            }

            canonical_games.push(CanonicalGameBuild {
                display_name,
                year,
                genre: final_genre,
                developer: tgdb_developer,
                publisher: tgdb_publisher,
                players: final_players,
                coop: tgdb_coop,
                rating: tgdb_rating,
                alternates: tgdb_alternates,
            });

            for &idx in indices {
                let entry = &nointro_entries[idx];
                let stem = entry
                    .rom_filename
                    .rfind('.')
                    .map(|i| &entry.rom_filename[..i])
                    .unwrap_or(&entry.rom_filename);
                rom_entries.push(RomEntryBuild {
                    filename_stem: stem.to_string(),
                    region: entry.region.clone(),
                    crc32: entry.crc32,
                    game_id,
                });
            }
        }

        eprintln!(
            "Game DB: {} - {} canonical games, {} ROM entries, {} TGDB matches",
            sys.folder_name,
            canonical_games.len(),
            rom_entries.len(),
            tgdb_match_count
        );

        // Insert canonical games and collect their actual SQLite rowids
        let mut canonical_game_ids: Vec<i64> = Vec::with_capacity(canonical_games.len());
        for game in canonical_games.iter() {
            let norm_genre = normalize_console_genre(&game.genre);
            let coop_val: Option<i64> = game.coop.map(|b| b as i64);
            stmt_cg.execute(params![
                sys.folder_name,
                game.display_name,
                game.year as i64,
                game.genre,
                game.developer,
                game.publisher,
                game.players as i64,
                coop_val,
                game.rating,
                norm_genre,
            ])?;
            let canonical_game_id = conn.last_insert_rowid();
            canonical_game_ids.push(canonical_game_id);

            // Insert alternates
            for alt in &game.alternates {
                stmt_alt.execute(params![canonical_game_id, sys.folder_name, alt])?;
            }
        }

        // Insert ROM entries (deduplicated by stem)
        let mut seen_stems: HashSet<&str> = HashSet::new();

        for entry in &rom_entries {
            let stem = entry.filename_stem.as_str();
            if !seen_stems.insert(stem) {
                continue;
            }
            let canonical_game_id = canonical_game_ids[entry.game_id];
            let norm_title = normalize_title(&entry.filename_stem);
            stmt_re.execute(params![
                sys.folder_name,
                stem,
                entry.region,
                entry.crc32 as i64,
                canonical_game_id,
                norm_title,
            ])?;
        }

        total_roms += rom_entries.len();
        total_games += canonical_games.len();
        total_tgdb_matches += tgdb_match_count;
    }

    eprintln!(
        "Game DB: Total {} ROM entries, {} canonical games, {} TGDB matches",
        total_roms, total_games, total_tgdb_matches
    );

    // Insert console release dates
    console_release_dates.sort();
    console_release_dates.dedup();
    let mut stmt_crd = conn.prepare(
        "INSERT OR REPLACE INTO console_release_dates (system, base_title, region, release_date, precision, source) VALUES (?1, ?2, ?3, ?4, ?5, 'tgdb')"
    )?;
    for (system, base_title, region, release_date, precision) in &console_release_dates {
        stmt_crd.execute(params![system, base_title, region, release_date, precision])?;
    }
    eprintln!(
        "Game DB: Inserted {} console release date rows",
        console_release_dates.len()
    );

    Ok(())
}

// =============================================================================
// Series DB insertion
// =============================================================================

#[derive(serde::Deserialize)]
struct WikidataSeriesEntryRaw {
    game_title: String,
    #[serde(default)]
    series_name: Option<String>,
    system: String,
    #[serde(default)]
    series_order: Option<i32>,
    #[serde(default)]
    follows: Option<String>,
    #[serde(default)]
    followed_by: Option<String>,
}

fn normalize_title_for_wikidata(title: &str) -> String {
    let trimmed = title.trim();
    let mut result = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn insert_series(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<()> {
    let series_path = sources_dir.join("wikidata").join("series.json");
    if !series_path.exists() {
        eprintln!("Series DB: series.json not found, skipping");
        return Ok(());
    }

    let file = match File::open(&series_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: could not open series.json: {}", e);
            return Ok(());
        }
    };

    let mut entries: Vec<WikidataSeriesEntryRaw> =
        match serde_json::from_reader(BufReader::new(file)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Warning: failed to parse series.json: {}", e);
                return Ok(());
            }
        };

    if entries.is_empty() {
        return Ok(());
    }

    // Reverse-link pass
    {
        let norm_to_idx: HashMap<String, Vec<usize>> = {
            let mut map: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, entry) in entries.iter().enumerate() {
                let norm = normalize_title_for_wikidata(&entry.game_title);
                if !norm.is_empty() {
                    map.entry(norm).or_default().push(i);
                }
            }
            map
        };

        type Fix = (usize, Option<String>, Option<String>, Option<String>);
        let mut fixes: Vec<Fix> = Vec::new();

        for i in 0..entries.len() {
            let entry = &entries[i];
            if let Some(ref followed_by) = entry.followed_by {
                let target_norm = normalize_title_for_wikidata(followed_by);
                if let Some(indices) = norm_to_idx.get(&target_norm) {
                    for &j in indices {
                        if entries[j].follows.as_ref().is_none_or(|s| s.is_empty()) {
                            fixes.push((j, Some(entry.game_title.clone()), None, None));
                        }
                    }
                }
            }
            if let Some(ref follows) = entry.follows {
                let target_norm = normalize_title_for_wikidata(follows);
                if let Some(indices) = norm_to_idx.get(&target_norm) {
                    for &j in indices {
                        if entries[j].followed_by.as_ref().is_none_or(|s| s.is_empty()) {
                            fixes.push((j, None, Some(entry.game_title.clone()), None));
                        }
                    }
                }
            }
            if let Some(ref series) = entry.series_name
                && !series.is_empty()
            {
                for target in [&entry.follows, &entry.followed_by].into_iter().flatten() {
                    let target_norm = normalize_title_for_wikidata(target);
                    if let Some(indices) = norm_to_idx.get(&target_norm) {
                        for &j in indices {
                            if entries[j].series_name.as_ref().is_none_or(|s| s.is_empty()) {
                                fixes.push((j, None, None, Some(series.clone())));
                            }
                        }
                    }
                }
            }
        }

        for (idx, follows_fix, followed_by_fix, series_fix) in fixes {
            if let Some(f) = follows_fix {
                entries[idx].follows = Some(f);
            }
            if let Some(f) = followed_by_fix {
                entries[idx].followed_by = Some(f);
            }
            if let Some(s) = series_fix {
                entries[idx].series_name = Some(s);
            }
        }
    }

    let mut stmt = conn.prepare(
        "INSERT INTO series_entries (game_title, series_name, system, series_order, follows, followed_by, normalized_title) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
    )?;
    let mut count = 0usize;

    for entry in &entries {
        let normalized = normalize_title_for_wikidata(&entry.game_title);
        if normalized.is_empty() {
            continue;
        }
        let series_name = entry.series_name.as_deref().unwrap_or("");
        let follows = entry.follows.as_deref().unwrap_or("");
        let followed_by = entry.followed_by.as_deref().unwrap_or("");
        if series_name.is_empty() && follows.is_empty() && followed_by.is_empty() {
            continue;
        }

        stmt.execute(params![
            entry.game_title,
            series_name,
            entry.system,
            entry.series_order,
            follows,
            followed_by,
            normalized,
        ])?;
        count += 1;
    }

    eprintln!("Series DB: Inserted {} entries", count);
    Ok(())
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let args = Args::parse();

    let sources_dir: PathBuf = if args.stub {
        // Find the workspace root relative to the current binary's location
        // or use a well-known relative path.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        // Go up from tools/build-catalog to workspace root, then into fixtures
        manifest_dir
            .parent() // tools/
            .and_then(|p| p.parent()) // workspace root
            .map(|p| p.join("replay-control-core").join("fixtures"))
            .unwrap_or_else(|| PathBuf::from("replay-control-core/fixtures"))
    } else {
        args.data_dir.clone()
    };

    eprintln!("build-catalog: sources_dir = {}", sources_dir.display());
    eprintln!("build-catalog: output      = {}", args.output.display());

    // Remove existing output file if it exists (fresh build)
    if args.output.exists() {
        std::fs::remove_file(&args.output).expect("Failed to remove existing catalog.sqlite");
    }

    let conn = Connection::open(&args.output).expect("Failed to open SQLite database");
    create_schema(&conn).expect("Failed to create schema");

    insert_arcade_games(&conn, &sources_dir).expect("Failed to insert arcade games");
    insert_console_games(&conn, &sources_dir).expect("Failed to insert console games");
    insert_series(&conn, &sources_dir).expect("Failed to insert series");

    // Metadata
    let is_stub = if args.stub { "1" } else { "0" };
    let generated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('mame_version', ?1)",
        params!["0.285"],
    )
    .expect("Failed to insert mame_version");
    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('generated_at', ?1)",
        params![generated_at],
    )
    .expect("Failed to insert generated_at");
    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('is_stub', ?1)",
        params![is_stub],
    )
    .expect("Failed to insert is_stub");

    // Final stats
    let arcade_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM arcade_games", [], |r| r.get(0))
        .unwrap_or(0);
    let canonical_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM canonical_games", [], |r| r.get(0))
        .unwrap_or(0);
    let rom_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rom_entries", [], |r| r.get(0))
        .unwrap_or(0);
    let series_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM series_entries", [], |r| r.get(0))
        .unwrap_or(0);

    println!("catalog.sqlite written to {}", args.output.display());
    println!("  arcade_games:    {}", arcade_count);
    println!("  canonical_games: {}", canonical_count);
    println!("  rom_entries:     {}", rom_count);
    println!("  series_entries:  {}", series_count);
    println!("  is_stub:         {}", is_stub);
}
