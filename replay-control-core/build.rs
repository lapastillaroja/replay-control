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

    // --- Game DB generation (non-arcade systems) ---
    generate_game_db(&out_dir, &sources_dir);

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
    println!(
        "cargo::rerun-if-changed={}",
        sources_dir.join("nplayers.ini").display()
    );

    // Collect all game entries keyed by rom_name.
    // We use a HashMap for deduplication — later sources with richer metadata win.
    let mut entries_map: HashMap<String, GameEntry> = HashMap::new();

    // Track Flycast ROM names so we can protect them from being overridden.
    // Flycast entries are hand-curated and should always be preserved.
    let mut flycast_rom_names: std::collections::HashSet<String> = std::collections::HashSet::new();

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
            entries_map.entry(entry.rom_name.clone()).or_insert(entry);
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
            if let Some(entry) = entries_map.get_mut(rom_name)
                && entry.category.is_empty()
            {
                entry.category = category.clone();
                applied += 1;
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
            if let Some(entry) = entries_map.get_mut(rom_name)
                && entry.category.is_empty()
            {
                entry.category = category.clone();
                applied += 1;
            }
        }
        println!(
            "cargo:warning=Arcade DB: Applied {} additional category overlays from current MAME catver",
            applied
        );
    }

    // 7. Parse nplayers.ini — overlay player counts for entries that still have players == 0.
    //    This is a fallback source: it only fills in player data where none exists from
    //    MAME XML or Flycast sources. Format: "romname=2P sim" or "romname=4P alt / 2P sim"
    let nplayers_path = sources_dir.join("nplayers.ini");
    if nplayers_path.exists() {
        let nplayers = parse_nplayers_ini(&nplayers_path);
        println!(
            "cargo:warning=Arcade DB: nplayers.ini loaded {} player count mappings",
            nplayers.len()
        );
        let mut applied = 0u32;
        for (rom_name, players) in &nplayers {
            if let Some(entry) = entries_map.get_mut(rom_name)
                && entry.players == 0
            {
                entry.players = *players;
                applied += 1;
            }
        }
        println!(
            "cargo:warning=Arcade DB: Applied {} player count overlays from nplayers.ini",
            applied
        );
    }

    // 8. Mark entries with "System / BIOS" category as BIOS, even if not flagged by the parser.
    //    This catches entries that weren't detected via isbios/runnable attributes but are
    //    categorized as BIOS in catver.ini.
    for entry in entries_map.values_mut() {
        if entry.category.starts_with("System / BIOS") {
            entry.is_bios = true;
        }
    }

    // 9. Filter out non-game machines by category.
    //    These are completely removed from the DB — no value to users on a retro gaming device.
    //    Must happen AFTER catver overlays so categories are available for filtering.
    let total_before_filter = entries_map.len();
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
    entries_map.retain(|_, entry| {
        // Keep BIOS entries — they're flagged and filtered at the app layer
        if entry.is_bios {
            return true;
        }
        if entry.category.is_empty() {
            return true; // Keep entries without a category (can't determine if non-game)
        }
        !non_game_prefixes
            .iter()
            .any(|prefix| entry.category.starts_with(prefix))
    });
    let non_game_filtered = total_before_filter - entries_map.len();
    println!(
        "cargo:warning=Arcade DB: Filtered {} non-game machines by category",
        non_game_filtered
    );

    // Convert to sorted vec for deterministic output
    let mut entries: Vec<GameEntry> = entries_map.into_values().collect();
    entries.sort_by(|a, b| a.rom_name.cmp(&b.rom_name));

    // Report build stats
    let bios_count = entries.iter().filter(|e| e.is_bios).count();
    let playable_count = entries.iter().filter(|e| !e.is_bios).count();
    println!(
        "cargo:warning=Arcade DB: Total entries: {} (playable: {}, BIOS: {}, non-game filtered: {})",
        entries.len(),
        playable_count,
        bios_count,
        non_game_filtered
    );

    // Report player count coverage
    let with_players = entries.iter().filter(|e| e.players > 0).count();
    let without_players = entries.len() - with_players;
    println!(
        "cargo:warning=Arcade DB: Player coverage: {}/{} ({:.1}%), missing: {}",
        with_players,
        entries.len(),
        with_players as f64 / entries.len() as f64 * 100.0,
        without_players
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
    is_bios: bool,
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
            is_bios: false,
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
    let mut current_is_bios = false;

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
                        current_is_bios = false;

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
                                b"isbios" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    current_is_bios = val == "yes";
                                }
                                _ => {}
                            }
                        }
                    }
                    b"description" | b"year" | b"manufacturer" if in_game => {
                        current_element = String::from_utf8_lossy(local_name.as_ref()).into_owned();
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
                                is_bios: current_is_bios,
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
    let mut current_is_bios = false;

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
                        current_is_bios = false;

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
                                b"runnable" => {
                                    let val = String::from_utf8_lossy(&attr.value);
                                    if val == "no" {
                                        current_is_bios = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    b"description" | b"year" | b"manufacturer" if in_game => {
                        current_element = String::from_utf8_lossy(local_name.as_ref()).into_owned();
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
                                current_status = String::from_utf8_lossy(&attr.value).into_owned();
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
                                is_bios: current_is_bios,
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
                                    let val = String::from_utf8_lossy(&attr.value).into_owned();
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
                        current_element = String::from_utf8_lossy(local_name.as_ref()).into_owned();
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
                                is_bios: false, // MAME current XML is pre-filtered to exclude BIOS
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

/// Parse nplayers.ini to extract rom_name -> max player count mappings.
///
/// Format:
/// ```ini
/// ;; comment lines
/// [NPlayers]
/// pacman=2P alt
/// sf2=2P sim
/// gauntlet=4P sim
/// ```
///
/// Player count values: "1P", "2P alt", "2P sim", "4P alt / 2P sim", etc.
/// We extract the maximum player count from the first number found.
/// Entries with "???", "Device", "Non-arcade", "BIOS", or "Pinball" are skipped.
fn parse_nplayers_ini(path: &Path) -> HashMap<String, u8> {
    let mut players_map = HashMap::new();
    let file = File::open(path).unwrap_or_else(|e| {
        panic!("Failed to open nplayers.ini at {}: {}", path.display(), e);
    });
    let reader = BufReader::new(file);

    let mut in_nplayers_section = false;

    for line in reader.lines() {
        let line = line.unwrap_or_default();
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue;
        }

        // Check for section headers
        if trimmed.starts_with('[') {
            in_nplayers_section = trimmed == "[NPlayers]";
            continue;
        }

        if in_nplayers_section {
            if let Some((rom_name, value)) = trimmed.split_once('=') {
                let rom_name = rom_name.trim();
                let value = value.trim();

                // Skip non-game entries
                if value == "???"
                    || value == "Device"
                    || value == "Non-arcade"
                    || value == "BIOS"
                    || value == "Pinball"
                {
                    continue;
                }

                // Parse player count from values like "2P sim", "4P alt / 2P sim", "1P"
                // For compound entries like "4P alt / 2P sim", take the first (max) number
                if let Some(players) = parse_nplayers_value(value) {
                    players_map.insert(rom_name.to_string(), players);
                }
            }
        }
    }

    players_map
}

/// Parse an nplayers.ini value string to extract the max player count.
///
/// Examples:
///   "1P"           -> 1
///   "2P alt"       -> 2
///   "2P sim"       -> 2
///   "4P alt / 2P sim" -> 4
///   "8P sim"       -> 8
fn parse_nplayers_value(value: &str) -> Option<u8> {
    // Find the first occurrence of a digit followed by 'P'
    // For compound values like "4P alt / 2P sim", the first number is the max
    for part in value.split('/') {
        let part = part.trim();
        if let Some(p_pos) = part.find('P') {
            // Extract digits immediately before 'P'
            let prefix = &part[..p_pos];
            if let Ok(n) = prefix.trim().parse::<u8>() {
                if n > 0 {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Normalize an arcade catver.ini category to the shared genre taxonomy.
///
/// Extracts the primary category (before " / ") and maps it to a shared genre.
fn normalize_arcade_genre(category: &str) -> &'static str {
    // Extract primary category: "Fighter / Versus" -> "Fighter"
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
        // Non-game categories
        "System" | "BIOS" | "Utilities" | "Electromechanical" | "Device" | "Rewritable"
        | "Not Coverage" | "Mature" => "Other",
        _ if category.is_empty() => "",
        _ => "Other",
    }
}

/// Normalize a libretro/TGDB genre string to the shared genre taxonomy.
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

/// Check if a No-Intro ROM name indicates a beta, prototype, sample, or demo.
fn is_beta_or_proto(name: &str) -> bool {
    name.contains("(Beta")
        || name.contains("(Proto")
        || name.contains("(Sample")
        || name.contains("(Demo")
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn generate_phf_map(out: &mut impl Write, entries: &[GameEntry]) {
    let mut map = phf_codegen::Map::new();

    for entry in entries {
        let norm_genre = normalize_arcade_genre(&entry.category);
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
                is_bios: {}, \
                parent: \"{}\", \
                category: \"{}\", \
                normalized_genre: \"{}\" \
            }}",
            escape_str(&entry.rom_name),
            escape_str(&entry.display_name),
            escape_str(&entry.year),
            escape_str(&entry.manufacturer),
            entry.players,
            rotation_variant(&entry.rotation),
            status_variant(&entry.status),
            entry.is_clone,
            entry.is_bios,
            escape_str(&entry.parent),
            escape_str(&entry.category),
            escape_str(norm_genre),
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

// =============================================================================
// Game DB generation for non-arcade systems
// =============================================================================

/// System configuration: maps RePlayOS folder name to No-Intro DAT filename
/// and the identifier used for generated Rust code.
struct SystemConfig {
    folder_name: &'static str,
    nointro_dat: &'static str,
    rust_prefix: &'static str,
    tgdb_platform_ids: &'static [u32],
}

const GAME_DB_SYSTEMS: &[SystemConfig] = &[
    SystemConfig {
        folder_name: "nintendo_nes",
        nointro_dat: "Nintendo - Nintendo Entertainment System.dat",
        rust_prefix: "NES",
        tgdb_platform_ids: &[7],
    },
    SystemConfig {
        folder_name: "nintendo_snes",
        nointro_dat: "Nintendo - Super Nintendo Entertainment System.dat",
        rust_prefix: "SNES",
        tgdb_platform_ids: &[6],
    },
    SystemConfig {
        folder_name: "nintendo_gb",
        nointro_dat: "Nintendo - Game Boy.dat",
        rust_prefix: "GB",
        tgdb_platform_ids: &[4],
    },
    SystemConfig {
        folder_name: "nintendo_gbc",
        nointro_dat: "Nintendo - Game Boy Color.dat",
        rust_prefix: "GBC",
        tgdb_platform_ids: &[41],
    },
    SystemConfig {
        folder_name: "nintendo_gba",
        nointro_dat: "Nintendo - Game Boy Advance.dat",
        rust_prefix: "GBA",
        tgdb_platform_ids: &[5],
    },
    SystemConfig {
        folder_name: "nintendo_n64",
        nointro_dat: "Nintendo - Nintendo 64.dat",
        rust_prefix: "N64",
        tgdb_platform_ids: &[3],
    },
    SystemConfig {
        folder_name: "sega_sms",
        nointro_dat: "Sega - Master System - Mark III.dat",
        rust_prefix: "SMS",
        tgdb_platform_ids: &[35],
    },
    SystemConfig {
        folder_name: "sega_smd",
        nointro_dat: "Sega - Mega Drive - Genesis.dat",
        rust_prefix: "SMD",
        tgdb_platform_ids: &[18, 36],
    },
    SystemConfig {
        folder_name: "sega_gg",
        nointro_dat: "Sega - Game Gear.dat",
        rust_prefix: "GG",
        tgdb_platform_ids: &[20],
    },
];

/// A ROM entry parsed from a No-Intro DAT file.
struct NoIntroEntry {
    /// Game name from the DAT (e.g., "Super Mario World (USA)")
    name: String,
    /// ROM filename (e.g., "Super Mario World (USA).sfc")
    rom_filename: String,
    /// Region code (e.g., "USA", "Europe", "Japan")
    region: String,
    /// CRC32 hash
    crc32: u32,
}

/// Metadata from TheGamesDB, keyed by normalized title + platform.
struct TgdbEntry {
    #[allow(dead_code)]
    title: String,
    year: u16,
    players: u8,
    genre_ids: Vec<u32>,
    #[allow(dead_code)]
    developer_ids: Vec<u32>,
}

/// Canonical game after grouping ROM variants.
struct CanonicalGameBuild {
    display_name: String,
    year: u16,
    genre: String,
    developer: String,
    players: u8,
}

/// ROM entry with its canonical game assignment.
struct RomEntryBuild {
    /// Filename stem (No-Intro name without extension)
    filename_stem: String,
    region: String,
    crc32: u32,
    /// Index into the canonical games array for this system
    game_id: usize,
}

/// Parse a No-Intro ClrMamePro-format DAT file.
///
/// Format:
/// ```text
/// game (
///     name "Super Mario World (USA)"
///     region "USA"
///     rom ( name "Super Mario World (USA).sfc" size 524288 crc B19ED489 ... )
/// )
/// ```
fn parse_nointro_dat(path: &Path) -> Vec<NoIntroEntry> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Warning: could not open {}: {}", path.display(), e);
            return Vec::new();
        }
    };
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    let mut in_game = false;
    let mut in_rom = false;
    let mut current_name = String::new();
    let mut current_region = String::new();
    let mut current_rom_name = String::new();
    let mut current_crc: u32 = 0;

    for line in reader.lines() {
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
                // If region wasn't explicitly set, try to extract from name
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

        // Parse fields inside a game block
        if let Some(val) = extract_quoted_field(trimmed, "name ") {
            if in_rom || trimmed.starts_with("rom (") || trimmed.contains("rom ( name") {
                // This is a rom name field
            } else {
                current_name = val;
            }
        }

        if let Some(val) = extract_quoted_field(trimmed, "region ") {
            current_region = val;
        }

        // Parse rom line: rom ( name "file.ext" size 123 crc ABCD1234 ... )
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

/// Extract a quoted value after a field name, e.g., `name "value"` -> `value`
fn extract_quoted_field(line: &str, field: &str) -> Option<String> {
    let rest = line.strip_prefix(field)?;
    let rest = rest.trim().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Extract a quoted value after a keyword anywhere in the line.
fn extract_quoted_after(line: &str, keyword: &str) -> Option<String> {
    let idx = line.find(keyword)?;
    let rest = &line[idx + keyword.len()..];
    let rest = rest.trim().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Extract a word (non-whitespace token) after a keyword.
fn extract_word_after(line: &str, keyword: &str) -> Option<String> {
    let idx = line.find(keyword)?;
    let rest = &line[idx + keyword.len()..];
    let word: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ')')
        .collect();
    if word.is_empty() { None } else { Some(word) }
}

/// Extract region from a No-Intro game name by looking at parenthesized tags.
fn extract_region_from_name(name: &str) -> String {
    // Look for the first parenthesized group which typically contains the region
    if let Some(start) = name.find('(')
        && let Some(end) = name[start..].find(')')
    {
        let tag = &name[start + 1..start + end];
        // Check if it's a known region
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
        // Return the whole tag content as region
        return tag.to_string();
    }
    String::new()
}

/// Parse a libretro metadata DAT file (maxusers or genre format).
///
/// Format:
/// ```text
/// game (
///     comment "Game Name (Region)"
///     users 2          // for maxusers
///     genre "Action"   // for genre
///     rom ( crc ABCD1234 )
/// )
/// ```
///
/// Returns a map of CRC32 -> value (either player count or genre string).
fn parse_libretro_meta_dat(path: &Path, field_name: &str) -> HashMap<u32, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return HashMap::new(),
    };
    let reader = BufReader::new(file);
    let mut result = HashMap::new();

    let mut in_game = false;
    let mut current_value = String::new();
    let mut current_crc: u32 = 0;

    for line in reader.lines() {
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

        // Parse the value field (users or genre)
        if let Some(rest) = trimmed.strip_prefix(field_name) {
            let rest = rest.trim();
            // Handle both quoted and unquoted values
            if let Some(quoted) = rest.strip_prefix('"') {
                if let Some(end) = quoted.find('"') {
                    current_value = quoted[..end].to_string();
                }
            } else {
                current_value = rest.to_string();
            }
        }

        // Parse rom line for CRC
        if (trimmed.starts_with("rom (") || trimmed.starts_with("rom("))
            && let Some(crc_str) = extract_word_after(trimmed, "crc ")
        {
            current_crc = u32::from_str_radix(&crc_str, 16).unwrap_or(0);
        }
    }

    result
}

/// Normalize a No-Intro game name to a grouping key for canonical game deduplication.
///
/// "Super Mario World (USA)" -> "super mario world"
/// "Legend of Zelda, The (USA) (Rev A)" -> "legend of zelda the"
fn normalize_title(name: &str) -> String {
    // Strip everything from the first '(' onward (removes region/revision tags)
    let base = name.split('(').next().unwrap_or(name).trim();

    // Normalize "Name, The" -> "The Name" -> then lowercase strips articles anyway
    // For grouping purposes, we just lowercase and remove punctuation
    let mut result = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }

    // Collapse whitespace
    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}

/// Derive a clean display name from a No-Intro game name.
///
/// "Super Mario World (USA)" -> "Super Mario World"
/// "Legend of Zelda, The (USA)" -> "The Legend of Zelda"
/// "Legend of Zelda, The - A Link to the Past (USA)" -> "The Legend of Zelda - A Link to the Past"
fn clean_display_name(name: &str) -> String {
    // Strip everything from the first '(' onward
    let base = name.split('(').next().unwrap_or(name).trim();

    // Handle "Name, The" or "Name, The - Subtitle" -> "The Name" or "The Name - Subtitle"
    // Look for ", The" followed by end of string, " - ", or " ~ "
    for article in &[", The", ", An", ", A"] {
        if let Some(idx) = base.find(article) {
            let after_article = &base[idx + article.len()..];
            // Only match if followed by nothing, " - ", or end of string
            if after_article.is_empty()
                || after_article.starts_with(" - ")
                || after_article.starts_with(" ~ ")
            {
                let prefix = &base[..idx];
                let art = &article[2..]; // Strip the leading ", "
                if after_article.is_empty() {
                    return format!("{art} {prefix}");
                } else {
                    return format!("{art} {prefix}{after_article}");
                }
            }
        }
    }

    base.to_string()
}

/// TheGamesDB genre ID to name mapping.
/// These are the standard TGDB genre IDs (1-30), which are stable.
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

/// Parse TheGamesDB JSON dump and build a lookup by normalized title + platform.
///
/// Returns: HashMap<(normalized_title, platform_id), TgdbEntry>
fn parse_tgdb_json(path: &Path) -> HashMap<(String, u32), TgdbEntry> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "Warning: could not open TheGamesDB JSON at {}: {}",
                path.display(),
                e
            );
            return HashMap::new();
        }
    };

    let reader = BufReader::new(file);
    let json: serde_json::Value = match serde_json::from_reader(reader) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Warning: failed to parse TheGamesDB JSON: {}", e);
            return HashMap::new();
        }
    };

    let mut result: HashMap<(String, u32), TgdbEntry> = HashMap::new();

    let games = match json["data"]["games"].as_array() {
        Some(arr) => arr,
        None => return result,
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

        // Extract year from release_date (format: "YYYY-MM-DD")
        let year: u16 = game["release_date"]
            .as_str()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse().ok())
            .unwrap_or(0);

        let players: u8 = game["players"]
            .as_u64()
            .map(|p| p.min(255) as u8)
            .unwrap_or(0);

        let genre_ids: Vec<u32> = game["genres"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();

        let developer_ids: Vec<u32> = game["developers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();

        let normalized = normalize_title_for_tgdb(&title);
        let key = (normalized, platform);

        // Only keep the first entry per title+platform (avoid overwriting)
        result.entry(key).or_insert(TgdbEntry {
            title,
            year,
            players,
            genre_ids,
            developer_ids,
        });
    }

    result
}

/// Normalize a title for matching against TheGamesDB.
/// Simpler normalization — lowercase, strip punctuation, collapse whitespace.
fn normalize_title_for_tgdb(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}

/// Generate the game_db.rs file with per-system PHF maps and canonical game tables.
fn generate_game_db(out_dir: &str, sources_dir: &Path) {
    let dest_path = Path::new(out_dir).join("game_db.rs");
    let mut out = BufWriter::new(File::create(&dest_path).unwrap());

    let nointro_dir = sources_dir.join("no-intro");
    let maxusers_dir = sources_dir.join("libretro-meta").join("maxusers");
    let genre_dir = sources_dir.join("libretro-meta").join("genre");
    let tgdb_path = sources_dir.join("thegamesdb-latest.json");

    // Register rerun-if-changed for all game DB data directories
    println!("cargo::rerun-if-changed={}", nointro_dir.display());
    println!("cargo::rerun-if-changed={}", maxusers_dir.display());
    println!("cargo::rerun-if-changed={}", genre_dir.display());
    println!("cargo::rerun-if-changed={}", tgdb_path.display());

    // Parse TheGamesDB JSON dump (shared across all systems)
    let tgdb = if tgdb_path.exists() {
        println!("cargo:warning=Game DB: Loading TheGamesDB JSON dump...");
        let tgdb = parse_tgdb_json(&tgdb_path);
        println!(
            "cargo:warning=Game DB: TheGamesDB loaded {} entries",
            tgdb.len()
        );
        tgdb
    } else {
        println!("cargo:warning=Game DB: TheGamesDB JSON not found, skipping metadata enrichment");
        HashMap::new()
    };

    // Track grand totals
    let mut total_roms = 0usize;
    let mut total_games = 0usize;
    let mut total_tgdb_matches = 0usize;
    let mut system_names: Vec<&str> = Vec::new();

    // Process each system
    for sys in GAME_DB_SYSTEMS {
        let dat_path = nointro_dir.join(sys.nointro_dat);
        if !dat_path.exists() {
            println!(
                "cargo:warning=Game DB: No-Intro DAT not found for {}, skipping",
                sys.folder_name
            );
            // Write empty statics for this system
            write_empty_system(&mut out, sys.rust_prefix);
            system_names.push(sys.folder_name);
            continue;
        }

        // 1. Parse No-Intro DAT
        let nointro_entries = parse_nointro_dat(&dat_path);
        println!(
            "cargo:warning=Game DB: {} - parsed {} ROM entries from No-Intro DAT",
            sys.folder_name,
            nointro_entries.len()
        );

        // 2. Parse libretro metadata (maxusers and genre by CRC)
        let maxusers_path = maxusers_dir.join(sys.nointro_dat);
        let maxusers: HashMap<u32, String> = if maxusers_path.exists() {
            parse_libretro_meta_dat(&maxusers_path, "users ")
        } else {
            HashMap::new()
        };
        println!(
            "cargo:warning=Game DB: {} - {} maxusers entries",
            sys.folder_name,
            maxusers.len()
        );

        let genre_path = genre_dir.join(sys.nointro_dat);
        let genres: HashMap<u32, String> = if genre_path.exists() {
            parse_libretro_meta_dat(&genre_path, "genre ")
        } else {
            HashMap::new()
        };
        println!(
            "cargo:warning=Game DB: {} - {} genre entries",
            sys.folder_name,
            genres.len()
        );

        // 3. Group ROM entries into canonical games by normalized title
        let mut game_groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, entry) in nointro_entries.iter().enumerate() {
            let key = normalize_title(&entry.name);
            game_groups.entry(key).or_default().push(idx);
        }

        // 4. Build canonical games and ROM entries
        // Sort group keys for deterministic output
        let mut group_keys: Vec<String> = game_groups.keys().cloned().collect();
        group_keys.sort();

        let mut canonical_games: Vec<CanonicalGameBuild> = Vec::new();
        let mut rom_entries: Vec<RomEntryBuild> = Vec::new();
        let mut tgdb_match_count = 0usize;

        for group_key in &group_keys {
            let indices = &game_groups[group_key];
            let game_id = canonical_games.len();

            // Pick the best representative entry for display name
            // Prefer USA/World region, then first entry
            let best_idx = indices
                .iter()
                .copied()
                .find(|&i| {
                    let r = nointro_entries[i].region.as_str();
                    r == "USA" || r == "World"
                })
                .unwrap_or(indices[0]);

            let display_name = clean_display_name(&nointro_entries[best_idx].name);

            // Try to get metadata from TheGamesDB
            let mut year: u16 = 0;
            let mut tgdb_players: u8 = 0;
            let mut tgdb_genre = String::new();

            // Try matching against each TGDB platform ID for this system
            let tgdb_normalized = normalize_title_for_tgdb(&display_name);
            for &platform_id in sys.tgdb_platform_ids {
                if let Some(tgdb_entry) = tgdb.get(&(tgdb_normalized.clone(), platform_id)) {
                    year = tgdb_entry.year;
                    tgdb_players = tgdb_entry.players;
                    if !tgdb_entry.genre_ids.is_empty() {
                        tgdb_genre = tgdb_genre_name(tgdb_entry.genre_ids[0]).to_string();
                    }
                    tgdb_match_count += 1;
                    break;
                }
            }

            // Get players and genre from libretro metadata (CRC-based, more reliable)
            // Use libretro data as primary, TGDB as fallback
            let mut final_players: u8 = 0;
            let mut final_genre = String::new();

            // Pass 1: Try primary (non-beta) ROMs first
            for &idx in indices {
                let name = &nointro_entries[idx].name;
                if is_beta_or_proto(name) {
                    continue;
                }
                let crc = nointro_entries[idx].crc32;
                if final_players == 0 {
                    if let Some(users_str) = maxusers.get(&crc) {
                        final_players = users_str.parse().unwrap_or(0);
                    }
                }
                if final_genre.is_empty() {
                    if let Some(genre_str) = genres.get(&crc) {
                        final_genre = genre_str.clone();
                    }
                }
                if final_players > 0 && !final_genre.is_empty() {
                    break;
                }
            }

            // Pass 2: Fall back to beta/proto ROMs if primary didn't match
            if final_players == 0 || final_genre.is_empty() {
                for &idx in indices {
                    let name = &nointro_entries[idx].name;
                    if !is_beta_or_proto(name) {
                        continue;
                    }
                    let crc = nointro_entries[idx].crc32;
                    if final_players == 0 {
                        if let Some(users_str) = maxusers.get(&crc) {
                            final_players = users_str.parse().unwrap_or(0);
                        }
                    }
                    if final_genre.is_empty() {
                        if let Some(genre_str) = genres.get(&crc) {
                            final_genre = genre_str.clone();
                        }
                    }
                    if final_players > 0 && !final_genre.is_empty() {
                        break;
                    }
                }
            }

            // Pass 3: Fall back to TGDB data if libretro didn't have it
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
                developer: String::new(), // Developer lookup not available in TGDB dump
                players: final_players,
            });

            // Create ROM entries for all variants
            for &idx in indices {
                let entry = &nointro_entries[idx];
                // Filename stem = ROM filename without extension
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

        println!(
            "cargo:warning=Game DB: {} - {} canonical games, {} ROM entries, {} TGDB matches",
            sys.folder_name,
            canonical_games.len(),
            rom_entries.len(),
            tgdb_match_count
        );

        let genre_coverage = rom_entries
            .iter()
            .filter(|r| !canonical_games[r.game_id].genre.is_empty())
            .count();
        let players_coverage = rom_entries
            .iter()
            .filter(|r| canonical_games[r.game_id].players > 0)
            .count();
        println!(
            "cargo:warning=Game DB: {} - genre coverage: {}/{} ({:.0}%), players coverage: {}/{} ({:.0}%)",
            sys.folder_name,
            genre_coverage,
            rom_entries.len(),
            if rom_entries.is_empty() {
                0.0
            } else {
                genre_coverage as f64 / rom_entries.len() as f64 * 100.0
            },
            players_coverage,
            rom_entries.len(),
            if rom_entries.is_empty() {
                0.0
            } else {
                players_coverage as f64 / rom_entries.len() as f64 * 100.0
            },
        );

        total_roms += rom_entries.len();
        total_games += canonical_games.len();
        total_tgdb_matches += tgdb_match_count;
        system_names.push(sys.folder_name);

        // 5. Generate Rust code for this system
        write_system_code(&mut out, sys.rust_prefix, &canonical_games, &rom_entries);
    }

    // Generate the dispatch functions
    write_dispatch_code(&mut out, &system_names);

    println!(
        "cargo:warning=Game DB: Total: {} ROM entries, {} canonical games, {} TGDB matches across {} systems",
        total_roms,
        total_games,
        total_tgdb_matches,
        system_names.len()
    );
}

