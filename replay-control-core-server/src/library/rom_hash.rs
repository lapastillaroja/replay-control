//! ROM hash-based identification using CRC32.
//!
//! Computes CRC32 hashes of cartridge-based ROM files at scan time and matches
//! them against the embedded No-Intro DAT data to get definitive ROM identification.
//! CD-based, computer/folder, and arcade systems are excluded (see [`is_hash_eligible`]).
//!
//! The hash result is cached in the `game_library` table keyed by file mtime
//! and size, so unchanged files do not need to be re-hashed on subsequent scans.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::game_db;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HashOptions {
    pub force_rehash: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HashStats {
    pub reused_exact: usize,
    pub reused_migrated: usize,
    pub reused_size_only: usize,
    pub computed: usize,
    pub forced_computed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Default)]
pub struct HashIdentifyResult {
    pub results: Vec<HashResult>,
    pub stats: HashStats,
}

/// Systems eligible for CRC32 hash-based identification.
///
/// These are cartridge-based systems with single-file ROMs that have
/// corresponding No-Intro DAT data compiled into the binary (i.e., they
/// appear in `GAME_DB_SYSTEMS` in build.rs and have a `_CRC_INDEX` PHF map).
///
/// Excluded categories:
/// - CD systems (PSX, Saturn, Sega CD, PCE-CD, Dreamcast, 3DO, CD-i, Neo Geo CD)
/// - Computer/folder systems (ScummVM, DOS/IBM PC, Sharp X68000, Amiga, C64, Amstrad)
///   except MSX cartridge images, which are small single-file ROMs with
///   No-Intro CRC data.
/// - Arcade (MAME, FBNeo — identified by romset name, not file content)
/// - Nintendo DS (excluded for now — ROMs average 64 MB, first-scan too slow)
struct HashSystemRule {
    system: &'static str,
    extensions: &'static [&'static str],
    excluded_name_markers: &'static [&'static str],
}

