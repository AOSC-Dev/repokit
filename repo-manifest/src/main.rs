use anyhow::{anyhow, Result};
use clap::Parser;
use log::{error, info, warn};
use std::{
    fs::{create_dir_all, read, write},
    path::Path,
    process,
};

mod parser;
mod scan;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    /// Specify the configuration file to use
    #[clap(short, long)]
    config: String,
}

fn main() {
    std::env::set_var("RUST_LOG", "info");
    env_logger::init();
    let matches = Args::parse();
    let config = &matches.config;
    info!("Reading config from {}...", config);
    let config_data = read(config);
    if let Err(e) = config_data {
        error!("Could not read the config file {}: {}", config, e);
        process::exit(1);
    }
    let config_data = parser::parse_config(&config_data.unwrap());
    if let Err(e) = config_data {
        error!("Could not parse the config file {}: {}", config, e);
        process::exit(1);
    }
    let config_data = config_data.unwrap();
    info!("Preflight scanning...");
    let root_path = parser::get_root_path(&config_data);
    let tarball_json = scan_tarballs(&root_path, config_data);
    let image_json = scan_images(&root_path);
    info!("Writing manifest...");
    let manifest_dir = Path::new(&root_path).join("manifest");
    if let Err(e) = create_dir_all(&manifest_dir) {
        error!("Could not create directory: {}", e);
        process::exit(1);
    }
    if let Err(e) = tarball_json {
        error!("Could not gather information about the tarballs: {}", e);
        process::exit(1);
    }
    if let Err(e) = image_json {
        error!("Could not gather information about the LiveKit: {}", e);
        process::exit(1);
    }
    if let Err(e) = write(manifest_dir.join("recipe.json"), tarball_json.unwrap()) {
        error!("Could not write the manifest: {}", e);
        process::exit(1);
    }
    if let Err(e) = write(manifest_dir.join("livekit.json"), image_json.unwrap()) {
        error!("Could not write the manifest: {}", e);
        process::exit(1);
    }
    info!("Manifest generated successfully.");
}

fn scan_images(root_path: &str) -> Result<String> {
    let files = scan::collect_iso(root_path)?;
    if files.is_empty() {
        return Err(anyhow!("No image was found."));
    }
    let previous_manifest_path = Path::new(root_path).join("manifest/livekit.json");
    let previous_manifest = read(previous_manifest_path);
    let scanned = if let Err(e) = previous_manifest {
        warn!("Failed to read the previous manifest: {}", e);
        warn!("Falling back to full scan!");
        info!("Scanning {} images...", files.len());
        scan::scan_files(&files, root_path, true)?
    } else {
        let existing_files: Vec<parser::Tarball> =
            serde_json::from_slice(previous_manifest.as_ref().unwrap())?;
        scan::increment_scan_files(files, existing_files, root_path, true)?
    };
    info!("Generating manifest...");

    Ok(serde_json::to_string(&scanned)?)
}

fn scan_tarballs(root_path: &str, config_data: parser::UserConfig) -> Result<String> {
    let files = scan::collect_tarballs(root_path)?;
    if files.is_empty() {
        return Err(anyhow!("No tarball was found."));
    }
    let previous_manifest_path = Path::new(root_path).join("manifest/recipe.json");
    let previous_manifest = read(previous_manifest_path);
    let scanned = if let Err(e) = previous_manifest {
        warn!("Failed to read the previous manifest: {}", e);
        warn!("Falling back to full scan!");
        info!("Scanning {} tarballs...", files.len());
        scan::scan_files(&scan::filter_files(files, &config_data), root_path, false)?
    } else {
        scan::smart_scan_files(previous_manifest.unwrap(), &config_data, files, root_path)?
    };
    info!("Generating manifest...");
    let variants = parser::assemble_variants(&config_data, scanned);
    let manifest = parser::assemble_manifest(config_data, variants);
    let json = parser::generate_manifest(&manifest)?;

    Ok(json)
}
