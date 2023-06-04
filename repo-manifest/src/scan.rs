use crate::parser::{
    flatten_variants, get_retro_arches, get_splitted_name, parse_manifest, SquashFs, Tarball,
    UserConfig,
};
use anyhow::{anyhow, Result};
use backhand::Squashfs as BackHandSquashfs;
use log::{error, info, warn};
use parking_lot::Mutex;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::{
    convert::TryInto,
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::Arc,
};
use walkdir::{DirEntry, WalkDir};
use xz2::read::XzDecoder;

macro_rules! unwrap_or_show_error {
    ($m:tt, $p:expr, $f:stmt) => {{
        let tmp = { $f };
        if let Err(e) = tmp {
            error!($m, $p, e);
            return;
        }
        tmp.unwrap()
    }};
    ($m:tt, $p:expr, $x:ident) => {{
        if let Err(e) = $x {
            error!($m, $p, e);
            return;
        }
        $x.unwrap()
    }};
}

// TODO: .img files should also be considered
#[inline]
fn is_tarball(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.ends_with(".tar.xz"))
        .unwrap_or(false)
}

#[inline]
fn is_squashfs(entry: &DirEntry) -> bool {
    let path = entry.path();
    let reader = std::fs::File::open(path);
    if reader.is_err() {
        return false;
    }
    let mut reader = reader.unwrap();
    let mut buffer = [0u8; 4];

    reader.read(&mut buffer).ok() == Some(4) && buffer == b"hsqs"[..]
}

#[inline]
fn is_install_media(entry: &DirEntry) -> bool {
    is_tarball(entry) || is_squashfs(entry)
}

#[inline]
fn is_iso(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.ends_with(".iso"))
        .unwrap_or(false)
}

/// Calculate the Sha256 checksum of the given stream
pub fn sha256sum<R: Read>(mut reader: R) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut reader, &mut hasher)?;

    Ok(hex::encode(hasher.finalize()))
}

/// Calculate the decompressed size of the given tarball
pub fn calculate_tarball_decompressed_size<R: Read + Seek>(mut reader: R) -> Result<u64> {
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| anyhow!("Could not seek {}", e))?;

    let size = {
        let mut buffer = [0u8; 4096];
        let mut decompress = XzDecoder::new(reader);

        loop {
            let size = decompress.read(&mut buffer)?;
            if size < 1 {
                break;
            }
        }

        decompress.total_out()
    };

    Ok(size)
}

pub fn calculate_squashfs_size_and_inode(archive: &Path) -> Result<(u64, u32)> {
    let archive = BufReader::new(std::fs::File::open(archive)?);
    let squashfs = BackHandSquashfs::from_reader(archive)?;
    let inodes = squashfs.superblock.inode_count;
    let reader = squashfs.into_filesystem_reader()?;
    let mut size = 0;

    for node in reader.files() {
        match &node.inner {
            backhand::InnerNode::File(f) => {
                size += f.basic.file_size;
            }
            _ => continue,
        }
    }

    Ok((size.into(), inodes))
}

fn collect_files<P: AsRef<Path>, F: Fn(&DirEntry) -> bool>(
    root: P,
    filter: F,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter() {
        if let Ok(entry) = entry {
            if entry.file_type().is_dir() || !filter(&entry) {
                continue;
            }
            files.push(entry.into_path());
        } else if let Err(e) = entry {
            error!("Could not stat() the entry: {}", e);
        }
    }

    Ok(files)
}

pub fn collect_tarballs<P: AsRef<Path>>(root: P) -> Result<Vec<PathBuf>> {
    collect_files(root, is_install_media)
}

pub fn collect_iso<P: AsRef<Path>>(root: P) -> Result<Vec<PathBuf>> {
    collect_files(root, is_iso)
}

