//! Disc RetroAchievements `rc_hash` — a faithful reimplementation of rcheevos
//! `src/rhash/hash_disc.c` + `cdreader.c` for the disc formats that appear on the
//! device: `.chd`, `.cue`+`.bin`/`.iso`/`.img` (raw 2352 / cooked 2048), and
//! `.gdi` (Dreamcast GD-ROM).
//!
//! Recipes (verified against rcheevos `develop`):
//!  - **PSX** (`rc_hash_psx`): find `SYSTEM.CNF` in the ISO9660 root of the data
//!    track, parse `BOOT = cdrom:\NAME;1` (fallback `PSX.EXE`), locate the exe,
//!    use its `PS-X EXE` header size when present, then
//!    `md5(exe_name_string ++ exe_bytes)` (the bare name is prepended).
//!  - **Sega CD / Mega-CD / Saturn** (`rc_hash_sega_cd`): `md5` of the first 512
//!    bytes of sector 0 (the IP.BIN / disc header); accepts `"SEGADISCSYSTEM  "`
//!    or `"SEGA SEGASATURN "` (Saturn genuinely shares this fn).
//!  - **Dreamcast** (`rc_hash_dreamcast`): GD-ROM track 3 (LBA 45000); IP.BIN must
//!    be `"SEGA SEGAKATANA "`; `md5(256-byte IP.BIN ++ boot-exe bytes)`. Uses the
//!    absolute-LBA model (PVD at `first_track_sector + 16`).
//!  - **3DO** (`rc_hash_3do`): Opera FS; `md5(132-byte header ++ LaunchMe bytes)`.
//!    Note: 3DO CHDs store *cooked* sectors (no 12-byte sync) → header skip 0.
//!  - **PC Engine CD** (`rc_hash_pce_track`): `md5(22-byte title ++ N boot
//!    sectors)`, else `BOOT.BIN`.
//!  - **Neo Geo CD** (`rc_hash_neogeo_cd`): `IPL.TXT` → `md5` of each `.PRG`.
//!  - **Atari Jaguar CD** (`rc_hash_jaguar_cd`): boot executable from the first
//!    track of the second session, with rcheevos' KART executable fallback for
//!    homebrew discs.
//!
//! All yield the same MD5 the RA Web API ships in `ra_hash`, so the result is
//! resolved through the catalog `ra_hash` table exactly like header-cart rc_hash.
//! Ported from the validated PoC (`poc/ra-hash/src/rc_hash_disc.rs`); PSX, Sega
//! CD, Saturn, 3DO and Dreamcast validated on-device against the RA hash set.

use md5::{Digest, Md5};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::library::rom_hash;
use crate::library::rom_hash::md5_digest_hex;
use replay_control_core::systems::{RcHashKind, find_system};

mod containers;
mod gamecube;
mod ps2;
mod wii;

/// rcheevos caps the bytes it MD5s at 64 MiB; mirror it for the PSX exe body.
const MAX_BUFFER_SIZE: usize = 64 * 1024 * 1024;
const MAX_M3U_BYTES: u64 = 8192;
const SECTOR_USER: usize = 2048;

/// A source of 2048-byte logical user-data sectors from the first data track,
/// addressed by absolute logical sector number (sector 0 = first sector of the
/// data track's user area) — the contract rcheevos cdreader provides.
pub trait SectorReader {
    /// Read the 2048 user-data bytes of *absolute* logical `sector` into `out`
    /// (>=2048). rcheevos addresses sectors by absolute disc LBA; a reader for a
    /// track starting at LBA N translates `sector - N` to a file offset.
    /// Returns bytes read (0 at end / before the track).
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize>;

    /// Absolute LBA of this track's first sector (rcheevos
    /// `rc_cd_first_track_sector`). 0 for single-track-at-LBA-0 readers
    /// (PSX / Sega CD / 3DO / Neo Geo CD); e.g. 45000 for a Dreamcast GD-ROM
    /// track 3. Used to locate the ISO9660 PVD at `first_track_sector + 16`.
    fn first_track_sector(&self) -> u32 {
        0
    }
}

// ---------------------------------------------------------------------------
// Raw 2352 / 2336 / 2048 reader (cue/bin, raw track, or a cooked .iso)
// ---------------------------------------------------------------------------

struct RawBinReader {
    file: std::fs::File,
    raw_sector_size: u64,
    header: u64, // bytes to skip inside a raw frame to reach user data
}

impl RawBinReader {
    fn open(path: &Path) -> io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        let mut head = [0u8; 2352];
        let n = file.read(&mut head)?;
        file.seek(SeekFrom::Start(0))?;
        let (raw_sector_size, header) = detect_framing(&head[..n], len);
        Ok(Self {
            file,
            raw_sector_size,
            header,
        })
    }
}

/// rcheevos cdreader.c framing detection (simplified): sync pattern + CD001.
fn detect_framing(head: &[u8], file_len: u64) -> (u64, u64) {
    const SYNC: [u8; 12] = [
        0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0,
    ];
    if head.len() >= 2352 && head[..12] == SYNC {
        // Mode2-form1 has a 24-byte header (CD001 lands at +25); else Mode1 (16).
        if head.len() >= 30 && &head[25..30] == b"CD001" {
            return (2352, 24);
        }
        return (2352, 16);
    }
    if file_len.is_multiple_of(2352) {
        return (2352, 16);
    }
    if file_len.is_multiple_of(2336) {
        return (2336, 8);
    }
    (2048, 0)
}

impl SectorReader for RawBinReader {
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
        let off = sector as u64 * self.raw_sector_size + self.header;
        self.file.seek(SeekFrom::Start(off))?;
        let mut got = 0;
        while got < SECTOR_USER {
            let n = self.file.read(&mut out[got..SECTOR_USER])?;
            if n == 0 {
                break;
            }
            got += n;
        }
        Ok(got)
    }
}

// ---------------------------------------------------------------------------
// CHD reader (pure-Rust `chd` crate): decompress hunks, extract the 2048 user
// bytes of each CD frame (CD unit = 2352 data + 96 subcode = 2448).
// ---------------------------------------------------------------------------

const CD_FRAME_SIZE: usize = 2352;
const CD_SUBCODE: usize = 96;
const CD_UNIT: usize = CD_FRAME_SIZE + CD_SUBCODE; // 2448

/// chdman pads each CD track to a 4-frame boundary in the CHD (MAME cdrom.cpp).
const CD_TRACK_PADDING: u32 = 4;

struct ChdReader {
    chd: chd::Chd<std::fs::File>,
    unit_bytes: usize,
    frames_per_hunk: usize,
    cmpbuf: Vec<u8>,
    hunk_cache: Vec<u8>,
    cached_hunk: Option<u32>,
    header_skip: usize, // 16 (mode1) or 24 (mode2 form1), detected from the data track
    frame_base: u32,    // CHD frame offset of the selected track
}

impl ChdReader {
    fn open(path: &Path) -> io::Result<Self> {
        Self::open_with_track(path, ChdTrackSelection::FirstData)
    }

    fn open_with_track(path: &Path, track_selection: ChdTrackSelection) -> io::Result<Self> {
        let f = std::fs::File::open(path)?;
        let chd =
            chd::Chd::open(f, None).map_err(|e| io::Error::other(format!("chd open: {e:?}")))?;
        let hunk_bytes = chd.header().hunk_size() as usize;
        let unit_bytes = chd.header().unit_bytes() as usize;
        if !matches!(unit_bytes, SECTOR_USER | CD_UNIT)
            || hunk_bytes == 0
            || hunk_bytes > 16 * 1024 * 1024
            || !hunk_bytes.is_multiple_of(unit_bytes)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported CHD geometry",
            ));
        }
        let frames_per_hunk = hunk_bytes
            .checked_div(unit_bytes)
            .unwrap_or(hunk_bytes / CD_UNIT);
        let cmpbuf = chd.get_hunksized_buffer();
        // Most recipes address the first DATA track; Jaguar CD addresses the
        // first track of session two, even when it is marked AUDIO.
        let meta_offset = chd.header().meta_offset();
        let (frame_base, probe_jaguar_boot) =
            match chd_track_offset(path, meta_offset, track_selection) {
                Ok(frame_base) => (frame_base, false),
                Err(error)
                    if track_selection == ChdTrackSelection::FirstOfSecondSession
                        && error.kind() == io::ErrorKind::NotFound =>
                {
                    (0, true)
                }
                Err(error) => return Err(error),
            };
        let mut me = Self {
            chd,
            unit_bytes,
            frames_per_hunk: frames_per_hunk.max(1),
            cmpbuf,
            hunk_cache: vec![0u8; hunk_bytes],
            cached_hunk: None,
            header_skip: 16,
            frame_base,
        };
        if probe_jaguar_boot {
            let tracks = chd_tracks(path, meta_offset)?;
            let mut frame = [0u8; CD_FRAME_SIZE];
            let mut found = false;
            for track in tracks.iter().skip(1) {
                me.frame_base = track.frame_base;
                if me.read_frame(0, &mut frame)? && jaguar_cd_boot_header(&frame).is_some() {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Jaguar CD CHD boot track not found",
                ));
            }
        }
        me.header_skip = if unit_bytes == SECTOR_USER {
            0
        } else {
            me.detect_header_skip()?
        };
        Ok(me)
    }

    fn load_hunk(&mut self, hunk: u32) -> io::Result<()> {
        if self.cached_hunk == Some(hunk) {
            return Ok(());
        }
        let mut h = self
            .chd
            .hunk(hunk)
            .map_err(|e| io::Error::other(format!("hunk {hunk}: {e:?}")))?;
        h.read_hunk_in(&mut self.cmpbuf, &mut self.hunk_cache)
            .map_err(|e| io::Error::other(format!("read hunk: {e:?}")))?;
        self.cached_hunk = Some(hunk);
        Ok(())
    }

    fn frame_raw(&mut self, frame: u32, out: &mut [u8; CD_FRAME_SIZE]) -> io::Result<bool> {
        if self.unit_bytes != CD_UNIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "raw CD frame requested from DVD CHD",
            ));
        }
        let hunk = frame / self.frames_per_hunk as u32;
        if hunk as usize >= self.chd.header().hunk_count() as usize {
            return Ok(false);
        }
        self.load_hunk(hunk)?;
        let idx = (frame % self.frames_per_hunk as u32) as usize;
        let start = idx * CD_UNIT;
        if start + CD_FRAME_SIZE > self.hunk_cache.len() {
            return Ok(false);
        }
        out.copy_from_slice(&self.hunk_cache[start..start + CD_FRAME_SIZE]);
        Ok(true)
    }

    fn detect_header_skip(&mut self) -> io::Result<usize> {
        const SYNC: [u8; 12] = [
            0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0,
        ];
        let mut raw = [0u8; CD_FRAME_SIZE];
        // Detect framing from the selected track's first frame, not the CHD's
        // frame 0 — on audio-first discs frame 0 is audio with no sync.
        if !self.frame_raw(self.frame_base, &mut raw)? {
            return Ok(16);
        }
        // No 12-byte sync at the start of a frame means the CHD stores cooked
        // user data at offset 0 (e.g. 3DO discs: the Opera ID lands at byte 0).
        // With a sync present it's a raw 2352 sector and the user data sits after
        // the 16-byte (Mode1) or 24-byte (Mode2 form1) header.
        if raw[..12] != SYNC {
            return Ok(0);
        }
        // Sector 16 (of the data track) holds the ISO9660 PVD: "CD001" follows the header.
        if self.frame_raw(self.frame_base + 16, &mut raw)? {
            for &skip in &[16usize, 24, 8, 0] {
                if skip + 6 <= CD_FRAME_SIZE && &raw[skip + 1..skip + 6] == b"CD001" {
                    return Ok(skip);
                }
            }
        }
        // Sync present but no CD001 (non-ISO filesystem in raw 2352): use the
        // mode byte at offset 15 (Mode2 -> 24, else Mode1 -> 16).
        Ok(if raw[15] == 2 { 24 } else { 16 })
    }
}

