//! PS2 BOOT2 executable hashing, matching rcheevos rc_hash_ps2.
use super::containers::{CsoReader, GzipReader};
use super::{
    ChdReader, MAX_BUFFER_SIZE, RawBinReader, SECTOR_USER, SectorReader, cue_selected_track,
    iso_find_file, iso_find_path, md5_digest_hex,
};
use md5::{Digest, Md5};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub(super) fn hash_path(path: &Path) -> io::Result<Option<String>> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut reader: Box<dyn SectorReader> = match extension.as_str() {
        "iso" => Box::new(RawBinReader {
            file: File::open(path)?,
            raw_sector_size: 2048,
            header: 0,
        }),
        "chd" => Box::new(ChdReader::open(path)?),
        "cso" | "ciso" | "zso" => Box::new(CsoReader::open(path)?),
        "gz" => Box::new(GzipReader::open(path)?),
        "cue" => {
            let track = cue_selected_track(path, |track| track.number == 1)?;
            if track.layout.payload_size != 2048 {
                return Ok(None);
            }
            Box::new(CueReader {
                file: File::open(track.path)?,
                base: track.data_offset + track.layout.payload_offset,
                stride: track.layout.physical_size,
            })
        }
        // Standalone ELFs have no SYSTEM.CNF/BOOT2 identity. They remain
        // launchable library entries, but must not fabricate a disc RA hash.
        _ => return Ok(None),
    };
    hash(reader.as_mut())
}

struct CueReader {
    file: File,
    base: u64,
    stride: u64,
}

impl SectorReader for CueReader {
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
        self.file
            .seek(SeekFrom::Start(self.base + u64::from(sector) * self.stride))?;
        self.file.read_exact(&mut out[..SECTOR_USER])?;
        Ok(SECTOR_USER)
    }
}

fn boot_name(cnf: &[u8]) -> Option<String> {
    let cnf = std::str::from_utf8(cnf.split(|byte| *byte == 0).next()?).ok()?;
    for line in cnf.lines() {
        let Some(value) = line
            .strip_prefix("BOOT2")
            .and_then(|s| s.trim_start().strip_prefix('='))
        else {
            continue;
        };
        let value = value.trim_start();
        let value = value
            .strip_prefix("cdrom0:")
            .unwrap_or(value)
            .trim_start_matches('\\');
        let name = value
            .split(|c: char| c == ';' || c.is_ascii_whitespace())
            .next()?;
        if !name.is_empty() && name.len() < 64 {
            return Some(name.to_string());
        }
    }
    None
}

