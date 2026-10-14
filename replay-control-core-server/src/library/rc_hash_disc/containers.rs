//! PS2 compressed ISO readers. Reads are bounded to one block/sector; no whole
//! DVD image is expanded in RAM or written to the user's ROM storage.
use super::{SECTOR_USER, SectorReader};
use flate2::read::{DeflateDecoder, MultiGzDecoder};
use lz4_flex::block::decompress_into;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// CSO v0/v1 and ZSO, following PCSX2's CsoFileReader and maxcso's format
/// specification. Index entries are fetched on demand rather than allocating
/// a multi-megabyte index for each concurrently hashed DVD.
pub(super) struct CsoReader {
    file: File,
    length: u64,
    block_size: usize,
    shift: u8,
    lz4: bool,
    file_size: u64,
    index_end: u64,
    block: Vec<u8>,
    cached_block: Option<u64>,
}

impl CsoReader {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut header = [0; 24];
        file.read_exact(&mut header)?;
        let length = u64::from_le_bytes(header[8..16].try_into().unwrap());
        let block_size = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        if !matches!(&header[..4], b"CISO" | b"ZISO")
            || header[20] > 1
            || header[21] > 20
            || !(2048..=16 * 1024 * 1024).contains(&block_size)
            || !block_size.is_power_of_two()
            || length == 0
            || length > 16 * 1024 * 1024 * 1024
        {
            return Err(invalid("invalid or unsupported CSO/ZSO header"));
        }
        let index_end = 24 + (length.div_ceil(block_size as u64) + 1) * 4;
        if index_end > file_size {
            return Err(invalid("truncated CSO/ZSO index"));
        }
        Ok(Self {
            file,
            length,
            block_size,
            shift: header[21],
            lz4: header[0] == b'Z',
            file_size,
            index_end,
            block: vec![0; block_size],
            cached_block: None,
        })
    }

    fn load_block(&mut self, block: u64) -> io::Result<()> {
        if self.cached_block == Some(block) {
            return Ok(());
        }
        self.file.seek(SeekFrom::Start(24 + block * 4))?;
        let mut entries = [0; 8];
        self.file.read_exact(&mut entries)?;
        let first = u32::from_le_bytes(entries[..4].try_into().unwrap());
        let next = u32::from_le_bytes(entries[4..].try_into().unwrap());
        let start = u64::from(first & 0x7fffffff) << self.shift;
        let end = u64::from(next & 0x7fffffff) << self.shift;
        if start < self.index_end
            || end < start
            || end > self.file_size
            || end - start > self.block_size as u64 + (1u64 << self.shift)
        {
            return Err(invalid("invalid CSO/ZSO block offsets"));
        }
        self.file.seek(SeekFrom::Start(start))?;
        let expected =
            (self.length - block * self.block_size as u64).min(self.block_size as u64) as usize;
        if first & 0x80000000 != 0 {
            if end - start < expected as u64 {
                return Err(invalid("truncated uncompressed CSO block"));
            }
            self.file.read_exact(&mut self.block[..expected])?;
        } else {
            let mut compressed = vec![0; (end - start) as usize];
            self.file.read_exact(&mut compressed)?;
            if self.lz4 {
                // CSO alignment can append padding. LZ4 has no end marker, so
                // decode until the expected output size, ignoring index padding.
                decode_lz4_block(&compressed, &mut self.block[..expected])?;
            } else {
                let mut decoder = DeflateDecoder::new(&compressed[..]);
                decoder.read_exact(&mut self.block[..expected])?;
                let mut extra = [0];
                if decoder.read(&mut extra)? != 0 {
                    return Err(invalid("oversized CSO block"));
                }
            }
        }
        self.cached_block = Some(block);
        Ok(())
    }
}

