use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::{fs, io};

use crate::watcher::{RuntimeConfig, RuntimeWatch};

const CONFIG_REL_PATH: &str = ".config/watch-dir/config.yaml";

/// On-disk configuration, mirroring the `config:` block of config.yaml.
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub watch: Vec<WatchEntry>,
    #[serde(rename = "file-types", default)]
    pub file_types: BTreeMap<String, Vec<String>>,
    #[serde(rename = "ignore-extensions", default)]
    pub ignore_extensions: Vec<String>,
    #[serde(default)]
    pub stability: Stability,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WatchEntry {
    pub path: String,
    #[serde(rename = "file-types", default, skip_serializing_if = "Option::is_none")]
    pub file_types: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Stability {
    #[serde(rename = "interval-ms", default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(rename = "required-stable-ticks", default = "default_ticks")]
    pub required_stable_ticks: u32,
}

impl Default for Stability {
    fn default() -> Self {
        Stability {
            interval_ms: default_interval_ms(),
            required_stable_ticks: default_ticks(),
        }
    }
}

fn default_interval_ms() -> u64 {
    1000
}

fn default_ticks() -> u32 {
    2
}

#[derive(Serialize, Deserialize)]
struct Root {
    config: Config,
}

impl Default for Config {
    fn default() -> Self {
        let mut file_types: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut add = |name: &str, exts: &[&str]| {
            file_types.insert(
                name.to_string(),
                exts.iter().map(|s| s.to_string()).collect(),
            );
        };
        add(
            "documents",
            &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "html", "drawio",
            ],
        );
        add(
            "images",
            &["jpg", "jpeg", "png", "gif", "tiff", "bmp", "svg", "webp", "ico"],
        );
        add(
            "videos",
            &[
                "mp4", "mov", "avi", "mkv", "flv", "m4v", "rmvb", "rm", "3gp", "mpg", "mpeg",
                "webm",
            ],
        );
        add(
            "audios",
            &["mp3", "wav", "wma", "ogg", "m4a", "aac", "aiff"],
        );
        add(
            "installers",
            &["exe", "msi", "iso", "dmg", "pkg", "deb", "rpm", "appimage"],
        );
        add(
            "archives",
            &["zip", "tar", "gz", "rar", "7z", "bz2", "xz", "tgz"],
        );
        add(
            "code",
            &["yml", "yaml", "conf", "json", "js", "ts", "py", "rs", "css", "sh"],
        );
        add("misc", &["winmd"]);

        Config {
            watch: vec![WatchEntry {
                path: "~/Downloads".to_string(),
                file_types: None,
            }],
            file_types,
            ignore_extensions: ["crdownload", "part", "download", "opdownload", "tmp", "partial"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            stability: Stability::default(),
        }
    }
}

pub fn config_path() -> PathBuf {
    home::home_dir()
        .expect("could not determine home directory")
        .join(CONFIG_REL_PATH)
}

pub fn log_path() -> PathBuf {
    home::home_dir()
        .expect("could not determine home directory")
        .join(".config/watch-dir/watcher.log")
}

/// Load the config from disk, creating a default one on first run.
///
/// A malformed file is reported but not overwritten, so a user's edits are
/// never silently clobbered by the defaults.
pub fn load_or_create() -> Config {
    let path = config_path();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(text) => match serde_yaml::from_str::<Root>(&text) {
                Ok(root) => return root.config,
                Err(err) => {
                    log::error!("Failed to parse {path:?}: {err}. Using defaults.");
                    return Config::default();
                }
            },
            Err(err) => {
                log::error!("Failed to read {path:?}: {err}. Using defaults.");
                return Config::default();
            }
        }
    }

    let config = Config::default();
    if let Err(err) = config.save() {
        log::error!("Failed to write default config: {err}");
    }
    config
}

impl Config {
    pub fn save(&self) -> io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_yaml::to_string(&Root {
            config: self.clone(),
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path, text)
    }

    /// Resolve paths and index extensions for the watcher runtime.
    pub fn to_runtime(&self) -> RuntimeConfig {
        let default_types = index_file_types(&self.file_types);
        let dirs = self
            .watch
            .iter()
            .map(|entry| {
                let types = match &entry.file_types {
                    Some(custom) => index_file_types(custom),
                    None => default_types.clone(),
                };
                RuntimeWatch {
                    path: resolve_path(&entry.path),
                    file_types: types,
                }
            })
            .collect();

        RuntimeConfig {
            dirs,
            ignore_extensions: self
                .ignore_extensions
                .iter()
                .map(|s| s.to_lowercase())
                .collect::<HashSet<_>>(),
            stability: self.stability,
        }
    }
}

fn index_file_types(map: &BTreeMap<String, Vec<String>>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (category, exts) in map {
        for ext in exts {
            out.insert(ext.to_lowercase(), category.clone());
        }
    }
    out
}

pub fn resolve_path(raw_path: &str) -> PathBuf {
    if let Some(stripped) = raw_path.strip_prefix("~/") {
        home::home_dir()
            .expect("could not determine home directory")
            .join(stripped)
    } else {
        PathBuf::from(raw_path)
    }
}
