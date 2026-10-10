//! rcheevos `rc_hash_wii`: disc headers, non-update partitions, or WAD content.
//! Compressed discs are read through nod with encryption rebuilt so their hash
//! matches the raw encrypted image expected by RetroAchievements.

use super::{gamecube, md5_digest_hex};
use md5::{Digest, Md5};
use nod::{Disc, OpenOptions};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const CLUSTER_DATA_SIZE: usize = 0x7c00;
const MAX_CLUSTER_COUNT: u64 = 1024;
const MAX_BUFFER_SIZE: u64 = 64 * 1024 * 1024;
const MAX_PARTITION_COUNT: u32 = 128;

pub(super) fn hash_path(path: &Path) -> io::Result<Option<String>> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "iso" | "wad" => hash(&mut File::open(path)?),
        "wbfs" | "rvz" | "wia" | "gcz" | "ciso" => {
            let options = OpenOptions {
                rebuild_encryption: true,
                ..Default::default()
            };
            let mut disc = Disc::new_with_options(path, &options).map_err(io::Error::other)?;
            hash(&mut disc)
        }
        _ => Ok(None),
    }
}

fn read_at(reader: &mut (impl Read + Seek), offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    reader.seek(SeekFrom::Start(offset))?;
    reader.read_exact(bytes)
}