pub fn increment_scan_files(
    files: Vec<PathBuf>,
    existing_files_tbl: Vec<Tarball>,
    existing_files_sq: Vec<SquashFs>,
    root_path: &str,
    raw: bool,
) -> Result<(Vec<Tarball>, Vec<SquashFs>)> {
    let root_path_buf = PathBuf::from(root_path);
    let mut new_existing_tarballs: Vec<Tarball> = Vec::new();
    let mut new_existing_sq: Vec<SquashFs> = Vec::new();
    let mut new_files_tbl: Vec<PathBuf> = Vec::new();
    let mut new_files_sq: Vec<PathBuf> = Vec::new();
    new_existing_tarballs.reserve(existing_files_tbl.len());
    new_existing_sq.reserve(existing_files_sq.len());

    let new_file_sq_len = files
        .iter()
        .filter(|x| x.extension().and_then(|x| x.to_str()) == Some("squashfs"))
        .collect::<Vec<_>>()
        .len();

    let new_files_tbl_len = files.len() - new_file_sq_len;

    new_files_tbl.reserve(new_file_sq_len);
    new_files_sq.reserve(new_files_tbl_len);
    for mut tarball in existing_files_tbl {
        let path = root_path_buf.join(&tarball.path);
        if files.contains(&path) {
            if let Some(filename) = PathBuf::from(&tarball.path).file_name() {
                if let Some(names) = get_splitted_name(&filename.to_string_lossy()) {
                    tarball.variant = names.0;
                    new_existing_tarballs.push(tarball);
                    continue;
                }
            }
            warn!("Unable to determine the variant for {}", tarball.path);
        }
    }
    for mut sq in existing_files_sq {
        let path = root_path_buf.join(&sq.path);
        if files.contains(&path) {
            if let Some(filename) = PathBuf::from(&sq.path).file_name() {
                if let Some(names) = get_splitted_name(&filename.to_string_lossy()) {
                    sq.variant = names.0;
                    new_existing_sq.push(sq);
                    continue;
                }
            }
            warn!("Unable to determine the variant for {}", sq.path);
        }
    }
    for file in files {
        if !new_existing_tarballs
            .iter()
            .any(|t| root_path_buf.join(&t.path) == file)
        {
            new_files_tbl.push(file);
        } else if !new_existing_sq
            .iter()
            .any(|t| root_path_buf.join(&t.path) == file)
        {
            new_files_sq.push(file);
        }
    }
    info!("Incrementally scanning {} tarballs...", new_files_tbl.len());
    let (diff_files_tbl, diff_files_sq) = scan_files(&new_files_tbl, root_path, raw)?;
    new_existing_tarballs.extend(diff_files_tbl);
    new_existing_sq.extend(diff_files_sq);

    Ok((new_existing_tarballs, new_existing_sq))
}

/// Filter all the files that do not exist in the configuration file
pub fn filter_files(files: Vec<PathBuf>, config: &UserConfig) -> Vec<PathBuf> {
    let mut filtered_files = Vec::new();
    filtered_files.reserve(files.len());
    let retro_arches = get_retro_arches(config);
    for file in files {
        if let Some(filename) = file.file_name() {
            if let Some(names) = get_splitted_name(&filename.to_string_lossy()) {
                if retro_arches.contains(&names.2) {
                    if config.distro.retro.contains_key(&names.0) {
                        filtered_files.push(file);
                        continue;
                    }
                    warn!(
                        "The variant `{} (retro)` is not in the config file.",
                        names.0
                    );
                } else if config.distro.mainline.contains_key(&names.0) {
                    filtered_files.push(file);
                } else {
                    warn!(
                        "The variant `{} (mainline)` is not in the config file.",
                        names.0
                    );
                }
            }
        }
    }

    filtered_files
}