fn hash(reader: &mut dyn SectorReader) -> io::Result<Option<String>> {
    let Some((cnf_sector, _)) = iso_find_file(reader, "SYSTEM.CNF")? else {
        return Ok(None);
    };
    let mut bytes = [0; SECTOR_USER];
    let read = reader.read_sector(cnf_sector, &mut bytes)?;
    let Some(name) = boot_name(&bytes[..read.min(2047)]) else {
        return Ok(None);
    };
    let parts: Vec<_> = name.split('\\').collect();
    let Some((mut sector, size)) = iso_find_path(reader, &parts)? else {
        return Ok(None);
    };
    let mut remaining = (size as usize).min(MAX_BUFFER_SIZE);
    if remaining == 0 {
        return Ok(None);
    }
    let mut digest = Md5::new();
    digest.update(name.as_bytes());
    while remaining > 0 {
        let count = remaining.min(SECTOR_USER);
        if reader.read_sector(sector, &mut bytes)? < count {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated PS2 executable",
            ));
        }
        digest.update(&bytes[..count]);
        remaining -= count;
        sector = sector
            .checked_add(1)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "PS2 sector overflow"))?;
    }
    Ok(Some(md5_digest_hex(digest.finalize().into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{
        Compression,
        write::{DeflateEncoder, GzEncoder},
    };
    use lz4_flex::block::compress;
    use std::io::Write;

    fn record(name: &[u8], sector: u32, size: u32) -> Vec<u8> {
        let mut record = vec![0; (33 + name.len()).next_multiple_of(2)];
        record[0] = record.len() as u8;
        record[2..6].copy_from_slice(&sector.to_le_bytes());
        record[10..14].copy_from_slice(&size.to_le_bytes());
        record[32] = name.len() as u8;
        record[33..33 + name.len()].copy_from_slice(name);
        record
    }

    fn fixture() -> Vec<u8> {
        // 147 cooked sectors: also divisible by 2352. This guards against
        // guessing raw-CD framing from an ISO's file length.
        let mut iso = vec![0; 147 * SECTOR_USER];
        iso[16 * SECTOR_USER + 1..16 * SECTOR_USER + 6].copy_from_slice(b"CD001");
        let root = record(b"\0", 20, 2048);
        iso[16 * SECTOR_USER + 156..16 * SECTOR_USER + 156 + root.len()].copy_from_slice(&root);
        let cnf = b"BOOT2 = cdrom0:\\SLUS_123.45;1\n";
        let entries = [
            record(b"SYSTEM.CNF;1", 21, cnf.len() as u32),
            record(b"SLUS_123.45;1", 22, 3000),
        ]
        .concat();
        iso[20 * SECTOR_USER..20 * SECTOR_USER + entries.len()].copy_from_slice(&entries);
        iso[21 * SECTOR_USER..21 * SECTOR_USER + cnf.len()].copy_from_slice(cnf);
        for (index, byte) in iso[22 * SECTOR_USER..22 * SECTOR_USER + 3000]
            .iter_mut()
            .enumerate()
        {
            *byte = index as u8;
        }
        iso
    }

    fn compressed_iso(iso: &[u8], lz4: bool) -> Vec<u8> {
        let blocks = iso.len() / SECTOR_USER;
        let mut output = vec![0; 24 + (blocks + 1) * 4];
        output[..4].copy_from_slice(if lz4 { b"ZISO" } else { b"CISO" });
        output[4..8].copy_from_slice(&24u32.to_le_bytes());
        output[8..16].copy_from_slice(&(iso.len() as u64).to_le_bytes());
        output[16..20].copy_from_slice(&2048u32.to_le_bytes());
        output[20] = 1;
        output[21] = 2; // alignment padding must not become compressed content
        for (index, block) in iso.chunks_exact(SECTOR_USER).enumerate() {
            output.resize(output.len().next_multiple_of(4), 0);
            let raw = index % 2 == 0;
            let offset = (output.len() as u32 >> 2) | if raw { 0x80000000 } else { 0 };
            output[24 + index * 4..28 + index * 4].copy_from_slice(&offset.to_le_bytes());
            if raw {
                output.extend_from_slice(block);
            } else if lz4 {
                output.extend_from_slice(&compress(block));
            } else {
                let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                encoder.write_all(block).unwrap();
                output.extend_from_slice(&encoder.finish().unwrap());
            }
        }
        output.resize(output.len().next_multiple_of(4), 0);
        let end = output.len() as u32 >> 2;
        output[24 + blocks * 4..28 + blocks * 4].copy_from_slice(&end.to_le_bytes());
        output
    }

    #[test]
    fn iso_cso_zso_gzip_and_cue_have_identical_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let iso = fixture();
        let mut expected = Md5::new();
        expected.update(b"SLUS_123.45");
        expected.update(&iso[22 * SECTOR_USER..22 * SECTOR_USER + 3000]);
        let expected = Some(md5_digest_hex(expected.finalize().into()));
        let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
        gzip.write_all(&iso).unwrap();
        for (name, bytes) in [
            ("game.iso", iso.clone()),
            ("game.cso", compressed_iso(&iso, false)),
            ("game.zso", compressed_iso(&iso, true)),
            ("game.iso.gz", gzip.finish().unwrap()),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(hash_path(&path).unwrap(), expected, "{name}");
        }
        let cue = dir.path().join("game.cue");
        // Stored pregap, with MODE1/2352 frames, rather than guessed framing.
        let mut bin = vec![0; 2352 * 2];
        for sector in iso.chunks_exact(SECTOR_USER) {
            bin.extend_from_slice(&[0; 16]);
            bin.extend_from_slice(sector);
            bin.extend_from_slice(&[0; 288]);
        }
        std::fs::write(dir.path().join("game.bin"), bin).unwrap();
        std::fs::write(
            &cue,
            "FILE \"game.bin\" BINARY\n TRACK 01 MODE1/2352\n INDEX 01 00:00:02\n",
        )
        .unwrap();
        assert_eq!(hash_path(&cue).unwrap(), expected);
        let path = dir.path().join("truncated.iso");
        std::fs::write(&path, &iso[..22 * SECTOR_USER + 2999]).unwrap();
        assert_eq!(
            hash_path(&path).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn boot2_is_not_ps1_boot() {
        assert_eq!(
            boot_name(b"BOOT = cdrom:\\WRONG;1\nBOOT2 = cdrom0:\\DIR\\SLUS_123.45;1\r\n"),
            Some("DIR\\SLUS_123.45".into())
        );
        assert_eq!(boot_name(b"BOOT = cdrom:\\PSX.EXE;1"), None);
        assert_eq!(boot_name(b"BOOT20 = cdrom0:\\WRONG;1"), None);
    }
}
