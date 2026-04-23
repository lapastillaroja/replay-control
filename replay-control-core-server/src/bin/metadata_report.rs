//! Metadata coverage report tool.
//!
//! Scans all systems with ROMs on a storage device and reports per-system,
//! per-field coverage for both embedded metadata (game_db / arcade_db) and
//! external metadata (LaunchBox import in metadata.db).
//!
//! Usage:
//!   cargo run --bin metadata_report --features metadata -- --storage-path /path/to/storage

use std::path::PathBuf;

use replay_control_core::rom_tags;
use replay_control_core::systems;
use replay_control_core::title_utils;
use replay_control_core_server::arcade_db;
use replay_control_core_server::game_db;
use replay_control_core_server::launchbox;
use replay_control_core_server::metadata_db::MetadataDb;
use replay_control_core_server::roms;
use replay_control_core_server::storage::{StorageKind, StorageLocation};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (storage_path, import_xml) = parse_args();
    let storage = StorageLocation::from_path(storage_path, StorageKind::Usb);

    // Optional: re-import LaunchBox metadata before generating the report.
    if let Some(xml_path) = import_xml {
        eprintln!("Building ROM index...");
        let rom_index = launchbox::build_rom_index(&storage.root).await;
        eprintln!("ROM index: {} entries", rom_index.len());

        eprintln!("Opening metadata DB...");
        let (mut conn, _db_path) =
            MetadataDb::open(&storage.root).expect("Failed to open metadata DB");

        eprintln!("Importing LaunchBox XML from {}...", xml_path.display());
        let (stats, _parse_result) = launchbox::import_launchbox(
            &xml_path,
            &rom_index,
            |total, matched, inserted| {
                eprint!("\r  Progress: {total} scanned, {matched} matched, {inserted} inserted");
            },
            |batch| MetadataDb::bulk_upsert(&mut conn, batch),
        )
        .expect("Import failed");

        eprintln!(
            "\nImport complete: {} source, {} matched, {} inserted, {} skipped",
            stats.total_source, stats.matched, stats.inserted, stats.skipped
        );
    }

    // Open external metadata DB (may not exist yet).
    let meta_conn = MetadataDb::open(&storage.root).ok().map(|(c, _)| c);

    let summaries = roms::scan_systems(&storage).await;
    let active: Vec<_> = summaries.iter().filter(|s| s.game_count > 0).collect();

    if active.is_empty() {
        eprintln!(
            "No systems with games found at {}",
            storage.roms_dir().display()
        );
        std::process::exit(1);
    }

    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                        METADATA COVERAGE REPORT                             ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    println!("║ Storage: {:<67}║", storage.root.display());
    println!("║ Systems with games: {:<56}║", active.len());
    println!(
        "║ Total ROMs: {:<64}║",
        active.iter().map(|s| s.game_count).sum::<usize>()
    );
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let mut grand_totals = GrandTotals::default();

    for summary in &active {
        let system_name = &summary.folder_name;
        let is_arcade = systems::is_arcade_system(system_name);

        let rom_list = match roms::list_roms(
            &storage,
            system_name,
            rom_tags::RegionPreference::default(),
            None,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  Error listing ROMs for {system_name}: {e}");
                continue;
            }
        };

        if rom_list.is_empty() {
            continue;
        }

        let total = rom_list.len();
        let mut embedded = EmbeddedCoverage::default();
        let mut external = ExternalCoverage::default();

        let arcade_batch = if is_arcade {
            let stems: Vec<&str> = rom_list
                .iter()
                .map(|r| title_utils::filename_stem(r.game.rom_filename.as_str()))
                .collect();
            arcade_db::lookup_arcade_games_batch(&stems).await
        } else {
            Default::default()
        };
        let (game_by_stem, game_by_norm) = if is_arcade {
            Default::default()
        } else {
            let stems: Vec<&str> = rom_list
                .iter()
                .map(|r| title_utils::filename_stem(r.game.rom_filename.as_str()))
                .collect();
            let by_stem = game_db::lookup_games_batch(system_name, &stems).await;
            let missing_norms: Vec<String> = rom_list
                .iter()
                .filter_map(|r| {
                    let f = r.game.rom_filename.as_str();
                    let stem = title_utils::filename_stem(f);
                    if by_stem.contains_key(stem) {
                        return None;
                    }
                    let n = game_db::normalize_filename(stem);
                    (!n.is_empty()).then_some(n)
                })
                .collect();
            let norm_refs: Vec<&str> = missing_norms.iter().map(String::as_str).collect();
            let by_norm = game_db::lookup_by_normalized_titles_batch(system_name, &norm_refs).await;
            (by_stem, by_norm)
        };

        for rom in &rom_list {
            let filename = &rom.game.rom_filename;

            // --- Embedded metadata ---
            if is_arcade {
                let rom_name = title_utils::filename_stem(filename);
                if let Some(info) = arcade_batch.get(rom_name) {
                    embedded.display_name += 1;
                    if !info.year.is_empty() {
                        embedded.year += 1;
                    }
                    if !info.manufacturer.is_empty() {
                        embedded.developer += 1; // manufacturer maps to developer slot
                    }
                    if info.players > 0 {
                        embedded.players += 1;
                    }
                    if !info.category.is_empty() {
                        embedded.genre += 1;
                    }
                    // Arcade-specific fields
                    embedded.region += 1; // rotation (reusing region slot for rotation)
                    if !info.normalized_genre.is_empty() {
                        embedded.normalized_genre += 1;
                    }
                    embedded.any += 1;
                }
            } else {
                // Try exact match first, then normalized
                let stem = title_utils::filename_stem(filename);

                let entry = game_by_stem.get(stem);
                let has_region = entry.is_some_and(|e| !e.region.is_empty());

                // Also try normalized for CanonicalGame (which has more fields)
                let canonical = match entry {
                    Some(e) => Some(&e.game),
                    None => {
                        let normalized = game_db::normalize_filename(stem);
                        if normalized.is_empty() {
                            None
                        } else {
                            game_by_norm.get(&normalized)
                        }
                    }
                };

                if let Some(game) = canonical {
                    embedded.display_name += 1;
                    if game.year > 0 {
                        embedded.year += 1;
                    }
                    if !game.genre.is_empty() {
                        embedded.genre += 1;
                    }
                    if !game.developer.is_empty() {
                        embedded.developer += 1;
                    }
                    if game.players > 0 {
                        embedded.players += 1;
                    }
                    if !game.normalized_genre.is_empty() {
                        embedded.normalized_genre += 1;
                    }
                    embedded.any += 1;
                }

                if has_region {
                    embedded.region += 1;
                }
            }

            // --- External metadata (SQLite) ---
            if let Some(ref conn) = meta_conn
                && let Ok(Some(meta)) = MetadataDb::lookup(conn, system_name, filename)
            {
                if meta.description.as_ref().is_some_and(|d| !d.is_empty()) {
                    external.description += 1;
                }
                if meta.rating.is_some() {
                    external.rating += 1;
                }
                if meta.publisher.as_ref().is_some_and(|p| !p.is_empty()) {
                    external.publisher += 1;
                }
                if meta.box_art_path.is_some() {
                    external.box_art += 1;
                }
                if meta.screenshot_path.is_some() {
                    external.screenshot += 1;
                }
                external.any += 1;
            }
        }

        // Print system report
        println!(
            "┌──────────────────────────────────────────────────────────────────────────────┐"
        );
        println!(
            "│ {:<40} {:>5} ROMs {:>20} │",
            summary.display_name,
            total,
            if is_arcade { "(arcade)" } else { "" }
        );
        println!(
            "├──────────────────────────────────────────────────────────────────────────────┤"
        );

        // Embedded section
        println!("│  EMBEDDED (compiled game_db / arcade_db)                                    │");
        println!(
            "│  ─────────────────────────────────────────────────                           │"
        );

        print_field("│", "Display Name", embedded.display_name, total);
        print_field("│", "Year", embedded.year, total);
        print_field(
            "│",
            if is_arcade { "Category" } else { "Genre" },
            embedded.genre,
            total,
        );
        print_field(
            "│",
            if is_arcade {
                "Manufacturer"
            } else {
                "Developer"
            },
            embedded.developer,
            total,
        );
        print_field("│", "Players", embedded.players, total);
        if !is_arcade {
            print_field("│", "Region", embedded.region, total);
        }
        print_field("│", "Normalized Genre", embedded.normalized_genre, total);
        print_field_bold("│", "Any Embedded", embedded.any, total);

        println!(
            "│                                                                              │"
        );

        // External section
        println!(
            "│  EXTERNAL (LaunchBox / libretro-thumbnails)                                  │"
        );
        println!(
            "│  ─────────────────────────────────────────────────────                        │"
        );
        print_field("│", "Description", external.description, total);
        print_field("│", "Rating", external.rating, total);
        print_field("│", "Publisher", external.publisher, total);
        print_field("│", "Box Art", external.box_art, total);
        print_field("│", "Screenshot", external.screenshot, total);
        print_field_bold("│", "Any External", external.any, total);

        println!(
            "└──────────────────────────────────────────────────────────────────────────────┘"
        );
        println!();

        // Accumulate grand totals
        grand_totals.total_roms += total;
        grand_totals.embedded.add(&embedded);
        grand_totals.external.add(&external);
    }

    // Print grand totals
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                              GRAND TOTALS                                   ║");
    println!("╠══════════════════════════════════════════════════════════════════════════════╣");
    let t = grand_totals.total_roms;
    println!("║  Total ROMs: {:<63}║", t);
    println!("║                                                                              ║");
    println!("║  EMBEDDED                                                                    ║");
    print_field_boxed("Display Name", grand_totals.embedded.display_name, t);
    print_field_boxed("Year", grand_totals.embedded.year, t);
    print_field_boxed("Genre / Category", grand_totals.embedded.genre, t);
    print_field_boxed("Developer / Mfr", grand_totals.embedded.developer, t);
    print_field_boxed("Players", grand_totals.embedded.players, t);
    print_field_boxed("Region", grand_totals.embedded.region, t);
    print_field_boxed(
        "Normalized Genre",
        grand_totals.embedded.normalized_genre,
        t,
    );
    print_field_boxed_bold("Any Embedded", grand_totals.embedded.any, t);
    println!("║                                                                              ║");
    println!("║  EXTERNAL                                                                    ║");
    print_field_boxed("Description", grand_totals.external.description, t);
    print_field_boxed("Rating", grand_totals.external.rating, t);
    print_field_boxed("Publisher", grand_totals.external.publisher, t);
    print_field_boxed("Box Art", grand_totals.external.box_art, t);
    print_field_boxed("Screenshot", grand_totals.external.screenshot, t);
    print_field_boxed_bold("Any External", grand_totals.external.any, t);
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
}

