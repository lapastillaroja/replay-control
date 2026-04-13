//! ROM hash-based identification using CRC32.
//!
//! Computes CRC32 hashes of cartridge-based ROM files at scan time and matches
//! them against the embedded No-Intro DAT data to get definitive ROM identification.
//! CD-based, computer/folder, and arcade systems are excluded (see [`is_hash_eligible`]).
//!
//! The hash result is cached in the `game_library` table keyed by file mtime,
//! so only new or modified files are re-hashed on subsequent scans.

use std::io;
use std::path::Path;

use crate::game_db;

/// Systems eligible for CRC32 hash-based identification.
///
/// These are cartridge-based systems with single-file ROMs that have
/// corresponding No-Intro DAT data compiled into the binary (i.e., they
/// appear in `GAME_DB_SYSTEMS` in build.rs and have a `_CRC_INDEX` PHF map).
///
/// Excluded categories:
/// - CD systems (PSX, Saturn, Sega CD, PCE-CD, Dreamcast, 3DO, CD-i, Neo Geo CD)
/// - Computer/folder systems (ScummVM, DOS/IBM PC, Sharp X68000, Amiga, C64, Amstrad)
/// - Arcade (MAME, FBNeo — identified by romset name, not file content)
/// - Nintendo DS (excluded for now — ROMs average 64 MB, first-scan too slow)
const HASH_ELIGIBLE_SYSTEMS: &[&str] = &[
    "nintendo_nes",
    "nintendo_snes",
    "nintendo_gb",
    "nintendo_gbc",
    "nintendo_gba",
    "nintendo_n64",
    "sega_sms",
    "sega_smd",
    "sega_gg",
    "sega_sg",
    "sega_32x",
];

/// Check whether a system is eligible for CRC32 hash-based identification.
///
/// Returns `true` for cartridge-based systems that have No-Intro CRC index data
/// compiled into the binary. Returns `false` for CD, computer, arcade, and
/// systems without DAT coverage.
pub fn is_hash_eligible(system: &str) -> bool {
    HASH_ELIGIBLE_SYSTEMS.contains(&system)
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
    /// No-Intro canonical name if CRC matched, None if no match.
    pub matched_name: Option<String>,
}

/// Cached hash data from the database.
#[derive(Debug, Clone)]
pub struct CachedHash {
    pub crc32: u32,
    pub hash_mtime: i64,
    pub matched_name: Option<String>,
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
pub fn hash_and_identify(
    system: &str,
    rom_files: &[(String, String, u64)], // (rom_filename, rom_path, size_bytes)
    cached_hashes: &std::collections::HashMap<String, CachedHash>,
    storage_root: &Path,
) -> Vec<HashResult> {
    if !is_hash_eligible(system) {
        return Vec::new();
    }

    let mut results = Vec::new();

    for (rom_filename, rom_path, _size_bytes) in rom_files {
        // Resolve the absolute path on disk from the rom_path.
        // rom_path is like "/roms/nintendo_nes/game.nes" relative to storage root.
        let abs_path = storage_root.join(rom_path.trim_start_matches('/'));

        // Get the current file mtime.
        let current_mtime = match file_mtime_secs(&abs_path) {
            Some(m) => m,
            None => continue, // Can't stat the file — skip.
        };

        // Check cache: if mtime matches, use cached hash.
        if let Some(cached) = cached_hashes.get(rom_filename)
            && cached.hash_mtime == current_mtime
        {
            results.push(HashResult {
                rom_filename: rom_filename.clone(),
                crc32: cached.crc32,
                mtime_secs: current_mtime,
                matched_name: cached.matched_name.clone(),
            });
            continue;
        }

        // Cache miss or stale — compute CRC32.
        let crc32 = match compute_crc32(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!("Failed to hash {}: {e}", abs_path.display());
                continue;
            }
        };

        // Look up the CRC32 in the No-Intro index.
        let matched_name =
            game_db::lookup_by_crc(system, crc32).map(|entry| entry.canonical_name.to_string());

        results.push(HashResult {
            rom_filename: rom_filename.clone(),
            crc32,
            mtime_secs: current_mtime,
            matched_name,
        });
    }

    results
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
    use std::sync::atomic::{AtomicU32, Ordering};

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

    #[test]
    fn hash_and_identify_skips_ineligible_system() {
        let results = hash_and_identify(
            "sony_psx",
            &[(
                "game.chd".to_string(),
                "/roms/sony_psx/game.chd".to_string(),
                700_000_000,
            )],
            &std::collections::HashMap::new(),
            Path::new("/tmp"),
        );
        assert!(results.is_empty());
    }

    #[test]
    fn hash_and_identify_uses_cache() {
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
                matched_name: Some("Cached Game Name".to_string()),
            },
        );

        let results = hash_and_identify(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].crc32, 0xDEADBEEF);
        assert_eq!(results[0].matched_name.as_deref(), Some("Cached Game Name"));
    }

    #[test]
    fn hash_and_identify_rehashes_on_mtime_change() {
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
                matched_name: Some("Old Name".to_string()),
            },
        );

        let results = hash_and_identify(
            "nintendo_snes",
            &[("game.sfc".to_string(), rom_path_str.to_string(), 100)],
            &cache,
            &tmp,
        );

        assert_eq!(results.len(), 1);
        // Should have recomputed, so CRC won't be 0xDEADBEEF
        assert_ne!(results[0].crc32, 0xDEADBEEF);
    }
}
