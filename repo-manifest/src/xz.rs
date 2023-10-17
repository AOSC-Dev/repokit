use anyhow::{bail, Result};
use scroll::{IOread, LE};
use std::io::{Read, Seek, SeekFrom};

fn read_varint<R: Read>(mut reader: R) -> Result<u64> {
    let mut v = 0u64;
    let mut d: u64;
    let mut shift = 0i32;

    loop {
        if shift == 63 {
            bail!("Bad shift value");
        }
        d = reader.ioread::<u8>()?.into();
        v |= ((d & 0x7f) as u64) << shift;
        shift += 7;

        if d & 0x80 == 0 {
            break;
        }
    }

    Ok(v)
}

pub fn calculate_xz_decompressed_size<R: Read + Seek>(mut reader: R) -> Result<u64> {
    let mut size: u64 = 0;
    reader.seek(SeekFrom::End(0))?;
    let mut pos = reader.stream_position()?;
    let mut buffer = [0u8; 2];
    let mut header_buffer = [0u8; 6];
    if pos & 3 != 0 {
        bail!("Invalid xz compressed stream: incorrect alignment");
    }
    loop {
        loop {
            if pos < 32 {
                bail!("Invalid xz compressed stream: bad stream length");
            }
            pos -= 4;
            reader.seek(SeekFrom::Start(pos + 2))?;
            reader.read_exact(&mut buffer)?;
            if buffer == [b'Y', b'Z'] {
                break;
            }
        }
        reader.seek(SeekFrom::Start(pos - 4))?;
        let new_pos = reader.ioread_with::<u32>(LE)?;
        pos -= ((new_pos as u64 + 1) << 2) + 8;
        reader.seek(SeekFrom::Start(pos + 1))?;
        let records = read_varint(&mut reader)?;
        for _ in 0..records {
            pos -= (read_varint(&mut reader)? + 3) & (!3);
            size += read_varint(&mut reader)?;
        }
        pos -= 12;
        reader.seek(SeekFrom::Start(pos))?;
        reader.read_exact(&mut header_buffer)?;
        if header_buffer != [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] {
            bail!("bad backward-header");
        }

        if pos < 1 {
            break;
        }
    }

    Ok(size)
}
