use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("arcade_db.rs");
    let mut out = BufWriter::new(File::create(&dest_path).unwrap());

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir);
    let arcade_dir = manifest_path.join("data").join("arcade");
    // Source data files live in data/ at the project root (one level up from replay-core/)
    let sources_dir = manifest_path.join("..").join("data");

    // Rerun if any data file changes
    println!(
        "cargo::rerun-if-changed={}",
        arcade_dir.join("flycast_games.csv").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        sources_dir.join("fbneo-arcade.dat").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        sources_dir.join("mame2003plus.xml").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        sources_dir.join("catver.ini").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        sources_dir.join("mame0285-arcade.xml").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        sources_dir.join("catver-mame-current.ini").display()
    );

    // Collect all game entries keyed by rom_name.
    // We use a HashMap for deduplication — later sources with richer metadata win.
    let mut entries_map: HashMap<String, GameEntry> = HashMap::new();

    // Track Flycast ROM names so we can protect them from being overridden.
    // Flycast entries are hand-curated and should always be preserved.
    let mut flycast_rom_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // 1. Parse Flycast (Naomi/Atomiswave) games — baseline data
    let flycast_path = arcade_dir.join("flycast_games.csv");
    if flycast_path.exists() {
        let flycast_entries = parse_csv(&flycast_path);
        println!(
            "cargo:warning=Arcade DB: Flycast CSV loaded {} entries",
            flycast_entries.len()
        );
        for entry in flycast_entries {
            flycast_rom_names.insert(entry.rom_name.clone());
            entries_map.insert(entry.rom_name.clone(), entry);
        }
    }

    // 2. Parse FBNeo arcade-only DAT (ClrMame Pro XML)
    //    Has: name, description, year, manufacturer, cloneof
    //    Missing: players, rotation, driver status
    let fbneo_path = sources_dir.join("fbneo-arcade.dat");
    if fbneo_path.exists() {
        let fbneo_entries = parse_fbneo_dat(&fbneo_path);
        println!(
            "cargo:warning=Arcade DB: FBNeo DAT loaded {} entries",
            fbneo_entries.len()
        );
        for entry in fbneo_entries {
            // FBNeo has no players/rotation/status, so it's not "richer" than
            // existing Flycast entries. Only insert if rom_name is new.
            entries_map
                .entry(entry.rom_name.clone())
                .or_insert(entry);
        }
    }

    // 3. Parse MAME 2003+ XML — richest metadata (has orientation, players, driver status)
    let mame_path = sources_dir.join("mame2003plus.xml");
    if mame_path.exists() {
        let mame_entries = parse_mame2003plus_xml(&mame_path);
        println!(
            "cargo:warning=Arcade DB: MAME 2003+ loaded {} entries",
            mame_entries.len()
        );
        for entry in mame_entries {
            let rom_name = entry.rom_name.clone();
            match entries_map.entry(rom_name) {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    // MAME 2003+ has richer metadata than FBNeo.
                    // Overwrite entries that lack players/rotation/status data
                    // (i.e., FBNeo-sourced entries), but preserve Flycast hand-curated
                    // entries that already have real metadata.
                    let existing = occupied.get();
                    if existing.players == 0
                        && existing.rotation == "unknown"
                        && existing.status == "unknown"
                    {
                        occupied.insert(entry);
                    }
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(entry);
                }
            }
        }
    }

    // 4. Parse MAME current (0.285) compact XML — richest and most up-to-date metadata
    //    This is a preprocessed extract of the full MAME listxml, containing only arcade
    //    entries with name, description, year, manufacturer, cloneof, rotation, players,
    //    and driver status. Overrides MAME 2003+ for games that exist in both (MAME current
    //    has more accurate/updated metadata). Adds many games not in MAME 2003+ or FBNeo.
    let mame_current_path = sources_dir.join("mame0285-arcade.xml");
    if mame_current_path.exists() {
        let mame_current_entries = parse_mame_current_xml(&mame_current_path);
        println!(
            "cargo:warning=Arcade DB: MAME current (0.285) loaded {} entries",
            mame_current_entries.len()
        );
        let mut new_count = 0u32;
        let mut override_count = 0u32;
        for entry in mame_current_entries {
            let rom_name = entry.rom_name.clone();
            match entries_map.entry(rom_name.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    // MAME current has the richest and most up-to-date metadata.
                    // Override entries from FBNeo and MAME 2003+, but preserve
                    // Flycast hand-curated entries (Naomi/Atomiswave).
                    if !flycast_rom_names.contains(&rom_name) {
                        occupied.insert(entry);
                        override_count += 1;
                    }
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(entry);
                    new_count += 1;
                }
            }
        }
        println!(
            "cargo:warning=Arcade DB: MAME current added {} new, overrode {} existing entries",
            new_count, override_count
        );
    }

    // 5. Parse catver.ini — overlay category data on all entries
    let catver_path = sources_dir.join("catver.ini");
    if catver_path.exists() {
        let categories = parse_catver_ini(&catver_path);
        println!(
            "cargo:warning=Arcade DB: catver.ini loaded {} category mappings",
            categories.len()
        );
        let mut applied = 0u32;
        for (rom_name, category) in &categories {
            if let Some(entry) = entries_map.get_mut(rom_name) {
                if entry.category.is_empty() {
                    entry.category = category.clone();
                    applied += 1;
                }
            }
        }
        println!(
            "cargo:warning=Arcade DB: Applied {} category overlays",
            applied
        );
    }

    // 6. Parse catver.ini for current MAME — supplements MAME 2003+ catver with
    //    categories for newer games. Only applies to entries that still lack a category.
    let catver_current_path = sources_dir.join("catver-mame-current.ini");
    if catver_current_path.exists() {
        let categories = parse_catver_ini(&catver_current_path);
        println!(
            "cargo:warning=Arcade DB: catver-mame-current.ini loaded {} category mappings",
            categories.len()
        );
        let mut applied = 0u32;
        for (rom_name, category) in &categories {
            if let Some(entry) = entries_map.get_mut(rom_name) {
                if entry.category.is_empty() {
                    entry.category = category.clone();
                    applied += 1;
                }
            }
        }
        println!(
            "cargo:warning=Arcade DB: Applied {} additional category overlays from current MAME catver",
            applied
        );
    }

    // Convert to sorted vec for deterministic output
    let mut entries: Vec<GameEntry> = entries_map.into_values().collect();
    entries.sort_by(|a, b| a.rom_name.cmp(&b.rom_name));

    println!(
        "cargo:warning=Arcade DB: Total unique entries: {}",
        entries.len()
    );

    // Generate the PHF map
    generate_phf_map(&mut out, &entries);
}

