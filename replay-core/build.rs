use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("arcade_db.rs");
    let mut out = BufWriter::new(File::create(&dest_path).unwrap());

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let data_dir = Path::new(&manifest_dir).join("data").join("arcade");

    // Rerun if any data file changes
    println!(
        "cargo::rerun-if-changed={}",
        data_dir.join("flycast_games.csv").display()
    );

    // Collect all game entries from all source files
    let mut entries: Vec<GameEntry> = Vec::new();

    // Parse Flycast (Naomi/Atomiswave) games
    let flycast_path = data_dir.join("flycast_games.csv");
    if flycast_path.exists() {
        parse_csv(&flycast_path, &mut entries);
    }

    // Future: parse FBNeo, MAME 2003+, MAME current data files here
    // Each parser appends to the same `entries` vec.
    // Deduplication would happen here if needed (by rom_name, preferring richer metadata).

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

fn parse_csv(path: &Path, entries: &mut Vec<GameEntry>) {
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
        "preliminary" => "DriverStatus::Preliminary",
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