fn parse_args() -> (PathBuf, Option<PathBuf>) {
    let args: Vec<String> = std::env::args().collect();
    let mut storage_path = None;
    let mut import_xml = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--storage-path" | "-s" => {
                i += 1;
                if i < args.len() {
                    storage_path = Some(PathBuf::from(&args[i]));
                }
            }
            "--import" => {
                i += 1;
                if i < args.len() {
                    import_xml = Some(PathBuf::from(&args[i]));
                }
            }
            "--help" | "-h" => {
                eprintln!("Usage: metadata_report --storage-path <PATH> [--import <XML>]");
                eprintln!();
                eprintln!("Generates a metadata coverage report for all systems with ROMs.");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  -s, --storage-path <PATH>  Path to storage root (e.g., /media/usb)");
                eprintln!(
                    "      --import <XML>         Import LaunchBox metadata XML before report"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                eprintln!("Usage: metadata_report --storage-path <PATH> [--import <XML>]");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let path = storage_path.unwrap_or_else(|| {
        eprintln!("Error: --storage-path is required");
        eprintln!("Usage: metadata_report --storage-path <PATH> [--import <XML>]");
        std::process::exit(1);
    });
    (path, import_xml)
}

fn pct(n: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        100.0 * n as f64 / total as f64
    }
}

