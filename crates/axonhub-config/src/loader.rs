use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_yaml::Value;

use crate::env::apply_env_overrides;
use crate::legacy::normalize_legacy_aliases;
use crate::preview::PreviewFormat;
use crate::types::Config;

pub fn load() -> Result<LoadedConfig> {
    LoadedConfig::load()
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub source: Option<PathBuf>,
}

impl LoadedConfig {
    pub fn load() -> Result<Self> {
        let mut merged = serde_yaml::to_value(Config::default())?;
        let source = find_config_path();

        if let Some(path) = source.as_ref() {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("failed to read config file: {}", path.display()))?;
            let mut file_value: Value = serde_yaml::from_str(&contents)
                .with_context(|| format!("failed to parse config file: {}", path.display()))?;
            normalize_legacy_aliases(&mut file_value);
            merge_values(&mut merged, file_value);
        }

        apply_env_overrides(&mut merged)?;

        let config: Config = serde_yaml::from_value(merged)?;
        config.ensure_loadable()?;

        Ok(Self { config, source })
    }

    pub fn preview(&self, format: PreviewFormat) -> Result<String> {
        self.config.preview(format)
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.config.get(key)
    }

    pub fn config_path(&self) -> Option<&Path> {
        self.source.as_deref()
    }
}

fn find_config_path() -> Option<PathBuf> {
    config_search_paths().into_iter().find(|path| path.exists())
}

pub fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("./config.yml"),
        PathBuf::from("/etc/axonhub/config.yml"),
    ];

    if let Some(home) = env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".config/axonhub/config.yml"));
    }

    paths.push(PathBuf::from("./conf/config.yml"));
    paths
}

fn merge_values(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Mapping(base_map), Value::Mapping(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(existing) => merge_values(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_value, overlay_value) => *base_value = overlay_value,
    }
}
