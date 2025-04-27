use anyhow::{bail, Result};
use scroll::{IOread, LE};
use std::io::{Read, Seek, SeekFrom};

pub fn calculate_gz_decompressed_size<R: Read + Seek>(mut reader: R) -> Result<u64> {
    let footer_pos = reader.seek(SeekFrom::End(-4))?;
    if footer_pos < 14 {
        bail!("Invalid gzip compressed stream: stream too short");
    }
    let mut size: u64 = reader.ioread_with::<u32>(LE)?.into();
    // gzip only stores 32-bit file sizes, if the compressed size is larger than
    // our decoded value, that means the file size is too large to fit in 32 bits.
    if (footer_pos * 2) > size {
        // compensate for wrapped around 32-bit size
        size = (1 << 32) + size;
    }

    Ok(size)
}
