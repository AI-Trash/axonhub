use anyhow::Result;
use serde_json::{Map, Value};

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
        let value = self.operator_view_json();
        canonical_key
            .split('.')
            .try_fold(&value, |current, segment| current.get(segment))
            .cloned()
    }

    fn operator_view_json(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("config is serializable");

        let Some(root) = value.as_object_mut() else {
            return value;
        };

        let Some(cache) = root.get_mut("cache").and_then(Value::as_object_mut) else {
            return value;
        };

        let Some(memory) = cache.get("memory").and_then(Value::as_object).cloned() else {
            return value;
        };

        inject_cache_memory_alias(cache, &memory, "expiration", "default_expiration");
        inject_cache_memory_alias(cache, &memory, "cleanup_interval", "cleanup_interval");

        value
    }
}

fn inject_cache_memory_alias(
    cache: &mut Map<String, Value>,
    memory: &Map<String, Value>,
    memory_key: &str,
    cache_key: &str,
) {
    if let Some(value) = memory.get(memory_key).cloned() {
        cache.insert(cache_key.to_owned(), value);
    }
}