// Find the end of the LZ4 sequence stream using its length fields, without
// interpreting padding as a new sequence. Actual decompression and reference
// validation remain the library's responsibility.
fn decode_lz4_block(input: &[u8], output: &mut [u8]) -> io::Result<()> {
    let mut pos = 0usize;
    let mut produced = 0usize;
    while produced < output.len() {
        let token = *input
            .get(pos)
            .ok_or_else(|| invalid("truncated LZ4 token"))?;
        pos += 1;
        let mut length = |base: usize| -> io::Result<usize> {
            let mut size = base;
            if base == 15 {
                loop {
                    let byte = *input
                        .get(pos)
                        .ok_or_else(|| invalid("truncated LZ4 length"))?;
                    pos += 1;
                    size = size
                        .checked_add(byte as usize)
                        .ok_or_else(|| invalid("LZ4 length overflow"))?;
                    if byte != 255 {
                        break;
                    }
                }
            }
            Ok(size)
        };
        let literals = length((token >> 4) as usize)?;
        pos = pos
            .checked_add(literals)
            .ok_or_else(|| invalid("LZ4 offset overflow"))?;
        produced = produced
            .checked_add(literals)
            .ok_or_else(|| invalid("LZ4 output overflow"))?;
        if pos > input.len() || produced > output.len() {
            return Err(invalid("invalid LZ4 literals"));
        }
        if produced == output.len() {
            break;
        }
        pos += 2; // match distance, validated by lz4_flex
        if pos > input.len() {
            return Err(invalid("truncated LZ4 match distance"));
        }
        let mut matched = (token & 15) as usize + 4;
        if token & 15 == 15 {
            loop {
                let byte = *input
                    .get(pos)
                    .ok_or_else(|| invalid("truncated LZ4 match"))?;
                pos += 1;
                matched = matched
                    .checked_add(byte as usize)
                    .ok_or_else(|| invalid("LZ4 match overflow"))?;
                if byte != 255 {
                    break;
                }
            }
        }
        produced = produced
            .checked_add(matched)
            .ok_or_else(|| invalid("LZ4 output overflow"))?;
        if produced > output.len() {
            return Err(invalid("oversized LZ4 match"));
        }
    }
    let actual = decompress_into(&input[..pos], output).map_err(io::Error::other)?;
    if actual != output.len() {
        return Err(invalid("short LZ4 block"));
    }
    Ok(())
}

impl SectorReader for CsoReader {
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
        let offset = u64::from(sector) * SECTOR_USER as u64;
        if offset >= self.length {
            return Ok(0);
        }
        let block = offset / self.block_size as u64;
        self.load_block(block)?;
        let within = (offset % self.block_size as u64) as usize;
        let count = (self.length - offset).min(SECTOR_USER as u64) as usize;
        out[..count].copy_from_slice(&self.block[within..within + count]);
        Ok(count)
    }
}

/// Gzip is sequential. Backward seeks restart decoding; forward seeks discard
/// bytes in bounded buffers. This avoids expanding DVD-sized images to disk.
pub(super) struct GzipReader {
    path: PathBuf,
    decoder: MultiGzDecoder<File>,
    position: u64,
}

impl GzipReader {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            decoder: MultiGzDecoder::new(File::open(path)?),
            position: 0,
        })
    }
}

impl SectorReader for GzipReader {
    fn read_sector(&mut self, sector: u32, out: &mut [u8]) -> io::Result<usize> {
        let target = u64::from(sector) * SECTOR_USER as u64;
        if target > 16 * 1024 * 1024 * 1024 {
            return Err(invalid("gzip disc offset exceeds DVD limit"));
        }
        if target < self.position {
            self.decoder = MultiGzDecoder::new(File::open(&self.path)?);
            self.position = 0;
        }
        let mut discard = [0; 64 * 1024];
        while self.position < target {
            let count = (target - self.position).min(discard.len() as u64) as usize;
            self.decoder.read_exact(&mut discard[..count])?;
            self.position += count as u64;
        }
        let mut count = 0;
        while count < SECTOR_USER {
            let read = self.decoder.read(&mut out[count..SECTOR_USER])?;
            if read == 0 {
                break;
            }
            count += read;
            self.position += read as u64;
        }
        Ok(count)
    }
}
