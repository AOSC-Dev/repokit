use anyhow::{bail, Result};
use scroll::{Cread, Pread as Pread_, LE};
use scroll_derive::Pread;
use std::{convert::TryInto, io::Read, path::Path};

// const COMPRESSION_TYPE: &[&str] = &["gzip", "lzo", "lzma", "xz", "lz4", "zstd"];
const RECORD_SIZES: &[u64] = &[0, 16, 0, 8, 8, 8, 4, 4, 0, 0, 8, 12, 12, 8, 8];

/// Collects the size of the squashfs file and the number of inodes.
///
/// Returns (size of the file, number of inodes)
pub fn collect_squashfs_size_and_inodes<P: AsRef<Path>>(input: P) -> Result<(u64, u32)> {
    let f = std::fs::File::open(input)?;
    let f = unsafe { memmap2::Mmap::map(&f)? };
    let super_block = parse_super_block(&f)?;
    let inode_tbl = &f[(super_block.inode_tbl as usize)..(super_block.dir_tbl as usize)];
    let inode_tbl = collect_inodes_table(inode_tbl)?;
    let full_size = collect_inodes_size(&inode_tbl, super_block.blksize)?;

    Ok((full_size, super_block.inode))
}

fn collect_inodes_size(decoded_data: &[u8], block_size: u32) -> Result<u64> {
    let mut pos = 0usize;
    let mut total_size = 0u64;

    while pos < decoded_data.len() {
        let (size, offset) = sizeof_inode(&decoded_data[pos..], block_size);
        if offset < 1 {
            bail!("invalid offset found in inode table at byte {}", pos);
        }
        total_size += size;
        pos += offset as usize + 16;
    }

    Ok(total_size)
}

fn collect_inodes_table(data: &[u8]) -> Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity(8192);
    let mut pos = 0usize;

    while pos < data.len() {
        // decode each block
        let block_header = data.cread::<u16>(pos);
        let compressed = (block_header & 0x8000) == 0;
        let block_size = block_header & 0x7fff;
        let block_end = pos + 2 + block_size as usize;
        if compressed {
            let mut decoder = xz2::read::XzDecoder::new(&data[(pos + 2)..(block_end)]);
            decoder.read_to_end(&mut buffer)?;
        } else {
            // just copy the data over
            buffer.extend_from_slice(&data[(pos + 2)..(block_end)]);
        }
        pos = block_end;
    }

    Ok(buffer)
}

#[derive(Debug, Copy, Clone, Pread)]
#[allow(dead_code)]
struct SqsSuper {
    magic: u32,
    inode: u32,
    mtime: u32,
    blksize: u32,
    frag: u32,
    compression: u16,
    blklog: u16,
    flags: u16,
    ids: u16,
    ver_major: u16,
    ver_minor: u16,
    root_inode: u64,
    bytes: u64,
    id_tbl: u64,
    xattrs_tbl: u64,
    inode_tbl: u64,
    dir_tbl: u64,
    frag_tbl: u64,
    export_tbl: u64,
}

#[derive(Debug, Copy, Clone, Pread)]
#[allow(dead_code)]
struct InodeHeader {
    inode_type: u16,
    permissions: u16,
    uid: u16,
    gid: u16,
    mtime: u32,
    inode_number: u32,
}

#[derive(Debug, Copy, Clone, Pread)]
#[allow(dead_code)]
struct FileInodeHeader {
    start: u32,
    frag_index: u32,
    offset: u32,
    size: u32,
    // u32 block_sizes[]
}

#[derive(Debug, Copy, Clone, Pread)]
#[allow(dead_code)]
struct ExtendedFileInodeHeader {
    start: u64,
    size: u64,
    sparse: u64,
    links: u32,
    frag_index: u32,
    offset: u32,
    xattr: u32,
    // u32 block_sizes[]
}

#[derive(Debug, Copy, Clone, Pread)]
#[allow(dead_code)]
struct SymlinkInodeHeader {
    count: u32,
    size: u32,
    // u8 path[]
}

impl FileInodeHeader {
    fn block_count(&self, block_size: u32) -> u32 {
        let base_count = self.size / block_size;
        if self.frag_index == 0xFFFFFFFF {
            if self.size % block_size > 0 {
                base_count + 1
            } else {
                base_count
            }
        } else {
            base_count
        }
    }
}

impl ExtendedFileInodeHeader {
    fn block_count(&self, block_size: u32) -> u64 {
        let base_count = self.size / block_size as u64;
        if self.frag_index == 0xFFFFFFFF {
            if self.size % block_size as u64 > 0 {
                base_count + 1
            } else {
                base_count
            }
        } else {
            base_count
        }
    }
}

impl SymlinkInodeHeader {
    fn byte_count(&self) -> u32 {
        self.size
    }
}

fn sizeof_extended_dir(data: &[u8], count: u16) -> usize {
    let mut pos = 0usize;
    for _ in 0..count {
        let size = data.cread_with::<u32>(pos + 8, LE) + 1;
        pos += size as usize + 12;
    }

    pos
}

fn sizeof_inode(data: &[u8], block_size: u32) -> (u64, u64) {
    if data.len() < 16 {
        return (0, 0);
    }
    let record: InodeHeader = data.pread_with(0, LE).unwrap();
    return match record.inode_type {
        1 | 4..=7 | 11..=14 => (0, RECORD_SIZES[record.inode_type as usize]),
        // file inode type
        2 => {
            let record: FileInodeHeader = data.pread_with(16, LE).unwrap();

            (
                record.size.into(),
                record.block_count(block_size) as u64 * 4u64 + 16,
            )
        }
        3 => {
            let record: SymlinkInodeHeader = data.pread_with(16, LE).unwrap();

            (0, record.byte_count() as u64 + 8)
        }
        // extended directory (common header size = 16; size offset = 4)
        8 => {
            let index_count = data.cread::<u16>(32);
            if index_count > 0 {
                (
                    0,
                    24u64 + sizeof_extended_dir(&data[40..], index_count) as u64,
                )
            } else {
                (0, 24u64)
            }
        }
        9 => {
            let record: ExtendedFileInodeHeader = data.pread_with(16, LE).unwrap();

            (
                record.size.into(),
                record.block_count(block_size) as u64 * 4u64 + 40,
            )
        }
        _ => (0, 0),
    };
}

fn parse_super_block(s: &[u8]) -> Result<SqsSuper> {
    if s.len() < 128 {
        bail!("File is too small to be a Squashfs image!");
    }
    let super_block: SqsSuper = s.pread_with(0, LE)?;

    if super_block.magic != 0x73717368 {
        bail!("Bad magic in super block!");
    }
    if super_block.blksize != 2u32.pow(super_block.blklog.into()) {
        bail!("Block size field is corrupted!");
    }
    if super_block.ver_major != 4 || super_block.ver_minor != 0 {
        bail!(
            "Squashfs version unsupported! (Got: {}.{})",
            super_block.ver_major,
            super_block.ver_minor
        );
    }
    if super_block.bytes > s.len().try_into()? {
        bail!("Squashfs size field is corrupted!");
    }

    Ok(super_block)
}