impl SectorReader for ChdReader {
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
        if self.unit_bytes == SECTOR_USER {
            if u64::from(sector) * SECTOR_USER as u64 >= self.chd.header().logical_bytes() {
                return Ok(0);
            }
            self.load_hunk(sector / self.frames_per_hunk as u32)?;
            let start = (sector as usize % self.frames_per_hunk) * SECTOR_USER;
            out[..SECTOR_USER].copy_from_slice(&self.hunk_cache[start..start + SECTOR_USER]);
            return Ok(SECTOR_USER);
        }
        let mut raw = [0u8; CD_FRAME_SIZE];
        // Sectors are addressed relative to the first data track (frame_base).
        if !self.frame_raw(self.frame_base + sector, &mut raw)? {
            return Ok(0);
        }
        let skip = self.header_skip;
        out[..SECTOR_USER].copy_from_slice(&raw[skip..skip + SECTOR_USER]);
        Ok(SECTOR_USER)
    }
}

trait RawFrameReader {
    fn read_frame(&mut self, frame: u32, out: &mut [u8; CD_FRAME_SIZE]) -> io::Result<bool>;
}

impl RawFrameReader for ChdReader {
    fn read_frame(&mut self, frame: u32, out: &mut [u8; CD_FRAME_SIZE]) -> io::Result<bool> {
        self.frame_raw(self.frame_base + frame, out)
    }
}

struct RawFrameFileReader {
    file: std::fs::File,
    physical_frame_size: u64,
    payload_offset: u64,
    payload_size: u64,
    base_offset: u64,
}

impl RawFrameFileReader {
    fn open(path: &Path, frame_size: u64) -> io::Result<Self> {
        Self::open_at(path, frame_size, 0)
    }

    fn open_at(path: &Path, frame_size: u64, base_offset: u64) -> io::Result<Self> {
        Self::open_layout(path, frame_size, 0, frame_size, base_offset)
    }

    fn open_layout(
        path: &Path,
        physical_frame_size: u64,
        payload_offset: u64,
        payload_size: u64,
        base_offset: u64,
    ) -> io::Result<Self> {
        if payload_size == 0 || payload_offset.saturating_add(payload_size) > physical_frame_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid raw frame layout",
            ));
        }
        Ok(Self {
            file: std::fs::File::open(path)?,
            physical_frame_size,
            payload_offset,
            payload_size,
            base_offset,
        })
    }
}

impl RawFrameReader for RawFrameFileReader {
    fn read_frame(&mut self, frame: u32, out: &mut [u8; CD_FRAME_SIZE]) -> io::Result<bool> {
        let mut got = 0;
        let mut logical_offset = frame as u64 * CD_FRAME_SIZE as u64;
        while got < out.len() {
            let physical_frame = logical_offset / self.payload_size;
            let within_payload = logical_offset % self.payload_size;
            let physical_offset = self
                .base_offset
                .checked_add(physical_frame.saturating_mul(self.physical_frame_size))
                .and_then(|offset| offset.checked_add(self.payload_offset + within_payload))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "frame overflow"))?;
            self.file.seek(SeekFrom::Start(physical_offset))?;
            let available = (self.payload_size - within_payload) as usize;
            let want = available.min(out.len() - got);
            let mut read = 0;
            while read < want {
                let count = self.file.read(&mut out[got + read..got + want])?;
                if count == 0 {
                    return Ok(false);
                }
                read += count;
            }
            got += want;
            logical_offset += want as u64;
        }
        Ok(true)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChdTrackSelection {
    FirstData,
    FirstOfSecondSession,
    TrackTwo,
}

struct ChdTrack {
    number: u32,
    session: u32,
    frame_base: u32,
    is_audio: bool,
}

/// CHD frame offset of the selected track, parsed from CHTR/CHT2 track and
/// CHSE session metadata. Jaguar CD uses the first track of session two, which
/// can be marked AUDIO; other disc recipes use the first DATA track.
///
/// Metadata is read directly off the on-disk linked list — the crate's
/// `metadata()` iterator needs an unstable feature. chdman lays tracks out
/// sequentially, each padded to a 4-frame boundary; `FRAMES` is the stored
/// count and the first `PREGAP` of them are the pregap (written even for a
/// virtual pregap), so INDEX 01 sits at `chdframeofs + pregap`.
fn chd_tracks(path: &Path, meta_off: Option<u64>) -> io::Result<Vec<ChdTrack>> {
    let Some(mut off) = meta_off else {
        return Ok(Vec::new());
    };
    let mut file = std::fs::File::open(path)?;
    let mut chd_ofs: u32 = 0;
    let mut session = 1u32;
    let mut tracks = Vec::new();
    while off != 0 {
        file.seek(SeekFrom::Start(off))?;
        let mut hdr = [0u8; 16];
        if file.read_exact(&mut hdr).is_err() {
            break;
        }
        let tag = &hdr[0..4];
        let length = u32::from_be_bytes([0, hdr[5], hdr[6], hdr[7]]) as usize;
        let next = u64::from_be_bytes([
            hdr[8], hdr[9], hdr[10], hdr[11], hdr[12], hdr[13], hdr[14], hdr[15],
        ]);
        if tag == b"CHSE" || tag == b"CHTR" || tag == b"CHT2" {
            let mut val = vec![0u8; length];
            if file.read_exact(&mut val).is_err() {
                break;
            }
            // CHD text metadata is NUL-terminated; parsing a field such as
            // `SESSION:2\0` directly as an integer would reject a valid image.
            let text = String::from_utf8_lossy(&val);
            let text = text.trim_end_matches('\0');
            let field = |key: &str| text.split_whitespace().find_map(|t| t.strip_prefix(key));
            if tag == b"CHSE" {
                session = field("SESSION:")
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid CHD session metadata")
                    })?;
                off = next;
                continue;
            }
            let ttype = field("TYPE:").unwrap_or("");
            let frames: u32 = field("FRAMES:").and_then(|s| s.parse().ok()).unwrap_or(0);
            let pregap: u32 = field("PREGAP:").and_then(|s| s.parse().ok()).unwrap_or(0);
            tracks.push(ChdTrack {
                number: tracks.len() as u32 + 1,
                session,
                frame_base: chd_ofs + pregap,
                is_audio: ttype == "AUDIO",
            });
            chd_ofs += frames.div_ceil(CD_TRACK_PADDING) * CD_TRACK_PADDING;
        }
        off = next;
    }
    Ok(tracks)
}

fn chd_track_offset(
    path: &Path,
    meta_off: Option<u64>,
    selection: ChdTrackSelection,
) -> io::Result<u32> {
    let tracks = chd_tracks(path, meta_off)?;
    if let Some(track) = tracks.iter().find(|track| match selection {
        ChdTrackSelection::FirstData => !track.is_audio,
        ChdTrackSelection::FirstOfSecondSession => track.session == 2,
        ChdTrackSelection::TrackTwo => track.number == 2,
    }) {
        return Ok(track.frame_base);
    }
    match selection {
        ChdTrackSelection::FirstData => Ok(0),
        ChdTrackSelection::FirstOfSecondSession => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Jaguar CD CHD has no second-session track",
        )),
        ChdTrackSelection::TrackTwo => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Jaguar CD CHD has no track 2",
        )),
    }
}

// ---------------------------------------------------------------------------
// GDI reader (Dreamcast GD-ROM). A .gdi lists tracks as:
//   <count>
//   <num> <start_lba> <type> <sector_size> <filename> <offset>
// type 4 = data, 0 = audio. Game data is in the high-density area (track 3 at
// LBA 45000). We open one track and address by absolute LBA, so a directory LBA
// of e.g. 45100 maps to frame (45100 - 45000) of track03.bin.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct GdiTrack {
    num: u32,
    start_lba: u32,
    is_data: bool,
    sector_size: u64,
    file: PathBuf,
}

fn parse_gdi(path: &Path) -> io::Result<Vec<GdiTrack>> {
    let text = std::fs::read_to_string(path)?;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tracks = Vec::new();
    for line in text.lines().skip(1) {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 6 {
            continue;
        }
        let num: u32 = toks[0].parse().unwrap_or(0);
        let start_lba: u32 = toks[1].parse().unwrap_or(0);
        let ttype: u32 = toks[2].parse().unwrap_or(0);
        let sector_size: u64 = toks[3].parse().unwrap_or(2352);
        // The filename is everything between sector_size and the trailing offset
        // (it may contain spaces); honour quotes when present.
        let fname = if line.contains('"') {
            line.split('"').nth(1).unwrap_or("").to_string()
        } else {
            toks[4..toks.len() - 1].join(" ")
        };
        tracks.push(GdiTrack {
            num,
            start_lba,
            is_data: ttype == 4,
            sector_size,
            file: parent.join(fname),
        });
    }
    if tracks.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty gdi"));
    }
    Ok(tracks)
}

struct GdiReader {
    file: std::fs::File,
    raw_sector_size: u64,
    header: u64,
    track_first_sector: u32,
}

impl GdiReader {
    fn open_track(track: &GdiTrack) -> io::Result<Self> {
        let mut file = std::fs::File::open(&track.file)?;
        let mut head = [0u8; 2352];
        let n = file.read(&mut head)?;
        file.seek(SeekFrom::Start(0))?;
        let len = file.metadata()?.len();
        let (raw_sector_size, header) = if track.sector_size == 2352 {
            detect_framing(&head[..n], len)
        } else {
            (track.sector_size, 0)
        };
        Ok(Self {
            file,
            raw_sector_size,
            header,
            track_first_sector: track.start_lba,
        })
    }
}

impl SectorReader for GdiReader {
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
        let Some(rel) = sector.checked_sub(self.track_first_sector) else {
            return Ok(0);
        };
        let off = rel as u64 * self.raw_sector_size + self.header;
        self.file.seek(SeekFrom::Start(off))?;
        let mut got = 0;
        while got < SECTOR_USER {
            let n = self.file.read(&mut out[got..SECTOR_USER])?;
            if n == 0 {
                break;
            }
            got += n;
        }
        Ok(got)
    }

    fn first_track_sector(&self) -> u32 {
        self.track_first_sector
    }
}

// ---------------------------------------------------------------------------
// ISO9660 root directory walk (enough to find a file by name in the root)
// ---------------------------------------------------------------------------

/// Locate a file in the ISO9660 root directory of the data track. Returns
/// `(start_sector, size_bytes)`. Case-insensitive, ignores the `;1` version
/// suffix — like rcheevos.
fn iso_find_file(r: &mut dyn SectorReader, name: &str) -> io::Result<Option<(u32, u32)>> {
    iso_find_path(r, &[name])
}

/// Walk an ISO9660 path (directories then a final file), starting at the
/// primary volume descriptor's root record. Returns the final component's
/// `(start_sector, size_bytes)`.
fn iso_find_path(r: &mut dyn SectorReader, path: &[&str]) -> io::Result<Option<(u32, u32)>> {
    let mut sec = [0u8; SECTOR_USER];
    // PVD at first_track_sector + 16 (absolute LBA), per rcheevos
    // rc_cd_find_file_sector. For a track starting at LBA 0 this is 16.
    if r.read_sector(r.first_track_sector() + 16, &mut sec)? == 0 {
        return Ok(None);
    }
    if &sec[1..6] != b"CD001" {
        return Ok(None);
    }
    let root = &sec[156..156 + 34];
    let mut lba = u32::from_le_bytes([root[2], root[3], root[4], root[5]]);
    let mut len = u32::from_le_bytes([root[10], root[11], root[12], root[13]]);
    let mut found = None;
    for component in path {
        found = iso_dir_find(r, lba, len, component)?;
        match found {
            Some((next_lba, next_len)) => {
                lba = next_lba;
                len = next_len;
            }
            None => return Ok(None),
        }
    }
    Ok(found)
}

/// Scan one directory extent for an entry by name (file or subdirectory).
fn iso_dir_find(
    r: &mut dyn SectorReader,
    dir_lba: u32,
    dir_len: u32,
    name: &str,
) -> io::Result<Option<(u32, u32)>> {
    let mut sec = [0u8; SECTOR_USER];
    let want = name.trim_start_matches('\\').to_ascii_uppercase();
    let sectors = dir_len.div_ceil(SECTOR_USER as u32);
    for i in 0..sectors {
        if r.read_sector(dir_lba + i, &mut sec)? == 0 {
            break;
        }
        let mut off = 0usize;
        while off < SECTOR_USER {
            let rlen = sec[off] as usize;
            if rlen == 0 {
                break; // rest of sector is padding
            }
            if rlen < 33 || off + rlen > SECTOR_USER {
                break;
            }
            let rec = &sec[off..off + rlen];
            let lba = u32::from_le_bytes([rec[2], rec[3], rec[4], rec[5]]);
            let size = u32::from_le_bytes([rec[10], rec[11], rec[12], rec[13]]);
            let nlen = rec[32] as usize;
            if 33 + nlen <= rlen {
                let fname_str: String = rec[33..33 + nlen]
                    .iter()
                    .take_while(|&&b| b != b';')
                    .map(|&b| b as char)
                    .collect();
                if fname_str.eq_ignore_ascii_case(&want) {
                    return Ok(Some((lba, size)));
                }
            }
            off += rlen;
        }
    }
    Ok(None)
}

