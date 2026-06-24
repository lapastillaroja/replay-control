// Port of replay-control-core/build.rs: parses game data files and writes
// catalog.sqlite instead of generating PHF Rust code.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use clap::Parser;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesRef, Event};
use quick_xml::reader::Reader;
use replay_control_core::arcade_board::ArcadeBoard;
use replay_control_core::library::resource_kind;
use replay_control_core::title_utils;
use rusqlite::{Connection, params};

mod community;

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

    /// Downgrade missing-source preflight errors to warnings and build a
    /// partial catalog anyway. For local/dev builds without every input (e.g.
    /// no TGDB API key) and keyless throwaway CI builds. Production/release
    /// builds must NOT pass this — a missing source is a shipping defect.
    #[arg(long)]
    allow_partial: bool,
}

// =============================================================================
// Schema
// =============================================================================

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        -- One row per (rom_name, source). Each upstream (FBNeo DAT, MAME 2003+,
        -- MAME current, Flycast CSV) writes its rows independently. The runtime
        -- looks up by rom_name (returns up to N rows) and merges fields by
        -- per-system priority, so a system can prefer its own upstream's
        -- curated name/manufacturer/etc. and fall back to others field-by-field.
        --
        -- The PK index `(rom_name, source)` covers `WHERE rom_name = ?` via
        -- leading-column prefix scan, so no separate index is needed.
        CREATE TABLE arcade_game (
            rom_name TEXT NOT NULL,
            source TEXT NOT NULL,
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
            normalized_genre TEXT NOT NULL DEFAULT '',
            -- ArcadeBoard::as_tag() resolved at insert time from the upstream's
            -- sourcefile; empty when the parser couldn't recognize it.
            board TEXT NOT NULL DEFAULT '',
            -- RetroAchievements game id + the RA hash that matched, resolved at
            -- insert time by md5(lowercase rom_name) against the RA Arcade hash
            -- set (rc_hash hashes the romset NAME). Empty when unmatched.
            ra_id TEXT NOT NULL DEFAULT '',
            ra_hash TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (rom_name, source)
        );

        -- Enables future board-keyed reverse lookups (filter dropdown
        -- population, all-games-on-this-board recommendations). Partial
        -- index keeps it small since most rows have an empty board tag.
        CREATE INDEX idx_ag_board ON arcade_game(board) WHERE board != '';

        CREATE TABLE canonical_game (
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
            normalized_genre TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX idx_cg_system ON canonical_game(system);

        CREATE TABLE rom_entry (
            id INTEGER PRIMARY KEY,
            system TEXT NOT NULL,
            filename_stem TEXT NOT NULL,
            region TEXT NOT NULL DEFAULT '',
            crc32 INTEGER NOT NULL DEFAULT 0,
            canonical_game_id INTEGER NOT NULL REFERENCES canonical_game(id),
            normalized_title TEXT NOT NULL DEFAULT '',
            -- No-Intro full-file MD5 (per dump). Used to join this dump to RA's
            -- ra_hash for whole-file cart systems (ra_hash == file md5).
            md5 TEXT NOT NULL DEFAULT '',
            -- RetroAchievements game id for THIS dump (hash-matched at build time
            -- for whole-file carts; empty otherwise). Per-dump → 100% precise;
            -- never title-derived. Header carts + discs match at scan time.
            ra_id TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX idx_re_stem ON rom_entry(system, filename_stem);
        CREATE INDEX idx_re_crc  ON rom_entry(system, crc32);
        CREATE INDEX idx_re_norm ON rom_entry(system, normalized_title);

        -- RetroAchievements hash → game id, per system, for RUNTIME matching
        -- (header carts NES/SNES/N64 and discs, whose RA hash must be computed
        -- from the actual ROM bytes at scan time). Carries every RA hash from the
        -- extract; whole-file carts also resolve via rom_entry.md5 at build time.
        CREATE TABLE ra_hash (
            system TEXT NOT NULL,
            hash TEXT NOT NULL,
            ra_id TEXT NOT NULL,
            PRIMARY KEY (system, hash)
        );
        -- Drives the canonical_game → rom_entry join in game_db::descriptions
        -- (and any future cg-side query that enumerates rom entries without a
        -- full scan).
        CREATE INDEX idx_re_cgid ON rom_entry(canonical_game_id);

        CREATE TABLE rom_alternate (
            canonical_game_id INTEGER NOT NULL,
            system TEXT NOT NULL,
            alternate_name TEXT NOT NULL
        );
        CREATE INDEX idx_ra_game ON rom_alternate(canonical_game_id, system);

        CREATE TABLE series_entry (
            id INTEGER PRIMARY KEY,
            game_title TEXT NOT NULL,
            series_name TEXT NOT NULL DEFAULT '',
            system TEXT NOT NULL,
            series_order INTEGER,
            follows TEXT NOT NULL DEFAULT '',
            followed_by TEXT NOT NULL DEFAULT '',
            normalized_title TEXT NOT NULL
        );
        CREATE INDEX idx_se_system ON series_entry(system, normalized_title);

        CREATE TABLE arcade_release_date (
            rom_name TEXT NOT NULL,
            year TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'mame'
        );

        CREATE TABLE console_release_date (
            system TEXT NOT NULL,
            base_title TEXT NOT NULL,
            region TEXT NOT NULL,
            release_date TEXT NOT NULL,
            precision TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'tgdb',
            PRIMARY KEY (system, base_title, region)
        );

        CREATE TABLE catalog_game_resource (
            system TEXT NOT NULL,
            normalized_title TEXT NOT NULL,
            resource_type TEXT NOT NULL,
            source TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            url TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            languages TEXT NOT NULL DEFAULT '',
            mime_type TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (system, normalized_title, resource_type, source, resource_id)
        );
        CREATE INDEX catalog_game_resource_idx_lookup
            ON catalog_game_resource(system, normalized_title, resource_type);

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
    /// `ArcadeBoard::as_tag()` (e.g. `"cps2"`) or empty if the parser couldn't
    /// recognize the upstream's driver sourcefile. The sourcefile itself never
    /// reaches the schema — it's resolved here and discarded.
    board: String,
}

/// `ArcadeBoard::as_tag()` from a parser-side raw sourcefile string, or empty.
/// Normalizes parser-shape quirks (FBNeo `d_` prefix), then defers to
/// `ArcadeBoard::from_sourcefile`, which owns every board↔sourcefile spelling
/// (MAME current, FBNeo, and MAME 2003+ legacy `.c`) in one table.
fn board_tag_from_sourcefile(raw: &str) -> String {
    ArcadeBoard::from_sourcefile(&normalize_sourcefile(raw))
        .map(|b| b.as_tag().to_string())
        .unwrap_or_default()
}

/// Strip the one parser-shape quirk `ArcadeBoard::from_sourcefile` doesn't
/// model: FBNeo emits `dir/d_board.cpp`, so drop the `d_` basename prefix.
/// MAME current (`dir/board.cpp`) and MAME 2003+ legacy (`board.c`) are matched
/// verbatim by `from_sourcefile` and pass through unchanged.
fn normalize_sourcefile(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Some((dir, basename)) = raw.rsplit_once('/')
        && let Some(rest) = basename.strip_prefix("d_")
    {
        return format!("{dir}/{rest}");
    }
    raw.to_string()
}

/// Derive an `ArcadeBoard` for a Flycast CSV entry, whose `display_name`
/// carries the board hint in a `GDS-` (Naomi 2) / `GDL-` (Atomiswave) prefix.
/// Default → Naomi 1.
fn flycast_board(display_name: &str) -> ArcadeBoard {
    if display_name.contains("GDS-") {
        ArcadeBoard::SegaNaomi2
    } else if display_name.contains("GDL-") {
        ArcadeBoard::SammyAtomiswave
    } else {
        ArcadeBoard::SegaNaomi
    }
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
        let display_name = record.get(1).unwrap_or("").to_string();
        let board = flycast_board(&display_name).as_tag().to_string();
        entries.push(ArcadeEntry {
            rom_name,
            display_name,
            year: record.get(2).unwrap_or("").to_string(),
            manufacturer: record.get(3).unwrap_or("").to_string(),
            players,
            rotation: record.get(5).unwrap_or("0").to_string(),
            status: record.get(6).unwrap_or("unknown").to_string(),
            is_clone,
            is_bios: false,
            parent: record.get(8).unwrap_or("").to_string(),
            category: record.get(9).unwrap_or("").to_string(),
            board,
        });
    }
    entries
}

/// Resolve a quick-xml general entity reference to its decoded text.
///
/// quick-xml 0.40 reports entity references inside character data (e.g. `&amp;`,
/// `&#x27;`) as standalone `Event::GeneralRef` events rather than folding them into
/// the surrounding `Event::Text`. A parser that only handles `Event::Text` drops the
/// entity entirely -- and with `trim_text` enabled also loses the spaces that
/// bordered it -- corrupting titles like "Dungeons & Dragons" into "DungeonsDragons".
/// `resolve_char_ref` handles the numeric forms; the five predefined named
/// entities are the only named refs these DATs use, so a static-string match
/// avoids an allocation on the common path.
fn resolve_general_ref(e: &BytesRef) -> String {
    // Numeric character references (&#NN; / &#xNN;).
    if let Ok(Some(ch)) = e.resolve_char_ref() {
        return ch.to_string();
    }
    match e.decode().as_deref() {
        Ok("amp") => "&",
        Ok("lt") => "<",
        Ok("gt") => ">",
        Ok("quot") => "\"",
        Ok("apos") => "'",
        _ => "",
    }
    .to_string()
}