struct GameEntry {
    rom_name: String,
    display_name: String,
    year: String,
    manufacturer: String,
    players: u8,
    rotation: String,
    status: String,
    is_clone: bool,
    parent: String,
    category: String,
}

fn parse_csv(path: &Path) -> Vec<GameEntry> {
    let mut entries = Vec::new();
    let mut rdr = csv::Reader::from_path(path).unwrap_or_else(|e| {
        panic!("Failed to open CSV at {}: {}", path.display(), e);
    });

    for result in rdr.records() {
        let record = result.unwrap_or_else(|e| {
            panic!("Failed to parse CSV record in {}: {}", path.display(), e);
        });

        let rom_name = record.get(0).unwrap_or("").to_string();
        if rom_name.is_empty() {
            continue;
        }

        let players_str = record.get(4).unwrap_or("0");
        let players: u8 = players_str.parse().unwrap_or(0);

        let is_clone_str = record.get(7).unwrap_or("false");
        let is_clone = is_clone_str == "true";

        entries.push(GameEntry {
            rom_name,
            display_name: record.get(1).unwrap_or("").to_string(),
            year: record.get(2).unwrap_or("").to_string(),
            manufacturer: record.get(3).unwrap_or("").to_string(),
            players,
            rotation: record.get(5).unwrap_or("0").to_string(),
            status: record.get(6).unwrap_or("unknown").to_string(),
            is_clone,
            parent: record.get(8).unwrap_or("").to_string(),
            category: record.get(9).unwrap_or("").to_string(),
        });
    }
    entries
}