/// Parse `BOOT = cdrom:\NAME;1` out of SYSTEM.CNF contents. Returns bare NAME.
fn parse_boot(cnf: &str) -> Option<String> {
    for line in cnf.lines() {
        let low = line.to_ascii_lowercase();
        if let Some(eq) = low.find("boot")
            && let Some(colon) = line[eq..].find('=')
        {
            let val = line[eq + colon + 1..]
                .trim()
                .trim_start_matches("cdrom:")
                .trim_start_matches("cdrom0:")
                .trim_start_matches('\\')
                .trim_start_matches('/');
            let name: String = val.chars().take_while(|&c| c != ';' && c != ' ').collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn read_file_bytes(r: &mut dyn SectorReader, start: u32, size: u32) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(size as usize);
    let mut sec = [0u8; SECTOR_USER];
    let mut remaining = size as usize;
    let mut s = start;
    while remaining > 0 {
        let n = r.read_sector(s, &mut sec)?;
        if n == 0 {
            break;
        }
        let take = remaining.min(n);
        out.extend_from_slice(&sec[..take]);
        remaining -= take;
        s += 1;
    }
    Ok(out)
}

/// PSP rc_hash over an opened UMD ISO reader: `PSP_GAME/PARAM.SFO` followed
/// by `PSP_GAME/SYSDIR/EBOOT.BIN`, one MD5 over both (rcheevos rc_hash_psp).
fn psp_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let Some((sfo_lba, sfo_size)) = iso_find_path(r, &["PSP_GAME", "PARAM.SFO"])? else {
        return Ok(None);
    };
    let Some((eboot_lba, eboot_size)) = iso_find_path(r, &["PSP_GAME", "SYSDIR", "EBOOT.BIN"])?
    else {
        return Ok(None);
    };
    // rcheevos caps each hashed file at MAX_BUFFER_SIZE (64 MiB) in
    // rc_hash_cd_file; mirror it so oversized executables hash identically.
    const RC_HASH_FILE_CAP: u32 = 64 * 1024 * 1024;
    let mut md5 = Md5::new();
    md5.update(read_file_bytes(r, sfo_lba, sfo_size.min(RC_HASH_FILE_CAP))?);
    md5.update(read_file_bytes(
        r,
        eboot_lba,
        eboot_size.min(RC_HASH_FILE_CAP),
    )?);
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

/// Whole-file MD5 for PSP `.pbp` eboots (rcheevos hashes the entire file).
fn whole_file_md5(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut md5 = Md5::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
    }
    Ok(md5_digest_hex(md5.finalize().into()))
}

/// PSX rc_hash over an opened data-track sector reader.
fn psx_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let exe_name = match iso_find_file(r, "SYSTEM.CNF")? {
        Some((lba, size)) => {
            let bytes = read_file_bytes(r, lba, size.min(4096))?;
            parse_boot(&String::from_utf8_lossy(&bytes)).unwrap_or_else(|| "PSX.EXE".to_string())
        }
        None => "PSX.EXE".to_string(),
    };

    let (lba, dir_size) = match iso_find_file(r, &exe_name)? {
        Some(v) => v,
        None => return Ok(None),
    };

    let mut sec = [0u8; SECTOR_USER];
    if r.read_sector(lba, &mut sec)? == 0 {
        return Ok(None);
    }
    let size = if &sec[0..8] == b"PS-X EXE" {
        u32::from_le_bytes([sec[28], sec[29], sec[30], sec[31]]) + 2048
    } else {
        dir_size
    };
    let size = size.min(MAX_BUFFER_SIZE as u32);

    let mut md5 = Md5::new();
    md5.update(exe_name.as_bytes());
    md5.update(&read_file_bytes(r, lba, size)?);
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

/// Sega CD / Mega-CD / Saturn rc_hash: MD5 of the first 512 bytes of sector 0
/// (the IP.BIN / disc header), gated on the SEGA magic. `None` if absent.
fn sega_cd_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let mut sec = [0u8; SECTOR_USER];
    if r.read_sector(0, &mut sec)? == 0 {
        return Ok(None);
    }
    let magic = &sec[0..16];
    if magic != b"SEGADISCSYSTEM  " && magic != b"SEGA SEGASATURN " {
        return Ok(None);
    }
    let mut md5 = Md5::new();
    md5.update(&sec[0..512]);
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

/// Dreamcast rc_hash (hash_disc.c rc_hash_dreamcast): read the 256-byte IP.BIN
/// at the first sector of the data track (GD-ROM track 3), require
/// "SEGA SEGAKATANA "; md5 = md5(256-byte IP.BIN ++ boot-exe bytes). The boot
/// exe name is at IP.BIN offset 96 (up to 16 chars, whitespace-terminated),
/// located via the ISO9660 directory of the same track.
fn dreamcast_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let base = r.first_track_sector();
    let mut sec = [0u8; SECTOR_USER];
    if r.read_sector(base, &mut sec)? == 0 {
        return Ok(None);
    }
    if &sec[0..16] != b"SEGA SEGAKATANA " {
        return Ok(None);
    }
    let mut md5 = Md5::new();
    md5.update(&sec[0..256]);

    let mut i = 0usize;
    while i < 16 && !sec[96 + i].is_ascii_whitespace() {
        i += 1;
    }
    if i == 0 {
        return Ok(None);
    }
    let exe: String = sec[96..96 + i].iter().map(|&b| b as char).collect();

    let (lba, size) = match iso_find_file(r, &exe)? {
        Some(v) => v,
        None => return Ok(None),
    };
    let size = (size as usize).min(MAX_BUFFER_SIZE) as u32;
    md5.update(&read_file_bytes(r, lba, size)?);
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

/// Dreamcast images on RePlayOS are `.gdi` (track 3 in the high-density area).
/// rcheevos opens track 3 first, falling back to the first data track.
fn dreamcast_hash_path(path: &Path) -> io::Result<Option<String>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "gdi" {
        // Non-GDI Dreamcast images would need CHD multitrack support; treat as a
        // best-effort single-track read (works only if data is at LBA 0).
        if let Some(mut reader) = open_reader(path)? {
            return dreamcast_hash(reader.as_mut());
        }
        return Ok(None);
    }
    let tracks = parse_gdi(path)?;
    let mut candidates: Vec<&GdiTrack> = Vec::new();
    if let Some(t3) = tracks.iter().find(|t| t.num == 3 && t.is_data) {
        candidates.push(t3);
    }
    if let Some(fd) = tracks.iter().find(|t| t.is_data)
        && !candidates.iter().any(|t| t.num == fd.num)
    {
        candidates.push(fd);
    }
    for t in candidates {
        let mut r = GdiReader::open_track(t)?;
        if let Some(h) = dreamcast_hash(&mut r)? {
            return Ok(Some(h));
        }
    }
    Ok(None)
}

/// 3-byte big-endian integer at `b[i..i+3]` (Opera FS / PCE fields).
fn be3(b: &[u8], i: usize) -> u32 {
    (b[i] as u32) * 65536 + (b[i + 1] as u32) * 256 + (b[i + 2] as u32)
}

/// 3DO rc_hash (hash_disc.c rc_hash_3do): Opera filesystem (NOT ISO9660).
/// md5 = md5(132-byte volume header ++ the "LaunchMe" file contents). Walks the
/// chained Opera root-directory blocks to find LaunchMe.
fn three_do_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    const OPERA_ID: [u8; 7] = [0x01, 0x5A, 0x5A, 0x5A, 0x5A, 0x5A, 0x01];
    let mut buf = [0u8; SECTOR_USER];
    if r.read_sector(0, &mut buf)? == 0 {
        return Ok(None);
    }
    if buf[0..7] != OPERA_ID {
        return Ok(None);
    }
    let mut md5 = Md5::new();
    md5.update(&buf[0..132]);

    let mut block_size = be3(&buf, 0x4D);
    let mut block_location = be3(&buf, 0x65).wrapping_mul(block_size);
    let mut sector = block_location / 2048;
    let mut size: u32 = 0;

    loop {
        if r.read_sector(sector, &mut buf)? == 0 {
            break;
        }
        let mut offset = (buf[0x12] as usize) * 256 + buf[0x13] as usize;
        let stop = (buf[0x0D] as usize) * 65536 + (buf[0x0E] as usize) * 256 + buf[0x0F] as usize;
        while offset < stop && offset + 0x48 <= SECTOR_USER {
            if buf[offset + 0x03] == 0x02 {
                let name_start = offset + 0x20;
                let name_end = (name_start..SECTOR_USER)
                    .find(|&j| buf[j] == 0)
                    .unwrap_or(SECTOR_USER);
                if buf[name_start..name_end].eq_ignore_ascii_case(b"LaunchMe") {
                    block_size = be3(&buf, offset + 0x0D);
                    block_location = be3(&buf, offset + 0x45).wrapping_mul(block_size);
                    size = be3(&buf, offset + 0x11);
                    break;
                }
            }
            offset += 0x48 + (buf[offset + 0x43] as usize) * 4;
        }
        if size != 0 {
            break;
        }
        let next = (buf[0x02] as usize) * 256 + buf[0x03] as usize;
        if next == 0xFFFF {
            break;
        }
        let off2 = next as u32 * block_size;
        sector = (block_location + off2) / 2048;
    }

    if size == 0 {
        return Ok(None);
    }
    let mut sector = block_location / 2048;
    let mut remaining = size;
    let mut tmp = [0u8; SECTOR_USER];
    while remaining > 2048 {
        if r.read_sector(sector, &mut tmp)? == 0 {
            break;
        }
        md5.update(tmp);
        sector += 1;
        remaining -= 2048;
    }
    if r.read_sector(sector, &mut tmp)? != 0 {
        md5.update(&tmp[..remaining as usize]);
    }
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

