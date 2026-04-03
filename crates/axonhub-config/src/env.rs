use std::env;

use anyhow::{anyhow, Context, Result};
use serde_yaml::{Mapping, Value};

pub(crate) fn apply_env_overrides(root: &mut Value) -> Result<()> {
    for override_spec in ENV_OVERRIDES {
        let Some(raw_value) =
            env::var_os(override_spec.env).and_then(|value| value.into_string().ok())
        else {
            continue;
        };

        let parsed = parse_env_value(override_spec.kind, &raw_value)
            .with_context(|| format!("failed to parse {}", override_spec.env))?;
        set_path(root, override_spec.path, parsed);
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn env_override_names() -> impl Iterator<Item = &'static str> {
    ENV_OVERRIDES.iter().map(|spec| spec.env)
}

fn set_path(root: &mut Value, path: &[&str], new_value: Value) {
    let mut current = root;

    for segment in &path[..path.len() - 1] {
        if !matches!(current, Value::Mapping(_)) {
            *current = Value::Mapping(Mapping::new());
        }

        let map = current.as_mapping_mut().expect("mapping just created");
        let key = Value::String((*segment).to_owned());
        if !map.contains_key(&key) {
            map.insert(key.clone(), Value::Mapping(Mapping::new()));
        }

        current = map.get_mut(&key).expect("key inserted");
    }

    let key = Value::String(path[path.len() - 1].to_owned());
    let map = current
        .as_mapping_mut()
        .expect("intermediate path is a mapping");
    map.insert(key, new_value);
}

fn parse_env_value(kind: EnvValueKind, raw: &str) -> Result<Value> {
    let trimmed = raw.trim();

    match kind {
        EnvValueKind::String => Ok(Value::String(raw.to_owned())),
        EnvValueKind::Bool => parse_bool(trimmed).map(Value::Bool),
        EnvValueKind::U32 => trimmed
            .parse::<u32>()
            .map(|value| serde_yaml::to_value(value).expect("u32 is serializable"))
            .map_err(Into::into),
        EnvValueKind::OptionalI64 => {
            if trimmed.is_empty() {
                Ok(Value::Null)
            } else {
                trimmed
                    .parse::<i64>()
                    .map(|value| serde_yaml::to_value(value).expect("i64 is serializable"))
                    .map_err(Into::into)
            }
        }
        EnvValueKind::StringList => {
            if trimmed.is_empty() {
                return Ok(Value::Sequence(Vec::new()));
            }

            if trimmed.starts_with('[') {
                let values: Vec<String> = serde_yaml::from_str(trimmed)?;
                return Ok(serde_yaml::to_value(values)?);
            }

            let values: Vec<String> = trimmed
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect();

            Ok(serde_yaml::to_value(values)?)
        }
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow!("invalid boolean value: {value}")),
    }
}

#[derive(Clone, Copy)]
enum EnvValueKind {
    String,
    Bool,
    U32,
    OptionalI64,
    StringList,
}

struct EnvOverride {
    env: &'static str,
    path: &'static [&'static str],
    kind: EnvValueKind,
}