const HASH_SYSTEM_RULES: &[HashSystemRule] = &[
    HashSystemRule {
        system: "nintendo_nes",
        extensions: &["nes", "unif", "unf", "fds"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "nintendo_snes",
        extensions: &["smc", "sfc", "swc", "fig", "bs", "st"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "nintendo_gb",
        extensions: &["gb", "sgb"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "nintendo_gbc",
        extensions: &["gbc", "sgbc"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "nintendo_gba",
        extensions: &["gba"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "nintendo_n64",
        extensions: &["z64", "n64", "v64", "bin", "u1"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "sega_sms",
        extensions: &["sms"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "sega_smd",
        extensions: &["md", "bin", "gen", "smd"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "sega_gg",
        extensions: &["gg"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "sega_sg",
        extensions: &["sg"],
        excluded_name_markers: &[],
    },
    HashSystemRule {
        system: "sega_32x",
        extensions: &["32x", "bin"],
        excluded_name_markers: &["sega cd 32x", "mega-cd 32x"],
    },
    HashSystemRule {
        system: "microsoft_msx",
        extensions: &["rom", "mx1", "mx2"],
        excluded_name_markers: &[],
    },
];

fn hash_rule(system: &str) -> Option<&'static HashSystemRule> {
    HASH_SYSTEM_RULES.iter().find(|rule| rule.system == system)
}

/// Check whether a system is eligible for CRC32 hash-based identification.
///
/// Returns `true` for cartridge-based systems that have No-Intro CRC index data
/// compiled into the binary. Returns `false` for CD, computer, arcade, and
/// systems without DAT coverage.
pub fn is_hash_eligible(system: &str) -> bool {
    hash_rule(system).is_some()
}

/// Check whether this specific ROM file should be CRC-identified.
///
/// Some systems are hybrid at the folder level. `sega_32x` contains both
/// cartridge ROMs and Sega CD 32X disc images; only the cartridge-shaped files
/// should be streamed for No-Intro CRC matching.
pub fn is_file_hash_eligible(system: &str, rom_filename: &str) -> bool {
    let Some(rule) = hash_rule(system) else {
        return false;
    };

    let ext = Path::new(rom_filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !rule.extensions.contains(&ext.as_str()) {
        return false;
    }

    let name = rom_filename.to_ascii_lowercase();
    !rule
        .excluded_name_markers
        .iter()
        .any(|marker| name.contains(marker))
}

/// Compute the CRC32 hash of a file using a streaming buffered reader.
///
/// Uses a 64 KB buffer to avoid loading the entire file into memory.
/// The CRC32 computation itself is negligible compared to I/O time.
pub fn compute_crc32(path: &Path) -> io::Result<u32> {
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    let header_skip = detect_header_skip(path);
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);

    // Skip ROM header bytes if needed (e.g., iNES 16-byte header).
    if header_skip > 0 {
        let mut skip_buf = vec![0u8; header_skip];
        reader.read_exact(&mut skip_buf)?;
    }

    let mut hasher = crc32fast::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize())
}

/// Detect how many header bytes to skip before hashing, based on file extension.
///
/// Some ROM formats have headers that are not part of the canonical ROM data:
/// - NES: 16-byte iNES/NES 2.0 header (all .nes files)
/// - Atari Lynx: 64-byte Handy header (detected by magic bytes, but we use
///   extension-based heuristic since nearly all .lnx files have it)
///
/// For N64 ROMs (.v64, .n64 byte-swapped formats), we hash as-is since No-Intro
/// DATs include CRCs for the native .z64 big-endian format. Byte-swapped ROMs
/// will simply not match, which is acceptable (they fall through to filename
/// matching). Full byte-order normalization can be added later if needed.
fn detect_header_skip(path: &Path) -> usize {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "nes" => 16, // iNES / NES 2.0 header
        "lnx" => 64, // Atari Lynx Handy header
        _ => 0,
    }
}

/// Result of hashing and identifying a single ROM file.
#[derive(Debug, Clone)]
pub struct HashResult {
    /// The ROM filename (not path).
    pub rom_filename: String,
    /// Computed CRC32 hash.
    pub crc32: u32,
    /// File mtime as seconds since UNIX epoch.
    pub mtime_secs: i64,
    /// File size observed when the CRC32 was computed or reused.
    pub size_bytes: u64,
    /// No-Intro canonical name if CRC matched, None if no match.
    pub matched_name: Option<String>,
}

/// Cached hash data from the database.
#[derive(Debug, Clone)]
pub struct CachedHash {
    pub crc32: u32,
    pub hash_mtime: i64,
    pub hash_size_bytes: Option<u64>,
    pub matched_name: Option<String>,
}

/// Reuse a cached hash when the current file identity still matches.
///
/// This is the shared discovery-time cache rule: exact size+mtime is best,
/// legacy rows without `hash_size_bytes` can reuse by mtime, and the existing
/// conservative same-size path avoids rehashing when only mtime drifted.
pub fn reusable_cached_hash(
    rom_filename: &str,
    cached: &CachedHash,
    current_mtime: i64,
    current_size: u64,
) -> Option<HashResult> {
    let reusable = match cached.hash_size_bytes {
        Some(cached_size) => cached_size == current_size,
        None => cached.hash_mtime == current_mtime,
    };
    reusable.then(|| HashResult {
        rom_filename: rom_filename.to_string(),
        crc32: cached.crc32,
        mtime_secs: current_mtime,
        size_bytes: current_size,
        matched_name: cached.matched_name.clone(),
    })
}

/// Hash and identify a batch of ROM files for a single system.
///
/// For each ROM file:
/// 1. Check if a cached hash exists and the mtime matches (skip rehashing)
/// 2. If stale or missing: read the file, compute CRC32, look up in the
///    system's CRC index
/// 3. Return results for all files (both cached and freshly computed)
///
/// The `cached_hashes` map is keyed by rom_filename and contains previously
/// stored hash data from the database.
///
/// The `rom_dir_root` is the parent of the storage root, used to resolve
/// `rom_path` to an absolute filesystem path.
pub async fn hash_and_identify(
    system: &str,
    rom_files: &[(String, String, u64)], // (rom_filename, rom_path, size_bytes)
    cached_hashes: &std::collections::HashMap<String, CachedHash>,
    storage_root: &Path,
) -> HashIdentifyResult {
    if !is_hash_eligible(system) {
        return HashIdentifyResult::default();
    }

    hash_and_identify_with_options(
        system,
        rom_files,
        cached_hashes,
        storage_root,
        HashOptions::default(),
    )
    .await
}

pub async fn hash_and_identify_with_options(
    system: &str,
    rom_files: &[(String, String, u64)], // (rom_filename, rom_path, size_bytes)
    cached_hashes: &std::collections::HashMap<String, CachedHash>,
    storage_root: &Path,
    options: HashOptions,
) -> HashIdentifyResult {
    hash_and_identify_with_options_and_cancel(
        system,
        rom_files,
        cached_hashes,
        storage_root,
        options,
        None,
    )
    .await
}

pub async fn hash_and_identify_with_options_and_cancel(
    system: &str,
    rom_files: &[(String, String, u64)], // (rom_filename, rom_path, size_bytes)
    cached_hashes: &std::collections::HashMap<String, CachedHash>,
    storage_root: &Path,
    options: HashOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> HashIdentifyResult {
    if !is_hash_eligible(system) {
        return HashIdentifyResult::default();
    }

    enum Pending {
        Cached(HashResult),
        NeedsLookup {
            rom_filename: String,
            crc32: u32,
            mtime_secs: i64,
            size_bytes: u64,
        },
    }

    enum Reuse {
        Exact,
        Migrated,
        SizeOnly,
    }

    // Phase 1 does per-file std::fs::metadata + File::open + read of
    // potentially megabytes of ROM content. Keep it off tokio workers.
    let rom_files_owned = rom_files.to_vec();
    let cached_hashes_owned = cached_hashes.clone();
    let storage_root_owned = storage_root.to_path_buf();
    let system_owned = system.to_string();
    let cancel_owned = cancel.clone();
    let (pending, stats): (Vec<Pending>, HashStats) = {
        {
            tokio::task::spawn_blocking(move || {
                let mut pending = Vec::with_capacity(rom_files_owned.len());
                let mut stats = HashStats::default();
                for (rom_filename, rom_path, size_bytes) in &rom_files_owned {
                    if cancel_owned
                        .as_ref()
                        .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
                    {
                        break;
                    }
                    if !is_file_hash_eligible(&system_owned, rom_filename) {
                        stats.skipped += 1;
                        continue;
                    }

                    let abs_path = storage_root_owned.join(rom_path.trim_start_matches('/'));

                    let Some(current_mtime) = file_mtime_secs(&abs_path) else {
                        stats.skipped += 1;
                        continue;
                    };

                    if !options.force_rehash
                        && let Some(cached) = cached_hashes_owned.get(rom_filename)
                    {
                        let reuse_kind = match cached.hash_size_bytes {
                            Some(cached_size)
                                if cached.hash_mtime == current_mtime
                                    && cached_size == *size_bytes =>
                            {
                                Some(Reuse::Exact)
                            }
                            None if cached.hash_mtime == current_mtime => Some(Reuse::Migrated),
                            Some(cached_size) if cached_size == *size_bytes => {
                                Some(Reuse::SizeOnly)
                            }
                            _ => None,
                        };

                        if let Some(kind) = reuse_kind
                            && let Some(result) = reusable_cached_hash(
                                rom_filename,
                                cached,
                                current_mtime,
                                *size_bytes,
                            )
                        {
                            match kind {
                                Reuse::Exact => stats.reused_exact += 1,
                                Reuse::Migrated => stats.reused_migrated += 1,
                                Reuse::SizeOnly => stats.reused_size_only += 1,
                            }
                            pending.push(Pending::Cached(result));
                            continue;
                        }
                    }

                    match compute_crc32(&abs_path) {
                        Ok(crc32) => pending.push(Pending::NeedsLookup {
                            rom_filename: rom_filename.clone(),
                            crc32,
                            mtime_secs: current_mtime,
                            size_bytes: *size_bytes,
                        }),
                        Err(e) => {
                            stats.skipped += 1;
                            tracing::debug!("Failed to hash {}: {e}", abs_path.display());
                            continue;
                        }
                    }

                    if options.force_rehash {
                        stats.forced_computed += 1;
                    } else {
                        stats.computed += 1;
                    }
                }
                (pending, stats)
            })
            .await
            .unwrap_or_default()
        }
    };

    let fresh_crcs: Vec<u32> = pending
        .iter()
        .filter_map(|p| match p {
            Pending::NeedsLookup { crc32, .. } => Some(*crc32),
            _ => None,
        })
        .collect();
    let matches = if fresh_crcs.is_empty() {
        std::collections::HashMap::new()
    } else {
        game_db::lookup_by_crcs_batch(system, &fresh_crcs).await
    };

    let results = pending
        .into_iter()
        .map(|p| match p {
            Pending::Cached(r) => r,
            Pending::NeedsLookup {
                rom_filename,
                crc32,
                mtime_secs,
                size_bytes,
            } => HashResult {
                rom_filename,
                crc32,
                mtime_secs,
                size_bytes,
                matched_name: matches.get(&crc32).map(|e| e.canonical_name.clone()),
            },
        })
        .collect();
    HashIdentifyResult { results, stats }
}

/// Get a file's mtime as seconds since the UNIX epoch.
fn file_mtime_secs(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tempdir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("replay-hash-test-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn hash_eligible_cartridge_systems() {
        assert!(is_hash_eligible("nintendo_nes"));
        assert!(is_hash_eligible("nintendo_snes"));
        assert!(is_hash_eligible("nintendo_gb"));
        assert!(is_hash_eligible("nintendo_gbc"));
        assert!(is_hash_eligible("nintendo_gba"));
        assert!(is_hash_eligible("nintendo_n64"));
        assert!(is_hash_eligible("sega_sms"));
        assert!(is_hash_eligible("sega_smd"));
        assert!(is_hash_eligible("sega_gg"));
        assert!(is_hash_eligible("sega_sg"));
        assert!(is_hash_eligible("sega_32x"));
        assert!(is_hash_eligible("microsoft_msx"));
    }

    #[test]
    fn hash_ineligible_systems() {
        // CD systems
        assert!(!is_hash_eligible("sony_psx"));
        assert!(!is_hash_eligible("sega_st"));
        assert!(!is_hash_eligible("sega_cd"));
        assert!(!is_hash_eligible("sega_dc"));
        // Arcade
        assert!(!is_hash_eligible("arcade_fbneo"));
        assert!(!is_hash_eligible("arcade_mame"));
        // Computer/folder
        assert!(!is_hash_eligible("scummvm"));
        assert!(!is_hash_eligible("ibm_pc"));
        assert!(!is_hash_eligible("sharp_x68k"));
        // DS excluded for now
        assert!(!is_hash_eligible("nintendo_ds"));
    }

    #[test]
    fn msx_hashes_only_cartridge_files() {
        assert!(is_file_hash_eligible("microsoft_msx", "Aleste (Japan).rom"));
        assert!(is_file_hash_eligible("microsoft_msx", "Game.mx1"));
        assert!(is_file_hash_eligible("microsoft_msx", "Game.mx2"));
        assert!(!is_file_hash_eligible("microsoft_msx", "Disk Game.dsk"));
        assert!(!is_file_hash_eligible("microsoft_msx", "Multi Disk.m3u"));
        assert!(!is_file_hash_eligible("microsoft_msx", "Tape Game.cas"));
    }

    #[test]
    fn compute_crc32_simple_file() {
        let tmp = tempdir();
        let path = tmp.join("test.sfc");
        fs::write(&path, b"hello world").unwrap();

        let crc = compute_crc32(&path).unwrap();
        // CRC32 of "hello world" is a well-known value
        assert_eq!(crc, 0x0D4A_1185);
    }

    #[test]
    fn compute_crc32_nes_skips_header() {
        let tmp = tempdir();
        let path = tmp.join("test.nes");
        // 16 bytes of header + "hello world" as ROM data
        let mut content = vec![0u8; 16];
        content.extend_from_slice(b"hello world");
        fs::write(&path, &content).unwrap();

        let crc = compute_crc32(&path).unwrap();
        // Should match CRC32 of just "hello world" (header skipped)
        assert_eq!(crc, 0x0D4A_1185);
    }

    #[test]
    fn compute_crc32_nonexistent_file() {
        let result = compute_crc32(Path::new("/nonexistent/path.sfc"));
        assert!(result.is_err());
    }

    #[test]
    fn header_skip_by_extension() {
        assert_eq!(detect_header_skip(Path::new("game.nes")), 16);
        assert_eq!(detect_header_skip(Path::new("game.NES")), 16);
        assert_eq!(detect_header_skip(Path::new("game.lnx")), 64);
        assert_eq!(detect_header_skip(Path::new("game.sfc")), 0);
        assert_eq!(detect_header_skip(Path::new("game.md")), 0);
        assert_eq!(detect_header_skip(Path::new("game.gb")), 0);
    }

    #[tokio::test]
    async fn hash_and_identify_skips_ineligible_system() {
        let results = hash_and_identify(
            "sony_psx",
            &[(
                "game.chd".to_string(),
                "/roms/sony_psx/game.chd".to_string(),
                700_000_000,
            )],
            &std::collections::HashMap::new(),
            Path::new("/tmp"),
        )
        .await;
        assert!(results.results.is_empty());
    }

    #[tokio::test]
    async fn hash_and_identify_uses_cache() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/nintendo_snes");
        fs::create_dir_all(&roms_dir).unwrap();
        let rom_path_str = "/roms/nintendo_snes/game.sfc";
        let abs_path = tmp.join("roms/nintendo_snes/game.sfc");
        fs::write(&abs_path, b"some rom data").unwrap();

        let mtime = file_mtime_secs(&abs_path).unwrap();

        let mut cache = std::collections::HashMap::new();
        cache.insert(
            "game.sfc".to_string(),
            CachedHash {
                crc32: 0xDEADBEEF,
                hash_mtime: mtime,
                hash_size_bytes: Some(100),
                matched_name: Some("Cached Game Name".to_string()),
            },
        );

        let results = hash_and_identify(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
        )
        .await;

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].crc32, 0xDEADBEEF);
        assert_eq!(
            results.results[0].matched_name.as_deref(),
            Some("Cached Game Name")
        );
        assert_eq!(results.stats.reused_exact, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_rehashes_on_mtime_change() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/nintendo_snes");
        fs::create_dir_all(&roms_dir).unwrap();
        let rom_path_str = "/roms/nintendo_snes/game.sfc";
        let abs_path = tmp.join("roms/nintendo_snes/game.sfc");
        fs::write(&abs_path, b"some rom data").unwrap();

        // Cache with a stale mtime
        let mut cache = std::collections::HashMap::new();
        cache.insert(
            "game.sfc".to_string(),
            CachedHash {
                crc32: 0xDEADBEEF,
                hash_mtime: 0, // Very old mtime — won't match current
                hash_size_bytes: Some(99),
                matched_name: Some("Old Name".to_string()),
            },
        );

        let results = hash_and_identify(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
        )
        .await;

        assert_eq!(results.results.len(), 1);
        // Should have recomputed, so CRC won't be 0xDEADBEEF
        assert_ne!(results.results[0].crc32, 0xDEADBEEF);
        assert_eq!(results.stats.computed, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_reuses_migrated_cache_on_matching_mtime() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/nintendo_snes");
        fs::create_dir_all(&roms_dir).unwrap();
        let rom_path_str = "/roms/nintendo_snes/game.sfc";
        let abs_path = tmp.join("roms/nintendo_snes/game.sfc");
        fs::write(&abs_path, b"some rom data").unwrap();

        let mtime = file_mtime_secs(&abs_path).unwrap();

        let mut cache = std::collections::HashMap::new();
        cache.insert(
            "game.sfc".to_string(),
            CachedHash {
                crc32: 0xDEADBEEF,
                hash_mtime: mtime,
                hash_size_bytes: None,
                matched_name: Some("Cached Game Name".to_string()),
            },
        );

        let results = hash_and_identify(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
        )
        .await;

        assert_eq!(results.results[0].crc32, 0xDEADBEEF);
        assert_eq!(results.results[0].size_bytes, 100);
        assert_eq!(results.stats.reused_migrated, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_reuses_same_size_when_mtime_drifts() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/nintendo_snes");
        fs::create_dir_all(&roms_dir).unwrap();
        let rom_path_str = "/roms/nintendo_snes/game.sfc";
        let abs_path = tmp.join("roms/nintendo_snes/game.sfc");
        fs::write(&abs_path, b"some rom data").unwrap();

        let mut cache = std::collections::HashMap::new();
        cache.insert(
            "game.sfc".to_string(),
            CachedHash {
                crc32: 0xDEADBEEF,
                hash_mtime: 0,
                hash_size_bytes: Some(100),
                matched_name: Some("Cached Game Name".to_string()),
            },
        );

        let results = hash_and_identify(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
        )
        .await;

        assert_eq!(results.results[0].crc32, 0xDEADBEEF);
        assert_eq!(results.stats.reused_size_only, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_force_rehash_ignores_same_size_cache() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/nintendo_snes");
        fs::create_dir_all(&roms_dir).unwrap();
        let rom_path_str = "/roms/nintendo_snes/game.sfc";
        let abs_path = tmp.join("roms/nintendo_snes/game.sfc");
        fs::write(&abs_path, b"some rom data").unwrap();

        let mut cache = std::collections::HashMap::new();
        cache.insert(
            "game.sfc".to_string(),
            CachedHash {
                crc32: 0xDEADBEEF,
                hash_mtime: file_mtime_secs(&abs_path).unwrap(),
                hash_size_bytes: Some(100),
                matched_name: Some("Cached Game Name".to_string()),
            },
        );

        let results = hash_and_identify_with_options(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
            HashOptions { force_rehash: true },
        )
        .await;

        assert_ne!(results.results[0].crc32, 0xDEADBEEF);
        assert_eq!(results.stats.forced_computed, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_cancelled_before_work_returns_no_results() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/nintendo_nes");
        fs::create_dir_all(&roms_dir).unwrap();
        let abs_path = tmp.join("roms/nintendo_nes/game.nes");
        fs::write(&abs_path, b"some rom data").unwrap();
        let cancel = Arc::new(AtomicBool::new(true));

        let results = hash_and_identify_with_options_and_cancel(
            "nintendo_nes",
            &[(
                "game.nes".to_string(),
                "/roms/nintendo_nes/game.nes".to_string(),
                13,
            )],
            &std::collections::HashMap::new(),
            &tmp,
            HashOptions::default(),
            Some(cancel),
        )
        .await;

        assert!(results.results.is_empty());
        assert_eq!(results.stats, HashStats::default());
    }

    #[tokio::test]
    async fn hash_and_identify_skips_32x_cd_images() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/sega_32x");
        fs::create_dir_all(&roms_dir).unwrap();
        let abs_path = tmp.join("roms/sega_32x/Corpse Killer (USA) (Sega CD 32X).chd");
        fs::write(&abs_path, b"disc data").unwrap();

        let results = hash_and_identify(
            "sega_32x",
            &[(
                "Corpse Killer (USA) (Sega CD 32X).chd".to_string(),
                "/roms/sega_32x/Corpse Killer (USA) (Sega CD 32X).chd".to_string(),
                100,
            )],
            &std::collections::HashMap::new(),
            &tmp,
        )
        .await;

        assert!(results.results.is_empty());
        assert_eq!(results.stats.skipped, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_skips_tagged_32x_cd_bin() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/sega_32x");
        fs::create_dir_all(&roms_dir).unwrap();
        let abs_path = tmp.join("roms/sega_32x/Corpse Killer (USA) (Sega CD 32X).bin");
        fs::write(&abs_path, b"disc track data").unwrap();

        let results = hash_and_identify(
            "sega_32x",
            &[(
                "Corpse Killer (USA) (Sega CD 32X).bin".to_string(),
                "/roms/sega_32x/Corpse Killer (USA) (Sega CD 32X).bin".to_string(),
                100,
            )],
            &std::collections::HashMap::new(),
            &tmp,
        )
        .await;

        assert!(results.results.is_empty());
        assert_eq!(results.stats.skipped, 1);
    }

    #[tokio::test]
    async fn hash_and_identify_hashes_32x_cartridges() {
        let tmp = tempdir();
        let roms_dir = tmp.join("roms/sega_32x");
        fs::create_dir_all(&roms_dir).unwrap();
        let abs_path = tmp.join("roms/sega_32x/Doom (USA).32x");
        fs::write(&abs_path, b"cartridge data").unwrap();

        let results = hash_and_identify(
            "sega_32x",
            &[(
                "Doom (USA).32x".to_string(),
                "/roms/sega_32x/Doom (USA).32x".to_string(),
                14,
            )],
            &std::collections::HashMap::new(),
            &tmp,
        )
        .await;

        assert_eq!(results.results.len(), 1);
        assert_eq!(results.stats.computed, 1);
        assert_eq!(results.stats.skipped, 0);
    }
}