/// PC Engine CD rc_hash (hash_disc.c rc_hash_pce_track): on the first data
/// track, read sector first_track_sector+1; if "PC Engine CD-ROM SYSTEM" at
/// offset 32: md5 = md5(22-byte title @106 ++ the N boot sectors named at bytes
/// 0..3). Otherwise hash the BOOT.BIN file.
fn pce_cd_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let base = r.first_track_sector();
    let mut buf = [0u8; SECTOR_USER];
    if r.read_sector(base + 1, &mut buf)? < 128 {
        return Ok(None);
    }
    if &buf[32..32 + 23] == b"PC Engine CD-ROM SYSTEM" {
        let mut md5 = Md5::new();
        md5.update(&buf[106..128]);
        let mut sector = ((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | buf[2] as u32;
        let mut num = buf[3] as u32;
        sector += base;
        while num > 0 {
            if r.read_sector(sector, &mut buf)? == 0 {
                break;
            }
            md5.update(buf);
            sector += 1;
            num -= 1;
        }
        return Ok(Some(md5_digest_hex(md5.finalize().into())));
    }
    if let Some((lba, size)) = iso_find_file(r, "BOOT.BIN")?
        && (size as usize) < MAX_BUFFER_SIZE
    {
        let mut md5 = Md5::new();
        md5.update(&read_file_bytes(r, lba, size)?);
        return Ok(Some(md5_digest_hex(md5.finalize().into())));
    }
    Ok(None)
}

/// Neo Geo CD rc_hash (hash_disc.c rc_hash_neogeo_cd): on track 1, read IPL.TXT,
/// and for every ".PRG" file it references, hash that file's contents (in order)
/// into a single md5.
fn neogeo_cd_hash(r: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let (sector, _size) = match iso_find_file(r, "IPL.TXT")? {
        Some(v) => v,
        None => return Ok(None),
    };
    let mut buf = [0u8; SECTOR_USER];
    if r.read_sector(sector, &mut buf)? == 0 {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&buf[..1024]);
    let mut md5 = Md5::new();
    let mut any = false;
    for raw_line in text.split(['\n', '\x1a']) {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(pos) = line.to_ascii_uppercase().find(".PRG") else {
            continue;
        };
        let fname = &line[..pos + 4];
        match iso_find_file(r, fname)? {
            Some((lba, size)) => {
                let size = (size as usize).min(MAX_BUFFER_SIZE) as u32;
                md5.update(&read_file_bytes(r, lba, size)?);
                any = true;
            }
            None => return Ok(None),
        }
    }
    if !any {
        return Ok(None);
    }
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

fn byteswap16(frame: &mut [u8]) {
    for pair in frame.as_chunks_mut::<2>().0 {
        pair.swap(0, 1);
    }
}

fn jaguar_cd_executable(
    reader: &mut dyn RawFrameReader,
    start_offset: usize,
    size: usize,
    byteswapped: bool,
) -> io::Result<Option<String>> {
    let mut remaining = size.min(MAX_BUFFER_SIZE);
    let mut frame_num = 0u32;
    let mut offset = start_offset;
    let mut md5 = Md5::new();
    while remaining > 0 {
        let mut frame = [0u8; CD_FRAME_SIZE];
        if !reader.read_frame(frame_num, &mut frame)? {
            return Ok(None);
        }
        if byteswapped {
            byteswap16(&mut frame);
        }
        let available = CD_FRAME_SIZE.saturating_sub(offset);
        let take = remaining.min(available);
        md5.update(&frame[offset..offset + take]);
        remaining -= take;
        offset = 0;
        frame_num += 1;
    }
    Ok(Some(md5_digest_hex(md5.finalize().into())))
}

/// The common homebrew bootloader is not a game identity. rcheevos hashes the
/// KART executable in disc track 2 instead.
enum JaguarCdBootHash {
    Game(String),
    HomebrewLoader,
}

fn jaguar_cd_boot_header(first: &[u8; CD_FRAME_SIZE]) -> Option<(usize, bool)> {
    const HEADER: &[u8; 32] = b"ATARI APPROVED DATA HEADER ATRI ";
    const SWAPPED_HEADER: &[u8; 32] = b"TARA IPARPVODED TA AEHDAREA RT I";
    (64..CD_FRAME_SIZE - 44).find_map(|offset| {
        if &first[offset..offset + HEADER.len()] == HEADER {
            Some((offset, false))
        } else if &first[offset..offset + SWAPPED_HEADER.len()] == SWAPPED_HEADER {
            Some((offset, true))
        } else {
            None
        }
    })
}

/// Atari Jaguar CD rc_hash, ported from rcheevos `rc_hash_jaguar_cd`.
fn jaguar_cd_boot_hash(reader: &mut dyn RawFrameReader) -> io::Result<Option<JaguarCdBootHash>> {
    const HOMEBREW_BOOT_HASH: &str = "254487b59ab21bc005338e85cbf9fd2f";

    let mut first = [0u8; CD_FRAME_SIZE];
    if !reader.read_frame(0, &mut first)? {
        return Ok(None);
    }
    let Some((header_offset, byteswapped)) = jaguar_cd_boot_header(&first) else {
        return Ok(None);
    };

    let size_offset = header_offset + 36;
    let size = if byteswapped {
        u32::from_be_bytes([
            first[size_offset + 1],
            first[size_offset],
            first[size_offset + 3],
            first[size_offset + 2],
        ])
    } else {
        u32::from_be_bytes(first[size_offset..size_offset + 4].try_into().unwrap())
    } as usize;
    if size == 0 {
        return Ok(None);
    }
    let boot = jaguar_cd_executable(reader, size_offset + 4, size, byteswapped)?;
    // CHD audio codecs can normalize the byte order of a CUE/BIN track, so
    // recognize the shared loader by its content hash in either encoding.
    if boot.as_deref() != Some(HOMEBREW_BOOT_HASH) {
        return Ok(boot.map(JaguarCdBootHash::Game));
    }

    Ok(Some(JaguarCdBootHash::HomebrewLoader))
}

fn jaguar_cd_homebrew_hash(reader: &mut dyn RawFrameReader) -> io::Result<Option<String>> {
    let mut first = [0u8; CD_FRAME_SIZE];
    if !reader.read_frame(0, &mut first)? {
        return Ok(None);
    }
    let byteswapped = match &first[0x5E..0x66] {
        b"RT!IRTKA" => true,
        b"TRI!TRAK" => false,
        _ => return Ok(None),
    };
    let size_bytes = &first[0xA6..0xAA];
    let kart_size = if byteswapped {
        u32::from_be_bytes([size_bytes[1], size_bytes[0], size_bytes[3], size_bytes[2]])
    } else {
        u32::from_be_bytes(size_bytes.try_into().unwrap())
    } as usize;
    jaguar_cd_executable(reader, 0xAA, kart_size, byteswapped)
}

fn jaguar_cd_hash(
    boot_track: &mut dyn RawFrameReader,
    open_track_two: impl FnOnce() -> io::Result<Option<Box<dyn RawFrameReader>>>,
) -> io::Result<Option<String>> {
    match jaguar_cd_boot_hash(boot_track)? {
        Some(JaguarCdBootHash::Game(hash)) => Ok(Some(hash)),
        Some(JaguarCdBootHash::HomebrewLoader) => match open_track_two()? {
            Some(mut track_two) => jaguar_cd_homebrew_hash(track_two.as_mut()),
            None => Ok(None),
        },
        None => Ok(None),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CueFrameLayout {
    physical_size: u64,
    payload_offset: u64,
    payload_size: u64,
}

impl CueFrameLayout {
    fn from_track_kind(kind: &str) -> Option<Self> {
        if kind.eq_ignore_ascii_case("AUDIO") {
            return Some(Self {
                physical_size: 2352,
                payload_offset: 0,
                payload_size: 2352,
            });
        }
        let (mode, size) = kind.split_once('/')?;
        let physical_size = size.parse().ok()?;
        let (payload_offset, payload_size) =
            match (mode.to_ascii_uppercase().as_str(), physical_size) {
                ("MODE1", 2048) => (0, 2048),
                ("MODE1", 2352) => (16, 2048),
                ("MODE2", 2336) => (8, 2048),
                ("MODE2", 2352) => (24, 2048),
                _ => return None,
            };
        Some(Self {
            physical_size,
            payload_offset,
            payload_size,
        })
    }
}

struct CueTrackLocation {
    path: PathBuf,
    layout: CueFrameLayout,
    data_offset: u64,
}

#[derive(Debug)]
struct CueTrack {
    file_name: String,
    layout: CueFrameLayout,
    session: u32,
    number: u32,
    first_index: Option<u64>,
    file_offset: u64,
}

fn cue_command<'a>(line: &'a str, command: &str) -> Option<&'a str> {
    let prefix = line.get(..command.len())?;
    if !prefix.eq_ignore_ascii_case(command) {
        return None;
    }
    let rest = &line[command.len()..];
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim_start())
}

fn cue_file_name(rest: &str) -> Option<String> {
    let rest = rest.trim();
    if let Some(quoted) = rest.strip_prefix('"') {
        return quoted.split_once('"').map(|(name, _)| name.to_string());
    }
    rest.split_whitespace().next().map(str::to_string)
}

fn cue_msf_frames(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: u64 = parts.next()?.parse().ok()?;
    let frames: u64 = parts.next()?.parse().ok()?;
    (parts.next().is_none() && seconds < 60 && frames < 75)
        .then_some((minutes * 60 + seconds) * 75 + frames)
}

/// Locate the first track of Jaguar CD's second session, matching rcheevos'
/// `RC_HASH_CDTRACK_FIRST_OF_SECOND_SESSION` selection. Jaguar dumps commonly
/// declare the boot track as AUDIO even though it contains the console header.
fn cue_first_track_of_second_session(cue: &Path) -> io::Result<CueTrackLocation> {
    cue_selected_track(cue, |track| track.session == 2)
}

fn cue_track_two(cue: &Path) -> io::Result<CueTrackLocation> {
    cue_selected_track(cue, |track| track.number == 2)
}

fn cue_selected_track(
    cue: &Path,
    selected: impl Fn(&CueTrack) -> bool,
) -> io::Result<CueTrackLocation> {
    let parent = cue.parent().unwrap_or(Path::new("."));
    let mut current_file: Option<String> = None;
    let mut session = 1u32;
    let mut previous_track: Option<CueTrack> = None;
    let mut current_track: Option<CueTrack> = None;
    for line in std::fs::read_to_string(cue)?.lines() {
        let line = line.trim();
        if let Some(rest) = cue_command(line, "REM") {
            if let Some(value) = cue_command(rest, "SESSION") {
                session = value
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid CUE session")
                    })?;
            }
            continue;
        }
        if let Some(rest) = cue_command(line, "FILE") {
            current_track = None;
            previous_track = None;
            current_file = cue_file_name(rest);
            continue;
        }
        if let Some(rest) = cue_command(line, "TRACK") {
            if current_track.as_ref().is_some_and(&selected) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "selected CUE track has no INDEX 01",
                ));
            }
            previous_track = current_track.take();
            let mut fields = rest.split_whitespace();
            let number = fields
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid CUE track number")
                })?;
            let kind = fields.next().unwrap_or("");
            let file_name = current_file
                .clone()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TRACK before FILE"))?;
            let layout = CueFrameLayout::from_track_kind(kind).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "unsupported CUE track mode")
            })?;
            current_track = Some(CueTrack {
                file_name,
                layout,
                session,
                number,
                first_index: None,
                file_offset: 0,
            });
            continue;
        }
        if let Some(rest) = cue_command(line, "INDEX") {
            let Some(track) = current_track.as_mut() else {
                continue;
            };
            let mut fields = rest.split_whitespace();
            let index = fields.next().and_then(|value| value.parse::<u8>().ok());
            let frames = fields
                .next()
                .and_then(cue_msf_frames)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid CUE INDEX"))?;
            let first_index = *track.first_index.get_or_insert(frames);
            if previous_track.is_none() {
                track.file_offset = first_index
                    .checked_mul(track.layout.physical_size)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "CUE offset overflow")
                    })?;
            }
            if track.first_index == Some(frames)
                && let Some(previous) = &previous_track
                && previous.file_name == track.file_name
            {
                let previous_first = previous.first_index.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "previous CUE track has no INDEX",
                    )
                })?;
                let previous_frames = frames.checked_sub(previous_first).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "CUE track indexes go backwards")
                })?;
                track.file_offset = previous
                    .file_offset
                    .checked_add(
                        previous_frames
                            .checked_mul(previous.layout.physical_size)
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "CUE track offset overflow",
                                )
                            })?,
                    )
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "CUE track offset overflow")
                    })?;
            }
            if index != Some(1) || !selected(track) {
                continue;
            }
            let pregap_frames = frames.checked_sub(first_index).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CUE INDEX 01 precedes INDEX 00")
            })?;
            let data_offset = track
                .file_offset
                .checked_add(
                    pregap_frames
                        .checked_mul(track.layout.physical_size)
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidData, "CUE track offset overflow")
                        })?,
                )
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "CUE track offset overflow")
                })?;
            let path = find_file_case_insensitive(parent, &track.file_name)
                .unwrap_or_else(|| parent.join(&track.file_name));
            return Ok(CueTrackLocation {
                path,
                layout: track.layout,
                data_offset,
            });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "selected track not found in CUE",
    ))
}

const CDI_V2_ID: u32 = 0x8000_0004;
const CDI_V3_ID: u32 = 0x8000_0005;
const CDI_V35_ID: u32 = 0x8000_0006;
const CDI_TRACK_MARKER: [u8; 20] = [
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF,
];

