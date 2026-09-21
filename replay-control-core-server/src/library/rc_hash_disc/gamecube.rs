//! rcheevos rc_hash_gamecube, over the logical ISO bytes of ISO/RVZ images.
use super::md5_digest_hex;
use md5::{Digest, Md5};
use nod::Disc;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

pub(super) fn hash_path(path: &Path) -> io::Result<Option<String>> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "iso" => hash(&mut File::open(path)?),
        "rvz" => hash(&mut Disc::new(path).map_err(io::Error::other)?),
        _ => Ok(None),
    }
}

fn read_at(reader: &mut (impl Read + Seek), offset: u64, bytes: &mut [u8]) -> io::Result<()> {
    reader.seek(SeekFrom::Start(offset))?;
    reader.read_exact(bytes)
}

fn big_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes[..4].try_into().unwrap())
}

fn hash(reader: &mut (impl Read + Seek)) -> io::Result<Option<String>> {
    let mut magic = [0; 4];
    read_at(reader, 0x1c, &mut magic)?;
    if magic != [0xc2, 0x33, 0x9f, 0x3d] {
        return Ok(None);
    }
    let mut sizes = [0; 8];
    read_at(reader, 0x2454, &mut sizes)?;
    let size = (0x2460 + u64::from(big_u32(&sizes)) + u64::from(big_u32(&sizes[4..])))
        .min(1024 * 1024) as usize;
    let mut buffer = vec![0; 1024 * 1024];
    read_at(reader, 0, &mut buffer[..size])?;
    let mut digest = Md5::new();
    digest.update(&buffer[..size]);
    let dol_offset = u64::from(big_u32(&buffer[0x420..]));
    let mut segments = [0; 0xd8];
    read_at(reader, dol_offset, &mut segments)?;
    for index in 0..18 {
        let offset = u64::from(big_u32(&segments[index * 4..]));
        let mut remaining = u64::from(big_u32(&segments[0x90 + index * 4..]));
        // These are absolute disc offsets in rcheevos' recipe, not relative
        // to the DOL header. Preserve that identity even for unusual images.
        reader.seek(SeekFrom::Start(offset))?;
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            reader.read_exact(&mut buffer[..count])?;
            digest.update(&buffer[..count]);
            remaining -= count as u64;
        }
    }
    Ok(Some(md5_digest_hex(digest.finalize().into())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn selected_segments_and_truncation() {
        let mut disc = vec![0; 0x4000];
        disc[0x1c..0x20].copy_from_slice(&[0xc2, 0x33, 0x9f, 0x3d]);
        disc[0x420..0x424].copy_from_slice(&0x3000u32.to_be_bytes());
        disc[0x3000..0x3004].copy_from_slice(&0x3800u32.to_be_bytes());
        disc[0x3090..0x3094].copy_from_slice(&3u32.to_be_bytes());
        disc[0x3800..0x3803].copy_from_slice(b"abc");
        let mut expected = Md5::new();
        expected.update(&disc[..0x2460]);
        expected.update(b"abc");
        let expected = Some(md5_digest_hex(expected.finalize().into()));
        assert_eq!(hash(&mut Cursor::new(&disc)).unwrap(), expected);
        disc[0x3900] = 1; // unselected content must not affect identity
        assert_eq!(hash(&mut Cursor::new(&disc)).unwrap(), expected);
        disc.truncate(0x3802);
        assert_eq!(
            hash(&mut Cursor::new(&disc)).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