const ENV_OVERRIDES: &[EnvOverride] = &[
    EnvOverride {
        env: "AXONHUB_SERVER_HOST",
        path: &["server", "host"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_PORT",
        path: &["server", "port"],
        kind: EnvValueKind::U32,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_NAME",
        path: &["server", "name"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_BASE_PATH",
        path: &["server", "base_path"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_READ_TIMEOUT",
        path: &["server", "read_timeout"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_REQUEST_TIMEOUT",
        path: &["server", "request_timeout"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_LLM_REQUEST_TIMEOUT",
        path: &["server", "llm_request_timeout"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_THREAD_HEADER",
        path: &["server", "trace", "thread_header"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_TRACE_HEADER",
        path: &["server", "trace", "trace_header"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_REQUEST_HEADER",
        path: &["server", "trace", "request_header"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_EXTRA_TRACE_HEADERS",
        path: &["server", "trace", "extra_trace_headers"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_EXTRA_TRACE_BODY_FIELDS",
        path: &["server", "trace", "extra_trace_body_fields"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_CLAUDE_CODE_TRACE_ENABLED",
        path: &["server", "trace", "claude_code_trace_enabled"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_TRACE_CODEX_TRACE_ENABLED",
        path: &["server", "trace", "codex_trace_enabled"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_DASHBOARD_ALL_TIME_TOKEN_STATS_SOFT_TTL",
        path: &["server", "dashboard", "all_time_token_stats_soft_ttl"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_DASHBOARD_ALL_TIME_TOKEN_STATS_HARD_TTL",
        path: &["server", "dashboard", "all_time_token_stats_hard_ttl"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_DEBUG",
        path: &["server", "debug"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_DISABLE_SSL_VERIFY",
        path: &["server", "disable_ssl_verify"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_ENABLED",
        path: &["server", "cors", "enabled"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_DEBUG",
        path: &["server", "cors", "debug"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_ALLOWED_ORIGINS",
        path: &["server", "cors", "allowed_origins"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_ALLOWED_METHODS",
        path: &["server", "cors", "allowed_methods"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_ALLOWED_HEADERS",
        path: &["server", "cors", "allowed_headers"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_EXPOSED_HEADERS",
        path: &["server", "cors", "exposed_headers"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_ALLOW_CREDENTIALS",
        path: &["server", "cors", "allow_credentials"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_CORS_MAX_AGE",
        path: &["server", "cors", "max_age"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_SERVER_API_AUTH_ALLOW_NO_AUTH",
        path: &["server", "api", "auth", "allow_no_auth"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_DB_DIALECT",
        path: &["db", "dialect"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_DB_DSN",
        path: &["db", "dsn"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_DB_DEBUG",
        path: &["db", "debug"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_LOG_NAME",
        path: &["log", "name"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_DEBUG",
        path: &["log", "debug"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_LOG_SKIP_LEVEL",
        path: &["log", "skip_level"],
        kind: EnvValueKind::U32,
    },
    EnvOverride {
        env: "AXONHUB_LOG_LEVEL",
        path: &["log", "level"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_LEVEL_KEY",
        path: &["log", "level_key"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_TIME_KEY",
        path: &["log", "time_key"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_CALLER_KEY",
        path: &["log", "caller_key"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_FUNCTION_KEY",
        path: &["log", "function_key"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_NAME_KEY",
        path: &["log", "name_key"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_ENCODING",
        path: &["log", "encoding"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_INCLUDES",
        path: &["log", "includes"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_LOG_EXCLUDES",
        path: &["log", "excludes"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_LOG_OUTPUT",
        path: &["log", "output"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_FILE_PATH",
        path: &["log", "file", "path"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_LOG_FILE_MAX_SIZE",
        path: &["log", "file", "max_size"],
        kind: EnvValueKind::U32,
    },
    EnvOverride {
        env: "AXONHUB_LOG_FILE_MAX_AGE",
        path: &["log", "file", "max_age"],
        kind: EnvValueKind::U32,
    },
    EnvOverride {
        env: "AXONHUB_LOG_FILE_MAX_BACKUPS",
        path: &["log", "file", "max_backups"],
        kind: EnvValueKind::U32,
    },
    EnvOverride {
        env: "AXONHUB_LOG_FILE_LOCAL_TIME",
        path: &["log", "file", "local_time"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_METRICS_ENABLED",
        path: &["metrics", "enabled"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_METRICS_EXPORTER_TYPE",
        path: &["metrics", "exporter", "type"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_METRICS_EXPORTER_ENDPOINT",
        path: &["metrics", "exporter", "endpoint"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_METRICS_EXPORTER_INSECURE",
        path: &["metrics", "exporter", "insecure"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_GC_CRON",
        path: &["gc", "cron"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_GC_VACUUM_ENABLED",
        path: &["gc", "vacuum_enabled"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_GC_VACUUM_FULL",
        path: &["gc", "vacuum_full"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_MODE",
        path: &["cache", "mode"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_DEFAULT_EXPIRATION",
        path: &["cache", "default_expiration"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_CLEANUP_INTERVAL",
        path: &["cache", "cleanup_interval"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_MEMORY_EXPIRATION",
        path: &["cache", "memory", "expiration"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_MEMORY_CLEANUP_INTERVAL",
        path: &["cache", "memory", "cleanup_interval"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_ADDR",
        path: &["cache", "redis", "addr"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_URL",
        path: &["cache", "redis", "url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_USERNAME",
        path: &["cache", "redis", "username"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_PASSWORD",
        path: &["cache", "redis", "password"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_DB",
        path: &["cache", "redis", "db"],
        kind: EnvValueKind::OptionalI64,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_TLS",
        path: &["cache", "redis", "tls"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_TLS_INSECURE_SKIP_VERIFY",
        path: &["cache", "redis", "tls_insecure_skip_verify"],
        kind: EnvValueKind::Bool,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_REDIS_EXPIRATION",
        path: &["cache", "redis", "expiration"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_QUOTA_CHECK_INTERVAL",
        path: &["provider_quota", "check_interval"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CODEX_AUTHORIZE_URL",
        path: &["provider_edge", "codex", "authorize_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CODEX_TOKEN_URL",
        path: &["provider_edge", "codex", "token_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CODEX_CLIENT_ID",
        path: &["provider_edge", "codex", "client_id"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CODEX_REDIRECT_URI",
        path: &["provider_edge", "codex", "redirect_uri"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CODEX_SCOPES",
        path: &["provider_edge", "codex", "scopes"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CODEX_USER_AGENT",
        path: &["provider_edge", "codex", "user_agent"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CLAUDECODE_AUTHORIZE_URL",
        path: &["provider_edge", "claudecode", "authorize_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CLAUDECODE_TOKEN_URL",
        path: &["provider_edge", "claudecode", "token_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CLAUDECODE_CLIENT_ID",
        path: &["provider_edge", "claudecode", "client_id"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CLAUDECODE_REDIRECT_URI",
        path: &["provider_edge", "claudecode", "redirect_uri"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CLAUDECODE_SCOPES",
        path: &["provider_edge", "claudecode", "scopes"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_CLAUDECODE_USER_AGENT",
        path: &["provider_edge", "claudecode", "user_agent"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_AUTHORIZE_URL",
        path: &["provider_edge", "antigravity", "authorize_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_TOKEN_URL",
        path: &["provider_edge", "antigravity", "token_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_ID",
        path: &["provider_edge", "antigravity", "client_id"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_SECRET",
        path: &["provider_edge", "antigravity", "client_secret"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_REDIRECT_URI",
        path: &["provider_edge", "antigravity", "redirect_uri"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_SCOPES",
        path: &["provider_edge", "antigravity", "scopes"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_LOAD_ENDPOINTS",
        path: &["provider_edge", "antigravity", "load_endpoints"],
        kind: EnvValueKind::StringList,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_USER_AGENT",
        path: &["provider_edge", "antigravity", "user_agent"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_METADATA",
        path: &["provider_edge", "antigravity", "client_metadata"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_COPILOT_DEVICE_CODE_URL",
        path: &["provider_edge", "copilot", "device_code_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_COPILOT_ACCESS_TOKEN_URL",
        path: &["provider_edge", "copilot", "access_token_url"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_COPILOT_CLIENT_ID",
        path: &["provider_edge", "copilot", "client_id"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_PROVIDER_EDGE_COPILOT_SCOPE",
        path: &["provider_edge", "copilot", "scope"],
        kind: EnvValueKind::String,
    },
];