fn read_le_u16(file: &mut std::fs::File) -> io::Result<u16> {
    let mut bytes = [0u8; 2];
    file.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_le_u32(file: &mut std::fs::File) -> io::Result<u32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn cdi_sector_size(mode: u32, code: u32) -> u64 {
    match (mode, code) {
        (0, 2) => 2352,
        (1 | 2, 0) => 2048,
        (2, 1) => 2336,
        _ => 2352,
    }
}

/// Locate the first track in Jaguar CD's second session inside a DiscJuggler
/// image. The descriptor layout follows the parser used by Virtual Jaguar.
type CdiTrackLocation = (u64, u64); // Byte offset and sector size.

fn cdi_jaguar_tracks(path: &Path) -> io::Result<(CdiTrackLocation, Option<CdiTrackLocation>)> {
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    if file_size < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CDI trailer missing",
        ));
    }
    file.seek(SeekFrom::End(-8))?;
    let version = read_le_u32(&mut file)?;
    let header_offset = read_le_u32(&mut file)? as u64;
    if !matches!(version, CDI_V2_ID | CDI_V3_ID | CDI_V35_ID) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported CDI version",
        ));
    }
    let header_position = if version == CDI_V35_ID {
        file_size.checked_sub(header_offset)
    } else {
        Some(header_offset)
    }
    .filter(|position| *position < file_size - 8)
    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid CDI header offset"))?;
    file.seek(SeekFrom::Start(header_position))?;

    let session_count = read_le_u16(&mut file)?;
    let mut byte_offset = 0u64;
    let mut fallback = None;
    let mut boot_track = None;
    let mut track_two = None;
    let mut track_number = 0u32;
    for session in 0..session_count {
        let track_count = read_le_u16(&mut file)?;
        for _ in 0..track_count {
            track_number += 1;
            let new_format = read_le_u32(&mut file)?;
            if new_format != 0 {
                file.seek(SeekFrom::Current(8))?;
            }
            let mut marker = [0u8; 20];
            file.read_exact(&mut marker)?;
            if marker != CDI_TRACK_MARKER {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid CDI track marker",
                ));
            }
            file.seek(SeekFrom::Current(4))?;
            let mut filename_length = [0u8; 1];
            file.read_exact(&mut filename_length)?;
            file.seek(SeekFrom::Current(i64::from(filename_length[0]) + 19))?;
            let index_format = read_le_u32(&mut file)?;
            file.seek(SeekFrom::Current(if index_format == 0x8000_0000 {
                10
            } else {
                2
            }))?;

            let mut track_data = [0u8; 0x57];
            file.read_exact(&mut track_data)?;
            let field = |offset: usize| {
                u32::from_le_bytes(track_data[offset..offset + 4].try_into().unwrap())
            };
            let pregap = field(0x00) as u64;
            let length = field(0x04) as u64;
            let mode = field(0x0E);
            let total_length = field(0x22) as u64;
            let sector_size = cdi_sector_size(mode, field(0x36));

            if version != CDI_V2_ID {
                file.seek(SeekFrom::Current(5))?;
                if read_le_u32(&mut file)? == u32::MAX {
                    file.seek(SeekFrom::Current(78))?;
                }
            }

            let data_offset = byte_offset
                .checked_add(pregap.saturating_mul(sector_size))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CDI track overflow"))?;
            fallback.get_or_insert((data_offset, sector_size));
            if track_number == 2 {
                track_two = Some((data_offset, sector_size));
            }
            if session == 1 && boot_track.is_none() {
                boot_track = Some((data_offset, sector_size));
            }
            let frames = if total_length != 0 {
                total_length
            } else {
                pregap.saturating_add(length)
            };
            byte_offset = byte_offset
                .checked_add(frames.saturating_mul(sector_size))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CDI image overflow"))?;
        }
        file.seek(SeekFrom::Current(if version == CDI_V2_ID {
            12
        } else {
            13
        }))?;
    }

    let (declared_offset, sector_size) = boot_track
        .or(fallback)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CDI contains no tracks"))?;
    let corrected_offset = cdi_corrected_boot_offset(&mut file, file_size, declared_offset)?;
    let track_two = track_two.and_then(|(offset, size)| {
        let adjusted_offset = if corrected_offset >= declared_offset {
            offset.checked_add(corrected_offset - declared_offset)
        } else {
            offset.checked_sub(declared_offset - corrected_offset)
        };
        adjusted_offset
            .filter(|offset| *offset < file_size)
            .map(|offset| (offset, size))
    });
    Ok(((corrected_offset, sector_size), track_two))
}

/// Some widely circulated CDI images have a small, undescribed global data
/// shift. The fixed 32-byte Jaguar boot signature provides a safe landmark.
fn cdi_corrected_boot_offset(
    file: &mut std::fs::File,
    file_size: u64,
    declared_offset: u64,
) -> io::Result<u64> {
    const WINDOW: u64 = 65_536;
    const HEADER_DELTA: u64 = 0x42;
    const HEADER: &[u8; 32] = b"ATARI APPROVED DATA HEADER ATRI ";
    const SWAPPED_HEADER: &[u8; 32] = b"TARA IPARPVODED TA AEHDAREA RT I";

    let start = declared_offset.saturating_sub(WINDOW);
    let end = declared_offset.saturating_add(WINDOW).min(file_size);
    let mut bytes = vec![0u8; end.saturating_sub(start) as usize];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut bytes)?;
    let mut best: Option<(u64, u64)> = None;
    for (index, candidate) in bytes.windows(HEADER.len()).enumerate() {
        if candidate != HEADER && candidate != SWAPPED_HEADER {
            continue;
        }
        let absolute = start + index as u64;
        let Some(content_start) = absolute.checked_sub(HEADER_DELTA) else {
            continue;
        };
        let distance = content_start.abs_diff(declared_offset);
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((content_start, distance));
        }
    }
    Ok(best.map_or(declared_offset, |(offset, _)| offset))
}

fn jaguar_cd_hash_path(path: &Path) -> io::Result<Option<String>> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "chd" => jaguar_cd_hash(
            &mut ChdReader::open_with_track(path, ChdTrackSelection::FirstOfSecondSession)?,
            || {
                ChdReader::open_with_track(path, ChdTrackSelection::TrackTwo)
                    .map(|reader| Some(Box::new(reader) as Box<dyn RawFrameReader>))
            },
        ),
        "cue" => {
            let track = cue_first_track_of_second_session(path)?;
            if !track.path.is_file() {
                return Ok(None);
            }
            jaguar_cd_hash(
                &mut RawFrameFileReader::open_layout(
                    &track.path,
                    track.layout.physical_size,
                    track.layout.payload_offset,
                    track.layout.payload_size,
                    track.data_offset,
                )?,
                || {
                    let track = cue_track_two(path)?;
                    if !track.path.is_file() {
                        return Ok(None);
                    }
                    RawFrameFileReader::open_layout(
                        &track.path,
                        track.layout.physical_size,
                        track.layout.payload_offset,
                        track.layout.payload_size,
                        track.data_offset,
                    )
                    .map(|reader| Some(Box::new(reader) as Box<dyn RawFrameReader>))
                },
            )
        }
        "cdi" => {
            let ((offset, frame_size), track_two) = cdi_jaguar_tracks(path)?;
            jaguar_cd_hash(
                &mut RawFrameFileReader::open_at(path, frame_size, offset)?,
                || match track_two {
                    Some((offset, frame_size)) => {
                        RawFrameFileReader::open_at(path, frame_size, offset)
                            .map(|reader| Some(Box::new(reader) as Box<dyn RawFrameReader>))
                    }
                    None => Ok(None),
                },
            )
        }
        "bin" | "img" => jaguar_cd_hash(
            &mut RawFrameFileReader::open(path, CD_FRAME_SIZE as u64)?,
            || Ok(None),
        ),
        _ => Ok(None),
    }
}

/// Open the data track of a disc image (CHD or cue/bin/iso) as a sector reader.
fn open_reader(path: &Path) -> io::Result<Option<Box<dyn SectorReader>>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    Ok(match ext.as_str() {
        "chd" => Some(Box::new(ChdReader::open(path)?)),
        "cue" => {
            let bin = first_bin_from_cue(path)?;
            // A .cue pointing at a missing track file is an incomplete dump, not a
            // transient error — treat it as unidentifiable (terminal) rather than
            // erroring, so the identity phase marks it resolved instead of
            // retrying it forever.
            if !bin.is_file() {
                return Ok(None);
            }
            Some(Box::new(RawBinReader::open(&bin)?))
        }
        "bin" | "iso" | "img" => Some(Box::new(RawBinReader::open(path)?)),
        _ => None,
    })
}

fn first_bin_from_cue(cue: &Path) -> io::Result<PathBuf> {
    for line in std::fs::read_to_string(cue)?.lines() {
        if let Some(rest) = line.trim().strip_prefix("FILE ") {
            let name = rest
                .trim()
                .trim_start_matches('"')
                .split('"')
                .next()
                .unwrap_or("");
            let parent = cue.parent().unwrap_or(Path::new("."));
            let candidate = parent.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
            // Cue sheets often disagree with the on-disk casing of the BIN (e.g.
            // ".BIN" in the sheet vs ".bin" on a case-sensitive NFS mount). Fall
            // back to a case-insensitive directory match before giving up.
            if let Some(found) = find_file_case_insensitive(parent, name) {
                return Ok(found);
            }
            return Ok(candidate);
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "no FILE in cue"))
}

fn find_file_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
    let want = name.to_ascii_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let entry_name = entry.file_name();
        (entry_name.to_str()?.to_ascii_lowercase() == want).then(|| entry.path())
    })
}

/// Compute the disc `rc_hash` for `path` per the given RePlayOS system. Returns
/// `None` for unsupported systems/formats or when the recipe can't identify the
/// disc (e.g. missing magic / boot file). Supported: PSX, Sega CD (Mega-CD),
/// Saturn, Dreamcast (.gdi), 3DO, PC Engine CD, Neo Geo CD, Atari Jaguar CD,
/// GameCube, Wii, and PS2.
pub fn compute_disc_rc_hash(system: &str, path: &Path) -> io::Result<Option<String>> {
    let Some(kind) = disc_rc_hash_kind(system) else {
        return Ok(None);
    };
    // Dreamcast addresses GD-ROM track 3 (a .gdi), so it picks its own track
    // rather than the generic first-track reader.
    if kind == RcHashKind::Dreamcast {
        return dreamcast_hash_path(path);
    }
    if kind == RcHashKind::JaguarCd {
        return jaguar_cd_hash_path(path);
    }
    if kind == RcHashKind::GameCube {
        return gamecube::hash_path(path);
    }
    if kind == RcHashKind::Wii {
        return wii::hash_path(path);
    }
    if kind == RcHashKind::Ps2 {
        return ps2::hash_path(path);
    }
    // PSP `.pbp` eboots are container files, not disc images: whole-file MD5.
    if kind == RcHashKind::Psp
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pbp"))
    {
        return whole_file_md5(path).map(Some);
    }
    let Some(mut reader) = open_reader(path)? else {
        return Ok(None);
    };
    match kind {
        RcHashKind::Psx => psx_hash(reader.as_mut()),
        RcHashKind::Psp => psp_hash(reader.as_mut()),
        // Saturn shares rc_hash_sega_cd (one fn, also accepts "SEGA SEGASATURN ").
        RcHashKind::SegaCd => sega_cd_hash(reader.as_mut()),
        RcHashKind::ThreeDo => three_do_hash(reader.as_mut()),
        RcHashKind::PceCd => pce_cd_hash(reader.as_mut()),
        RcHashKind::NeoGeoCd => neogeo_cd_hash(reader.as_mut()),
        RcHashKind::JaguarCd => unreachable!("handled by jaguar_cd_hash_path"),
        _ => Ok(None),
    }
}

/// The system's disc rc_hash recipe from the `SYSTEMS` registry, or `None`
/// for cart recipes and systems without RA hashing.
fn disc_rc_hash_kind(system: &str) -> Option<RcHashKind> {
    find_system(system)
        .and_then(|sys| sys.rc_hash_kind)
        .filter(|kind| kind.is_disc())
}

/// Systems with a runtime disc-`rc_hash` recipe wired up (see
/// [`compute_disc_rc_hash`]), from the `SYSTEMS` registry.
pub fn is_disc_rc_hash_system(system: &str) -> bool {
    disc_rc_hash_kind(system).is_some()
}

fn looks_like_m3u_reference(line: &str) -> bool {
    let ext = Path::new(line)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "chd"
            | "cue"
            | "bin"
            | "iso"
            | "img"
            | "gdi"
            | "gcm"
            | "ciso"
            | "gcz"
            | "rvz"
            | "wia"
            | "wbfs"
            | "wad"
    )
}

fn parse_m3u_lines(m3u_path: &Path) -> io::Result<Vec<String>> {
    let file = std::fs::File::open(m3u_path)?;
    let reader = BufReader::new(file.take(MAX_M3U_BYTES));
    let mut lines = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim().trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.chars().any(|ch| ch == '\0') {
            break;
        }
        let reference = trimmed.trim_matches('"').trim();
        if looks_like_m3u_reference(reference) {
            lines.push(reference.to_string());
        }
    }

    Ok(lines)
}

