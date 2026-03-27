use serde::{Deserialize, Serialize};

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
            read_timeout: "30s".to_owned(),
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