/// Write empty statics for a system that has no data.
fn write_empty_system(out: &mut impl Write, prefix: &str) {
    writeln!(out, "static {prefix}_GAMES: &[CanonicalGame] = &[];").unwrap();
    writeln!(
        out,
        "static {prefix}_ROM_DB: phf::Map<&'static str, GameEntry> = phf::Map {{ key: 0, disps: &[], entries: &[] }};"
    )
    .unwrap();
    writeln!(
        out,
        "static {prefix}_CRC_INDEX: phf::Map<u32, &'static str> = phf::Map {{ key: 0, disps: &[], entries: &[] }};"
    )
    .unwrap();
    writeln!(
        out,
        "static {prefix}_NORM_INDEX: phf::Map<&'static str, u16> = phf::Map {{ key: 0, disps: &[], entries: &[] }};"
    )
    .unwrap();
    writeln!(out).unwrap();
}

/// Write the generated Rust code for a single system.
fn write_system_code(
    out: &mut impl Write,
    prefix: &str,
    games: &[CanonicalGameBuild],
    rom_entries: &[RomEntryBuild],
) {
    // 1. Canonical games array
    writeln!(out, "static {prefix}_GAMES: &[CanonicalGame] = &[").unwrap();
    for game in games {
        let norm_genre = normalize_console_genre(&game.genre);
        writeln!(
            out,
            "    CanonicalGame {{ display_name: \"{}\", year: {}, genre: \"{}\", developer: \"{}\", players: {}, normalized_genre: \"{}\" }},",
            escape_str(&game.display_name),
            game.year,
            escape_str(&game.genre),
            escape_str(&game.developer),
            game.players,
            escape_str(norm_genre),
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // 2. ROM DB (filename_stem -> GameEntry) via PHF
    //    Deduplicate filename stems — if multiple ROMs share the same filename
    //    stem (e.g., different dumps of the same ROM), keep only the first.
    if rom_entries.is_empty() {
        writeln!(
            out,
            "static {prefix}_ROM_DB: phf::Map<&'static str, GameEntry> = phf::Map {{ key: 0, disps: &[], entries: &[] }};"
        )
        .unwrap();
    } else {
        let mut map = phf_codegen::Map::new();
        let mut seen_stems: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for entry in rom_entries {
            if !seen_stems.insert(&entry.filename_stem) {
                continue; // Skip duplicate filename stems
            }
            let value = format!(
                "GameEntry {{ canonical_name: \"{}\", region: \"{}\", crc32: 0x{:08X}, game: &{prefix}_GAMES[{}] }}",
                escape_str(&entry.filename_stem),
                escape_str(&entry.region),
                entry.crc32,
                entry.game_id,
            );
            map.entry(&entry.filename_stem, &value);
        }
        writeln!(
            out,
            "static {prefix}_ROM_DB: phf::Map<&'static str, GameEntry> = {};",
            map.build()
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    // 3. CRC32 index (crc32 -> filename_stem) via PHF
    if rom_entries.is_empty() {
        writeln!(
            out,
            "static {prefix}_CRC_INDEX: phf::Map<u32, &'static str> = phf::Map {{ key: 0, disps: &[], entries: &[] }};"
        )
        .unwrap();
    } else {
        // Only include entries with non-zero CRC32, and deduplicate CRCs
        // (if multiple ROMs have the same CRC, only keep the first)
        let mut seen_crcs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut crc_entries: Vec<(u32, &str)> = Vec::new();
        for entry in rom_entries {
            if entry.crc32 != 0 && seen_crcs.insert(entry.crc32) {
                crc_entries.push((entry.crc32, &entry.filename_stem));
            }
        }

        let mut map = phf_codegen::Map::new();
        for (crc, stem) in &crc_entries {
            map.entry(*crc, &format!("\"{}\"", escape_str(stem)));
        }
        writeln!(
            out,
            "static {prefix}_CRC_INDEX: phf::Map<u32, &'static str> = {};",
            map.build()
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    // 4. Normalized title index (normalized_title -> game_id) via PHF
    //    Maps normalized game titles to canonical game indices for fuzzy matching.
    //    Multiple ROM entries may share the same normalized title (regional variants,
    //    translations, etc.) — we keep only the first canonical game per normalized title.
    if games.is_empty() {
        writeln!(
            out,
            "static {prefix}_NORM_INDEX: phf::Map<&'static str, u16> = phf::Map {{ key: 0, disps: &[], entries: &[] }};"
        )
        .unwrap();
    } else {
        // Build a map from normalized title -> game_id, deduplicating
        let mut norm_map: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (game_id, game) in games.iter().enumerate() {
            let normalized = normalize_title(&game.display_name);
            if !normalized.is_empty() {
                norm_map.entry(normalized).or_insert(game_id);
            }
        }

        let mut map = phf_codegen::Map::new();
        // Sort for deterministic output
        let mut norm_entries: Vec<(&str, usize)> =
            norm_map.iter().map(|(k, &v)| (k.as_str(), v)).collect();
        norm_entries.sort_by_key(|(k, _)| k.to_string());
        for (norm_title, game_id) in &norm_entries {
            map.entry(*norm_title, &format!("{}u16", game_id));
        }
        writeln!(
            out,
            "static {prefix}_NORM_INDEX: phf::Map<&'static str, u16> = {};",
            map.build()
        )
        .unwrap();

        println!(
            "cargo:warning=Game DB: {} - {} normalized title index entries",
            prefix,
            norm_entries.len()
        );
    }
    writeln!(out).unwrap();
}

/// Write the dispatch functions that route system folder names to per-system maps.
fn write_dispatch_code(out: &mut impl Write, system_names: &[&str]) {
    // System list constant
    writeln!(out, "static GAME_DB_SYSTEMS: &[&str] = &[").unwrap();
    for name in system_names {
        writeln!(out, "    \"{name}\",").unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // Dispatch function for ROM DB
    writeln!(
        out,
        "fn get_system_db(system: &str) -> Option<&'static phf::Map<&'static str, GameEntry>> {{"
    )
    .unwrap();
    writeln!(out, "    match system {{").unwrap();
    for sys in GAME_DB_SYSTEMS {
        writeln!(
            out,
            "        \"{}\" => Some(&{}_ROM_DB),",
            sys.folder_name, sys.rust_prefix
        )
        .unwrap();
    }
    writeln!(out, "        _ => None,").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Dispatch function for CRC index
    writeln!(
        out,
        "fn get_system_crc_index(system: &str) -> Option<&'static phf::Map<u32, &'static str>> {{"
    )
    .unwrap();
    writeln!(out, "    match system {{").unwrap();
    for sys in GAME_DB_SYSTEMS {
        writeln!(
            out,
            "        \"{}\" => Some(&{}_CRC_INDEX),",
            sys.folder_name, sys.rust_prefix
        )
        .unwrap();
    }
    writeln!(out, "        _ => None,").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Dispatch function for normalized title index
    writeln!(
        out,
        "fn get_system_norm_index(system: &str) -> Option<&'static phf::Map<&'static str, u16>> {{"
    )
    .unwrap();
    writeln!(out, "    match system {{").unwrap();
    for sys in GAME_DB_SYSTEMS {
        writeln!(
            out,
            "        \"{}\" => Some(&{}_NORM_INDEX),",
            sys.folder_name, sys.rust_prefix
        )
        .unwrap();
    }
    writeln!(out, "        _ => None,").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();

    writeln!(out).unwrap();

    // Dispatch function for canonical games array
    writeln!(
        out,
        "fn get_system_games(system: &str) -> Option<&'static [CanonicalGame]> {{"
    )
    .unwrap();
    writeln!(out, "    match system {{").unwrap();
    for sys in GAME_DB_SYSTEMS {
        writeln!(
            out,
            "        \"{}\" => Some({}_GAMES),",
            sys.folder_name, sys.rust_prefix
        )
        .unwrap();
    }
    writeln!(out, "        _ => None,").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}