pub fn smart_scan_files(
    manifest: Vec<u8>,
    config: &UserConfig,
    files: Vec<PathBuf>,
    root_path: &str,
) -> Result<(Vec<Tarball>, Vec<SquashFs>)> {
    let files = filter_files(files, config);
    let manifest = parse_manifest(&manifest);
    if let Err(e) = manifest {
        warn!("Failed to read the previous manifest: {}", e);
        warn!("Falling back to full scan!");
        info!("Scanning {} tarballs...", files.len());
        return scan_files(&files, root_path, false);
    }
    let manifest = manifest.unwrap();
    let existing_files = flatten_variants(manifest);

    increment_scan_files(files, existing_files.0, existing_files.1, root_path, false)
}

pub fn scan_files(
    files: &[PathBuf],
    root_path: &str,
    raw: bool,
) -> Result<(Vec<Tarball>, Vec<SquashFs>)> {
    let results: Vec<Tarball> = Vec::new();
    let squashfs_results = Vec::new();
    let results_shared = Arc::new(Mutex::new(results));
    let sq_results_shared = Arc::new(Mutex::new(squashfs_results));
    files.par_iter().for_each(|p| {
        info!("Scanning {}...", p.display());
        let rel_path = p.strip_prefix(root_path);
        let path = unwrap_or_show_error!(
            "Could get the relative path {}: {:?}",
            p.display(),
            rel_path
        );
        let filename = unwrap_or_show_error!(
            "Could not determine filename {}: {}",
            p.display(),
            path.file_name().ok_or_else(|| anyhow!("None value found"))
        );
        let names = unwrap_or_show_error!(
            "Could not parse the filename {}: {}",
            p.display(),
            get_splitted_name(&filename.to_string_lossy())
                .ok_or_else(|| anyhow!("None value found"))
        );
        let mut f = unwrap_or_show_error!("Could not open {}: {}", p.display(), File::open(p));

        let mut buffer = [0u8; 4];
        let size = unwrap_or_show_error!("Could not open {}: {}", p.display(), f.read(&mut buffer));
        if size != 4 {
            error!("File size to small: {}", p.display());
            return;
        }

        let is_squashfs = buffer == b"hsqs"[..];

        let (real_size, inode) = if raw {
            (
                unwrap_or_show_error!(
                    "Could not read file as stream {}: {}",
                    p.display(),
                    f.seek(SeekFrom::End(0))
                        .map_err(|e| anyhow!("Could not seek {}", e))
                ),
                None,
            )
        } else if is_squashfs {
            let (size, inode) = unwrap_or_show_error!(
                "Could not read file as stream {}: {}",
                p.display(),
                calculate_squashfs_size_and_inode(p)
            );

            (size, Some(inode))
        } else {
            let size = unwrap_or_show_error!(
                "Could not read file as stream {}: {}",
                p.display(),
                calculate_tarball_decompressed_size(&f)
            );

            (size, None)
        };

        let inst_size: i64 = real_size.try_into().unwrap();
        let f_metadata =
            unwrap_or_show_error!("Could not read metadata {}: {}", p.display(), f.metadata());
        let download_size = f_metadata.len();
        let download_size: i64 = download_size.try_into().unwrap();
        unwrap_or_show_error!(
            "Could not seek() {}: {}",
            p.display(),
            f.seek(SeekFrom::Start(0))
        );
        let sha256sum = unwrap_or_show_error!(
            "Could not update sha256sum of {}: {}",
            p.display(),
            sha256sum(&f)
        );
        let mut results = results_shared.lock();
        let mut squashfs_results = sq_results_shared.lock();
        if is_squashfs {
            squashfs_results.push(SquashFs {
                arch: names.2,
                date: names.1,
                variant: names.0,
                download_size,
                inst_size,
                path: path.to_string_lossy().to_string(),
                sha256sum,
                inodes: inode.unwrap(),
            })
        } else {
            results.push(Tarball {
                arch: names.2,
                date: names.1,
                variant: names.0,
                download_size,
                inst_size,
                path: path.to_string_lossy().to_string(),
                sha256sum,
            });
        }
    });

    Ok((
        Arc::try_unwrap(results_shared).unwrap().into_inner(),
        Arc::try_unwrap(sq_results_shared).unwrap().into_inner(),
    ))
}
