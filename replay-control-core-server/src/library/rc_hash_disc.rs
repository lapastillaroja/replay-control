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
//!
//! All yield the same MD5 the RA Web API ships in `ra_hash`, so the result is
//! resolved through the catalog `ra_hash` table exactly like header-cart rc_hash.
//! Ported from the validated PoC (`poc/ra-hash/src/rc_hash_disc.rs`); PSX, Sega
//! CD, Saturn, 3DO and Dreamcast validated on-device against the RA hash set.

use md5::{Digest, Md5};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::library::rom_hash;

/// rcheevos caps the bytes it MD5s at 64 MiB; mirror it for the PSX exe body.
const MAX_BUFFER_SIZE: usize = 64 * 1024 * 1024;
const SECTOR_USER: usize = 2048;

fn md5_hex(d: [u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

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

struct ChdReader {
    chd: chd::Chd<std::fs::File>,
    frames_per_hunk: usize,
    cmpbuf: Vec<u8>,
    hunk_cache: Vec<u8>,
    cached_hunk: Option<u32>,
    header_skip: usize, // 16 (mode1) or 24 (mode2 form1), detected from sector 16
}

impl ChdReader {
    fn open(path: &Path) -> io::Result<Self> {
        let f = std::fs::File::open(path)?;
        let chd =
            chd::Chd::open(f, None).map_err(|e| io::Error::other(format!("chd open: {e:?}")))?;
        let hunk_bytes = chd.header().hunk_size() as usize;
        let unit_bytes = chd.header().unit_bytes() as usize;
        let frames_per_hunk = hunk_bytes
            .checked_div(unit_bytes)
            .unwrap_or(hunk_bytes / CD_UNIT);
        let cmpbuf = chd.get_hunksized_buffer();
        let mut me = Self {
            chd,
            frames_per_hunk: frames_per_hunk.max(1),
            cmpbuf,
            hunk_cache: vec![0u8; hunk_bytes],
            cached_hunk: None,
            header_skip: 16,
        };
        me.header_skip = me.detect_header_skip()?;
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
        if !self.frame_raw(0, &mut raw)? {
            return Ok(16);
        }
        // No 12-byte sync at the start of a frame means the CHD stores cooked
        // user data at offset 0 (e.g. 3DO discs: the Opera ID lands at byte 0).
        // With a sync present it's a raw 2352 sector and the user data sits after
        // the 16-byte (Mode1) or 24-byte (Mode2 form1) header.
        if raw[..12] != SYNC {
            return Ok(0);
        }
        // Sector 16 holds the ISO9660 PVD: "CD001" follows the sector header.
        if self.frame_raw(16, &mut raw)? {
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
        let mut raw = [0u8; CD_FRAME_SIZE];
        if !self.frame_raw(sector, &mut raw)? {
            return Ok(0);
        }
        let skip = self.header_skip;
        out[..SECTOR_USER].copy_from_slice(&raw[skip..skip + SECTOR_USER]);
        Ok(SECTOR_USER)
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
    let root_lba = u32::from_le_bytes([root[2], root[3], root[4], root[5]]);
    let root_len = u32::from_le_bytes([root[10], root[11], root[12], root[13]]);
    let want = name.trim_start_matches('\\').to_ascii_uppercase();
    let sectors = root_len.div_ceil(SECTOR_USER as u32);
    for i in 0..sectors {
        if r.read_sector(root_lba + i, &mut sec)? == 0 {
            break;
        }
        let mut off = 0usize;
        while off < SECTOR_USER {
            let rlen = sec[off] as usize;
            if rlen == 0 {
                break; // rest of sector is padding
            }
            if off + rlen > SECTOR_USER {
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
    Ok(Some(md5_hex(md5.finalize().into())))
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
    Ok(Some(md5_hex(md5.finalize().into())))
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
    Ok(Some(md5_hex(md5.finalize().into())))
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
    Ok(Some(md5_hex(md5.finalize().into())))
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
        return Ok(Some(md5_hex(md5.finalize().into())));
    }
    if let Some((lba, size)) = iso_find_file(r, "BOOT.BIN")?
        && (size as usize) < MAX_BUFFER_SIZE
    {
        let mut md5 = Md5::new();
        md5.update(&read_file_bytes(r, lba, size)?);
        return Ok(Some(md5_hex(md5.finalize().into())));
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
    Ok(Some(md5_hex(md5.finalize().into())))
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
/// Saturn, Dreamcast (.gdi), 3DO, PC Engine CD, Neo Geo CD.
pub fn compute_disc_rc_hash(system: &str, path: &Path) -> io::Result<Option<String>> {
    // Dreamcast addresses GD-ROM track 3 (a .gdi), so it picks its own track
    // rather than the generic first-track reader.
    if system == "sega_dc" {
        return dreamcast_hash_path(path);
    }
    let Some(mut reader) = open_reader(path)? else {
        return Ok(None);
    };
    match system {
        "sony_psx" => psx_hash(reader.as_mut()),
        // Saturn shares rc_hash_sega_cd (one fn, also accepts "SEGA SEGASATURN ").
        "sega_cd" | "sega_st" => sega_cd_hash(reader.as_mut()),
        "panasonic_3do" => three_do_hash(reader.as_mut()),
        "nec_pcecd" => pce_cd_hash(reader.as_mut()),
        "snk_ngcd" => neogeo_cd_hash(reader.as_mut()),
        _ => Ok(None),
    }
}

/// Systems with a runtime disc-`rc_hash` recipe wired up (see
/// [`compute_disc_rc_hash`]). Mirror of the `match` above.
pub fn is_disc_rc_hash_system(system: &str) -> bool {
    matches!(
        system,
        "sony_psx" | "sega_cd" | "sega_st" | "sega_dc" | "panasonic_3do" | "nec_pcecd" | "snk_ngcd"
    )
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
    cached_hashes: &std::collections::HashMap<String, rom_hash::CachedHash>,
    storage_root: &Path,
    options: rom_hash::HashOptions,
    cancel: Option<Arc<AtomicBool>>,
) -> rom_hash::HashIdentifyResult {
    use rom_hash::{HashIdentifyResult, HashResult, HashStats, reusable_cached_hash};

    if !is_disc_rc_hash_system(system) {
        return HashIdentifyResult::default();
    }

    let rom_files = rom_files.to_vec();
    let cached = cached_hashes.clone();
    let root = storage_root.to_path_buf();
    let sys = system.to_string();
    let cancel_owned = cancel.clone();

    // CHD decompression is CPU/IO-heavy — keep it off the tokio workers.
    let (mut results, stats): (Vec<HashResult>, HashStats) =
        tokio::task::spawn_blocking(move || {
            let mut results = Vec::with_capacity(rom_files.len());
            let mut stats = HashStats::default();
            for (rom_filename, rom_path, size_bytes) in &rom_files {
                if cancel_owned
                    .as_ref()
                    .is_some_and(|c| c.load(Ordering::Relaxed))
                {
                    break;
                }
                let abs_path = root.join(rom_path.trim_start_matches('/'));
                let Some(current_mtime) = rom_hash::file_mtime_secs(&abs_path) else {
                    stats.skipped += 1;
                    continue;
                };
                // Reuse the persisted rc_hash when the file identity is unchanged.
                if !options.force_rehash
                    && let Some(c) = cached.get(rom_filename)
                    && let Some(reused) =
                        reusable_cached_hash(rom_filename, c, current_mtime, *size_bytes)
                {
                    stats.reused_exact += 1;
                    results.push(reused);
                    continue;
                }
                match compute_disc_rc_hash(&sys, &abs_path) {
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
                            size_bytes: *size_bytes,
                            matched_name: None,
                            ra_id: None,
                            rc_hash,
                        });
                    }
                    Err(e) => {
                        stats.skipped += 1;
                        tracing::debug!("disc rc_hash failed for {}: {e}", abs_path.display());
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
        assert_eq!(got, md5_hex(exp.finalize().into()));
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
