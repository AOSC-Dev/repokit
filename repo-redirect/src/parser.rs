use anyhow::Result;
use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use log::error;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::task::spawn_blocking;

use crate::SharedDistMap;

type TarballMap = HashMap<String, Tarball>;

#[derive(Deserialize, Debug, Clone)]
pub struct Tarball {
    pub arch: String,
    pub date: String,
    pub path: String,
    pub sha256sum: String,
}

#[derive(Deserialize)]
pub struct Variant {
    #[serde(rename = "description-tr")]
    description_id: String,
    tarballs: Vec<Tarball>,
}

/// AOSC OS Tarball Recipe structure
#[derive(Deserialize)]
pub struct Recipe {
    pub version: usize,
    variants: Vec<Variant>,
}

macro_rules! monitor_recipe {
    ($path:ident, $shared_map:ident, $parser:expr) => {{
        let mut inotify = Inotify::init()?;
        let mut buffer = [0; 32];
        inotify.add_watch($path.as_ref(), WatchMask::CREATE | WatchMask::MODIFY)?;
        let mut stream = inotify.event_stream(&mut buffer)?;

        loop {
            match $parser($path.as_ref()).await {
                Ok(new_map) => {
                    $shared_map.retain(|k, _| new_map.contains_key(k));
                    for (k, variant) in new_map.into_iter() {
                        $shared_map.insert(k, variant);
                    }
                }
                Err(err) => error!("Error parsing recipe: {}", err),
            }

            if let Some(_) = stream.next().await {
                continue;
            } else {
                break;
            }
        }
    }};
}

#[inline]
fn get_variant_id(description: &str) -> Option<&str> {
    let mut splitted = description.split('-');

    splitted.next()
}

pub async fn monitor_recipe<P: AsRef<Path>>(path: P, shared_map: SharedDistMap) -> Result<()> {
    monitor_recipe!(path, shared_map, parse_recipe);

    Ok(())
}

pub async fn monitor_livekit<P: AsRef<Path>>(path: P, shared_map: SharedDistMap) -> Result<()> {
    monitor_recipe!(path, shared_map, parse_livekit);

    Ok(())
}

pub async fn parse_livekit<P: AsRef<Path>>(path: P) -> Result<TarballMap> {
    let mut f = File::open(path).await?;
    let mut content = Vec::new();
    let mut new_map: TarballMap = HashMap::new();
    f.read_to_end(&mut content).await?;
    let content: Vec<Tarball> = spawn_blocking(move || serde_json::from_slice(&content)).await??;
    // get the latest tarball for each variant
    for tarball in content {
        let option_id = &tarball.arch;
        if let Some(existing_tarball) = new_map.get(option_id) {
            // ignore the one with the date "latest"
            if tarball.date == "latest" || tarball.date < existing_tarball.date {
                continue;
            }
        }
        new_map.insert(option_id.to_string(), tarball);
    }

    Ok(new_map)
}

pub async fn parse_recipe<P: AsRef<Path>>(path: P) -> Result<TarballMap> {
    let mut f = File::open(path).await?;
    let mut content = Vec::new();
    let mut new_map: TarballMap = HashMap::new();
    f.read_to_end(&mut content).await?;
    let content: Recipe = spawn_blocking(move || serde_json::from_slice(&content)).await??;
    for variant in content.variants {
        let variant_id = get_variant_id(&variant.description_id);
        if variant_id.is_none() {
            continue;
        }
        let variant_id = variant_id.unwrap();
        // get the latest tarball for each variant
        for tarball in variant.tarballs {
            let option_id = format!("{}.{}", variant_id, tarball.arch);
            if let Some(existing_tarball) = new_map.get(&option_id) {
                // ignore the one with the date "latest"
                if tarball.date == "latest" || tarball.date < existing_tarball.date {
                    continue;
                }
            }
            new_map.insert(option_id, tarball);
        }
    }

    Ok(new_map)
}

#[tokio::test]
async fn test_parsing() {
    let map = parse_recipe("./tests/recipe.json").await.unwrap();
    dbg!(map);
}

#[tokio::test]
async fn test_parsing_lk() {
    let map = parse_livekit("./tests/livekit.json").await.unwrap();
    dbg!(map);
}