fn bar(n: usize, total: usize) -> String {
    let width = 20;
    let filled = (width * n).checked_div(total).unwrap_or(0);
    let empty = width - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn print_field(prefix: &str, label: &str, count: usize, total: usize) {
    println!(
        "{prefix}    {:<20} {:>5}/{:<5} {:>5.1}%  {}                  {prefix}",
        label,
        count,
        total,
        pct(count, total),
        bar(count, total)
    );
}

fn print_field_bold(prefix: &str, label: &str, count: usize, total: usize) {
    println!(
        "{prefix}    \x1b[1m{:<20} {:>5}/{:<5} {:>5.1}%  {}\x1b[0m                  {prefix}",
        label,
        count,
        total,
        pct(count, total),
        bar(count, total)
    );
}

fn print_field_boxed(label: &str, count: usize, total: usize) {
    println!(
        "║    {:<20} {:>5}/{:<5} {:>5.1}%  {}                  ║",
        label,
        count,
        total,
        pct(count, total),
        bar(count, total)
    );
}

fn print_field_boxed_bold(label: &str, count: usize, total: usize) {
    println!(
        "║    \x1b[1m{:<20} {:>5}/{:<5} {:>5.1}%  {}\x1b[0m                  ║",
        label,
        count,
        total,
        pct(count, total),
        bar(count, total)
    );
}

#[derive(Default)]
struct EmbeddedCoverage {
    display_name: usize,
    year: usize,
    genre: usize,
    developer: usize,
    players: usize,
    region: usize,
    normalized_genre: usize,
    any: usize,
}

impl EmbeddedCoverage {
    fn add(&mut self, other: &Self) {
        self.display_name += other.display_name;
        self.year += other.year;
        self.genre += other.genre;
        self.developer += other.developer;
        self.players += other.players;
        self.region += other.region;
        self.normalized_genre += other.normalized_genre;
        self.any += other.any;
    }
}

#[derive(Default)]
struct ExternalCoverage {
    description: usize,
    rating: usize,
    publisher: usize,
    box_art: usize,
    screenshot: usize,
    any: usize,
}

impl ExternalCoverage {
    fn add(&mut self, other: &Self) {
        self.description += other.description;
        self.rating += other.rating;
        self.publisher += other.publisher;
        self.box_art += other.box_art;
        self.screenshot += other.screenshot;
        self.any += other.any;
    }
}

#[derive(Default)]
struct GrandTotals {
    total_roms: usize,
    embedded: EmbeddedCoverage,
    external: ExternalCoverage,
}