/// Append a decoded text fragment to the matching `ArcadeEntry` field.
///
/// Accepts both the long element names used by the FBNeo / MAME 2003+ DATs
/// (`description` / `year` / `manufacturer`) and the short tags in MAME's
/// current XML (`d` / `y` / `f`), so all three parsers share one dispatch for
/// both `Event::Text` and `Event::GeneralRef`.
fn append_arcade_text(
    element: &str,
    text: &str,
    description: &mut String,
    year: &mut String,
    manufacturer: &mut String,
) {
    match element {
        "description" | "d" => description.push_str(text),
        "year" | "y" => year.push_str(text),
        "manufacturer" | "f" => manufacturer.push_str(text),
        _ => {}
    }
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
    // No `trim_text`: quick-xml splits character data at entity references, and
    // trimming each chunk would drop the spaces bordering an entity (see
    // `resolve_general_ref`). Whitespace is trimmed once per field on store.
    let mut buf = Vec::new();

    let mut in_game = false;
    let mut current_name = String::new();
    let mut current_cloneof = String::new();
    let mut current_description = String::new();
    let mut current_year = String::new();
    let mut current_manufacturer = String::new();
    let mut current_element = String::new();
    let mut current_is_bios = false;
    let mut current_sourcefile = String::new();

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
                    current_sourcefile.clear();
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
                            b"sourcefile" => {
                                current_sourcefile =
                                    String::from_utf8_lossy(&attr.value).into_owned()
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
                append_arcade_text(
                    &current_element,
                    &e.decode().unwrap_or_default(),
                    &mut current_description,
                    &mut current_year,
                    &mut current_manufacturer,
                );
            }
            Ok(Event::GeneralRef(ref e)) if in_game => {
                append_arcade_text(
                    &current_element,
                    &resolve_general_ref(e),
                    &mut current_description,
                    &mut current_year,
                    &mut current_manufacturer,
                );
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"game" if in_game => {
                    if !current_name.is_empty() {
                        entries.push(ArcadeEntry {
                            rom_name: current_name.clone(),
                            display_name: current_description.trim().to_string(),
                            year: current_year.trim().to_string(),
                            manufacturer: current_manufacturer.trim().to_string(),
                            players: 0,
                            rotation: "unknown".to_string(),
                            status: "unknown".to_string(),
                            is_clone: !current_cloneof.is_empty(),
                            is_bios: current_is_bios,
                            parent: current_cloneof.clone(),
                            category: String::new(),
                            board: board_tag_from_sourcefile(&current_sourcefile),
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
    // No `trim_text`: see `parse_fbneo_dat` / `resolve_general_ref`. Whitespace is
    // trimmed once per field on store.
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
    let mut current_sourcefile = String::new();

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
                    current_sourcefile.clear();
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
                            b"sourcefile" => {
                                current_sourcefile =
                                    String::from_utf8_lossy(&attr.value).into_owned()
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
                append_arcade_text(
                    &current_element,
                    &e.decode().unwrap_or_default(),
                    &mut current_description,
                    &mut current_year,
                    &mut current_manufacturer,
                );
            }
            Ok(Event::GeneralRef(ref e)) if in_game => {
                append_arcade_text(
                    &current_element,
                    &resolve_general_ref(e),
                    &mut current_description,
                    &mut current_year,
                    &mut current_manufacturer,
                );
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"game" if in_game => {
                    if !current_name.is_empty() {
                        entries.push(ArcadeEntry {
                            rom_name: current_name.clone(),
                            display_name: current_description.trim().to_string(),
                            year: current_year.trim().to_string(),
                            manufacturer: current_manufacturer.trim().to_string(),
                            players: current_players,
                            rotation: current_orientation.clone(),
                            status: current_status.clone(),
                            is_clone: !current_cloneof.is_empty(),
                            is_bios: current_is_bios,
                            parent: current_cloneof.clone(),
                            category: String::new(),
                            board: board_tag_from_sourcefile(&current_sourcefile),
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
    // No `trim_text`: see `parse_fbneo_dat` / `resolve_general_ref`. Whitespace is
    // trimmed once per field on store.
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
    let mut current_sourcefile = String::new();

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
                    current_sourcefile.clear();
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
                            b"sourcefile" => {
                                current_sourcefile =
                                    String::from_utf8_lossy(&attr.value).into_owned()
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
                append_arcade_text(
                    &current_element,
                    &e.decode().unwrap_or_default(),
                    &mut current_description,
                    &mut current_year,
                    &mut current_manufacturer,
                );
            }
            Ok(Event::GeneralRef(ref e)) if in_machine => {
                append_arcade_text(
                    &current_element,
                    &resolve_general_ref(e),
                    &mut current_description,
                    &mut current_year,
                    &mut current_manufacturer,
                );
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"m" if in_machine => {
                    if !current_name.is_empty() {
                        entries.push(ArcadeEntry {
                            rom_name: current_name.clone(),
                            display_name: current_description.trim().to_string(),
                            year: current_year.trim().to_string(),
                            manufacturer: current_manufacturer.trim().to_string(),
                            players: current_players,
                            rotation: current_rotate.clone(),
                            status: current_status.clone(),
                            is_clone: !current_cloneof.is_empty(),
                            is_bios: false,
                            parent: current_cloneof.clone(),
                            category: String::new(),
                            board: board_tag_from_sourcefile(&current_sourcefile),
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
        "Casino" | "Gambling" | "Slot Machine" => "Board & Card",
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

/// Subdirectory under the data dir holding downloaded / derived third-party
/// inputs (No-Intro, MAME / FBNeo DATs, catver, nplayers, TGDB, libretro /
/// MiSTer / retrokit). These are regenerable and gitignored, kept separate from
/// the committed curated data (`arcade/`, `community/`, `shmups-wiki/`,
/// `wikidata/`) so a build cache that restores this dir can never clobber a
/// source-of-truth file. Populated by the download scripts under `data/upstream`.
fn upstream(sources_dir: &Path) -> PathBuf {
    sources_dir.join("upstream")
}

fn insert_arcade_games(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<()> {
    let arcade_dir = sources_dir.join("arcade");

    // Per-source row buckets. Each loader fills its own bucket; rows are
    // written directly to arcade_game with the source tag — no merge.
    // Categorisation overlays (catver, nplayers) apply to the *runtime
    // merged* result, so they're applied across all buckets indiscriminately
    // (the merge picks per-system priority so a category from one bucket
    // can fall back to another).
    let mut buckets: Vec<(&'static str, Vec<ArcadeEntry>)> = Vec::new();

    let flycast_path = arcade_dir.join("flycast_games.csv");
    if flycast_path.exists() {
        let entries = parse_csv(&flycast_path);
        eprintln!("Arcade DB: Flycast CSV loaded {} entries", entries.len());
        buckets.push(("naomi", entries));
    }

    let fbneo_path = upstream(sources_dir).join("fbneo-arcade.dat");
    if fbneo_path.exists() {
        let entries = parse_fbneo_dat(&fbneo_path);
        eprintln!("Arcade DB: FBNeo DAT loaded {} entries", entries.len());
        buckets.push(("fbneo", entries));
    }

    let mame_2k3p_path = upstream(sources_dir).join("mame2003plus.xml");
    if mame_2k3p_path.exists() {
        let entries = parse_mame2003plus_xml(&mame_2k3p_path);
        eprintln!("Arcade DB: MAME 2003+ loaded {} entries", entries.len());
        buckets.push(("mame_2k3p", entries));
    }

    let mame_current_path = upstream(sources_dir).join("mame0285-arcade.xml");
    if mame_current_path.exists() {
        let entries = parse_mame_current_xml(&mame_current_path);
        eprintln!("Arcade DB: MAME current loaded {} entries", entries.len());
        buckets.push(("mame", entries));
    }

    // Apply category overlays to every bucket — catver.ini lacks source-
    // attribution, so a category found in either ini fills any row whose
    // category is empty. Same for nplayers.
    let catver: HashMap<String, String> = {
        let mut m: HashMap<String, String> = HashMap::new();
        for ini in ["catver.ini", "catver-mame-current.ini"] {
            let path = upstream(sources_dir).join(ini);
            if path.exists() {
                m.extend(parse_catver_ini(&path));
            }
        }
        m
    };
    let nplayers: HashMap<String, u8> = {
        let path = upstream(sources_dir).join("nplayers.ini");
        if path.exists() {
            parse_nplayers_ini(&path)
        } else {
            HashMap::new()
        }
    };

    let mut total_inserted = 0u32;
    let mut total_bios = 0u32;
    let mut total_ra = 0u32;

    // RA "Arcade" hash → ra_id. Matched per row by md5(lowercase rom_name).
    let arcade_ra = load_arcade_ra_hash_map(sources_dir);

    let mut stmt = conn.prepare(
        "INSERT INTO arcade_game \
         (rom_name, source, display_name, year, manufacturer, players, rotation, status, \
          is_clone, is_bios, parent, category, normalized_genre, board, ra_id, ra_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?;
    let mut stmt_rd = conn
        .prepare("INSERT INTO arcade_release_date (rom_name, year, source) VALUES (?1, ?2, ?3)")?;
    let mut rd_count = 0u32;

    for (source, mut entries) in buckets {
        // Apply overlays + BIOS marking per row.
        for entry in &mut entries {
            if entry.category.is_empty()
                && let Some(c) = catver.get(&entry.rom_name)
            {
                entry.category = c.clone();
            }
            if entry.players == 0
                && let Some(p) = nplayers.get(&entry.rom_name)
            {
                entry.players = *p;
            }
            if entry.category.starts_with("System / BIOS") {
                entry.is_bios = true;
            }
        }
        entries.sort_by(|a, b| a.rom_name.cmp(&b.rom_name));

        for entry in &entries {
            if entry.is_bios {
                total_bios += 1;
            }
            let norm_genre = normalize_arcade_genre(&entry.category);
            // RA Arcade match: rc_hash for Arcade is md5(lowercase romset_name).
            let ra_hash = md5_hex(&entry.rom_name.to_lowercase());
            let (ra_id, ra_hash) = match arcade_ra.get(&ra_hash) {
                Some(id) => (id.as_str(), ra_hash.as_str()),
                None => ("", ""),
            };
            if !ra_id.is_empty() {
                total_ra += 1;
            }
            stmt.execute(params![
                entry.rom_name,
                source,
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
                entry.board,
                ra_id,
                ra_hash,
            ])?;
            total_inserted += 1;

            if !entry.is_bios {
                let y = entry.year.trim();
                if y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) {
                    stmt_rd.execute(params![entry.rom_name, y, source])?;
                    rd_count += 1;
                }
            }
        }
    }

    eprintln!(
        "Arcade DB: Inserted {} rows ({} BIOS, {} RetroAchievements matches)",
        total_inserted, total_bios, total_ra
    );
    eprintln!("Arcade DB: Inserted {} release date rows", rd_count);

    Ok(())
}

// =============================================================================
// Console data structures
// =============================================================================

struct SystemConfig {
    folder_name: &'static str,
    nointro_dats: &'static [&'static str],
    tgdb_platform_ids: &'static [u32],
}

const GAME_DB_SYSTEMS: &[SystemConfig] = &[
    // Nintendo cartridge/handheld
    SystemConfig {
        folder_name: "nintendo_nes",
        nointro_dats: &["Nintendo - Nintendo Entertainment System.dat"],
        tgdb_platform_ids: &[7],
    },
    SystemConfig {
        folder_name: "nintendo_snes",
        nointro_dats: &["Nintendo - Super Nintendo Entertainment System.dat"],
        tgdb_platform_ids: &[6],
    },
    SystemConfig {
        folder_name: "nintendo_gb",
        nointro_dats: &["Nintendo - Game Boy.dat"],
        tgdb_platform_ids: &[4],
    },
    SystemConfig {
        folder_name: "nintendo_gbc",
        nointro_dats: &["Nintendo - Game Boy Color.dat"],
        tgdb_platform_ids: &[41],
    },
    SystemConfig {
        folder_name: "nintendo_gba",
        nointro_dats: &["Nintendo - Game Boy Advance.dat"],
        tgdb_platform_ids: &[5],
    },
    SystemConfig {
        folder_name: "nintendo_n64",
        nointro_dats: &["Nintendo - Nintendo 64.dat"],
        tgdb_platform_ids: &[3],
    },
    // Sega cartridge/handheld
    SystemConfig {
        folder_name: "sega_sms",
        nointro_dats: &["Sega - Master System - Mark III.dat"],
        tgdb_platform_ids: &[35],
    },
    SystemConfig {
        folder_name: "sega_smd",
        nointro_dats: &["Sega - Mega Drive - Genesis.dat"],
        tgdb_platform_ids: &[18, 36],
    },
    SystemConfig {
        folder_name: "sega_gg",
        nointro_dats: &["Sega - Game Gear.dat"],
        tgdb_platform_ids: &[20],
    },
    SystemConfig {
        folder_name: "sega_sg",
        nointro_dats: &["Sega - SG-1000.dat"],
        tgdb_platform_ids: &[4949],
    },
    SystemConfig {
        folder_name: "sega_32x",
        nointro_dats: &["Sega - 32X.dat"],
        tgdb_platform_ids: &[33],
    },
    // Atari
    SystemConfig {
        folder_name: "atari_2600",
        nointro_dats: &[],
        tgdb_platform_ids: &[22],
    },
    SystemConfig {
        folder_name: "atari_5200",
        nointro_dats: &[],
        tgdb_platform_ids: &[26],
    },
    SystemConfig {
        folder_name: "atari_7800",
        nointro_dats: &[],
        tgdb_platform_ids: &[27],
    },
    SystemConfig {
        folder_name: "atari_jaguar",
        nointro_dats: &[],
        tgdb_platform_ids: &[28],
    },
    SystemConfig {
        folder_name: "atari_lynx",
        nointro_dats: &[],
        tgdb_platform_ids: &[4924],
    },
    // NEC
    SystemConfig {
        folder_name: "nec_pce",
        nointro_dats: &[],
        tgdb_platform_ids: &[34],
    },
    SystemConfig {
        folder_name: "nec_pcecd",
        nointro_dats: &[],
        tgdb_platform_ids: &[4955],
    },
    // Nintendo (no DAT yet)
    SystemConfig {
        folder_name: "nintendo_ds",
        nointro_dats: &[],
        tgdb_platform_ids: &[8],
    },
    // SNK
    SystemConfig {
        folder_name: "snk_ng",
        nointro_dats: &[],
        tgdb_platform_ids: &[24],
    },
    SystemConfig {
        folder_name: "snk_ngcd",
        nointro_dats: &[],
        tgdb_platform_ids: &[4956],
    },
    SystemConfig {
        folder_name: "snk_ngp",
        nointro_dats: &[],
        tgdb_platform_ids: &[4922, 4923],
    },
    // Disc-based consoles
    SystemConfig {
        folder_name: "sony_psx",
        nointro_dats: &[],
        tgdb_platform_ids: &[10],
    },
    SystemConfig {
        folder_name: "sega_dc",
        nointro_dats: &[],
        tgdb_platform_ids: &[16],
    },
    SystemConfig {
        folder_name: "sega_st",
        nointro_dats: &[],
        tgdb_platform_ids: &[17],
    },
    SystemConfig {
        folder_name: "sega_cd",
        nointro_dats: &[],
        tgdb_platform_ids: &[21],
    },
    SystemConfig {
        folder_name: "panasonic_3do",
        nointro_dats: &[],
        tgdb_platform_ids: &[25],
    },
    SystemConfig {
        folder_name: "philips_cdi",
        nointro_dats: &[],
        tgdb_platform_ids: &[4917],
    },
    // Computer systems
    SystemConfig {
        folder_name: "amstrad_cpc",
        nointro_dats: &[],
        tgdb_platform_ids: &[4914],
    },
    SystemConfig {
        folder_name: "commodore_ami",
        nointro_dats: &[],
        tgdb_platform_ids: &[4911],
    },
    SystemConfig {
        folder_name: "commodore_amicd",
        nointro_dats: &[],
        tgdb_platform_ids: &[4947],
    },
    SystemConfig {
        folder_name: "commodore_c64",
        nointro_dats: &[],
        tgdb_platform_ids: &[40],
    },
    SystemConfig {
        folder_name: "ibm_pc",
        nointro_dats: &[],
        tgdb_platform_ids: &[1],
    },
    SystemConfig {
        folder_name: "microsoft_msx",
        nointro_dats: &["Microsoft - MSX.dat", "Microsoft - MSX2.dat"],
        tgdb_platform_ids: &[4929],
    },
    SystemConfig {
        folder_name: "sharp_x68k",
        nointro_dats: &[],
        tgdb_platform_ids: &[4931],
    },
    SystemConfig {
        folder_name: "sinclair_zx",
        nointro_dats: &[],
        tgdb_platform_ids: &[4913],
    },
];

struct NoIntroEntry {
    name: String,
    rom_filename: String,
    region: String,
    crc32: u32,
    md5: String,
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
    md5: String,
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
    let mut current_md5 = String::new();

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
            current_md5.clear();
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
                    md5: current_md5.clone(),
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
            if let Some(md5_str) = extract_word_after(trimmed, "md5 ") {
                current_md5 = md5_str.to_lowercase();
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
    // Strip [...] bracket tags (TOSEC crack/hack/slot markers, No-Intro flags)
    let without_brackets: String = {
        let mut out = String::with_capacity(name.len());
        let mut depth = 0usize;
        for ch in name.chars() {
            match ch {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                _ if depth == 0 => out.push(ch),
                _ => {}
            }
        }
        out
    };
    // Strip from first '(' onwards (region/year/publisher parens)
    let before_paren = without_brackets
        .split('(')
        .next()
        .unwrap_or(&without_brackets)
        .trim_end();
    // Strip trailing TOSEC version " vX.Y[suffix]" — version always precedes paren tags
    let base = match before_paren.rfind(" v") {
        Some(idx) if before_paren[idx + 2..].starts_with(|c: char| c.is_ascii_digit()) => {
            before_paren[..idx].trim_end()
        }
        _ => before_paren,
    };
    // Article inversion: "Title, The" → "The Title"
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

/// Build the display-name suffix for a TOSEC filename stem.
///
/// TOSEC structure: `Title version (Year)(Publisher)(Country)[Flags]`
/// Groups 0 (year) and 1 (publisher) are always skipped. Groups 2+ are classified:
/// - Hardware IDs (AGA, ECS, …), disk/tape counts, language codes → skipped
/// - Everything else, including region codes and edition tags → appended as `(tag)`
///
/// Returns a string like `" (US) (Updated)"` — empty when no qualifying groups exist.
fn tosec_display_suffix(stem: &str) -> String {
    const HARDWARE: &[&str] = &["AGA", "ECS", "OCS", "CD32", "CDTV"];

    let mut suffix = String::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut group_count = 0usize;
    for (i, ch) in stem.char_indices() {
        match ch {
            '(' => {
                if depth == 0 {
                    start = i + 1;
                }
                depth += 1;
            }
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    group_count += 1;
                    // groups[0] = year, groups[1] = publisher — skip both
                    if group_count > 2 {
                        let g = &stem[start..i];
                        if !HARDWARE.contains(&g) && !is_tosec_metadata_group(g) {
                            suffix.push(' ');
                            suffix.push('(');
                            suffix.push_str(g);
                            suffix.push(')');
                        }
                    }
                }
            }
            _ => {}
        }
    }
    suffix
}

/// Returns true for TOSEC paren groups that are metadata noise, not edition tags:
/// disk/tape/side counts, language codes, multi-language markers, and bare years.
fn is_tosec_metadata_group(g: &str) -> bool {
    // Disk/tape/side counts: "Disk 1 of 2", "Side A", "Tape 1 of 3"
    if g.starts_with("Disk ") || g.starts_with("Side ") || g.starts_with("Tape ") {
        return true;
    }
    // 4-digit year (safety net for year appearing outside group 0)
    if g.len() == 4 && g.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // Language codes: all lowercase letters/hyphens, e.g. "en", "de", "en-it"
    if !g.is_empty() && g.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return true;
    }
    // Multi-language marker: "M3", "M4", etc.
    g.len() >= 2 && g.starts_with('M') && g[1..].chars().all(|c| c.is_ascii_digit())
}

/// Parse `whdload_db.xml` into a `filename → display name` map.
/// The `filename` attribute on each `<game>` matches the LHA archive stem
/// (e.g. `SuperFrog_v1.1_0485`); `<name>` is the clean human-readable title.
fn parse_whdload_db(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    // Skip gracefully when the file is absent, mirroring the Amiga DAT loop in
    // `insert_amiga_games`. The real build can't actually reach this with a
    // missing file -- `whdload_db.xml` is in `REQUIRED_SOURCES`, so the strict
    // preflight hard-fails first. Only `--stub` builds (which skip preflight and
    // read the fixture tree, where the Amiga sources aren't bundled) land here;
    // they just get WHDLoad entries without the cleaned display names.
    if !path.exists() {
        eprintln!("whdload_db: {} not found, skipping", path.display());
        return map;
    }
    let mut reader = Reader::from_file(path).unwrap_or_else(|e| {
        panic!(
            "whdload_db.xml at {} could not be read: {e}",
            path.display()
        )
    });
    // No `trim_text`: entity references split character data into chunks, and the
    // name is accumulated across events until </name>. Trimming each chunk would
    // drop the spaces bordering an entity (see `resolve_general_ref`) -- e.g.
    // "4th & Inches" would otherwise lose the "& " entirely.
    let mut buf = Vec::new();
    let mut current_filename = String::new();
    let mut current_name = String::new();
    let mut in_name = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"game" => {
                    current_filename.clear();
                    for attr in e.attributes().filter_map(|a| a.ok()) {
                        if attr.key.local_name().as_ref() == b"filename" {
                            // Unescape so an entity in the archive name (e.g.
                            // "4th&amp;Inches_..." for "4th&Inches_....lha") keys
                            // under the literal "&" the WHDLoad DAT stem uses.
                            current_filename = attr
                                .normalized_value(XmlVersion::Implicit1_0)
                                .map(|v| v.into_owned())
                                .unwrap_or_else(|_| {
                                    String::from_utf8_lossy(&attr.value).into_owned()
                                });
                        }
                    }
                }
                b"name" if !current_filename.is_empty() => {
                    in_name = true;
                    current_name.clear();
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) if in_name => {
                current_name.push_str(&e.decode().unwrap_or_default());
            }
            Ok(Event::GeneralRef(ref e)) if in_name => {
                current_name.push_str(&resolve_general_ref(e));
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"name" => {
                if in_name {
                    let name = current_name.trim();
                    if !name.is_empty() {
                        map.insert(current_filename.clone(), name.to_string());
                    }
                }
                in_name = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => panic!(
                "error parsing whdload_db.xml at position {}: {e:?}",
                reader.error_position()
            ),
            _ => {}
        }
        buf.clear();
    }
    eprintln!("whdload_db: loaded {} name entries", map.len());
    map
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

/// One RetroAchievements entry from `data/retroachievements/<system>.json`.
///
/// These files are committed (refreshed by `scripts/retroachievements-gamelist-extract.py`,
/// which needs a RetroAchievements API key — see `data/retroachievements/README.md`),
/// mirroring the wikidata/shmups committed-data pattern. Each entry carries RA's
/// `hashes` (the `ra_hash` values) — the build input for hash-based matching across
/// every realm (whole-file carts, header carts, discs, arcade). When a per-system
/// file is absent the RA pass is a clean no-op for that system.
#[derive(serde::Deserialize)]
struct RaEntry {
    ra_id: String,
    #[serde(default)]
    hashes: Vec<String>,
}

/// Hex MD5 of a string — RA's Arcade hash is `md5(lowercase romset_name)`.
fn md5_hex(input: &str) -> String {
    use md5::{Digest, Md5};
    let digest = Md5::new().chain_update(input.as_bytes()).finalize();
    let mut out = String::with_capacity(32);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Load the RA Arcade `hash → ra_id` map from `data/retroachievements/arcade.json`.
///
/// Missing file is a clean no-op; a present-but-broken file panics (same policy
/// as [`load_retroachievements_map`]). Keys are lowercased RA hashes.
fn load_arcade_ra_hash_map(sources_dir: &Path) -> HashMap<String, String> {
    let path = sources_dir.join("retroachievements").join("arcade.json");
    if !path.exists() {
        return HashMap::new();
    }
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("RetroAchievements: failed to read {}: {e}", path.display()));
    let entries: Vec<RaEntry> = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("RetroAchievements: failed to parse {}: {e}", path.display()));
    let mut map = HashMap::new();
    for entry in entries {
        if entry.ra_id.is_empty() {
            continue;
        }
        for hash in entry.hashes {
            map.insert(hash.to_lowercase(), entry.ra_id.clone());
        }
    }
    eprintln!("RetroAchievements: {} arcade hashes loaded", map.len());
    map
}

/// Insert Amiga identification data (TOSEC disk images + Redump CD32) as plain
/// `canonical_game` + `rom_entry` rows. Amiga has no No-Intro DAT and no
/// RetroAchievements console, so this is identification only: at scan time the
/// app matches user files by CRC first (TOSEC/Redump carry crc32) and falls back
/// to filename (TOSEC rom names are the same on disk). Full TOSEC is ingested —
/// including cracked/variant dumps — because the Amiga scene is crack-heavy and
/// users' on-disk files are frequently those exact variants.
/// (folder_name, TGDB platform ids, &[(DAT filename, provenance source tag)]).
struct AmigaSystemSpec {
    system: &'static str,
    tgdb_platform_ids: &'static [u32],
    /// `(DAT filename under upstream/amiga/, provenance source tag)`.
    dats: &'static [(&'static str, &'static str)],
}

fn insert_amiga_games(
    conn: &Connection,
    sources_dir: &Path,
    tgdb_data: &TgdbData,
) -> rusqlite::Result<()> {
    let amiga_dir = upstream(sources_dir).join("amiga");
    // whdload_db.xml provides clean display names for WHDLoad LHA entries
    // (e.g. "SuperFrog" instead of the DAT's "SuperFrog (Europe)"). Keyed by
    // the LHA archive stem that also appears in the WHDLoad DAT's rom filename.
    let whdload_names = parse_whdload_db(&amiga_dir.join("whdload_db.xml"));
    // commodore_ami loads all three naming conventions seen on disk — WHDLoad
    // (.lha, the RePlayOS default), No-Intro (.ipf), TOSEC (.adf) — so a user's
    // files match whichever convention they use. First DAT wins on a stem clash
    // (WHDLoad preferred). Metadata (genre/dev/year) comes from TGDB, since the
    // identification DATs carry none.
    let systems = &[
        AmigaSystemSpec {
            system: "commodore_ami",
            tgdb_platform_ids: &[4911],
            dats: &[
                ("Commodore - Amiga (WHDLoad).dat", "whdload"),
                ("Commodore - Amiga (No-Intro IPF).dat", "nointro"),
                ("Commodore - Amiga (TOSEC).dat", "tosec"),
            ],
        },
        AmigaSystemSpec {
            system: "commodore_amicd",
            tgdb_platform_ids: &[4947],
            dats: &[("Commodore - CD32.dat", "redump")],
        },
    ];
    let mut stmt_cg = conn.prepare(
        "INSERT INTO canonical_game (system, display_name, year, genre, developer, publisher, players, coop, rating, normalized_genre, description, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '', ?11)"
    )?;
    let mut stmt_re = conn.prepare(
        "INSERT OR IGNORE INTO rom_entry (system, filename_stem, region, crc32, canonical_game_id, normalized_title, md5) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
    )?;
    let mut total = 0usize;
    let mut total_tgdb = 0usize;
    for AmigaSystemSpec {
        system,
        tgdb_platform_ids: platform_ids,
        dats,
    } in systems
    {
        // Dedup stems across ALL of a system's DATs so the same game in two
        // naming conventions doesn't double-insert; first DAT in the list wins.
        let mut seen_stems: HashSet<String> = HashSet::new();
        for (dat, source) in *dats {
            let path = amiga_dir.join(dat);
            if !path.exists() {
                eprintln!("Amiga DB: {} not found, skipping", path.display());
                continue;
            }
            let entries = parse_nointro_dat(&path);
            let mut inserted = 0usize;
            for entry in &entries {
                let stem = entry
                    .rom_filename
                    .rfind('.')
                    .map(|i| &entry.rom_filename[..i])
                    .unwrap_or(&entry.rom_filename)
                    .to_string();
                if !seen_stems.insert(stem.clone()) {
                    continue;
                }
                // For WHDLoad entries, prefer the clean name from whdload_db.xml
                // (e.g. "SuperFrog") over the DAT-derived name which may carry
                // region tags or version suffixes. Fall back to clean_display_name
                // for entries not in whdload_db and for all non-WHDLoad sources.
                //
                // For No-Intro IPF and TOSEC entries, append the TOSEC region
                // code when present so regional variants are distinguishable
                // (e.g. "Nitro (US)" vs "Nitro").
                // `display_name` is what we store (carries the region suffix so
                // variants stay distinguishable); `lookup_name` is the clean base
                // title we send to TGDB. They must differ for non-WHDLoad rows —
                // appending "(US)" before lookup makes `tgdb_lookup` normalize to
                // "nitro us" and miss, dropping metadata for exactly these variants.
                let (display_name, lookup_name) = if *source == "whdload" {
                    let name = whdload_names
                        .get(&stem)
                        .cloned()
                        .unwrap_or_else(|| clean_display_name(&entry.name));
                    (name.clone(), name)
                } else {
                    let base = clean_display_name(&entry.name);
                    let mut display = base.clone();
                    display.push_str(&tosec_display_suffix(&stem));
                    (display, base)
                };
                // Enrich from TGDB by the clean base name; the DATs supply no
                // genre/dev/year of their own. Default = "no match" (all-empty).
                let matched = tgdb_lookup(&lookup_name, platform_ids, tgdb_data);
                total_tgdb += matched.is_some() as usize;
                let m = matched.unwrap_or_default();
                let norm_genre = normalize_console_genre(&m.genre);
                stmt_cg.execute(params![
                    system,
                    display_name,
                    m.year as i64,
                    m.genre,
                    m.developer,
                    m.publisher,
                    m.players as i64,
                    m.coop.map(|b| b as i64),
                    m.rating,
                    norm_genre,
                    source,
                ])?;
                let cg_id = conn.last_insert_rowid();
                let norm_title = normalize_title(&stem);
                stmt_re.execute(params![
                    system,
                    stem,
                    entry.region,
                    entry.crc32 as i64,
                    cg_id,
                    norm_title,
                    entry.md5,
                ])?;
                inserted += 1;
            }
            total += inserted;
            eprintln!("Amiga DB: {system} - {inserted} games from {dat}");
        }
        // Insert whdload_db.xml entries whose stems are absent from all DATs.
        // The WHDLoad DAT is No-Intro–tracked (only the latest verified version
        // per title), so older LHA versions (e.g. Superfrog_v1.5_0035 vs v1.6)
        // are missing from the DAT but present in whdload_db.xml.  Adding them
        // here gives those files a proper display name instead of the raw stem.
        if *system == "commodore_ami" {
            let mut extra = 0usize;
            for (filename, display_name) in &whdload_names {
                if !seen_stems.insert(filename.clone()) {
                    continue;
                }
                let matched = tgdb_lookup(display_name, platform_ids, tgdb_data);
                total_tgdb += matched.is_some() as usize;
                let m = matched.unwrap_or_default();
                let norm_genre = normalize_console_genre(&m.genre);
                stmt_cg.execute(params![
                    system,
                    display_name,
                    m.year as i64,
                    m.genre,
                    m.developer,
                    m.publisher,
                    m.players as i64,
                    m.coop.map(|b| b as i64),
                    m.rating,
                    norm_genre,
                    "whdload",
                ])?;
                let cg_id = conn.last_insert_rowid();
                let norm_title = normalize_title(filename);
                stmt_re.execute(params![system, filename, "", 0i64, cg_id, norm_title, "",])?;
                extra += 1;
            }
            total += extra;
            eprintln!(
                "Amiga DB: {system} - {extra} extra games from whdload_db.xml (not in WHDLoad DAT)"
            );
        }
    }
    eprintln!("Amiga DB: Total {total} games inserted ({total_tgdb} TGDB metadata matches)");
    Ok(())
}

/// Populate the catalog's RetroAchievements linkage from the committed extracts.
///
/// Two outputs, both hash-based (never title-matched — title matching mis-binds
/// hacks/subsets; see retroachievements plan §10.7):
///
/// 1. The `ra_hash(system, hash, ra_id)` table — every RA hash for every cart/disc
///    system. This is the RUNTIME lookup table: header carts (NES/SNES/N64) and
///    discs compute their `rc_hash` from ROM bytes at scan time and resolve it here.
/// 2. `rom_entry.ra_id` for whole-file cart dumps — stamped at build time by joining
///    `rom_entry.md5 == ra_hash.hash` (for whole-file systems the RA hash *is* the
///    file md5). Header carts' full-file md5 never equals their header-stripped RA
///    hash, so they stay empty here and resolve at runtime instead — correct.
///
/// Arcade is excluded: it is matched separately in `insert_arcade_games` by
/// `md5(romset name)` and stamped directly onto `arcade_game.ra_id`.
fn populate_retroachievements(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<()> {
    let ra_dir = sources_dir.join("retroachievements");
    let Ok(read_dir) = std::fs::read_dir(&ra_dir) else {
        eprintln!(
            "RetroAchievements: {} not found — skipping RA linkage",
            ra_dir.display()
        );
        return Ok(());
    };

    let mut stmt_rh =
        conn.prepare("INSERT OR IGNORE INTO ra_hash (system, hash, ra_id) VALUES (?1, ?2, ?3)")?;
    let mut total_hashes = 0usize;

    for dir_entry in read_dir.flatten() {
        let path = dir_entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(system) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // Arcade has its own md5(romset)-based path; it never resolves via ra_hash.
        if system == "arcade" {
            continue;
        }
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("RetroAchievements: failed to read {}: {e}", path.display())
        });
        let entries: Vec<RaEntry> = serde_json::from_str(&raw).unwrap_or_else(|e| {
            panic!("RetroAchievements: failed to parse {}: {e}", path.display())
        });
        let mut system_hashes = 0usize;
        for entry in entries {
            if entry.ra_id.is_empty() {
                continue;
            }
            for hash in entry.hashes {
                let inserted =
                    stmt_rh.execute(params![system, hash.to_lowercase(), entry.ra_id])?;
                system_hashes += inserted;
            }
        }
        total_hashes += system_hashes;
        eprintln!("RetroAchievements: {system_hashes} hashes loaded for {system}");
    }

    let stamped = stamp_whole_file_rom_entry_ra_ids(conn)?;
    eprintln!(
        "RetroAchievements: {total_hashes} hashes in ra_hash; {stamped} whole-file rom_entry.ra_id stamped"
    );
    Ok(())
}

/// Whole-file carts: stamp `rom_entry.ra_id` where the dump's full-file md5 equals
/// an RA hash for that system (for whole-file systems the RA hash *is* the file
/// md5). Header carts/discs have md5 ≠ RA hash, so they stay empty and match at
/// runtime instead. Returns the number of rows stamped.
fn stamp_whole_file_rom_entry_ra_ids(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE rom_entry SET ra_id = (
             SELECT h.ra_id FROM ra_hash h
             WHERE h.system = rom_entry.system AND h.hash = rom_entry.md5
         )
         WHERE md5 != '' AND EXISTS (
             SELECT 1 FROM ra_hash h
             WHERE h.system = rom_entry.system AND h.hash = rom_entry.md5
         )",
        [],
    )
}

/// TheGamesDB data loaded once and shared by the console and Amiga insert passes
/// (the JSON dump is ~145 MB — parse it once, not per pass).
struct TgdbData {
    games: HashMap<(String, u32), TgdbEntry>,
    regional_dates: TgdbRegionalDatesMap,
    developers: HashMap<u32, String>,
    publishers: HashMap<u32, String>,
    genres: HashMap<u32, String>,
}

fn load_tgdb_data(sources_dir: &Path) -> TgdbData {
    let tgdb_path = upstream(sources_dir).join("thegamesdb-latest.json");
    let (games, regional_dates) = if tgdb_path.exists() {
        eprintln!("Game DB: Loading TheGamesDB JSON dump...");
        let parsed = parse_tgdb_json(&tgdb_path);
        eprintln!("Game DB: TheGamesDB loaded {} entries", parsed.0.len());
        parsed
    } else {
        eprintln!("Game DB: TheGamesDB JSON not found, skipping metadata enrichment");
        (HashMap::new(), HashMap::new())
    };
    let developers = load_tgdb_name_map(&upstream(sources_dir).join("tgdb-developers.json"));
    let publishers = load_tgdb_name_map(&upstream(sources_dir).join("tgdb-publishers.json"));
    let genres = load_tgdb_name_map(&upstream(sources_dir).join("tgdb-genres.json"));
    eprintln!(
        "Game DB: TGDB lookups: {} devs, {} pubs, {} genres",
        developers.len(),
        publishers.len(),
        genres.len()
    );
    TgdbData {
        games,
        regional_dates,
        developers,
        publishers,
        genres,
    }
}

/// Metadata resolved from a TGDB title match.
#[derive(Default)]
struct TgdbMatch {
    year: u16,
    players: u8,
    genre: String,
    developer: String,
    publisher: String,
    coop: Option<bool>,
    rating: String,
    alternates: Vec<String>,
}

/// Look up TGDB metadata for a display name across the given platform ids
/// (first match wins). Shared by the console and Amiga passes; the console pass
/// additionally collects regional release dates separately.
fn tgdb_lookup(display_name: &str, platform_ids: &[u32], tgdb: &TgdbData) -> Option<TgdbMatch> {
    let norm = normalize_title_for_tgdb(display_name);
    for &platform_id in platform_ids {
        if let Some(e) = tgdb.games.get(&(norm.clone(), platform_id)) {
            let genre = if e.genre_ids.is_empty() {
                String::new()
            } else {
                tgdb.genres
                    .get(&e.genre_ids[0])
                    .cloned()
                    .unwrap_or_else(|| tgdb_genre_name(e.genre_ids[0]).to_string())
            };
            let developer = e
                .developer_ids
                .first()
                .and_then(|id| tgdb.developers.get(id))
                .map(|n| normalize_developer(n))
                .unwrap_or_default();
            let publisher = e
                .publisher_ids
                .first()
                .and_then(|id| tgdb.publishers.get(id))
                .cloned()
                .unwrap_or_default();
            return Some(TgdbMatch {
                year: e.year,
                players: e.players,
                genre,
                developer,
                publisher,
                coop: e.coop,
                rating: e.rating.clone(),
                alternates: e.alternates.clone(),
            });
        }
    }
    None
}

fn insert_console_games(
    conn: &Connection,
    sources_dir: &Path,
    tgdb_data: &TgdbData,
) -> rusqlite::Result<()> {
    let nointro_dir = upstream(sources_dir).join("no-intro");
    let maxusers_dir = upstream(sources_dir).join("libretro-meta").join("maxusers");
    let genre_dir = upstream(sources_dir).join("libretro-meta").join("genre");

    // TGDB metadata matching goes through `tgdb_lookup`; this function only needs
    // the games map (system-presence check) and regional dates directly.
    let tgdb = &tgdb_data.games;
    let tgdb_regional_dates = &tgdb_data.regional_dates;

    let mut total_roms = 0usize;
    let mut total_games = 0usize;
    let mut total_tgdb_matches = 0usize;
    let mut console_release_date: Vec<(String, String, &'static str, String, &'static str)> =
        Vec::new();

    let mut stmt_cg = conn.prepare(
        "INSERT INTO canonical_game (system, display_name, year, genre, developer, publisher, players, coop, rating, normalized_genre, description, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, '', ?11)"
    )?;
    let mut stmt_re = conn.prepare(
        "INSERT OR IGNORE INTO rom_entry (system, filename_stem, region, crc32, canonical_game_id, normalized_title, md5) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
    )?;
    let mut stmt_alt = conn.prepare(
        "INSERT INTO rom_alternate (canonical_game_id, system, alternate_name) VALUES (?1, ?2, ?3)",
    )?;

    for sys in GAME_DB_SYSTEMS {
        if sys.nointro_dats.is_empty() {
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

        let mut nointro_entries = Vec::new();
        for dat_name in sys.nointro_dats {
            let dat_path = nointro_dir.join(dat_name);
            if !dat_path.exists() {
                eprintln!(
                    "Game DB: No-Intro DAT {} not found for {}, skipping",
                    dat_name, sys.folder_name
                );
                continue;
            }
            nointro_entries.extend(parse_nointro_dat(&dat_path));
        }
        if nointro_entries.is_empty() {
            eprintln!(
                "Game DB: no No-Intro entries found for {}, skipping",
                sys.folder_name
            );
            continue;
        }
        eprintln!(
            "Game DB: {} - parsed {} ROM entries",
            sys.folder_name,
            nointro_entries.len()
        );

        let mut maxusers: HashMap<u32, String> = HashMap::new();
        for dat_name in sys.nointro_dats {
            let maxusers_path = maxusers_dir.join(dat_name);
            if maxusers_path.exists() {
                maxusers.extend(parse_libretro_meta_dat(&maxusers_path, "users "));
            }
        }

        let mut genres: HashMap<u32, String> = HashMap::new();
        for dat_name in sys.nointro_dats {
            let genre_path = genre_dir.join(dat_name);
            if genre_path.exists() {
                genres.extend(parse_libretro_meta_dat(&genre_path, "genre "));
            }
        }

        // Group ROM entries into canonical games by normalized title
        let mut game_groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, entry) in nointro_entries.iter().enumerate() {
            let key = normalize_title(&entry.name);
            game_groups.entry(key).or_default().push(idx);
        }

        let mut group_keys: Vec<String> = game_groups.keys().cloned().collect();
        group_keys.sort();

        let mut canonical_game: Vec<CanonicalGameBuild> = Vec::new();
        let mut rom_entry: Vec<RomEntryBuild> = Vec::new();
        let mut tgdb_match_count = 0usize;

        for group_key in &group_keys {
            let indices = &game_groups[group_key];
            let game_id = canonical_game.len();

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
                                console_release_date.push((
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

            if let Some(m) = tgdb_lookup(&display_name, sys.tgdb_platform_ids, tgdb_data) {
                year = m.year;
                tgdb_players = m.players;
                tgdb_genre = m.genre;
                tgdb_alternates = m.alternates;
                tgdb_developer = m.developer;
                tgdb_publisher = m.publisher;
                tgdb_coop = m.coop;
                tgdb_rating = m.rating;
                tgdb_match_count += 1;
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

            canonical_game.push(CanonicalGameBuild {
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
                rom_entry.push(RomEntryBuild {
                    filename_stem: stem.to_string(),
                    region: entry.region.clone(),
                    crc32: entry.crc32,
                    md5: entry.md5.clone(),
                    game_id,
                });
            }
        }

        eprintln!(
            "Game DB: {} - {} canonical games, {} ROM entries, {} TGDB matches",
            sys.folder_name,
            canonical_game.len(),
            rom_entry.len(),
            tgdb_match_count
        );

        // Insert canonical games and collect their actual SQLite rowids
        let mut canonical_game_ids: Vec<i64> = Vec::with_capacity(canonical_game.len());
        for game in canonical_game.iter() {
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
                resource_kind::NOINTRO_SOURCE,
            ])?;
            let canonical_game_id = conn.last_insert_rowid();
            canonical_game_ids.push(canonical_game_id);

            // Insert alternates
            for alt in &game.alternates {
                stmt_alt.execute(params![canonical_game_id, sys.folder_name, alt])?;
            }
        }

        // RetroAchievements ids are stamped later, by hash, in
        // populate_retroachievements() — never title-matched here (title matching
        // mis-binds hacks/subsets; see retroachievements plan §10.7).

        // Insert ROM entries (deduplicated by stem)
        let mut seen_stems: HashSet<&str> = HashSet::new();

        for entry in &rom_entry {
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
                entry.md5,
            ])?;
        }

        total_roms += rom_entry.len();
        total_games += canonical_game.len();
        total_tgdb_matches += tgdb_match_count;
    }

    eprintln!(
        "Game DB: Total {} ROM entries, {} canonical games, {} TGDB matches",
        total_roms, total_games, total_tgdb_matches
    );

    // Insert console release dates
    console_release_date.sort();
    console_release_date.dedup();
    let mut stmt_crd = conn.prepare(
        "INSERT OR REPLACE INTO console_release_date (system, base_title, region, release_date, precision, source) VALUES (?1, ?2, ?3, ?4, ?5, 'tgdb')"
    )?;
    for (system, base_title, region, release_date, precision) in &console_release_date {
        stmt_crd.execute(params![system, base_title, region, release_date, precision])?;
    }
    eprintln!(
        "Game DB: Inserted {} console release date rows",
        console_release_date.len()
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
        "INSERT INTO series_entry (game_title, series_name, system, series_order, follows, followed_by, normalized_title) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
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
// Catalog game resources
// =============================================================================

const MISTER_MANUAL_REPOS: &[(&str, &str)] = &[
    ("manualsdb-3do", "panasonic_3do"),
    ("manualsdb-atari2600", "atari_2600"),
    ("manualsdb-atari5200", "atari_5200"),
    ("manualsdb-atari7800", "atari_7800"),
    ("manualsdb-atarilynx", "atari_lynx"),
    ("manualsdb-cdi", "philips_cdi"),
    ("manualsdb-fds", "nintendo_nes"),
    ("manualsdb-gameboy", "nintendo_gb"),
    ("manualsdb-gamegear", "sega_gg"),
    ("manualsdb-gba", "nintendo_gba"),
    ("manualsdb-gbc", "nintendo_gbc"),
    ("manualsdb-jaguar", "atari_jaguar"),
    ("manualsdb-jaguarcd", "atari_jaguar"),
    ("manualsdb-megadrive", "sega_smd"),
    ("manualsdb-n64", "nintendo_n64"),
    ("manualsdb-neogeoaes", "snk_ng"),
    ("manualsdb-neogeocd", "snk_ngcd"),
    ("manualsdb-nes", "nintendo_nes"),
    ("manualsdb-ngp", "snk_ngp"),
    ("manualsdb-ngpc", "snk_ngp"),
    ("manualsdb-psx", "sony_psx"),
    ("manualsdb-sega32x", "sega_32x"),
    ("manualsdb-segasaturn", "sega_st"),
    ("manualsdb-segasg1000", "sega_sg"),
    ("manualsdb-segacd", "sega_cd"),
    ("manualsdb-sms", "sega_sms"),
    ("manualsdb-snes", "nintendo_snes"),
    ("manualsdb-turbografx16", "nec_pce"),
    ("manualsdb-turbografxcd", "nec_pcecd"),
];

const RETROKIT_MANUAL_FOLDERS: &[(&str, &[&str])] = &[
    ("3do", &["panasonic_3do"]),
    ("amiga", &["commodore_ami"]),
    (
        "arcade",
        &[
            "arcade_mame",
            "arcade_fbneo",
            "arcade_mame_2k3p",
            "arcade_dc",
            "arcade_stv",
        ],
    ),
    ("atari2600", &["atari_2600"]),
    ("atari5200", &["atari_5200"]),
    ("atari7800", &["atari_7800"]),
    ("atarijaguar", &["atari_jaguar"]),
    ("atarilynx", &["atari_lynx"]),
    ("c64", &["commodore_c64"]),
    ("dreamcast", &["sega_dc"]),
    ("gamegear", &["sega_gg"]),
    ("gb", &["nintendo_gb"]),
    ("gba", &["nintendo_gba"]),
    ("gbc", &["nintendo_gbc"]),
    ("mastersystem", &["sega_sms"]),
    ("megadrive", &["sega_smd"]),
    ("n64", &["nintendo_n64"]),
    ("nds", &["nintendo_ds"]),
    ("neogeo", &["snk_ng"]),
    ("neogeocd", &["snk_ngcd"]),
    ("nes", &["nintendo_nes"]),
    ("ngp", &["snk_ngp"]),
    ("pc", &["ibm_pc", "scummvm"]),
    ("pcengine", &["nec_pce"]),
    ("pce-cd", &["nec_pcecd"]),
    ("psx", &["sony_psx"]),
    ("saturn", &["sega_st"]),
    ("sega32x", &["sega_32x"]),
    ("segacd", &["sega_cd"]),
    ("sg-1000", &["sega_sg"]),
    ("snes", &["nintendo_snes"]),
];

struct CatalogResourceBuild {
    system: String,
    normalized_title: String,
    resource_type: &'static str,
    source: &'static str,
    resource_id: String,
    url: String,
    title: String,
    languages: String,
    mime_type: &'static str,
}

fn sha256_resource_id(url: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, url.as_bytes());
    let mut out = String::from("urlhash:");
    for byte in digest.as_ref() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn canonical_manual_url(url: &str) -> String {
    let sonicretro_image_path = url
        .strip_prefix("http://info.sonicretro.org/images/")
        .or_else(|| url.strip_prefix("https://info.sonicretro.org/images/"));

    if let Some(path) = sonicretro_image_path {
        return match path {
            // The SonicRetro file page still exists, but the local media URL
            // returns 404. The file is available through the linked CDN mirror.
            "6/6e/Sonic_Blast_GG_US_Manual.pdf" => {
                "https://retrocdn.net/images/6/6e/Sonic_Blast_GG_US_Manual.pdf".to_string()
            }
            _ => format!("https://info.sonicretro.org/images/{path}"),
        };
    }

    url.to_string()
}

fn should_skip_manual_url(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();

    // These legacy/manual-host families were audited in May 2026. They are
    // dead, stale, access-controlled, robot-protected, or require invalid TLS,
    // which makes them poor catalog resources for end-user Save/download flows.
    lower.starts_with("https://the-eye.eu/public/books/retrowith.in/manuals/")
        || lower.starts_with("https://amiga.abime.net/manual/")
        || lower.starts_with(
            "https://forums.atariage.com/applications/core/interface/file/attachment.php",
        )
        || lower.starts_with("https://dl3.emu-land.net/manuals/")
        || lower.starts_with("https://commodore.bombjack.org/")
        || lower.starts_with("http://commodore.bombjack.org/")
        || lower.starts_with("https://www.nintendo.com/consumer/gameslist/manuals/")
        || lower.starts_with("http://www.nintendo.com/consumer/gameslist/manuals/")
        || lower.starts_with("https://retro-commodore.eu/files/downloads/amigamanuals-xiik.net/")
        || lower
            .starts_with("https://www.retro-commodore.eu/files/downloads/amigamanuals-xiik.net/")
        || lower.starts_with("http://www.stadium64.com/manuals/")
        || lower.starts_with("https://www.stadium64.com/manuals/")
}

fn manual_title_from_path(path: &str) -> Option<(String, String)> {
    let stem = Path::new(path).file_stem()?.to_str()?.trim();
    if stem.is_empty() {
        return None;
    }

    if let Some(rest) = stem.strip_prefix("![")
        && let Some((tag, title)) = rest.split_once("] ")
    {
        let title = title.trim();
        if !title.is_empty() {
            return Some((title.to_string(), format!("{title} ({})", tag.trim())));
        }
    }

    Some((stem.to_string(), stem.to_string()))
}

fn insert_catalog_resources(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<()> {
    let mut resources = Vec::new();
    resources.extend(load_mister_manual_resources(sources_dir));
    resources.extend(load_retrokit_manual_resources(sources_dir));
    resources.extend(load_shmups_wiki_resources(sources_dir));

    let mut stmt = conn.prepare(
        "INSERT OR IGNORE INTO catalog_game_resource
         (system, normalized_title, resource_type, source, resource_id, url, title, languages, mime_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for row in &resources {
        stmt.execute(params![
            row.system,
            row.normalized_title,
            row.resource_type,
            row.source,
            row.resource_id,
            row.url,
            row.title,
            row.languages,
            row.mime_type,
        ])?;
    }

    eprintln!("Catalog resources: Inserted {} rows", resources.len());
    Ok(())
}

fn load_mister_manual_resources(sources_dir: &Path) -> Vec<CatalogResourceBuild> {
    let mut out = Vec::new();
    let dir = upstream(sources_dir).join("mister-manuals");
    for &(repo, system) in MISTER_MANUAL_REPOS {
        let path = dir.join(format!("{repo}.csv"));
        if !path.exists() {
            continue;
        }
        let mut rdr = match csv::Reader::from_path(&path) {
            Ok(rdr) => rdr,
            Err(e) => {
                eprintln!(
                    "Warning: failed to open MiSTer manuals CSV {}: {e}",
                    path.display()
                );
                continue;
            }
        };
        let headers = match rdr.headers() {
            Ok(h) => h.clone(),
            Err(e) => {
                eprintln!(
                    "Warning: failed to read MiSTer manuals CSV headers {}: {e}",
                    path.display()
                );
                continue;
            }
        };
        let path_idx = headers.iter().position(|h| h.eq_ignore_ascii_case("Path"));
        let url_idx = headers.iter().position(|h| h.eq_ignore_ascii_case("URL"));
        let (Some(path_idx), Some(url_idx)) = (path_idx, url_idx) else {
            eprintln!(
                "Warning: MiSTer manuals CSV {} missing Path/URL columns",
                path.display()
            );
            continue;
        };

        for record in rdr.records().flatten() {
            let Some(raw_path) = record
                .get(path_idx)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let Some(raw_url) = record.get(url_idx).map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            let url = canonical_manual_url(raw_url);
            if should_skip_manual_url(&url) {
                continue;
            }
            let Some((match_title, display_title)) = manual_title_from_path(raw_path) else {
                continue;
            };
            let normalized_title = title_utils::normalize_title_for_metadata(&match_title);
            if normalized_title.is_empty() {
                continue;
            }
            out.push(CatalogResourceBuild {
                system: system.to_string(),
                normalized_title,
                resource_type: resource_kind::MANUAL,
                source: resource_kind::MISTER_MANUALS_SOURCE,
                resource_id: sha256_resource_id(&url),
                url,
                title: display_title,
                languages: "en".to_string(),
                mime_type: "application/pdf",
            });
        }
    }
    out
}

fn load_retrokit_manual_resources(sources_dir: &Path) -> Vec<CatalogResourceBuild> {
    let mut out = Vec::new();
    let dir = upstream(sources_dir).join("retrokit-manuals");
    for &(folder, systems) in RETROKIT_MANUAL_FOLDERS {
        let path = dir.join(format!("{folder}-sources.tsv"));
        if !path.exists() {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let parts: Vec<&str> = line.trim().splitn(3, '\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let title = parts[0].trim();
            let languages = parts[1].trim();
            let raw_url = parts[2].trim();
            if title.is_empty() || raw_url.is_empty() {
                continue;
            }
            let url = canonical_manual_url(raw_url);
            if should_skip_manual_url(&url) {
                continue;
            }
            let normalized_title = title_utils::normalize_title_for_metadata(title);
            if normalized_title.is_empty() {
                continue;
            }
            for &system in systems {
                out.push(CatalogResourceBuild {
                    system: system.to_string(),
                    normalized_title: normalized_title.clone(),
                    resource_type: resource_kind::MANUAL,
                    source: resource_kind::RETROKIT_SOURCE,
                    resource_id: sha256_resource_id(&url),
                    url: url.clone(),
                    title: title.to_string(),
                    languages: languages.to_string(),
                    mime_type: "application/pdf",
                });
            }
        }
    }
    out
}

#[derive(serde::Deserialize)]
struct ShmupsWikiBuildEntry {
    normalized_title: String,
    page_title: String,
    #[serde(default)]
    video_index: bool,
    #[serde(default)]
    video_index_inherits_from: Option<String>,
    // Section anchor on the parent's Video Index page (already MediaWiki-encoded,
    // e.g. "Version_1.5"). Only set alongside video_index_inherits_from; when
    // present the inherited link deep-links to the variant's section.
    #[serde(default)]
    video_index_anchor: Option<String>,
}

fn load_shmups_wiki_resources(sources_dir: &Path) -> Vec<CatalogResourceBuild> {
    let path = sources_dir.join("shmups-wiki/games.json");
    let Ok(file) = File::open(&path) else {
        return Vec::new();
    };
    let entries: Vec<ShmupsWikiBuildEntry> = match serde_json::from_reader(BufReader::new(file)) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!(
                "Warning: failed to parse Shmups Wiki JSON {}: {e}",
                path.display()
            );
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for entry in entries {
        if entry.normalized_title.is_empty() || entry.page_title.is_empty() {
            continue;
        }
        let page_url = shmups_wiki_page_url(&entry.page_title);
        out.push(CatalogResourceBuild {
            system: resource_kind::GLOBAL_SYSTEM.to_string(),
            normalized_title: entry.normalized_title.clone(),
            resource_type: resource_kind::STRATEGY_GUIDE,
            source: resource_kind::SHMUPS_WIKI_SOURCE,
            resource_id: entry.page_title.clone(),
            url: page_url.clone(),
            title: entry.page_title.clone(),
            languages: String::new(),
            mime_type: "text/html",
        });
        if entry.video_index {
            out.push(CatalogResourceBuild {
                system: resource_kind::GLOBAL_SYSTEM.to_string(),
                normalized_title: entry.normalized_title,
                resource_type: resource_kind::VIDEO_INDEX,
                source: resource_kind::SHMUPS_WIKI_SOURCE,
                resource_id: entry.page_title.clone(),
                url: format!("{page_url}/Video_Index"),
                title: entry.page_title,
                languages: String::new(),
                mime_type: "text/html",
            });
        } else if let Some(parent) = entry.video_index_inherits_from {
            let parent_url = shmups_wiki_page_url(&parent);
            // Deep-link to the variant's section when the extract resolved one;
            // otherwise link to the Video Index page top.
            let url = match entry.video_index_anchor.as_deref() {
                Some(anchor) if !anchor.is_empty() => {
                    format!("{parent_url}/Video_Index#{anchor}")
                }
                _ => format!("{parent_url}/Video_Index"),
            };
            out.push(CatalogResourceBuild {
                system: resource_kind::GLOBAL_SYSTEM.to_string(),
                normalized_title: entry.normalized_title,
                resource_type: resource_kind::VIDEO_INDEX,
                source: resource_kind::SHMUPS_WIKI_SOURCE,
                resource_id: parent.clone(),
                url,
                title: parent,
                languages: String::new(),
                mime_type: "text/html",
            });
        }
    }
    out
}

fn shmups_wiki_page_url(page_title: &str) -> String {
    let with_underscores = page_title.replace(' ', "_");
    format!(
        "https://shmups.wiki/library/{}",
        mediawiki_path_encode(&with_underscores)
    )
}

fn mediawiki_path_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        if is_mediawiki_path_safe(*byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn is_mediawiki_path_safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.' | b'_' | b'~' | b'!' | b'(' | b')' | b'*' | b'\''
        )
}

/// Fold every column of every row of `sql` (type-agnostically) into `context`,
/// prefixed by `label`. Lets the version digest cover whole tables without
/// hand-coding per-column reads or worrying about INTEGER vs TEXT columns.
fn fold_query(
    context: &mut ring::digest::Context,
    conn: &Connection,
    label: &str,
    sql: &str,
) -> rusqlite::Result<()> {
    use rusqlite::types::Value;
    context.update(label.as_bytes());
    context.update(b"\n");
    let mut stmt = conn.prepare(sql)?;
    let ncols = stmt.column_count();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        for idx in 0..ncols {
            match row.get::<_, Value>(idx)? {
                Value::Null => context.update(&[0x01]),
                Value::Integer(i) => context.update(i.to_string().as_bytes()),
                Value::Real(f) => context.update(f.to_string().as_bytes()),
                Value::Text(s) => context.update(s.as_bytes()),
                Value::Blob(b) => context.update(&b),
            }
            context.update(&[0x00]);
        }
        context.update(b"\n");
    }
    Ok(())
}

/// Content fingerprint of every catalog table that determines `game_library`
/// output. The per-storage `enrichment_inputs_version` stamp is compared against
/// this on boot; any mismatch triggers a reconcile rescan (see background.rs
/// `phase_enrichment_inputs_reconcile`). It must therefore move whenever ANY
/// scan/enrichment input changes — identification (`rom_entry` / `arcade_game`),
/// metadata (`canonical_game`), RA linkage (`ra_hash` + per-dump `ra_id`),
/// aliases, series, release dates, or media resources. Hashing only a subset
/// (as an earlier version did) lets an identification-only catalog change — e.g.
/// new Amiga DATs adding `rom_entry` rows with no description/RA — slip through
/// and silently fail to re-identify a user's library.
fn catalog_enrichment_inputs_version(conn: &Connection) -> rusqlite::Result<String> {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    // Console metadata (enrichment writes these onto game_library rows).
    fold_query(
        &mut ctx,
        conn,
        "canonical_game",
        "SELECT system, display_name, year, genre, developer, publisher, players, coop,
                rating, normalized_genre, description, source
         FROM canonical_game ORDER BY system, display_name, year, id",
    )?;
    // Console identification: which dump maps to which game, its CRC/md5, and its
    // per-dump RA id. The cg.display_name join captures dump→game re-mappings.
    fold_query(
        &mut ctx,
        conn,
        "rom_entry",
        "SELECT re.system, re.filename_stem, re.region, re.crc32, re.md5,
                re.normalized_title, re.ra_id, cg.display_name
         FROM rom_entry re JOIN canonical_game cg ON cg.id = re.canonical_game_id
         ORDER BY re.system, re.filename_stem, re.crc32",
    )?;
    // Arcade identification + metadata + RA linkage.
    fold_query(
        &mut ctx,
        conn,
        "arcade_game",
        "SELECT rom_name, source, display_name, year, manufacturer, players, rotation,
                status, is_clone, is_bios, parent, category, normalized_genre, board,
                ra_id, ra_hash
         FROM arcade_game ORDER BY rom_name, source",
    )?;
    // RA runtime lookup table (header carts + discs).
    fold_query(
        &mut ctx,
        conn,
        "ra_hash",
        "SELECT system, hash, ra_id FROM ra_hash ORDER BY system, hash",
    )?;
    // Aliases, series grouping, release dates, and media resources are all
    // enrichment inputs mirrored onto game_library rows.
    fold_query(
        &mut ctx,
        conn,
        "rom_alternate",
        "SELECT ra.system, ra.alternate_name, cg.display_name
         FROM rom_alternate ra JOIN canonical_game cg ON cg.id = ra.canonical_game_id
         ORDER BY ra.system, ra.alternate_name, cg.display_name",
    )?;
    fold_query(
        &mut ctx,
        conn,
        "series_entry",
        "SELECT system, normalized_title, game_title, series_name, series_order,
                follows, followed_by
         FROM series_entry ORDER BY system, normalized_title, game_title",
    )?;
    fold_query(
        &mut ctx,
        conn,
        "console_release_date",
        "SELECT system, base_title, region, release_date, precision, source
         FROM console_release_date ORDER BY system, base_title, region",
    )?;
    fold_query(
        &mut ctx,
        conn,
        "arcade_release_date",
        "SELECT rom_name, year, source FROM arcade_release_date ORDER BY rom_name, source",
    )?;
    fold_query(
        &mut ctx,
        conn,
        "catalog_game_resource",
        "SELECT system, normalized_title, resource_type, source, resource_id, url, title,
                languages, mime_type
         FROM catalog_game_resource
         ORDER BY system, normalized_title, resource_type, source, resource_id",
    )?;

    let digest = ctx.finish();
    let mut out = String::new();
    for byte in digest.as_ref() {
        out.push_str(&format!("{byte:02x}"));
    }
    Ok(out)
}

// =============================================================================
// Preflight
// =============================================================================

/// A source the production catalog build reads. Every input that contributes
/// rows or enrichment is listed — a missing one silently produces a catalog
/// with coverage gaps (e.g. no shmups links, no RA flags, no player counts),
/// which is a shipping defect, so a full build fails fast here.
///
/// `--stub` builds skip preflight (they read fixtures). `--allow-partial`
/// downgrades the failure to a warning for keyless local/throwaway builds.
enum RequiredSource {
    /// A single file that must exist and be non-empty.
    File(&'static str),
    /// A directory that must exist and contain at least one entry whose name
    /// ends with `suffix` (the per-system/per-repo collections).
    Dir {
        path: &'static str,
        suffix: &'static str,
    },
}

/// Paths relative to the data dir. Downloaded inputs live under `upstream/`;
/// committed curated data (`wikidata/`, `shmups-wiki/`, `retroachievements/`,
/// `community/`, `arcade/`) stays at the data-dir root.
const REQUIRED_SOURCES: &[RequiredSource] = &[
    // ── Core game data (rows) ──
    RequiredSource::File("upstream/fbneo-arcade.dat"),
    RequiredSource::File("upstream/mame2003plus.xml"),
    RequiredSource::File("upstream/mame0285-arcade.xml"),
    RequiredSource::Dir {
        path: "upstream/no-intro",
        suffix: ".dat",
    },
    RequiredSource::File("upstream/thegamesdb-latest.json"),
    RequiredSource::File("arcade/flycast_games.csv"),
    // ── Arcade overlays (categories, player counts) ──
    RequiredSource::File("upstream/catver.ini"),
    RequiredSource::File("upstream/catver-mame-current.ini"),
    RequiredSource::File("upstream/nplayers.ini"),
    // ── TGDB ID→name lookups (need a TGDB API key; see download-tgdb-lookups.sh) ──
    RequiredSource::File("upstream/tgdb-developers.json"),
    RequiredSource::File("upstream/tgdb-publishers.json"),
    RequiredSource::File("upstream/tgdb-genres.json"),
    // ── libretro metadata overlays ──
    RequiredSource::Dir {
        path: "upstream/libretro-meta/maxusers",
        suffix: ".dat",
    },
    RequiredSource::Dir {
        path: "upstream/libretro-meta/genre",
        suffix: ".dat",
    },
    // ── Amiga identification ──
    RequiredSource::File("upstream/amiga/whdload_db.xml"),
    RequiredSource::Dir {
        path: "upstream/amiga",
        suffix: ".dat",
    },
    // ── Manual indexes ──
    RequiredSource::Dir {
        path: "upstream/mister-manuals",
        suffix: ".csv",
    },
    RequiredSource::Dir {
        path: "upstream/retrokit-manuals",
        suffix: "-sources.tsv",
    },
    // ── Committed curated data ──
    RequiredSource::File("wikidata/series.json"),
    RequiredSource::File("shmups-wiki/games.json"),
    RequiredSource::Dir {
        path: "retroachievements",
        suffix: ".json",
    },
    RequiredSource::Dir {
        path: "community",
        suffix: ".json",
    },
];

/// Return `Some(reason)` if the source is missing/empty, else `None`.
fn missing_source_reason(sources_dir: &Path, source: &RequiredSource) -> Option<String> {
    match source {
        RequiredSource::File(rel) => {
            let path = sources_dir.join(rel);
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_file() && meta.len() > 0 => None,
                Ok(meta) if meta.len() == 0 => Some(format!("{rel} (empty)")),
                Ok(_) => Some(format!("{rel} (not a file)")),
                Err(_) => Some(format!("{rel} (missing)")),
            }
        }
        RequiredSource::Dir { path, suffix } => {
            let dir = sources_dir.join(path);
            let has_entry = std::fs::read_dir(&dir).ok().is_some_and(|mut entries| {
                entries.any(|e| {
                    e.ok().is_some_and(|e| {
                        e.file_name().to_string_lossy().ends_with(suffix)
                            && e.metadata().map(|m| m.len() > 0).unwrap_or(false)
                    })
                })
            });
            (!has_entry).then(|| format!("{path}/ (no non-empty *{suffix})"))
        }
    }
}

/// Check that every required source is present and non-empty.
///
/// When `allow_partial` is set, missing sources are logged as warnings and the
/// build proceeds (producing a partial catalog); otherwise the first missing
/// source aborts the build.
fn preflight_check(sources_dir: &Path, allow_partial: bool) -> Result<(), String> {
    let missing: Vec<String> = REQUIRED_SOURCES
        .iter()
        .filter_map(|source| missing_source_reason(sources_dir, source))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    let mut msg = String::new();
    msg.push_str(&format!(
        "{} required source(s) missing or empty under {}:\n",
        missing.len(),
        sources_dir.display(),
    ));
    for reason in &missing {
        msg.push_str(&format!("  - {reason}\n"));
    }
    msg.push_str(
        "\nRun the data download scripts to produce them:\n\
         \n  ./scripts/download-arcade-data.sh\n\
         \n  ./scripts/download-metadata.sh\n\
         \n  TGDB_API_KEY=... ./scripts/download-tgdb-lookups.sh\n\
         \n  python3 scripts/wikidata-series-extract.py > data/wikidata/series.json\n\
         \nmame0285-arcade.xml additionally needs `7z` (p7zip-full) and `python3` \
         installed on the build host; without them download-arcade-data.sh skips it \
         with a warning, producing a catalog that omits MAME 0.285 games.\n\
         \nThe TGDB lookups need a TheGamesDB API key — see scripts/.env / \
         download-tgdb-lookups.sh.\n\
         \nRelease builds use the committed data/wikidata/series.json snapshot and \
         should not query live Wikidata SPARQL.\n\
         \nPass --stub to build from replay-control-core/fixtures/ instead, or \
         --allow-partial to build a deliberately partial catalog anyway.\n",
    );

    if allow_partial {
        eprintln!("build-catalog: WARNING — building a PARTIAL catalog (--allow-partial)\n\n{msg}");
        Ok(())
    } else {
        Err(msg)
    }
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

    if !args.stub
        && let Err(msg) = preflight_check(&sources_dir, args.allow_partial)
    {
        eprintln!("build-catalog: ERROR\n\n{msg}");
        std::process::exit(1);
    }

    // Remove existing output file if it exists (fresh build)
    if args.output.exists() {
        std::fs::remove_file(&args.output).expect("Failed to remove existing catalog.sqlite");
    }

    let conn = Connection::open(&args.output).expect("Failed to open SQLite database");
    create_schema(&conn).expect("Failed to create schema");

    let tgdb_data = load_tgdb_data(&sources_dir);
    insert_arcade_games(&conn, &sources_dir).expect("Failed to insert arcade games");
    insert_console_games(&conn, &sources_dir, &tgdb_data).expect("Failed to insert console games");
    insert_amiga_games(&conn, &sources_dir, &tgdb_data).expect("Failed to insert Amiga games");
    populate_retroachievements(&conn, &sources_dir)
        .expect("Failed to populate RetroAchievements linkage");
    community::insert_community_entries(&conn, &sources_dir)
        .expect("Failed to insert community entries");
    insert_series(&conn, &sources_dir).expect("Failed to insert series");
    insert_catalog_resources(&conn, &sources_dir).expect("Failed to insert catalog resources");

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
    let enrichment_inputs_version = catalog_enrichment_inputs_version(&conn)
        .expect("Failed to compute catalog_enrichment_inputs_version");
    conn.execute(
        "INSERT INTO db_meta (key, value) VALUES ('catalog_enrichment_inputs_version', ?1)",
        params![enrichment_inputs_version],
    )
    .expect("Failed to insert catalog_enrichment_inputs_version");

    // Final stats
    let arcade_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM arcade_game", [], |r| r.get(0))
        .unwrap_or(0);
    let canonical_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM canonical_game", [], |r| r.get(0))
        .unwrap_or(0);
    let rom_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM rom_entry", [], |r| r.get(0))
        .unwrap_or(0);
    let series_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM series_entry", [], |r| r.get(0))
        .unwrap_or(0);
    let resource_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM catalog_game_resource", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);

    println!("catalog.sqlite written to {}", args.output.display());
    println!("  arcade_game:    {}", arcade_count);
    println!("  canonical_game: {}", canonical_count);
    println!("  rom_entry:     {}", rom_count);
    println!("  series_entry:  {}", series_count);
    println!("  game_resource: {}", resource_count);
    println!("  is_stub:         {}", is_stub);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_sources_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("build-catalog-test-{unique}"))
    }

    /// Write `content` to a uniquely-named temp file and return its path. The
    /// caller removes the parent dir (via `path.parent()`) when finished.
    fn write_temp_file(name: &str, content: &str) -> PathBuf {
        let dir = temp_sources_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    /// Materialize a complete, valid source tree from `REQUIRED_SOURCES`:
    /// a non-empty file for every `File`, and a dir holding one non-empty
    /// matching entry for every `Dir`.
    fn materialize_required_sources(dir: &Path) {
        for source in REQUIRED_SOURCES {
            match source {
                RequiredSource::File(rel) => {
                    let path = dir.join(rel);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(&path, b"x").unwrap();
                }
                RequiredSource::Dir { path, suffix } => {
                    let d = dir.join(path);
                    fs::create_dir_all(&d).unwrap();
                    fs::write(d.join(format!("entry{suffix}")), b"x").unwrap();
                }
            }
        }
    }

    #[test]
    fn preflight_passes_with_complete_sources() {
        let dir = temp_sources_dir();
        materialize_required_sources(&dir);
        assert!(preflight_check(&dir, false).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preflight_fails_on_missing_source_unless_allow_partial() {
        let dir = temp_sources_dir();
        materialize_required_sources(&dir);

        // A missing required file fails the strict check…
        fs::remove_file(dir.join("upstream/tgdb-developers.json")).unwrap();
        let err = preflight_check(&dir, false).unwrap_err();
        assert!(err.contains("tgdb-developers.json"), "{err}");

        // …and --allow-partial downgrades it to a warning.
        assert!(preflight_check(&dir, true).is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn preflight_rejects_empty_files_and_empty_collection_dirs() {
        let dir = temp_sources_dir();
        materialize_required_sources(&dir);

        // An empty required file is "not available".
        let empty = dir.join("shmups-wiki/games.json");
        fs::write(&empty, b"").unwrap();
        assert!(
            preflight_check(&dir, false)
                .unwrap_err()
                .contains("games.json")
        );
        fs::write(&empty, b"x").unwrap();

        // A collection dir with no matching entry is "not available".
        let nointro = dir.join("upstream/no-intro");
        fs::remove_dir_all(&nointro).unwrap();
        fs::create_dir_all(&nointro).unwrap();
        assert!(
            preflight_check(&dir, false)
                .unwrap_err()
                .contains("no-intro")
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_general_ref_decodes_named_and_numeric_entities() {
        // Predefined named entities.
        assert_eq!(resolve_general_ref(&BytesRef::new("amp")), "&");
        assert_eq!(resolve_general_ref(&BytesRef::new("apos")), "'");
        assert_eq!(resolve_general_ref(&BytesRef::new("quot")), "\"");
        assert_eq!(resolve_general_ref(&BytesRef::new("lt")), "<");
        assert_eq!(resolve_general_ref(&BytesRef::new("gt")), ">");
        // Numeric character references (hex and decimal both map to apostrophe).
        assert_eq!(resolve_general_ref(&BytesRef::new("#x27")), "'");
        assert_eq!(resolve_general_ref(&BytesRef::new("#39")), "'");
        // Unknown entity resolves to empty rather than panicking.
        assert_eq!(resolve_general_ref(&BytesRef::new("bogus")), "");
    }

    // Regression for issue #66: entity references in <description>/<d> text were
    // dropped along with their bordering spaces ("Dungeons & Dragons" became
    // "DungeonsDragons", "Warriors' Dreams" became "WarriorsDreams"), which broke
    // thumbnail filename matching for CPS2 games like sfa and ddsom.

    #[test]
    fn parse_mame_current_xml_preserves_entities_in_descriptions() {
        let path = write_temp_file(
            "mame-entities.xml",
            r#"<?xml version="1.0"?>
<mame version="0.285">
<m name="sfa" rotate="0" players="2" status="good"><d>Street Fighter Alpha: Warriors&#x27; Dreams (Europe 950727)</d><y>1995</y><f>Capcom</f></m>
<m name="ddsom" rotate="0" players="4" status="good"><d>Dungeons &amp; Dragons: Shadow over Mystara (Europe 960619)</d><y>1996</y><f>Capcom</f></m>
</mame>
"#,
        );
        let entries = parse_mame_current_xml(&path);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();

        let sfa = entries.iter().find(|e| e.rom_name == "sfa").unwrap();
        assert_eq!(
            sfa.display_name,
            "Street Fighter Alpha: Warriors' Dreams (Europe 950727)"
        );
        let ddsom = entries.iter().find(|e| e.rom_name == "ddsom").unwrap();
        assert_eq!(
            ddsom.display_name,
            "Dungeons & Dragons: Shadow over Mystara (Europe 960619)"
        );
    }

    #[test]
    fn parse_fbneo_dat_preserves_entities_in_descriptions() {
        let path = write_temp_file(
            "fbneo-entities.dat",
            r#"<?xml version="1.0"?>
<datafile>
<game name="ddsom" sourcefile="capcom/d_cps2.cpp">
<description>Dungeons &amp; Dragons: Shadow over Mystara (Europe 960619)</description>
<year>1996</year>
<manufacturer>Capcom</manufacturer>
</game>
<game name="entitydemo" sourcefile="capcom/d_cps2.cpp">
<description>Capcom&apos;s &quot;Best&quot; &lt;Demo&gt;</description>
<year>1996</year>
<manufacturer>Capcom</manufacturer>
</game>
</datafile>
"#,
        );
        let entries = parse_fbneo_dat(&path);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();

        let ddsom = entries.iter().find(|e| e.rom_name == "ddsom").unwrap();
        assert_eq!(
            ddsom.display_name,
            "Dungeons & Dragons: Shadow over Mystara (Europe 960619)"
        );
        let demo = entries.iter().find(|e| e.rom_name == "entitydemo").unwrap();
        assert_eq!(demo.display_name, r#"Capcom's "Best" <Demo>"#);
    }

    #[test]
    fn parse_mame2003plus_xml_preserves_entities_in_descriptions() {
        let path = write_temp_file(
            "mame2003plus-entities.xml",
            r#"<?xml version="1.0"?>
<mame>
<game name="ddsom" sourcefile="cps2.c">
<description>Dungeons &amp; Dragons: Shadow over Mystara (Euro 960619)</description>
<year>1996</year>
<manufacturer>Capcom</manufacturer>
<video orientation="horizontal"/>
<input players="4"/>
<driver status="good"/>
</game>
</mame>
"#,
        );
        let entries = parse_mame2003plus_xml(&path);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();

        let ddsom = entries.iter().find(|e| e.rom_name == "ddsom").unwrap();
        assert_eq!(
            ddsom.display_name,
            "Dungeons & Dragons: Shadow over Mystara (Euro 960619)"
        );
    }

    #[test]
    fn parse_whdload_db_missing_file_returns_empty() {
        // A `--stub` build reads the fixture tree, which doesn't bundle the
        // Amiga sources. The parser must skip gracefully (like the Amiga DAT
        // loop) rather than panic. The real build is protected by the strict
        // preflight, which requires whdload_db.xml up front.
        let path = Path::new("/nonexistent/whdload_db.xml");
        assert!(parse_whdload_db(path).is_empty());
    }

    #[test]
    fn parse_whdload_db_preserves_entities_in_names() {
        // Real whdload_db.xml entries: the <name> previously truncated at the first
        // text chunk, so "4th & Inches" became "4th" (the entity ended the insert).
        let path = write_temp_file(
            "whdload-entities.xml",
            r#"<?xml version="1.0"?>
<whdbooter>
<game filename="4th&amp;Inches_v1.1_1230" sha1="37e24f5e72fbc8fc518360e8b1dd2ed160e145b7">
<name>4th &amp; Inches</name>
</game>
<game filename="1000ccTurbo_v1.0" sha1="6997ff430c6381d7fe46b78ea37a6a2c0bdcdb71">
<name>1000cc Turbo</name>
</game>
</whdbooter>
"#,
        );
        let map = parse_whdload_db(&path);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();

        assert_eq!(
            map.get("4th&Inches_v1.1_1230").map(String::as_str),
            Some("4th & Inches")
        );
        // Plain name (no entity) still round-trips.
        assert_eq!(
            map.get("1000ccTurbo_v1.0").map(String::as_str),
            Some("1000cc Turbo")
        );
    }

    #[test]
    fn insert_arcade_games_retains_full_mame_categories() {
        let dir = temp_sources_dir();
        // Downloaded inputs (MAME XML, catver) live under upstream/ — mirror the
        // production layout the loaders read from.
        let up = upstream(&dir);
        fs::create_dir_all(&up).unwrap();
        fs::write(
            up.join("mame0285-arcade.xml"),
            r#"<?xml version="1.0"?>
<mame version="0.285">
<m name="ssipkr30" cloneof="ssipkr24" rotate="0" players="1" status="good"><d>SSI Poker (v3.0)</d><y>1988</y><f>SSI</f></m>
<m name="100lions" rotate="0" players="1" status="good"><d>100 Lions</d><y>2006</y><f>Aristocrat</f></m>
<m name="apple2gsr0p" rotate="0" status="good"><d>Apple IIgs (ROM00 prototype)</d><y>1986</y><f>Apple</f></m>
</mame>
"#,
        )
        .unwrap();
        fs::write(
            up.join("catver-mame-current.ini"),
            "[Category]\nssipkr30=Gambling / Cards\n100lions=Slot Machine / Video Slot\napple2gsr0p=Computer / Home System\n",
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        insert_arcade_games(&conn, &dir).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM arcade_game WHERE rom_name IN ('ssipkr30', '100lions', 'apple2gsr0p')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        let genre: String = conn
            .query_row(
                "SELECT normalized_genre FROM arcade_game WHERE rom_name = 'ssipkr30'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(genre, "Board & Card");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn insert_catalog_resources_includes_shmups_wiki_links() {
        let dir = temp_sources_dir();
        fs::create_dir_all(dir.join("shmups-wiki")).unwrap();
        fs::write(
            dir.join("shmups-wiki/games.json"),
            r#"[
  {"normalized_title":"battlegaregga","page_title":"Battle Garegga","video_index":true},
  {"normalized_title":"rtypefinal2trial","page_title":"R-Type Final 2 (Trial)"}
]"#,
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        insert_catalog_resources(&conn, &dir).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_game_resource WHERE source = ?1",
                [resource_kind::SHMUPS_WIKI_SOURCE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        let url: String = conn
            .query_row(
                "SELECT url FROM catalog_game_resource
                 WHERE normalized_title = 'rtypefinal2trial'
                   AND resource_type = ?1",
                [resource_kind::STRATEGY_GUIDE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(url, "https://shmups.wiki/library/R-Type_Final_2_(Trial)");

        let video_url: String = conn
            .query_row(
                "SELECT url FROM catalog_game_resource
                 WHERE normalized_title = 'battlegaregga'
                   AND resource_type = ?1",
                [resource_kind::VIDEO_INDEX],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            video_url,
            "https://shmups.wiki/library/Battle_Garegga/Video_Index"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn catalog_enrichment_inputs_version_changes_when_description_changes() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO canonical_game \
             (system, display_name, description, publisher) \
             VALUES ('nintendo_snes', 'Super Mario World', 'old description', 'old publisher')",
            [],
        )
        .unwrap();
        let canonical_game_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO rom_entry \
             (system, filename_stem, canonical_game_id, normalized_title) \
             VALUES ('nintendo_snes', 'Super Mario World', ?1, 'super mario world')",
            [canonical_game_id],
        )
        .unwrap();

        let before = catalog_enrichment_inputs_version(&conn).unwrap();
        conn.execute(
            "UPDATE canonical_game SET description = 'new description'",
            [],
        )
        .unwrap();
        let after = catalog_enrichment_inputs_version(&conn).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn catalog_enrichment_inputs_version_changes_when_publisher_changes() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO canonical_game \
             (system, display_name, publisher) \
             VALUES ('commodore_ami', 'AmigaVision', 'old publisher')",
            [],
        )
        .unwrap();
        let canonical_game_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO rom_entry \
             (system, filename_stem, canonical_game_id, normalized_title) \
             VALUES ('commodore_ami', 'AmigaVision', ?1, 'amigavision')",
            [canonical_game_id],
        )
        .unwrap();

        let before = catalog_enrichment_inputs_version(&conn).unwrap();
        conn.execute("UPDATE canonical_game SET publisher = 'new publisher'", [])
            .unwrap();
        let after = catalog_enrichment_inputs_version(&conn).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn stamp_whole_file_rom_entry_ra_ids_matches_by_md5() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO canonical_game (system, display_name) VALUES ('nintendo_gb', 'Tetris')",
            [],
        )
        .unwrap();
        let cg = conn.last_insert_rowid();
        // Whole-file cart: its full-file md5 equals the RA hash → should be stamped.
        conn.execute(
            "INSERT INTO rom_entry (system, filename_stem, canonical_game_id, md5) \
             VALUES ('nintendo_gb', 'Tetris (World)', ?1, 'abc123')",
            [cg],
        )
        .unwrap();
        // A dump with no md5 (or a non-matching one) must stay empty — precision.
        conn.execute(
            "INSERT INTO rom_entry (system, filename_stem, canonical_game_id, md5) \
             VALUES ('nintendo_gb', 'Tetris (Headerless)', ?1, 'nomatch')",
            [cg],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ra_hash (system, hash, ra_id) VALUES ('nintendo_gb', 'abc123', '999')",
            [],
        )
        .unwrap();

        let stamped = stamp_whole_file_rom_entry_ra_ids(&conn).unwrap();
        assert_eq!(stamped, 1);

        let matched: String = conn
            .query_row(
                "SELECT ra_id FROM rom_entry WHERE filename_stem = 'Tetris (World)'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(matched, "999");
        let unmatched: String = conn
            .query_row(
                "SELECT ra_id FROM rom_entry WHERE filename_stem = 'Tetris (Headerless)'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unmatched, "");
    }

    #[test]
    fn catalog_enrichment_inputs_version_changes_when_ra_id_changes() {
        let conn = Connection::open_in_memory().unwrap();
        create_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO canonical_game \
             (system, display_name) \
             VALUES ('nintendo_snes', 'Super Mario World')",
            [],
        )
        .unwrap();
        let canonical_game_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO rom_entry \
             (system, filename_stem, canonical_game_id, normalized_title) \
             VALUES ('nintendo_snes', 'Super Mario World', ?1, 'super mario world')",
            [canonical_game_id],
        )
        .unwrap();

        // Refreshing the RA extract assigns an id (per-dump, on rom_entry) with no
        // schema change; the version must change so the per-storage stamp goes
        // stale and re-enrich runs on next boot.
        let before = catalog_enrichment_inputs_version(&conn).unwrap();
        conn.execute("UPDATE rom_entry SET ra_id = '228'", [])
            .unwrap();
        let after = catalog_enrichment_inputs_version(&conn).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn tosec_display_suffix_appends_region_and_edition() {
        // Region code appended
        assert_eq!(tosec_display_suffix("Nitro (1990)(Psygnosis)(US)"), " (US)");
        // No qualifying groups → empty suffix
        assert_eq!(tosec_display_suffix("Nitro (1990)(Psygnosis)"), "");
        // Region after disk-count: disk stripped, region kept
        assert_eq!(
            tosec_display_suffix("Batman - The Movie (1989)(Ocean)(PAL)(Disk 1 of 2)"),
            " (PAL)"
        );
        assert_eq!(
            tosec_display_suffix("Killing Game Show, The (1990)(Psygnosis)(Disk 1 of 2)(US)"),
            " (US)"
        );
        // Hardware tag stripped, not a region
        assert_eq!(
            tosec_display_suffix("Blobz (1996)(Apex Systems)(AGA)(Disk 1 of 2)"),
            ""
        );
        // Language code stripped
        assert_eq!(
            tosec_display_suffix("1000 Miglia (1992)(Simulmondo)(en-it)"),
            ""
        );
        // Nordic region codes
        assert_eq!(
            tosec_display_suffix("Hugo (1994)(ITE)(SE)(Disk 1 of 8)"),
            " (SE)"
        );
        assert_eq!(
            tosec_display_suffix("Skaermtrolden Hugo v1.20 (1991)(ITE)(DK)(Disk 1 of 3)"),
            " (DK)"
        );
        // Edition tag (Updated) is kept
        assert_eq!(
            tosec_display_suffix("A320 Airbus (1991)(Thalion)(Updated)"),
            " (Updated)"
        );
        // No edition tag on the base variant
        assert_eq!(tosec_display_suffix("A320 Airbus (1991)(Thalion)"), "");
        // Region + edition both appended
        assert_eq!(
            tosec_display_suffix("Game (1990)(Dev)(US)(Updated)"),
            " (US) (Updated)"
        );
        // Rev tag kept
        assert_eq!(tosec_display_suffix("Game (1990)(Dev)(Rev A)"), " (Rev A)");
    }

    #[test]
    fn clean_display_name_strips_tosec_version_and_brackets() {
        // TOSEC version suffix before paren
        assert_eq!(
            clean_display_name("'Nam 1965-1975 v1.0 (Europe)"),
            "'Nam 1965-1975"
        );
        // TOSEC slot number in brackets only
        assert_eq!(
            clean_display_name("'Nam 1965-1975 [0249]"),
            "'Nam 1965-1975"
        );
        // TOSEC crack/hack tags combined with version
        assert_eq!(
            clean_display_name("'Nam 1965-1975 v1.0 [cr QTX][h BTL]"),
            "'Nam 1965-1975"
        );
        // Standard No-Intro region tag (existing behaviour preserved)
        assert_eq!(clean_display_name("1000cc Turbo (Europe)"), "1000cc Turbo");
        // Article inversion still works
        assert_eq!(
            clean_display_name("Legend of Zelda, The (USA)"),
            "The Legend of Zelda"
        );
        // No version: untouched beyond paren strip
        assert_eq!(clean_display_name("Lemmings (Europe)"), "Lemmings");
        // Lowercase 'v' not preceded by space — not treated as version
        assert_eq!(clean_display_name("v-rally (Europe)"), "v-rally");
    }
}
