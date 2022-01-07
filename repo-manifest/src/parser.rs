use anyhow::Result;
use log::warn;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

// mirror manifests
#[derive(Serialize, Deserialize)]
pub struct Mirror {
    name: String,
    #[serde(rename = "name-tr")]
    name_tr: String,
    loc: String,
    #[serde(rename = "loc-tr")]
    loc_tr: String,
    url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tarball {
    pub arch: String,
    pub date: String,
    #[serde(skip)]
    pub variant: String,
    #[serde(rename = "downloadSize")]
    pub download_size: i64,
    #[serde(rename = "instSize")]
    pub inst_size: i64,
    pub path: String,
    pub sha256sum: String,
}

#[derive(Serialize, Deserialize)]
pub struct Variant {
    name: String,
    retro: bool,
    description: String,
    #[serde(rename = "description-tr")]
    description_tr: String,
    tarballs: Vec<Tarball>,
}

#[derive(Serialize, Deserialize)]
pub struct Bulletin {
    #[serde(rename = "type")]
    type_: String,
    title: String,
    #[serde(rename = "title-tr")]
    title_tr: String,
    body: String,
    #[serde(rename = "body-tr")]
    body_tr: String,
}

#[derive(Serialize, Deserialize)]
pub struct Recipe {
    version: usize,
    bulletin: Bulletin,
    variants: Vec<Variant>,
    mirrors: Vec<Mirror>,
}

// config manifest
#[derive(Serialize, Deserialize)]
pub struct UserBasicConfig {
    path: String,
    retro_arches: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct UserMirrorConfig {
    name: String,
    loc: String,
    url: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserVariantConfig {
    name: String,
    description: String,
}

#[derive(Serialize, Deserialize)]
pub struct UserDistroConfig {
    pub mainline: HashMap<String, UserVariantConfig>,
    pub retro: HashMap<String, UserVariantConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct UserConfig {
    config: UserBasicConfig,
    bulletin: Bulletin,
    mirrors: Vec<Mirror>,
    pub distro: UserDistroConfig,
}

impl Variant {
    pub fn new(
        name: String,
        key: String,
        description: String,
        retro: bool,
        tarballs: Vec<Tarball>,
    ) -> Self {
        Variant {
            name,
            retro,
            description,
            description_tr: format!("{}{}-description", key, if retro { "-retro" } else { "" }),
            tarballs,
        }
    }
}

#[inline]
pub fn parse_config(data: &[u8]) -> Result<UserConfig> {
    Ok(toml::from_slice(data)?)
}

pub fn parse_manifest(data: &[u8]) -> Result<Recipe> {
    Ok(serde_json::from_slice(data)?)
}

pub fn flatten_variants(recipe: Recipe) -> Vec<Tarball> {
    let mut results = Vec::new();
    for variant in recipe.variants {
        results.extend(variant.tarballs);
    }

    results
}

pub fn get_root_path(config: &UserConfig) -> String {
    config.config.path.clone()
}

pub fn get_retro_arches(config: &UserConfig) -> Vec<String> {
    config.config.retro_arches.clone()
}

pub fn generate_manifest(manifest: &Recipe) -> Result<String> {
    Ok(serde_json::to_string(manifest)?)
}

pub fn assemble_variants(config: &UserConfig, files: Vec<Tarball>) -> Vec<Variant> {
    let mut variants: HashMap<String, Variant> = HashMap::new();
    let mut variants_r: HashMap<String, Variant> = HashMap::new();
    let mut results = Vec::new();
    for (k, v) in config.distro.mainline.iter() {
        variants.insert(
            k.to_owned(),
            Variant::new(
                v.name.to_owned(),
                k.to_owned(),
                v.description.to_owned(),
                false,
                Vec::new(),
            ),
        );
    }
    for (k, v) in config.distro.retro.iter() {
        variants_r.insert(
            k.to_owned(),
            Variant::new(
                v.name.to_owned(),
                k.to_owned(),
                v.description.to_owned(),
                true,
                Vec::new(),
            ),
        );
    }
    let retro_arches = &config.config.retro_arches;
    for file in files {
        let v;
        if retro_arches.contains(&file.arch) {
            v = variants_r.get_mut(&file.variant);
        } else {
            v = variants.get_mut(&file.variant);
        }
        if let Some(v) = v {
            v.tarballs.push(file);
        } else {
            warn!("The variant `{}` is not in the config file.", file.variant);
        }
    }
    for (_, variant) in variants {
        results.push(variant);
    }
    for (_, variant) in variants_r {
        results.push(variant);
    }

    results
}

pub fn assemble_manifest(config: UserConfig, variants: Vec<Variant>) -> Recipe {
    Recipe {
        version: 1,
        bulletin: config.bulletin,
        mirrors: config.mirrors,
        variants,
    }
}

// parser combinators
// AOSC OS tarball names have the following pattern:
// aosc-os_<variant>_<date>_<arch>.<ext>
// aosc-os_base_20200526_amd64.tar.xz
pub fn get_splitted_name(name: &str) -> Option<(String, String, String)> {
    let mut splitted = name.split('_');
    splitted.next()?;
    let variant = splitted.next()?;
    let date = splitted.next()?;
    let mut rest = splitted.next()?.split('.');
    let arch = rest.next()?;

    Some((variant.to_owned(), date.to_owned(), arch.to_owned()))
}

#[test]
fn test_split_name() {
    let names = get_splitted_name("aosc-os_base_20200526_amd64.tar.xz").unwrap();
    assert_eq!(
        names,
        ("base".to_owned(), "20200526".to_owned(), "amd64".to_owned())
    );
}
