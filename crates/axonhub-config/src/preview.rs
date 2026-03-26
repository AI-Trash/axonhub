use anyhow::Result;

use crate::contract::canonical_key_for_get;
use crate::types::Config;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PreviewFormat {
    Json,
    Yaml,
}

impl PreviewFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "json" => Some(Self::Json),
            "yml" | "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

impl Config {
    pub fn preview(&self, format: PreviewFormat) -> Result<String> {
        match format {
            PreviewFormat::Json => Ok(serde_json::to_string_pretty(self)?),
            PreviewFormat::Yaml => Ok(serde_yaml::to_string(self)?
                .trim_start_matches("---\n")
                .to_owned()),
        }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        let canonical_key = canonical_key_for_get(key)?;
        let value = serde_json::to_value(self).ok()?;
        canonical_key
            .split('.')
            .try_fold(&value, |current, segment| current.get(segment))
            .cloned()
    }
}
