use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub db: DbConfig,
    pub log: LogConfig,
    pub server: ServerConfig,
    pub metrics: MetricsConfig,
    pub gc: GcConfig,
    pub cache: CacheConfig,
    pub provider_quota: ProviderQuotaConfig,
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
        let value = serde_json::to_value(self).ok()?;
        key.split('.')
            .try_fold(&value, |current, segment| current.get(segment))
            .cloned()
    }

    pub fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.server.port == 0 || self.server.port > 65_535 {
            errors.push("server.port must be between 1 and 65535".to_owned());
        }

        if self.db.dsn.trim().is_empty() {
            errors.push("db.dsn cannot be empty".to_owned());
        }

        if self.log.name.trim().is_empty() {
            errors.push("log.name cannot be empty".to_owned());
        }

        if self.server.cors.enabled && self.server.cors.allowed_origins.is_empty() {
            errors.push(
                "server.cors.allowed_origins cannot be empty when CORS is enabled".to_owned(),
            );
        }

        errors
    }

    fn ensure_loadable(&self) -> Result<()> {
        ensure_log_level(&self.log.level)?;

        for (field, value) in self.duration_fields() {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }

            humantime::parse_duration(trimmed)
                .with_context(|| format!("invalid duration for {field}: {trimmed}"))?;
        }

        Ok(())
    }

    fn duration_fields(&self) -> Vec<(&'static str, &str)> {
        vec![
            ("server.read_timeout", &self.server.read_timeout),
            ("server.request_timeout", &self.server.request_timeout),
            (
                "server.llm_request_timeout",
                &self.server.llm_request_timeout,
            ),
            (
                "server.dashboard.all_time_token_stats_soft_ttl",
                &self.server.dashboard.all_time_token_stats_soft_ttl,
            ),
            (
                "server.dashboard.all_time_token_stats_hard_ttl",
                &self.server.dashboard.all_time_token_stats_hard_ttl,
            ),
            ("server.cors.max_age", &self.server.cors.max_age),
            (
                "provider_quota.check_interval",
                &self.provider_quota.check_interval,
            ),
            ("cache.memory.expiration", &self.cache.memory.expiration),
            (
                "cache.memory.cleanup_interval",
                &self.cache.memory.cleanup_interval,
            ),
            ("cache.redis.expiration", &self.cache.redis.expiration),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u32,
    pub name: String,
    pub base_path: String,
    pub read_timeout: String,
    pub request_timeout: String,
    pub llm_request_timeout: String,
    pub trace: TraceConfig,
    pub dashboard: DashboardConfig,
    pub debug: bool,
    pub disable_ssl_verify: bool,
    pub cors: CorsConfig,
    pub api: ApiConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_owned(),
            port: 8090,
            name: "AxonHub".to_owned(),
            base_path: String::new(),
            read_timeout: String::new(),
            request_timeout: "30s".to_owned(),
            llm_request_timeout: "600s".to_owned(),
            trace: TraceConfig::default(),
            dashboard: DashboardConfig::default(),
            debug: false,
            disable_ssl_verify: false,
            cors: CorsConfig::default(),
            api: ApiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    pub thread_header: String,
    pub trace_header: String,
    pub request_header: String,
    pub extra_trace_headers: Vec<String>,
    pub extra_trace_body_fields: Vec<String>,
    pub claude_code_trace_enabled: bool,
    pub codex_trace_enabled: bool,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            thread_header: "AH-Thread-Id".to_owned(),
            trace_header: "AH-Trace-Id".to_owned(),
            request_header: String::new(),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub all_time_token_stats_soft_ttl: String,
    pub all_time_token_stats_hard_ttl: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            all_time_token_stats_soft_ttl: "1h".to_owned(),
            all_time_token_stats_hard_ttl: "24h".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    pub enabled: bool,
    pub debug: bool,
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub exposed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: String,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            debug: false,
            allowed_origins: vec!["http://localhost:8090".to_owned()],
            allowed_methods: vec![
                "GET".to_owned(),
                "POST".to_owned(),
                "DELETE".to_owned(),
                "PATCH".to_owned(),
                "PUT".to_owned(),
                "OPTIONS".to_owned(),
                "HEAD".to_owned(),
            ],
            allowed_headers: vec![
                "Content-Type".to_owned(),
                "Authorization".to_owned(),
                "X-API-Key".to_owned(),
                "X-Goog-Api-Key".to_owned(),
                "X-Project-ID".to_owned(),
                "X-Thread-ID".to_owned(),
                "X-Trace-ID".to_owned(),
            ],
            exposed_headers: Vec::new(),
            allow_credentials: false,
            max_age: "30m".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiConfig {
    pub auth: ApiAuthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiAuthConfig {
    pub allow_no_auth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbConfig {
    pub dialect: String,
    pub dsn: String,
    pub debug: bool,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            dialect: "sqlite3".to_owned(),
            dsn: "file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)".to_owned(),
            debug: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    pub name: String,
    pub debug: bool,
    pub skip_level: u32,
    pub level: String,
    pub level_key: String,
    pub time_key: String,
    pub caller_key: String,
    pub function_key: String,
    pub name_key: String,
    pub encoding: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub output: String,
    pub file: LogFileConfig,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            name: "axonhub".to_owned(),
            debug: false,
            skip_level: 1,
            level: "info".to_owned(),
            level_key: "level".to_owned(),
            time_key: "time".to_owned(),
            caller_key: "label".to_owned(),
            function_key: String::new(),
            name_key: "logger".to_owned(),
            encoding: "json".to_owned(),
            includes: Vec::new(),
            excludes: Vec::new(),
            output: "stdio".to_owned(),
            file: LogFileConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileConfig {
    pub path: String,
    pub max_size: u32,
    pub max_age: u32,
    pub max_backups: u32,
    pub local_time: bool,
}

impl Default for LogFileConfig {
    fn default() -> Self {
        Self {
            path: "logs/axonhub.log".to_owned(),
            max_size: 100,
            max_age: 30,
            max_backups: 10,
            local_time: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub exporter: MetricsExporterConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsExporterConfig {
    #[serde(rename = "type")]
    pub exporter_type: String,
    pub endpoint: String,
    pub insecure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcConfig {
    pub cron: String,
    pub vacuum_enabled: bool,
    pub vacuum_full: bool,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            cron: "0 2 * * *".to_owned(),
            vacuum_enabled: false,
            vacuum_full: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub mode: String,
    pub memory: MemoryCacheConfig,
    pub redis: RedisCacheConfig,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            mode: "memory".to_owned(),
            memory: MemoryCacheConfig::default(),
            redis: RedisCacheConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCacheConfig {
    pub expiration: String,
    pub cleanup_interval: String,
}

impl Default for MemoryCacheConfig {
    fn default() -> Self {
        Self {
            expiration: "5m".to_owned(),
            cleanup_interval: "10m".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedisCacheConfig {
    pub addr: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub db: Option<i64>,
    pub tls: bool,
    pub tls_insecure_skip_verify: bool,
    pub expiration: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderQuotaConfig {
    pub check_interval: String,
}

impl Default for ProviderQuotaConfig {
    fn default() -> Self {
        Self {
            check_interval: "20m".to_owned(),
        }
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

fn ensure_log_level(level: &str) -> Result<()> {
    match level.trim().to_ascii_lowercase().as_str() {
        "debug" | "info" | "warn" | "warning" | "error" | "panic" | "fatal" => Ok(()),
        value => Err(anyhow!("invalid log level '{value}'")),
    }
}

fn normalize_legacy_aliases(root: &mut Value) {
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
    let default_expiration = cache_map.remove(Value::String("default_expiration".to_owned()));
    let cleanup_interval = cache_map.remove(Value::String("cleanup_interval".to_owned()));

    if !cache_map.contains_key(&memory_key) {
        cache_map.insert(memory_key.clone(), Value::Mapping(Mapping::new()));
    }

    let Some(memory_value) = cache_map.get_mut(&memory_key) else {
        return;
    };

    let Some(memory_map) = memory_value.as_mapping_mut() else {
        return;
    };

    insert_if_missing(memory_map, "expiration", default_expiration);
    insert_if_missing(memory_map, "cleanup_interval", cleanup_interval);
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

fn apply_env_overrides(root: &mut Value) -> Result<()> {
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
        path: &["cache", "memory", "expiration"],
        kind: EnvValueKind::String,
    },
    EnvOverride {
        env: "AXONHUB_CACHE_CLEANUP_INTERVAL",
        path: &["cache", "memory", "cleanup_interval"],
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
];