fn resolve_m3u_reference(line: &str, m3u_parent: &Path, storage_root: &Path) -> Option<PathBuf> {
    let normalized = line.replace('\\', "/");
    let as_path = Path::new(&normalized);
    if as_path.is_absolute() && as_path.is_file() {
        return Some(as_path.to_path_buf());
    }

    let mut candidates = Vec::new();
    if let Some(after_roms) = normalized.split("/roms/").nth(1) {
        candidates.push(storage_root.join("roms").join(after_roms));
    }
    candidates.push(storage_root.join(normalized.trim_start_matches('/')));
    candidates.push(m3u_parent.join(&normalized));
    if let Some(filename) = as_path.file_name() {
        candidates.push(m3u_parent.join(filename));
    }

    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
        if let (Some(parent), Some(name)) = (candidate.parent(), candidate.file_name())
            && let Some(name) = name.to_str()
            && let Some(found) = find_file_case_insensitive(parent, name)
        {
            return Some(found);
        }
    }

    None
}

fn first_disc_from_m3u(m3u_path: &Path, storage_root: &Path) -> Option<PathBuf> {
    let m3u_parent = m3u_path.parent()?;
    for line in parse_m3u_lines(m3u_path).ok()? {
        if let Some(path) = resolve_m3u_reference(&line, m3u_parent, storage_root) {
            return Some(path);
        }
    }
    None
}