/// Parse FBNeo ClrMame Pro XML DAT file using streaming SAX parser.
///
/// Format:
/// ```xml
/// <datafile>
///   <game name="sf2" cloneof="" romof="" sourcefile="...">
///     <description>Street Fighter II...</description>
///     <year>1991</year>
///     <manufacturer>Capcom</manufacturer>
///     <rom .../>
///   </game>
/// </datafile>
/// ```
fn parse_fbneo_dat(path: &Path) -> Vec<GameEntry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_file(path).unwrap_or_else(|e| {
        panic!("Failed to open FBNeo DAT at {}: {}", path.display(), e);
    });
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    // State for current game being parsed
    let mut in_game = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_element = String::new(); // tracks which child element we're inside

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
                    b"game" => {
                        in_game = true;
                        current_name.clear();
                        current_cloneof.clear();
                        current_description.clear();
                        current_year.clear();
                        current_manufacturer.clear();

                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            match attr.key.local_name().as_ref() {
                                b"name" => {
                                    current_name =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                b"cloneof" => {
                                    current_cloneof =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"description" | b"year" | b"manufacturer" if in_game => {
                        current_element =
                            String::from_utf8_lossy(local_name.as_ref()).into_owned();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_game => {
                let text = e.unescape().unwrap_or_default();
                match current_element.as_str() {
                    "description" => current_description.push_str(&text),
                    "year" => current_year.push_str(&text),
                    "manufacturer" => current_manufacturer.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
                    b"game" if in_game => {
                        if !current_name.is_empty() {
                            let is_clone = !current_cloneof.is_empty();
                            entries.push(GameEntry {
                                rom_name: current_name.clone(),
                                display_name: current_description.clone(),
                                year: current_year.clone(),
                                manufacturer: current_manufacturer.clone(),
                                players: 0,
                                rotation: "unknown".to_string(),
                                status: "unknown".to_string(),
                                is_clone,
                                parent: current_cloneof.clone(),
                                category: String::new(),
                            });
                        }
                        in_game = false;
                    }
                    b"description" | b"year" | b"manufacturer" => {
                        current_element.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!(
                "Error parsing FBNeo DAT at position {}: {:?}",
                reader.error_position(),
                e
            ),
            _ => {}
        }
        buf.clear();
    }

    entries
}

/// Parse MAME 2003+ XML file using streaming SAX parser.
///
/// This format has richer metadata than FBNeo, including:
/// - `<video orientation="horizontal|vertical" />`
/// - `<input players="2" />`
/// - `<driver status="good|imperfect|preliminary" />`
fn parse_mame2003plus_xml(path: &Path) -> Vec<GameEntry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_file(path).unwrap_or_else(|e| {
        panic!("Failed to open MAME 2003+ XML at {}: {}", path.display(), e);
    });
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    // State for current game
    let mut in_game = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_orientation = String::new();
    let mut current_players: u8 = 0;
    let mut current_status = String::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
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

                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            match attr.key.local_name().as_ref() {
                                b"name" => {
                                    current_name =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                b"cloneof" => {
                                    current_cloneof =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"description" | b"year" | b"manufacturer" if in_game => {
                        current_element =
                            String::from_utf8_lossy(local_name.as_ref()).into_owned();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) if in_game => {
                let local_name = e.local_name();
                match local_name.as_ref() {
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
                                let val = String::from_utf8_lossy(&attr.value);
                                current_players = val.parse().unwrap_or(0);
                            }
                        }
                    }
                    b"driver" => {
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            if attr.key.local_name().as_ref() == b"status" {
                                current_status =
                                    String::from_utf8_lossy(&attr.value).into_owned();
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_game => {
                let text = e.unescape().unwrap_or_default();
                match current_element.as_str() {
                    "description" => current_description.push_str(&text),
                    "year" => current_year.push_str(&text),
                    "manufacturer" => current_manufacturer.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
                    b"game" if in_game => {
                        if !current_name.is_empty() {
                            let is_clone = !current_cloneof.is_empty();
                            entries.push(GameEntry {
                                rom_name: current_name.clone(),
                                display_name: current_description.clone(),
                                year: current_year.clone(),
                                manufacturer: current_manufacturer.clone(),
                                players: current_players,
                                rotation: current_orientation.clone(),
                                status: current_status.clone(),
                                is_clone,
                                parent: current_cloneof.clone(),
                                category: String::new(),
                            });
                        }
                        in_game = false;
                    }
                    b"description" | b"year" | b"manufacturer" => {
                        current_element.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!(
                "Error parsing MAME 2003+ XML at position {}: {:?}",
                reader.error_position(),
                e
            ),
            _ => {}
        }
        buf.clear();
    }

    entries
}

/// Parse MAME current (0.285) compact arcade XML.
///
/// This is a preprocessed extract of the full MAME listxml, generated by
/// `scripts/extract-mame-arcade.py`. Non-arcade entries (BIOS, devices,
/// mechanical, non-runnable) have already been filtered out.
///
/// Format:
/// ```xml
/// <mame version="0.285">
/// <m name="sf2" cloneof="sf2j" rotate="0" players="2" status="good">
///   <d>Street Fighter II...</d><y>1991</y><f>Capcom</f>
/// </m>
/// </mame>
/// ```
///
/// Attributes on `<m>`: name (required), cloneof, rotate (0/90/180/270),
/// players, status (good/imperfect/preliminary).
/// Child elements: `<d>` description, `<y>` year, `<f>` manufacturer.
fn parse_mame_current_xml(path: &Path) -> Vec<GameEntry> {
    let mut entries = Vec::new();
    let mut reader = Reader::from_file(path).unwrap_or_else(|e| {
        panic!(
            "Failed to open MAME current XML at {}: {}",
            path.display(),
            e
        );
    });
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    // State for current machine
    let mut in_machine = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_rotate = String::new();
    let mut current_players: u8 = 0;
    let mut current_status = String::new();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
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
                                    current_name =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                b"cloneof" => {
                                    current_cloneof =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                b"rotate" => {
                                    let val =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                    current_rotate = val;
                                }
                                b"players" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    current_players = val.parse().unwrap_or(0);
                                }
                                b"status" => {
                                    current_status =
                                        String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                _ => {}
                            }
                        }
                    }
                    b"d" | b"y" | b"f" if in_machine => {
                        current_element =
                            String::from_utf8_lossy(local_name.as_ref()).into_owned();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_machine => {
                let text = e.unescape().unwrap_or_default();
                match current_element.as_str() {
                    "d" => current_description.push_str(&text),
                    "y" => current_year.push_str(&text),
                    "f" => current_manufacturer.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                match local_name.as_ref() {
                    b"m" if in_machine => {
                        if !current_name.is_empty() {
                            let is_clone = !current_cloneof.is_empty();
                            entries.push(GameEntry {
                                rom_name: current_name.clone(),
                                display_name: current_description.clone(),
                                year: current_year.clone(),
                                manufacturer: current_manufacturer.clone(),
                                players: current_players,
                                rotation: current_rotate.clone(),
                                status: current_status.clone(),
                                is_clone,
                                parent: current_cloneof.clone(),
                                category: String::new(),
                            });
                        }
                        in_machine = false;
                    }
                    b"d" | b"y" | b"f" => {
                        current_element.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!(
                "Error parsing MAME current XML at position {}: {:?}",
                reader.error_position(),
                e
            ),
            _ => {}
        }
        buf.clear();
    }

    entries
}

/// Parse catver.ini to extract rom_name -> category mappings.
///
/// Format:
/// ```ini
/// ;comment lines
/// [Category]
/// pacman=Maze / Collect
/// sf2=Fighter / 2D
/// ```
///
/// We only care about the `[Category]` section.
fn parse_catver_ini(path: &Path) -> HashMap<String, String> {
    let mut categories = HashMap::new();
    let file = File::open(path).unwrap_or_else(|e| {
        panic!("Failed to open catver.ini at {}: {}", path.display(), e);
    });
    let reader = BufReader::new(file);

    let mut in_category_section = false;

    for line in reader.lines() {
        let line = line.unwrap_or_default();
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }

        // Check for section headers
        if trimmed.starts_with('[') {
            in_category_section = trimmed == "[Category]";
            continue;
        }

        if in_category_section {
            if let Some((rom_name, category)) = trimmed.split_once('=') {
                let rom_name = rom_name.trim();
                let category = category.trim();
                if !rom_name.is_empty() && !category.is_empty() {
                    categories.insert(rom_name.to_string(), category.to_string());
                }
            }
        }
    }

    categories
}

fn rotation_variant(rot: &str) -> &'static str {
    match rot {
        "0" => "Rotation::Horizontal",
        "90" => "Rotation::Vertical",
        "180" => "Rotation::Horizontal",
        "270" => "Rotation::Vertical",
        _ => "Rotation::Unknown",
    }
}

fn status_variant(status: &str) -> &'static str {
    match status {
        "good" | "working" => "DriverStatus::Working",
        "imperfect" => "DriverStatus::Imperfect",
        "preliminary" | "protection" => "DriverStatus::Preliminary",
        _ => "DriverStatus::Unknown",
    }
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn generate_phf_map(out: &mut impl Write, entries: &[GameEntry]) {
    let mut map = phf_codegen::Map::new();

    for entry in entries {
        let value = format!(
            "ArcadeGameInfo {{ \
                rom_name: \"{}\", \
                display_name: \"{}\", \
                year: \"{}\", \
                manufacturer: \"{}\", \
                players: {}, \
                rotation: {}, \
                status: {}, \
                is_clone: {}, \
                parent: \"{}\", \
                category: \"{}\" \
            }}",
            escape_str(&entry.rom_name),
            escape_str(&entry.display_name),
            escape_str(&entry.year),
            escape_str(&entry.manufacturer),
            entry.players,
            rotation_variant(&entry.rotation),
            status_variant(&entry.status),
            entry.is_clone,
            escape_str(&entry.parent),
            escape_str(&entry.category),
        );
        map.entry(&entry.rom_name, &value);
    }

    writeln!(
        out,
        "static ARCADE_DB: phf::Map<&'static str, ArcadeGameInfo> = {};",
        map.build()
    )
    .unwrap();
}