fn read_u32(reader: &mut (impl Read + Seek), offset: u64) -> io::Result<u32> {
    let mut bytes = [0; 4];
    read_at(reader, offset, &mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

fn append_range(
    reader: &mut (impl Read + Seek),
    digest: &mut Md5,
    offset: u64,
    mut length: u64,
) -> io::Result<()> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut buffer = [0; CLUSTER_DATA_SIZE];
    while length > 0 {
        let count = length.min(buffer.len() as u64) as usize;
        reader.read_exact(&mut buffer[..count])?;
        digest.update(&buffer[..count]);
        length -= count as u64;
    }
    Ok(())
}

fn hash(reader: &mut (impl Read + Seek)) -> io::Result<Option<String>> {
    let mut magic = [0; 4];
    read_at(reader, 0x18, &mut magic)?;
    let mut digest = Md5::new();
    if magic == [0x5d, 0x1c, 0x9e, 0xa3] {
        if !append_disc(reader, &mut digest)? {
            return Ok(None);
        }
    } else {
        read_at(reader, 4, &mut magic)?;
        if magic != *b"Is\0\0" {
            return Ok(None);
        }
        append_wad(reader, &mut digest)?;
    }
    Ok(Some(md5_digest_hex(digest.finalize().into())))
}

fn append_disc(reader: &mut (impl Read + Seek), digest: &mut Md5) -> io::Result<bool> {
    let mut encryption_flag = [0];
    read_at(reader, 0x61, &mut encryption_flag)?;
    append_range(reader, digest, 0, 0x80)?;
    append_range(reader, digest, 0x4e000, 4)?;

    let mut partitions = Vec::new();
    for table in 0..4 {
        let count = read_u32(reader, 0x40000 + table * 8)?;
        if count > MAX_PARTITION_COUNT
            || partitions.len() + count as usize > MAX_PARTITION_COUNT as usize
        {
            return Ok(false);
        }
        let table_offset = u64::from(read_u32(reader, 0x40004 + table * 8)?) << 2;
        for entry in 0..count {
            let offset = table_offset + u64::from(entry) * 8;
            partitions.push((
                u64::from(read_u32(reader, offset)?) << 2,
                read_u32(reader, offset + 4)?,
            ));
        }
    }
    if partitions.is_empty() {
        return Ok(false);
    }

    for (partition_base, partition_type) in partitions {
        if partition_type == 1 {
            continue; // RePlayOS/rcheevos ignore update partitions.
        }
        let tmd_size =
            u64::from(read_u32(reader, partition_base + 0x2a4)?).min(CLUSTER_DATA_SIZE as u64);
        let tmd_offset = u64::from(read_u32(reader, partition_base + 0x2a8)?) << 2;
        append_range(reader, digest, partition_base + tmd_offset, tmd_size)?;

        let data_offset = u64::from(read_u32(reader, partition_base + 0x2b8)?) << 2;
        let data_size = u64::from(read_u32(reader, partition_base + 0x2bc)?) << 2;
        if encryption_flag[0] == 0 {
            // Match rcheevos' absolute data-offset addressing exactly.
            for cluster in 0..(data_size / 0x8000).min(MAX_CLUSTER_COUNT) {
                append_range(
                    reader,
                    digest,
                    data_offset + cluster * 0x8000 + 0x400,
                    CLUSTER_DATA_SIZE as u64,
                )?;
            }
        } else {
            gamecube::append_partition(reader, digest, data_offset, 2)?;
        }
    }
    Ok(true)
}

fn aligned(value: u64, boundary: u64) -> u64 {
    (value + boundary - 1) & !(boundary - 1)
}

fn append_wad(reader: &mut (impl Read + Seek), digest: &mut Md5) -> io::Result<()> {
    let cert_size = aligned(u64::from(read_u32(reader, 0x08)?), 0x40);
    let ticket_size = aligned(u64::from(read_u32(reader, 0x10)?), 0x40);
    let tmd_size = aligned(u64::from(read_u32(reader, 0x14)?), 0x40).min(MAX_BUFFER_SIZE);
    let tmd_start = 0x40 + cert_size + ticket_size;
    append_range(reader, digest, tmd_start, tmd_size)?;

    let mut count_bytes = [0; 2];
    read_at(reader, tmd_start + 0x1de, &mut count_bytes)?;
    let content_count = u16::from_be_bytes(count_bytes);
    let mut content_offset = tmd_start + tmd_size;
    for index in 0..content_count {
        let size_offset = tmd_start + 0x1e4 + 8 + u64::from(index) * 0x24;
        let high = read_u32(reader, size_offset)?;
        let content_size = if high == 0 {
            aligned(u64::from(read_u32(reader, size_offset + 4)?), 0x10)
        } else {
            MAX_BUFFER_SIZE
        };
        append_range(
            reader,
            digest,
            content_offset,
            content_size.min(MAX_BUFFER_SIZE),
        )?;
        content_offset = aligned(content_offset + content_size, 0x40);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encrypted_disc_skips_update_partition_and_hashes_data_cluster() {
        let mut disc = vec![0; 0x90000];
        disc[..6].copy_from_slice(b"GAME01");
        disc[0x18..0x1c].copy_from_slice(&[0x5d, 0x1c, 0x9e, 0xa3]);
        disc[0x4e000..0x4e004].copy_from_slice(b"USA!");
        disc[0x40000..0x40004].copy_from_slice(&2u32.to_be_bytes());
        disc[0x40004..0x40008].copy_from_slice(&(0x40020u32 / 4).to_be_bytes());
        disc[0x40020..0x40024].copy_from_slice(&(0x50000u32 / 4).to_be_bytes());
        disc[0x40024..0x40028].copy_from_slice(&1u32.to_be_bytes());
        disc[0x40028..0x4002c].copy_from_slice(&(0x60000u32 / 4).to_be_bytes());
        disc[0x60000 + 0x2a4..0x60000 + 0x2a8].copy_from_slice(&4u32.to_be_bytes());
        disc[0x60000 + 0x2a8..0x60000 + 0x2ac].copy_from_slice(&(0x300u32 / 4).to_be_bytes());
        disc[0x60300..0x60304].copy_from_slice(b"TMD!");
        disc[0x60000 + 0x2b8..0x60000 + 0x2bc].copy_from_slice(&(0x80000u32 / 4).to_be_bytes());
        disc[0x60000 + 0x2bc..0x60000 + 0x2c0].copy_from_slice(&(0x8000u32 / 4).to_be_bytes());
        disc[0x80400..0x88000].fill(0x5a);

        let mut expected = Md5::new();
        expected.update(&disc[..0x80]);
        expected.update(&disc[0x4e000..0x4e004]);
        expected.update(b"TMD!");
        expected.update(&disc[0x80400..0x88000]);
        let expected = md5_digest_hex(expected.finalize().into());
        assert_eq!(hash(&mut Cursor::new(disc)).unwrap(), Some(expected));
    }

    #[test]
    fn decrypted_disc_hashes_partition_apploader_and_dol() {
        let mut disc = vec![0; 0x84000];
        disc[0x18..0x1c].copy_from_slice(&[0x5d, 0x1c, 0x9e, 0xa3]);
        disc[0x61] = 1;
        disc[0x4e000..0x4e004].copy_from_slice(b"JPN!");
        disc[0x40000..0x40004].copy_from_slice(&1u32.to_be_bytes());
        disc[0x40004..0x40008].copy_from_slice(&(0x40020u32 / 4).to_be_bytes());
        disc[0x40020..0x40024].copy_from_slice(&(0x60000u32 / 4).to_be_bytes());
        disc[0x60000 + 0x2a4..0x60000 + 0x2a8].copy_from_slice(&4u32.to_be_bytes());
        disc[0x60000 + 0x2a8..0x60000 + 0x2ac].copy_from_slice(&(0x300u32 / 4).to_be_bytes());
        disc[0x60300..0x60304].copy_from_slice(b"TMD!");
        disc[0x60000 + 0x2b8..0x60000 + 0x2bc].copy_from_slice(&(0x80000u32 / 4).to_be_bytes());
        disc[0x80000 + 0x420..0x80000 + 0x424].copy_from_slice(&(0x3000u32 / 4).to_be_bytes());
        disc[0x83000..0x83004].copy_from_slice(&(0x3800u32 / 4).to_be_bytes());
        disc[0x83090..0x83094].copy_from_slice(&1u32.to_be_bytes());
        disc[0x83800..0x83804].copy_from_slice(b"DOL!");

        let mut expected = Md5::new();
        expected.update(&disc[..0x80]);
        expected.update(&disc[0x4e000..0x4e004]);
        expected.update(b"TMD!");
        expected.update(&disc[0x80000..0x82460]);
        expected.update(b"DOL!");
        assert_eq!(
            hash(&mut Cursor::new(disc)).unwrap(),
            Some(md5_digest_hex(expected.finalize().into()))
        );
    }

    #[test]
    fn wad_hashes_aligned_tmd_and_content() {
        let mut wad = vec![0; 0x400];
        wad[4..8].copy_from_slice(b"Is\0\0");
        wad[0x14..0x18].copy_from_slice(&0x200u32.to_be_bytes());
        wad[0x40..0x240].fill(0x31);
        wad[0x40 + 0x1de..0x40 + 0x1e0].copy_from_slice(&1u16.to_be_bytes());
        wad[0x40 + 0x1e4 + 8..0x40 + 0x1e4 + 12].fill(0);
        wad[0x40 + 0x1e4 + 12..0x40 + 0x1e4 + 16].copy_from_slice(&0x11u32.to_be_bytes());
        wad[0x240..0x260].fill(0x42);
        let mut expected = Md5::new();
        expected.update(&wad[0x40..0x240]);
        expected.update(&wad[0x240..0x260]);
        assert_eq!(
            hash(&mut Cursor::new(wad)).unwrap(),
            Some(md5_digest_hex(expected.finalize().into()))
        );
    }

    #[test]
    fn unknown_file_has_no_wii_hash() {
        assert_eq!(hash(&mut Cursor::new(vec![0; 0x100])).unwrap(), None);
    }
}