/// Filesystem identity used for a disc hash row. Plain disc images hash
/// themselves; M3U playlists hash the first referenced disc while keeping the
/// playlist as the library/hash reuse key.
pub fn disc_hash_identity_path(path: &Path, storage_root: &Path) -> PathBuf {
    let is_m3u = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"));
    if is_m3u {
        first_disc_from_m3u(path, storage_root).unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

/// Hash + RA-resolve a batch of disc files for a disc system — the disc analogue
/// of [`crate::rom_hash::hash_and_identify`]. Reuses the persisted `rc_hash`
/// (keyed by mtime/size) to avoid re-reading multi-GB CHDs, computes the
/// boot-file `rc_hash` for changed/new files, and resolves `ra_id` through the
/// catalog `ra_hash` table. Disc dumps carry no No-Intro CRC, so `crc32` is 0 and
/// `matched_name` is `None`; only `rc_hash` + `ra_id` are produced.
pub async fn hash_and_identify_discs(
    system: &str,
    rom_files: &[(String, String, u64)], // (rom_filename, rom_path, size_bytes)
    stored_hashes: &std::collections::HashMap<String, rom_hash::StoredHash>,
    storage_root: &Path,
    options: rom_hash::HashOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> rom_hash::HashIdentifyResult {
    use rom_hash::{HashIdentifyResult, HashResult, HashStats, reusable_stored_hash};

    if !is_disc_rc_hash_system(system) {
        return HashIdentifyResult::default();
    }

    let rom_files = rom_files.to_vec();
    let stored = stored_hashes.clone();
    let root = storage_root.to_path_buf();
    let sys = system.to_string();
    let cancel_owned = cancel.clone();

    // CHD decompression is CPU/IO-heavy — keep it off the tokio workers.
    let (mut results, stats): (Vec<HashResult>, HashStats) =
        tokio::task::spawn_blocking(move || {
            let mut results = Vec::with_capacity(rom_files.len());
            let mut stats = HashStats::default();
            for (rom_filename, rom_path, _size_bytes) in &rom_files {
                if cancel_owned
                    .as_ref()
                    .is_some_and(|c| c.load(Ordering::Relaxed))
                {
                    break;
                }
                let abs_path = root.join(rom_path.trim_start_matches('/'));
                let target_path = disc_hash_identity_path(&abs_path, &root);
                let Some((current_mtime, current_size)) = rom_hash::file_mtime_size(&target_path)
                else {
                    stats.skipped += 1;
                    continue;
                };
                // Reuse the persisted rc_hash when the file identity is unchanged.
                if !options.force_rehash
                    && let Some(c) = stored.get(rom_filename)
                    && let Some(reused) =
                        reusable_stored_hash(rom_filename, c, current_mtime, current_size)
                {
                    stats.reused_exact += 1;
                    results.push(reused);
                    continue;
                }
                match compute_disc_rc_hash(&sys, &target_path) {
                    Ok(rc_hash) => {
                        if options.force_rehash {
                            stats.forced_computed += 1;
                        } else {
                            stats.computed += 1;
                        }
                        results.push(HashResult {
                            rom_filename: rom_filename.clone(),
                            crc32: 0,
                            mtime_secs: current_mtime,
                            size_bytes: current_size,
                            matched_name: None,
                            ra_id: None,
                            rc_hash,
                        });
                    }
                    Err(e) => {
                        stats.skipped += 1;
                        tracing::debug!("disc rc_hash failed for {}: {e}", target_path.display());
                    }
                }
            }
            (results, stats)
        })
        .await
        .unwrap_or_default();

    // Resolve ra_id from rc_hash against the catalog — identical contract to the
    // cart path: a failed lookup preserves the prior value rather than clearing.
    let rc_hashes: Vec<String> = results.iter().filter_map(|r| r.rc_hash.clone()).collect();
    if !rc_hashes.is_empty() {
        match crate::game_db::lookup_ra_id_by_rc_hash_batch(system, &rc_hashes).await {
            Some(ra_matches) => {
                for r in &mut results {
                    if let Some(h) = &r.rc_hash {
                        r.ra_id = ra_matches.get(h).cloned();
                    }
                }
            }
            None => tracing::warn!(
                "{system}: disc rc_hash → ra_id lookup failed; preserving existing ra_id"
            ),
        }
    }

    HashIdentifyResult { results, stats }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_boot_variants() {
        assert_eq!(
            parse_boot("BOOT = cdrom:\\SLUS_007.55;1\nTCB = 4\n").as_deref(),
            Some("SLUS_007.55")
        );
        assert_eq!(
            parse_boot("BOOT=cdrom:\\SCES_012.34;1").as_deref(),
            Some("SCES_012.34")
        );
        assert_eq!(
            parse_boot("BOOT = cdrom:SLPS_123.45;1\r\n").as_deref(),
            Some("SLPS_123.45")
        );
    }

    /// In-memory synthetic ISO so the PSX recipe is exercised without a real disc.
    struct MemDisc {
        sectors: Vec<[u8; SECTOR_USER]>,
    }
    impl SectorReader for MemDisc {
        fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
            match self.sectors.get(sector as usize) {
                Some(s) => {
                    out[..SECTOR_USER].copy_from_slice(s);
                    Ok(SECTOR_USER)
                }
                None => Ok(0),
            }
        }
    }

    struct MemRawFrames {
        frames: Vec<[u8; CD_FRAME_SIZE]>,
    }

    impl RawFrameReader for MemRawFrames {
        fn read_frame(&mut self, frame: u32, out: &mut [u8; CD_FRAME_SIZE]) -> io::Result<bool> {
            let Some(source) = self.frames.get(frame as usize) else {
                return Ok(false);
            };
            out.copy_from_slice(source);
            Ok(true)
        }
    }

    fn dir_record(name: &str, lba: u32, size: u32) -> Vec<u8> {
        let nlen = name.len();
        let rlen = 33 + nlen + (1 - (nlen & 1)); // pad to even
        let mut rec = vec![0u8; rlen];
        rec[0] = rlen as u8;
        rec[2..6].copy_from_slice(&lba.to_le_bytes());
        rec[10..14].copy_from_slice(&size.to_le_bytes());
        rec[32] = nlen as u8;
        rec[33..33 + nlen].copy_from_slice(name.as_bytes());
        rec
    }

    #[test]
    fn psx_recipe_end_to_end() {
        let mut sectors = vec![[0u8; SECTOR_USER]; 30];
        sectors[16][1..6].copy_from_slice(b"CD001");
        let root = dir_record("\u{0}", 20, SECTOR_USER as u32);
        sectors[16][156..156 + root.len()].copy_from_slice(&root);
        let mut off = 0;
        for r in [
            dir_record("SYSTEM.CNF;1", 21, 64),
            dir_record("SLUS_007.55;1", 22, 100),
        ] {
            sectors[20][off..off + r.len()].copy_from_slice(&r);
            off += r.len();
        }
        let cnf = b"BOOT = cdrom:\\SLUS_007.55;1\n";
        sectors[21][..cnf.len()].copy_from_slice(cnf);
        for (i, b) in sectors[22].iter_mut().take(100).enumerate() {
            *b = (i as u8).wrapping_mul(7);
        }
        let got = psx_hash(&mut MemDisc { sectors }).unwrap().unwrap();

        let mut exp = Md5::new();
        exp.update(b"SLUS_007.55");
        let body: Vec<u8> = (0..100u8).map(|i| i.wrapping_mul(7)).collect();
        exp.update(&body);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn psp_recipe_walks_nested_dirs() {
        let mut sectors = vec![[0u8; SECTOR_USER]; 40];
        sectors[16][1..6].copy_from_slice(b"CD001");
        let root = dir_record("\u{0}", 20, SECTOR_USER as u32);
        sectors[16][156..156 + root.len()].copy_from_slice(&root);
        // root: PSP_GAME dir at 21
        let psp_game = dir_record("PSP_GAME", 21, SECTOR_USER as u32);
        sectors[20][..psp_game.len()].copy_from_slice(&psp_game);
        // PSP_GAME: PARAM.SFO at 22 (48 bytes), SYSDIR at 23
        let mut off = 0;
        for r in [
            dir_record("PARAM.SFO;1", 22, 48),
            dir_record("SYSDIR", 23, SECTOR_USER as u32),
        ] {
            sectors[21][off..off + r.len()].copy_from_slice(&r);
            off += r.len();
        }
        // SYSDIR: EBOOT.BIN at 24 (100 bytes)
        let eboot = dir_record("EBOOT.BIN;1", 24, 100);
        sectors[23][..eboot.len()].copy_from_slice(&eboot);
        for (i, b) in sectors[22].iter_mut().take(48).enumerate() {
            *b = (i as u8).wrapping_mul(5);
        }
        for (i, b) in sectors[24].iter_mut().take(100).enumerate() {
            *b = (i as u8).wrapping_mul(11);
        }
        let got = psp_hash(&mut MemDisc { sectors }).unwrap().unwrap();

        let mut exp = Md5::new();
        let sfo: Vec<u8> = (0..48u8).map(|i| i.wrapping_mul(5)).collect();
        let eboot_body: Vec<u8> = (0..100u8).map(|i| i.wrapping_mul(11)).collect();
        exp.update(&sfo);
        exp.update(&eboot_body);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn psp_recipe_missing_structure_is_none() {
        // Valid ISO with no PSP_GAME dir (e.g. a PSX disc scanned as PSP).
        let mut sectors = vec![[0u8; SECTOR_USER]; 22];
        sectors[16][1..6].copy_from_slice(b"CD001");
        let root = dir_record("\u{0}", 20, SECTOR_USER as u32);
        sectors[16][156..156 + root.len()].copy_from_slice(&root);
        let other = dir_record("SYSTEM.CNF;1", 21, 10);
        sectors[20][..other.len()].copy_from_slice(&other);
        assert_eq!(psp_hash(&mut MemDisc { sectors }).unwrap(), None);
    }

    #[test]
    fn psp_pbp_hashes_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Homebrew Game.pbp");
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &body).unwrap();

        let got = compute_disc_rc_hash("sony_psp", &path).unwrap().unwrap();
        let mut exp = Md5::new();
        exp.update(&body);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn sega_cd_recipe_hashes_header() {
        let mut sectors = vec![[0u8; SECTOR_USER]; 1];
        sectors[0][0..16].copy_from_slice(b"SEGADISCSYSTEM  ");
        for (i, b) in sectors[0].iter_mut().take(512).enumerate().skip(16) {
            *b = (i as u8).wrapping_mul(3);
        }
        let got = sega_cd_hash(&mut MemDisc {
            sectors: sectors.clone(),
        })
        .unwrap()
        .unwrap();
        let mut exp = Md5::new();
        exp.update(&sectors[0][0..512]);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn sega_cd_recipe_rejects_non_sega() {
        let sectors = vec![[0u8; SECTOR_USER]; 1];
        assert_eq!(sega_cd_hash(&mut MemDisc { sectors }).unwrap(), None);
    }

    #[test]
    fn sega_cd_recipe_accepts_saturn_magic() {
        // Saturn shares rc_hash_sega_cd via the "SEGA SEGASATURN " magic.
        let mut sectors = vec![[0u8; SECTOR_USER]; 1];
        sectors[0][0..16].copy_from_slice(b"SEGA SEGASATURN ");
        let got = sega_cd_hash(&mut MemDisc {
            sectors: sectors.clone(),
        })
        .unwrap()
        .unwrap();
        let mut exp = Md5::new();
        exp.update(&sectors[0][0..512]);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn jaguar_cd_recipe_hashes_boot_executable_across_frames() {
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let mut raw = vec![0u8; CD_FRAME_SIZE * 2];
        let header_offset = 64;
        raw[header_offset..header_offset + 32].copy_from_slice(b"ATARI APPROVED DATA HEADER ATRI ");
        let size_offset = header_offset + 36;
        raw[size_offset..size_offset + 4].copy_from_slice(&(body.len() as u32).to_be_bytes());
        raw[size_offset + 4..size_offset + 4 + body.len()].copy_from_slice(&body);
        let frames = raw.as_chunks::<CD_FRAME_SIZE>().0.to_vec();

        let got = jaguar_cd_hash(&mut MemRawFrames { frames }, || Ok(None))
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        assert_eq!(got, md5_digest_hex(expected.finalize().into()));
    }

    #[test]
    fn jaguar_cd_homebrew_hashes_kart_executable_from_track_two() {
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let mut raw = vec![0u8; CD_FRAME_SIZE * 2];
        raw[0xAA..0xAA + body.len()].copy_from_slice(&body);
        for frame in raw.as_chunks_mut::<CD_FRAME_SIZE>().0 {
            byteswap16(frame);
        }
        raw[0x5E..0x66].copy_from_slice(b"RT!IRTKA");
        let size = (body.len() as u32).to_be_bytes();
        raw[0xA6..0xAA].copy_from_slice(&[size[1], size[0], size[3], size[2]]);
        let frames = raw.as_chunks::<CD_FRAME_SIZE>().0.to_vec();

        let got = jaguar_cd_homebrew_hash(&mut MemRawFrames { frames })
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        let expected = md5_digest_hex(expected.finalize().into());
        assert_eq!(got, expected);

        // CHD audio codecs may normalize the word order used by raw CUE/BINs.
        for frame in raw.as_chunks_mut::<CD_FRAME_SIZE>().0 {
            byteswap16(frame);
        }
        let frames = raw.as_chunks::<CD_FRAME_SIZE>().0.to_vec();
        let got = jaguar_cd_homebrew_hash(&mut MemRawFrames { frames })
            .unwrap()
            .unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn jaguar_cd_chd_selects_second_session_audio_track() {
        let mut image = vec![0u8; 32];
        let mut previous: Option<usize> = None;
        for (tag, value) in [
            (b"CHSE", "SESSION:1\0"),
            (
                b"CHT2",
                "TRACK:1 TYPE:MODE1 SUBTYPE:NONE FRAMES:101 PREGAP:0",
            ),
            (b"CHT2", "TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:9 PREGAP:0"),
            (b"CHSE", "SESSION:2\0"),
            (
                b"CHT2",
                "TRACK:3 TYPE:AUDIO SUBTYPE:NONE FRAMES:50 PREGAP:2",
            ),
            (
                b"CHT2",
                "TRACK:4 TYPE:MODE1 SUBTYPE:NONE FRAMES:30 PREGAP:0",
            ),
        ] {
            let offset = image.len();
            if let Some(previous) = previous {
                image[previous + 8..previous + 16].copy_from_slice(&(offset as u64).to_be_bytes());
            }
            image.extend_from_slice(tag);
            image.extend_from_slice(&(value.len() as u32).to_be_bytes());
            image.extend_from_slice(&0u64.to_be_bytes());
            image.extend_from_slice(value.as_bytes());
            previous = Some(offset);
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tracks.chd");
        std::fs::write(&path, image).unwrap();

        assert_eq!(
            chd_track_offset(&path, Some(32), ChdTrackSelection::FirstData).unwrap(),
            0
        );
        assert_eq!(
            chd_track_offset(&path, Some(32), ChdTrackSelection::FirstOfSecondSession).unwrap(),
            118
        );
        assert_eq!(
            chd_track_offset(&path, Some(32), ChdTrackSelection::TrackTwo).unwrap(),
            104
        );
    }

    #[test]
    fn jaguar_cd_chd_without_session_keeps_all_track_starts_for_boot_probe() {
        let mut image = vec![0u8; 32];
        let mut previous: Option<usize> = None;
        for (number, frames, pregap) in [(1, 101, 0), (2, 9, 0), (3, 50, 2)] {
            let offset = image.len();
            if let Some(previous) = previous {
                image[previous + 8..previous + 16].copy_from_slice(&(offset as u64).to_be_bytes());
            }
            let value =
                format!("TRACK:{number} TYPE:AUDIO SUBTYPE:NONE FRAMES:{frames} PREGAP:{pregap}");
            image.extend_from_slice(b"CHT2");
            image.extend_from_slice(&(value.len() as u32).to_be_bytes());
            image.extend_from_slice(&0u64.to_be_bytes());
            image.extend_from_slice(value.as_bytes());
            previous = Some(offset);
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tracks.chd");
        std::fs::write(&path, image).unwrap();

        let tracks = chd_tracks(&path, Some(32)).unwrap();
        assert_eq!(
            tracks
                .iter()
                .map(|track| track.frame_base)
                .collect::<Vec<_>>(),
            [0, 104, 118]
        );
        assert!(
            chd_track_offset(&path, Some(32), ChdTrackSelection::FirstOfSecondSession).is_err()
        );
    }

    fn jaguar_cd_boot_track(body: &[u8], minimum_size: usize) -> Vec<u8> {
        let header_offset = 64;
        let size_offset = header_offset + 36;
        let mut track = vec![0u8; minimum_size.max(size_offset + 4 + body.len())];
        track[header_offset..header_offset + 32]
            .copy_from_slice(b"ATARI APPROVED DATA HEADER ATRI ");
        track[size_offset..size_offset + 4].copy_from_slice(&(body.len() as u32).to_be_bytes());
        track[size_offset + 4..size_offset + 4 + body.len()].copy_from_slice(body);
        track
    }

    #[test]
    fn jaguar_cd_cue_uses_first_audio_track_of_second_session() {
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        std::fs::write(dir.path().join("track01.bin"), vec![0u8; CD_FRAME_SIZE]).unwrap();
        std::fs::write(
            dir.path().join("track02.bin"),
            jaguar_cd_boot_track(&body, CD_FRAME_SIZE * 2),
        )
        .unwrap();
        std::fs::write(
            &cue,
            "REM SESSION 01\n\
             FILE \"track01.bin\" BINARY\n\
               TRACK 01 AUDIO\n\
                 INDEX 01 00:00:00\n\
             REM SESSION 02\n\
             FILE \"track02.bin\" BINARY\n\
               TRACK 02 AUDIO\n\
                 INDEX 01 00:00:00\n",
        )
        .unwrap();

        let got = compute_disc_rc_hash("atari_jaguarcd", &cue)
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        assert_eq!(got, md5_digest_hex(expected.finalize().into()));
    }

    #[test]
    fn jaguar_cd_homebrew_cue_selects_track_two_before_second_session() {
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("homebrew.cue");
        std::fs::write(
            &cue,
            "REM SESSION 01\n\
             FILE \"track01.bin\" BINARY\n\
               TRACK 01 AUDIO\n\
                 INDEX 01 00:00:00\n\
             FILE \"track02.bin\" BINARY\n\
               TRACK 02 AUDIO\n\
                 INDEX 01 00:00:00\n\
             REM SESSION 02\n\
             FILE \"track03.bin\" BINARY\n\
               TRACK 03 AUDIO\n\
                 INDEX 01 00:00:00\n",
        )
        .unwrap();

        assert_eq!(
            cue_track_two(&cue).unwrap().path,
            dir.path().join("track02.bin")
        );
        assert_eq!(
            cue_first_track_of_second_session(&cue).unwrap().path,
            dir.path().join("track03.bin")
        );
    }

    #[test]
    fn jaguar_cd_cue_honors_shared_file_index_offset() {
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        let mut shared = vec![0x55; CD_FRAME_SIZE * 2];
        shared.extend_from_slice(&jaguar_cd_boot_track(&body, CD_FRAME_SIZE * 2));
        std::fs::write(dir.path().join("shared.bin"), shared).unwrap();
        std::fs::write(
            &cue,
            "REM SESSION 01\n\
             FILE \"shared.bin\" BINARY\n\
               TRACK 01 AUDIO\n\
                 INDEX 01 00:00:00\n\
             REM SESSION 02\n\
               TRACK 02 AUDIO\n\
                 INDEX 00 00:00:01\n\
                 INDEX 01 00:00:02\n",
        )
        .unwrap();

        let location = cue_first_track_of_second_session(&cue).unwrap();
        assert_eq!(location.data_offset, (CD_FRAME_SIZE * 2) as u64);
        let got = compute_disc_rc_hash("atari_jaguarcd", &cue)
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        assert_eq!(got, md5_digest_hex(expected.finalize().into()));
    }

    #[test]
    fn jaguar_cd_cue_shared_file_uses_each_tracks_physical_frame_size() {
        let body: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        let mut shared = vec![0x55; CD_FRAME_SIZE * 2];
        shared.extend_from_slice(&vec![0x66; 2048]);
        let mut boot = jaguar_cd_boot_track(&body, 2 * CD_FRAME_SIZE);
        boot.resize(boot.len().div_ceil(2048) * 2048, 0);
        shared.extend_from_slice(&boot);
        std::fs::write(dir.path().join("shared.bin"), shared).unwrap();
        std::fs::write(
            &cue,
            "REM SESSION 01\n\
             FILE \"shared.bin\" BINARY\n\
               TRACK 01 AUDIO\n\
                 INDEX 01 00:00:00\n\
             REM SESSION 02\n\
               TRACK 02 MODE1/2048\n\
                 INDEX 00 00:00:02\n\
                 INDEX 01 00:00:03\n",
        )
        .unwrap();

        let location = cue_first_track_of_second_session(&cue).unwrap();
        assert_eq!(location.data_offset, (CD_FRAME_SIZE * 2 + 2048) as u64);
        let got = compute_disc_rc_hash("atari_jaguarcd", &cue)
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        assert_eq!(got, md5_digest_hex(expected.finalize().into()));
    }

    #[test]
    fn jaguar_cd_cue_mode1_2048_hashes_contiguous_payload() {
        let body: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let dir = tempfile::tempdir().unwrap();
        let cue = dir.path().join("game.cue");
        let track = jaguar_cd_boot_track(&body, 3 * CD_FRAME_SIZE);
        let padded_size = track.len().div_ceil(2048) * 2048;
        let mut cooked = track;
        cooked.resize(padded_size, 0);
        std::fs::write(dir.path().join("track02.bin"), cooked).unwrap();
        std::fs::write(
            &cue,
            "REM SESSION 01\n\
             FILE \"track01.bin\" BINARY\n\
               TRACK 01 AUDIO\n\
                 INDEX 01 00:00:00\n\
             REM SESSION 02\n\
             FILE \"track02.bin\" BINARY\n\
               TRACK 02 MODE1/2048\n\
                 INDEX 01 00:00:00\n",
        )
        .unwrap();

        let got = compute_disc_rc_hash("atari_jaguarcd", &cue)
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        assert_eq!(got, md5_digest_hex(expected.finalize().into()));
    }

    fn append_cdi_track_descriptor(header: &mut Vec<u8>) {
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&CDI_TRACK_MARKER);
        header.extend_from_slice(&[0; 4]);
        header.push(0);
        header.extend_from_slice(&[0; 19]);
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes());
        let mut track_data = [0u8; 0x57];
        track_data[0x04..0x08].copy_from_slice(&1u32.to_le_bytes());
        track_data[0x22..0x26].copy_from_slice(&1u32.to_le_bytes());
        track_data[0x36..0x3A].copy_from_slice(&2u32.to_le_bytes());
        header.extend_from_slice(&track_data);
        header.extend_from_slice(&[0; 5]);
        header.extend_from_slice(&0u32.to_le_bytes());
    }

    #[test]
    fn jaguar_cd_cdi_locates_shifted_second_session_boot_track() {
        let body: Vec<u8> = (0..700u32).map(|i| (i % 251) as u8).collect();
        let mut boot_frame = [0u8; CD_FRAME_SIZE];
        let header_offset = 66;
        boot_frame[header_offset..header_offset + 32]
            .copy_from_slice(b"ATARI APPROVED DATA HEADER ATRI ");
        let size_offset = header_offset + 36;
        boot_frame[size_offset..size_offset + 4]
            .copy_from_slice(&(body.len() as u32).to_be_bytes());
        boot_frame[size_offset + 4..size_offset + 4 + body.len()].copy_from_slice(&body);

        // Model a common CDI quirk: image payload starts later than the
        // descriptor's cumulative track lengths claim.
        let mut image = vec![0u8; 128];
        image.extend_from_slice(&[0u8; CD_FRAME_SIZE]);
        image.extend_from_slice(&boot_frame);
        let header_offset = image.len() as u32;
        let mut descriptor = Vec::new();
        descriptor.extend_from_slice(&2u16.to_le_bytes());
        for _ in 0..2 {
            descriptor.extend_from_slice(&1u16.to_le_bytes());
            append_cdi_track_descriptor(&mut descriptor);
            descriptor.extend_from_slice(&[0; 13]);
        }
        image.extend_from_slice(&descriptor);
        image.extend_from_slice(&CDI_V3_ID.to_le_bytes());
        image.extend_from_slice(&header_offset.to_le_bytes());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.cdi");
        std::fs::write(&path, image).unwrap();
        let got = compute_disc_rc_hash("atari_jaguarcd", &path)
            .unwrap()
            .unwrap();
        let mut expected = Md5::new();
        expected.update(body);
        assert_eq!(got, md5_digest_hex(expected.finalize().into()));
    }

    #[test]
    fn dreamcast_recipe_hashes_ipbin_and_boot_exe() {
        // IP.BIN @ sector 0 (SEGAKATANA + boot name @96); ISO9660 PVD @16,
        // root @20, 1ST_READ.BIN @22. md5 = md5(256-byte IP.BIN ++ exe bytes).
        let mut sectors = vec![[0u8; SECTOR_USER]; 30];
        sectors[0][0..16].copy_from_slice(b"SEGA SEGAKATANA ");
        sectors[0][96..96 + 12].copy_from_slice(b"1ST_READ.BIN");
        sectors[0][108] = b' ';
        for (i, b) in sectors[0].iter_mut().take(256).enumerate().skip(112) {
            *b = (i as u8).wrapping_mul(11);
        }
        sectors[16][1..6].copy_from_slice(b"CD001");
        let root = dir_record("\u{0}", 20, SECTOR_USER as u32);
        sectors[16][156..156 + root.len()].copy_from_slice(&root);
        let rec = dir_record("1ST_READ.BIN;1", 22, 80);
        sectors[20][..rec.len()].copy_from_slice(&rec);
        for (i, b) in sectors[22].iter_mut().take(80).enumerate() {
            *b = (i as u8).wrapping_mul(7);
        }
        let got = dreamcast_hash(&mut MemDisc {
            sectors: sectors.clone(),
        })
        .unwrap()
        .unwrap();
        let mut exp = Md5::new();
        exp.update(&sectors[0][0..256]);
        let body: Vec<u8> = (0..80u8).map(|i| i.wrapping_mul(7)).collect();
        exp.update(&body);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn three_do_recipe_hashes_header_and_launchme() {
        // Opera volume @0 (id + block_size 2048 + root ptr 1); dir block @1 with
        // a LaunchMe file entry pointing at sector 2 (100 bytes).
        let mut sectors = vec![[0u8; SECTOR_USER]; 5];
        sectors[0][0..7].copy_from_slice(&[0x01, 0x5A, 0x5A, 0x5A, 0x5A, 0x5A, 0x01]);
        sectors[0][0x4D..0x50].copy_from_slice(&[0x00, 0x08, 0x00]); // block_size 2048
        sectors[0][0x65..0x68].copy_from_slice(&[0x00, 0x00, 0x01]); // root block 1
        // dir block @ sector 1
        sectors[1][0x12] = 0x00;
        sectors[1][0x13] = 0x14; // first entry offset = 20
        sectors[1][0x0D..0x10].copy_from_slice(&[0x00, 0x00, 0xFF]); // stop = 255
        let e = 20usize;
        sectors[1][e + 0x03] = 0x02; // file flag
        sectors[1][e + 0x0D..e + 0x10].copy_from_slice(&[0x00, 0x08, 0x00]); // block_size 2048
        sectors[1][e + 0x11..e + 0x14].copy_from_slice(&[0x00, 0x00, 0x64]); // size 100
        sectors[1][e + 0x45..e + 0x48].copy_from_slice(&[0x00, 0x00, 0x02]); // block_location 2
        sectors[1][e + 0x20..e + 0x20 + 8].copy_from_slice(b"LaunchMe");
        for (i, b) in sectors[2].iter_mut().take(100).enumerate() {
            *b = (i as u8).wrapping_mul(13);
        }
        let got = three_do_hash(&mut MemDisc {
            sectors: sectors.clone(),
        })
        .unwrap()
        .unwrap();
        let mut exp = Md5::new();
        exp.update(&sectors[0][0..132]);
        let body: Vec<u8> = (0..100u8).map(|i| i.wrapping_mul(13)).collect();
        exp.update(&body);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn pce_cd_recipe_hashes_title_and_boot_sectors() {
        // No PCE-CD discs on the device → this synthetic disc is the only check.
        let mut sectors = vec![[0u8; SECTOR_USER]; 5];
        sectors[1][32..32 + 23].copy_from_slice(b"PC Engine CD-ROM SYSTEM");
        let title: [u8; 22] = *b"GAME TITLE EXAMPLE  ZZ";
        sectors[1][106..128].copy_from_slice(&title);
        sectors[1][0..4].copy_from_slice(&[0, 0, 2, 1]); // boot: 1 sector @ LBA 2
        for (i, b) in sectors[2].iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(5);
        }
        let got = pce_cd_hash(&mut MemDisc { sectors }).unwrap().unwrap();
        let mut exp = Md5::new();
        exp.update(title);
        let boot: Vec<u8> = (0..2048).map(|i| (i as u8).wrapping_mul(5)).collect();
        exp.update(&boot);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn neogeo_cd_recipe_hashes_referenced_prg() {
        // No Neo Geo CD discs on the device → this synthetic disc is the only check.
        let mut sectors = vec![[0u8; SECTOR_USER]; 30];
        sectors[16][1..6].copy_from_slice(b"CD001");
        let root = dir_record("\u{0}", 20, SECTOR_USER as u32);
        sectors[16][156..156 + root.len()].copy_from_slice(&root);
        let mut off = 0;
        for r in [
            dir_record("IPL.TXT;1", 21, 32),
            dir_record("ABS.PRG;1", 22, 80),
        ] {
            sectors[20][off..off + r.len()].copy_from_slice(&r);
            off += r.len();
        }
        let ipl = b"ABS.PRG,0,0\r\n";
        sectors[21][..ipl.len()].copy_from_slice(ipl);
        for (i, b) in sectors[22].iter_mut().take(80).enumerate() {
            *b = (i as u8).wrapping_mul(3);
        }
        let got = neogeo_cd_hash(&mut MemDisc { sectors }).unwrap().unwrap();
        let mut exp = Md5::new();
        let body: Vec<u8> = (0..80u8).map(|i| i.wrapping_mul(3)).collect();
        exp.update(&body);
        assert_eq!(got, md5_digest_hex(exp.finalize().into()));
    }

    #[test]
    fn m3u_accepts_wii_and_gamecube_images() {
        assert!(is_disc_rc_hash_system("nintendo_wii"));
        for file in ["disc.wbfs", "disc.rvz", "disc.wia", "disc.gcm", "disc.wad"] {
            assert!(looks_like_m3u_reference(file), "{file}");
        }
    }

    fn write_sega_cd_bin(path: &Path) {
        let mut sector = [0u8; SECTOR_USER];
        sector[0..16].copy_from_slice(b"SEGADISCSYSTEM  ");
        for (i, b) in sector.iter_mut().take(512).enumerate().skip(16) {
            *b = (i as u8).wrapping_mul(3);
        }
        std::fs::write(path, sector).unwrap();
    }

    #[tokio::test]
    async fn m3u_hash_uses_first_referenced_disc_but_returns_playlist_key() {
        let temp = tempfile::tempdir().unwrap();
        let roms_dir = temp.path().join("roms").join("sega_cd");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let disc = roms_dir.join("Game (Disc 1).bin");
        let m3u = roms_dir.join("Game.m3u");
        write_sega_cd_bin(&disc);
        std::fs::write(&m3u, "Game (Disc 1).bin\n").unwrap();

        let expected = compute_disc_rc_hash("sega_cd", &disc).unwrap();
        let result = hash_and_identify_discs(
            "sega_cd",
            &[(
                "Game.m3u".to_string(),
                "/roms/sega_cd/Game.m3u".to_string(),
                std::fs::metadata(&m3u).unwrap().len(),
            )],
            &std::collections::HashMap::new(),
            temp.path(),
            rom_hash::HashOptions::default(),
            None,
        )
        .await;

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].rom_filename, "Game.m3u");
        assert_eq!(result.results[0].rc_hash, expected);
        assert_eq!(
            result.results[0].size_bytes,
            std::fs::metadata(&disc).unwrap().len()
        );
        assert_eq!(result.stats.computed, 1);
    }

    #[tokio::test]
    async fn m3u_hash_reuses_stored_playlist_hash_against_referenced_disc_identity() {
        let temp = tempfile::tempdir().unwrap();
        let roms_dir = temp.path().join("roms").join("sega_cd");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let disc = roms_dir.join("Game (Disc 1).bin");
        let m3u = roms_dir.join("Game.m3u");
        write_sega_cd_bin(&disc);
        std::fs::write(&m3u, "Game (Disc 1).bin\n").unwrap();

        let first = hash_and_identify_discs(
            "sega_cd",
            &[(
                "Game.m3u".to_string(),
                "/roms/sega_cd/Game.m3u".to_string(),
                std::fs::metadata(&m3u).unwrap().len(),
            )],
            &std::collections::HashMap::new(),
            temp.path(),
            rom_hash::HashOptions::default(),
            None,
        )
        .await;
        let first = first.results.into_iter().next().unwrap();

        let mut stored_hashes = std::collections::HashMap::new();
        stored_hashes.insert(
            "Game.m3u".to_string(),
            rom_hash::StoredHash {
                crc32: first.crc32,
                hash_mtime: first.mtime_secs,
                hash_size_bytes: Some(first.size_bytes),
                matched_name: first.matched_name.clone(),
                rc_hash: first.rc_hash.clone(),
                ra_id: first.ra_id.clone(),
            },
        );

        let second = hash_and_identify_discs(
            "sega_cd",
            &[(
                "Game.m3u".to_string(),
                "/roms/sega_cd/Game.m3u".to_string(),
                std::fs::metadata(&m3u).unwrap().len(),
            )],
            &stored_hashes,
            temp.path(),
            rom_hash::HashOptions::default(),
            None,
        )
        .await;

        assert_eq!(second.results.len(), 1);
        assert_eq!(second.results[0].rom_filename, "Game.m3u");
        assert_eq!(second.results[0].rc_hash, first.rc_hash);
        assert_eq!(second.stats.reused_exact, 1);
        assert_eq!(second.stats.computed, 0);
    }

    #[test]
    fn cue_with_missing_bin_is_unidentifiable_not_error() {
        // A .cue whose referenced .bin is absent must resolve to Ok(None)
        // (terminal "unmatched"), NOT Err — otherwise the identity phase keeps it
        // Failed and re-claims it forever (which flooded the log and crashed the
        // service). Regression guard for that bug.
        let temp = tempfile::tempdir().unwrap();
        let cue = temp.path().join("Game.cue");
        std::fs::write(&cue, "FILE \"Game.bin\" BINARY\n  TRACK 01 MODE1/2352\n").unwrap();
        assert_eq!(compute_disc_rc_hash("sega_st", &cue).unwrap(), None);
    }
}
