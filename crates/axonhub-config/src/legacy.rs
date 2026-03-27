use serde_yaml::{Mapping, Value};

pub(crate) fn normalize_legacy_aliases(root: &mut Value) {
    let Some(root_map) = root.as_mapping_mut() else {
        return;
    };

    let cache_key = Value::String("cache".to_owned());
    let Some(cache_value) = root_map.get_mut(&cache_key) else {
        return;
    };

    let Some(cache_map) = cache_value.as_mapping_mut() else {
        return;
    };

    let memory_key = Value::String("memory".to_owned());

    let default_expiration_key = Value::String("default_expiration".to_owned());
    let cleanup_interval_key = Value::String("cleanup_interval".to_owned());

    let default_expiration = cache_map.get(&default_expiration_key).cloned();
    let cleanup_interval = cache_map.get(&cleanup_interval_key).cloned();

    if !cache_map.contains_key(&memory_key) {
        cache_map.insert(memory_key.clone(), Value::Mapping(Mapping::new()));
    }

    let (resolved_default_expiration, resolved_cleanup_interval) = {
        let Some(memory_value) = cache_map.get_mut(&memory_key) else {
            return;
        };

        let Some(memory_map) = memory_value.as_mapping_mut() else {
            return;
        };

        upsert_from_option(memory_map, "expiration", default_expiration.clone());
        upsert_from_option(memory_map, "cleanup_interval", cleanup_interval.clone());

        (
            memory_map
                .get(Value::String("expiration".to_owned()))
                .cloned(),
            memory_map
                .get(Value::String("cleanup_interval".to_owned()))
                .cloned(),
        )
    };

    insert_if_missing(cache_map, "default_expiration", resolved_default_expiration);
    insert_if_missing(cache_map, "cleanup_interval", resolved_cleanup_interval);
}

fn insert_if_missing(target: &mut Mapping, key: &str, value: Option<Value>) {
    let target_key = Value::String(key.to_owned());
    if target.contains_key(&target_key) {
        return;
    }

    if let Some(value) = value {
        target.insert(target_key, value);
    }
}

fn upsert_from_option(target: &mut Mapping, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(Value::String(key.to_owned()), value);
    }
}
