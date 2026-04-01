use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::env::env_override_names;
use crate::{
    config_search_paths, load_for_cli, supported_config_aliases, supported_config_keys, Config,
    LoadedConfig, PreviewFormat,
};

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tracked_env_vars() -> Vec<&'static str> {
    let mut vars = vec!["HOME"];
    vars.extend(env_override_names());
    vars
}

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("axonhub-config-{name}-{unique}"))
}

struct TestFixture {
    root: PathBuf,
    workspace: PathBuf,
    home: PathBuf,
    original_dir: PathBuf,
    original_env: Vec<(&'static str, Option<OsString>)>,
}

impl TestFixture {
    fn new(name: &str) -> Self {
        let root = temp_dir(name);
        let workspace = root.join("workspace");
        let home = root.join("home");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(home.join(".config/axonhub")).unwrap();

        let original_dir = env::current_dir().unwrap();
        let original_env = tracked_env_vars()
            .into_iter()
            .map(|key| (key, env::var_os(key)))
            .collect::<Vec<_>>();

        for (key, _) in &original_env {
            env::remove_var(key);
        }

        env::set_var("HOME", &home);
        env::set_current_dir(&workspace).unwrap();

        Self {
            root,
            workspace,
            home,
            original_dir,
            original_env,
        }
    }

    fn write_workspace_file(&self, relative_path: &str, contents: &str) {
        let path = self.workspace.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn set_env(&self, key: &str, value: &str) {
        env::set_var(key, value);
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        env::set_current_dir(&self.original_dir).unwrap();

        for (key, value) in &self.original_env {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
        }

        fs::remove_dir_all(&self.root).ok();
    }
}

#[test]
fn config_search_paths_keep_current_order() {
    let _lock = test_guard();
    let fixture = TestFixture::new("search-paths");

    assert_eq!(
        config_search_paths(),
        vec![
            PathBuf::from("./config.yml"),
            PathBuf::from("/etc/axonhub/config.yml"),
            fixture.home.join(".config/axonhub/config.yml"),
            PathBuf::from("./conf/config.yml"),
        ]
    );
}

#[test]
fn load_prefers_root_config_and_preserves_defaults_and_legacy_aliases() {
    let _lock = test_guard();
    let fixture = TestFixture::new("root-config");
    fixture.write_workspace_file(
        "config.yml",
        r#"
server:
  name: "Root Config"
cache:
  default_expiration: "15m"
  cleanup_interval: "45m"
"#,
    );
    fixture.write_workspace_file(
        "conf/config.yml",
        r#"
server:
  name: "Conf Fallback"
"#,
    );

    let loaded = LoadedConfig::load().unwrap();

    assert_eq!(loaded.config_path(), Some(Path::new("./config.yml")));
    assert_eq!(loaded.config.server.name, "Root Config");
    assert_eq!(loaded.config.server.port, 8090);
    assert_eq!(loaded.config.db.dialect, "sqlite3");
    assert_eq!(loaded.config.server.read_timeout, "30s");
    assert_eq!(loaded.config.cache.memory.expiration, "15m");
    assert_eq!(loaded.config.cache.memory.cleanup_interval, "45m");
    assert_eq!(
        loaded.get("server.name"),
        Some(serde_json::json!("Root Config"))
    );
    assert_eq!(
        loaded.get("cache.default_expiration"),
        Some(serde_json::json!("15m"))
    );
    assert_eq!(
        loaded.get("cache.cleanup_interval"),
        Some(serde_json::json!("45m"))
    );
}

#[test]
fn load_uses_home_config_when_workspace_config_is_missing() {
    let _lock = test_guard();
    let fixture = TestFixture::new("home-config");
    fs::write(
        fixture.home.join(".config/axonhub/config.yml"),
        r#"
server:
  name: "Home Config"
db:
  dsn: "file:home.db"
"#,
    )
    .unwrap();

    let loaded = LoadedConfig::load().unwrap();

    assert_eq!(
        loaded.config_path(),
        Some(fixture.home.join(".config/axonhub/config.yml").as_path())
    );
    assert_eq!(loaded.config.server.name, "Home Config");
    assert_eq!(loaded.config.db.dsn, "file:home.db");
}

#[test]
fn load_applies_env_overrides_after_file_values() {
    let _lock = test_guard();
    let fixture = TestFixture::new("env-overrides");
    fixture.write_workspace_file(
        "config.yml",
        r#"
server:
  port: 9001
  name: "From File"
  api:
    auth:
      allow_no_auth: false
  trace:
    extra_trace_headers: ["File-Trace"]
    codex_trace_enabled: false
db:
  dialect: "sqlite3"
  dsn: "file:from-file.db"
  debug: false
"#,
    );
    fixture.set_env("AXONHUB_SERVER_PORT", "7123");
    fixture.set_env("AXONHUB_SERVER_NAME", "From Env");
    fixture.set_env("AXONHUB_SERVER_API_AUTH_ALLOW_NO_AUTH", "true");
    fixture.set_env(
        "AXONHUB_SERVER_TRACE_EXTRA_TRACE_HEADERS",
        "Trace-A, Trace-B",
    );
    fixture.set_env("AXONHUB_SERVER_TRACE_CODEX_TRACE_ENABLED", "true");
    fixture.set_env("AXONHUB_DB_DIALECT", "postgresql");
    fixture.set_env("AXONHUB_DB_DSN", "file:from-env.db");
    fixture.set_env("AXONHUB_DB_DEBUG", "true");
    fixture.set_env("AXONHUB_CACHE_DEFAULT_EXPIRATION", "25m");
    fixture.set_env("AXONHUB_CACHE_CLEANUP_INTERVAL", "55m");

    let loaded = LoadedConfig::load().unwrap();

    assert_eq!(loaded.config.server.port, 7123);
    assert_eq!(loaded.config.server.name, "From Env");
    assert!(loaded.config.server.api.auth.allow_no_auth);
    assert_eq!(
        loaded.config.server.trace.extra_trace_headers,
        vec!["Trace-A".to_owned(), "Trace-B".to_owned()]
    );
    assert!(loaded.config.server.trace.codex_trace_enabled);
    assert_eq!(loaded.config.db.dialect, "postgresql");
    assert_eq!(loaded.config.db.dsn, "file:from-env.db");
    assert!(loaded.config.db.debug);
    assert_eq!(loaded.config.cache.memory.expiration, "25m");
    assert_eq!(loaded.config.cache.memory.cleanup_interval, "55m");
    assert_eq!(
        loaded.get("cache.default_expiration"),
        Some(serde_json::json!("25m"))
    );
    assert_eq!(
        loaded.get("cache.cleanup_interval"),
        Some(serde_json::json!("55m"))
    );
}

#[test]
fn preview_parse_get_and_validation_keep_current_contract() {
    let config = Config::default();

    assert_eq!(PreviewFormat::parse("json"), Some(PreviewFormat::Json));
    assert_eq!(PreviewFormat::parse("yml"), Some(PreviewFormat::Yaml));
    assert_eq!(PreviewFormat::parse("yaml"), Some(PreviewFormat::Yaml));
    assert_eq!(PreviewFormat::parse("toml"), None);

    let yaml_preview = config.preview(PreviewFormat::Yaml).unwrap();
    assert!(!yaml_preview.starts_with("---\n"));
    assert!(yaml_preview.contains("port: 8090"));
    assert!(yaml_preview.contains("memory:"));
    assert!(yaml_preview.contains("expiration: 5m"));
    assert!(yaml_preview.contains("cleanup_interval: 10m"));

    let json_preview = config.preview(PreviewFormat::Json).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_preview).unwrap();
    assert_eq!(json["server"]["name"], "AxonHub");
    assert_eq!(json["server"]["read_timeout"], "30s");
    assert_eq!(config.get("server.port"), Some(serde_json::json!(8090)));
    assert_eq!(
        config.get("server.trace.request_header"),
        Some(serde_json::json!(""))
    );
    assert_eq!(
        config.get("cache.default_expiration"),
        Some(serde_json::json!("5m"))
    );
    assert_eq!(config.get("provider_edge.client_id"), None);
    assert_eq!(config.validation_errors(), Vec::<String>::new());

    let mut invalid = Config::default();
    invalid.server.port = 0;
    invalid.db.dsn = " ".to_owned();
    invalid.db.dialect = "oracle".to_owned();
    invalid.log.name = " ".to_owned();
    invalid.log.encoding = "xml".to_owned();
    invalid.log.output = "stderr".to_owned();
    invalid.cache.mode = "disk".to_owned();
    invalid.metrics.enabled = true;
    invalid.metrics.exporter.exporter_type = "bogus".to_owned();
    invalid.server.cors.enabled = true;
    invalid.server.cors.allowed_origins.clear();

    assert_eq!(
        invalid.validation_errors(),
        vec![
            "server.port must be between 1 and 65535".to_owned(),
            "db.dsn cannot be empty".to_owned(),
            "unsupported db.dialect 'oracle': supported values are sqlite3, sqlite, postgres, postgresql, pg, pgx, postgresdb".to_owned(),
            "log.name cannot be empty".to_owned(),
            "log.encoding must be one of: json, console".to_owned(),
            "log.output must be one of: stdio, file".to_owned(),
            "cache.mode must be one of: memory, redis, two-level".to_owned(),
            "metrics.exporter.type must be one of: stdout, otlpgrpc, otlphttp when metrics are enabled".to_owned(),
            "server.cors.allowed_origins cannot be empty when CORS is enabled".to_owned(),
        ]
    );
}

#[test]
fn supported_key_tables_document_current_config_surface() {
    assert!(supported_config_keys().len() > 20);
    assert!(supported_config_keys()
        .iter()
        .any(|entry| entry.key == "server.api.auth.allow_no_auth"));
    assert!(supported_config_keys()
        .iter()
        .any(|entry| entry.key == "cache.default_expiration"));
    assert!(supported_config_keys()
        .iter()
        .any(|entry| entry.key == "metrics.exporter.type"));
    assert!(supported_config_aliases()
        .iter()
        .any(|entry| entry.key == "cache.memory.expiration"
            && entry.canonical_key == "cache.default_expiration"));
}

#[test]
fn load_rejects_unsupported_config_keys_in_yaml() {
    let _lock = test_guard();
    let fixture = TestFixture::new("unsupported-yaml-key");
    fixture.write_workspace_file(
        "config.yml",
        r#"
server:
  name: "Bad Config"
provider_edge:
  codex_client_id: "legacy-only"
"#,
    );

    let error = LoadedConfig::load().unwrap_err().to_string();

    assert!(error.contains("failed to validate config file contract: ./config.yml"));
    assert!(error.contains("unsupported config key 'provider_edge'"));
    assert!(error.contains("conf/conf.go"));
    assert!(!error.contains("legacy Go backend"));
    assert!(!error.contains("migration-slice"));
}

#[test]
fn load_rejects_non_target_rust_dialects_in_yaml() {
    let _lock = test_guard();
    let fixture = TestFixture::new("non-target-rust-dialects");
    fixture.write_workspace_file(
        "config.yml",
        r#"
db:
  dialect: "mysql"
  dsn: "mysql://root:root@127.0.0.1:3306/axonhub"
"#,
    );

    let mysql_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(mysql_error.contains("unsupported db.dialect 'mysql'"));
    assert!(mysql_error.contains("sqlite3, sqlite, postgres, postgresql, pg, pgx, postgresdb"));

    fixture.write_workspace_file(
        "config.yml",
        r#"
db:
  dialect: "tidb"
  dsn: "mysql://root:root@127.0.0.1:4000/axonhub"
"#,
    );

    let tidb_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(tidb_error.contains("unsupported db.dialect 'tidb'"));

    fixture.write_workspace_file(
        "config.yml",
        r#"
db:
  dialect: "neon"
  dsn: "postgres://axonhub:secret@localhost/axonhub?sslmode=require"
"#,
    );

    let neon_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(neon_error.contains("unsupported db.dialect 'neon'"));

    fixture.write_workspace_file(
        "config.yml",
        r#"
db:
  dialect: "oracle"
  dsn: "oracle://unsupported"
"#,
    );

    let unknown_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(unknown_error.contains("unsupported db.dialect 'oracle'"));
    assert!(!unknown_error.contains("legacy-Go-only"));
    assert!(!unknown_error.contains("migration-slice"));
}

#[test]
fn load_rejects_non_target_rust_dialects_from_env_override() {
    let _lock = test_guard();
    let fixture = TestFixture::new("non-target-env-dialect");
    fixture.set_env("AXONHUB_DB_DIALECT", "mysql");
    fixture.set_env(
        "AXONHUB_DB_DSN",
        "mysql://axonhub:secret@localhost:3306/axonhub",
    );

    let mysql_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(mysql_error.contains("unsupported db.dialect 'mysql'"));

    fixture.set_env("AXONHUB_DB_DIALECT", "tidb");
    fixture.set_env("AXONHUB_DB_DSN", "mysql://root:root@127.0.0.1:4000/axonhub");

    let tidb_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(tidb_error.contains("unsupported db.dialect 'tidb'"));

    fixture.set_env("AXONHUB_DB_DIALECT", "neon");
    fixture.set_env(
        "AXONHUB_DB_DSN",
        "postgres://axonhub:secret@localhost/axonhub?sslmode=disable",
    );

    let neon_error = LoadedConfig::load().unwrap_err().to_string();
    assert!(neon_error.contains("unsupported db.dialect 'neon'"));
}

#[test]
fn load_accepts_supported_sqlite_and_postgres_dialects() {
    let _lock = test_guard();
    let fixture = TestFixture::new("supported-dialects");

    fixture.set_env("AXONHUB_DB_DIALECT", "sqlite");
    fixture.set_env(
        "AXONHUB_DB_DSN",
        "file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)",
    );
    let sqlite_loaded = LoadedConfig::load().unwrap();
    assert_eq!(sqlite_loaded.config.db.dialect, "sqlite");

    fixture.set_env("AXONHUB_DB_DIALECT", "postgres");
    fixture.set_env(
        "AXONHUB_DB_DSN",
        "postgres://axonhub:secret@localhost/axonhub?sslmode=disable",
    );
    let postgres_loaded = LoadedConfig::load().unwrap();
    assert_eq!(postgres_loaded.config.db.dialect, "postgres");
}

#[test]
fn load_accepts_nested_cache_aliases_and_prefers_flat_go_keys() {
    let _lock = test_guard();
    let fixture = TestFixture::new("cache-aliases");
    fixture.write_workspace_file(
        "config.yml",
        r#"
cache:
  memory:
    expiration: "11m"
    cleanup_interval: "22m"
"#,
    );

    let loaded = LoadedConfig::load().unwrap();

    assert_eq!(loaded.config.cache.memory.expiration, "11m");
    assert_eq!(loaded.config.cache.memory.cleanup_interval, "22m");
    assert_eq!(
        loaded.get("cache.default_expiration"),
        Some(serde_json::json!("11m"))
    );
    assert_eq!(
        loaded.get("cache.cleanup_interval"),
        Some(serde_json::json!("22m"))
    );
    assert_eq!(
        loaded.get("cache.memory.expiration"),
        Some(serde_json::json!("11m"))
    );
    assert_eq!(
        loaded.get("cache.memory.cleanup_interval"),
        Some(serde_json::json!("22m"))
    );
}

#[test]
fn get_accepts_nested_alias_keys_for_internal_config_access() {
    let config = Config::default();

    assert_eq!(
        config.get("cache.default_expiration"),
        Some(serde_json::json!("5m"))
    );
    assert_eq!(
        config.get("cache.cleanup_interval"),
        Some(serde_json::json!("10m"))
    );
    assert_eq!(
        config.get("cache.memory.expiration"),
        Some(serde_json::json!("5m"))
    );
    assert_eq!(
        config.get("cache.memory.cleanup_interval"),
        Some(serde_json::json!("10m"))
    );
}

#[test]
fn ensure_loadable_rejects_invalid_supported_value_shapes() {
    let mut config = Config::default();
    config.log.level = "verbose".to_owned();
    assert_eq!(
        config.ensure_loadable().unwrap_err().to_string(),
        "invalid log level 'verbose'"
    );

    let mut config = Config::default();
    config.log.encoding = "xml".to_owned();
    assert_eq!(
        config.ensure_loadable().unwrap_err().to_string(),
        "invalid log encoding 'xml'"
    );

    let mut config = Config::default();
    config.log.output = "stderr".to_owned();
    assert_eq!(
        config.ensure_loadable().unwrap_err().to_string(),
        "invalid log output 'stderr'"
    );

    let mut config = Config::default();
    config.cache.mode = "disk".to_owned();
    assert_eq!(
        config.ensure_loadable().unwrap_err().to_string(),
        "invalid cache mode 'disk'"
    );

    let mut config = Config::default();
    config.metrics.enabled = true;
    config.metrics.exporter.exporter_type = "bogus".to_owned();
    assert_eq!(
        config.ensure_loadable().unwrap_err().to_string(),
        "invalid metrics exporter type 'bogus'"
    );
}

#[test]
fn cli_load_allows_values_that_go_defers_to_config_validate() {
    let _lock = test_guard();
    let fixture = TestFixture::new("cli-load-semantic-validation");
    fixture.write_workspace_file(
        "config.yml",
        r#"
log:
  encoding: "xml"
  output: "stderr"
cache:
  mode: "disk"
metrics:
  enabled: true
  exporter:
    type: "bogus"
"#,
    );

    assert_eq!(
        LoadedConfig::load().unwrap_err().to_string(),
        "invalid log encoding 'xml'"
    );

    let loaded = load_for_cli().unwrap();
    assert_eq!(loaded.config.log.encoding, "xml");
    assert_eq!(loaded.config.log.output, "stderr");
    assert_eq!(loaded.config.cache.mode, "disk");
    assert_eq!(loaded.config.metrics.exporter.exporter_type, "bogus");
    assert_eq!(
        loaded.config.validation_errors(),
        vec![
            "log.encoding must be one of: json, console".to_owned(),
            "log.output must be one of: stdio, file".to_owned(),
            "cache.mode must be one of: memory, redis, two-level".to_owned(),
            "metrics.exporter.type must be one of: stdout, otlpgrpc, otlphttp when metrics are enabled"
                .to_owned(),
        ]
    );
}

#[test]
fn cli_load_keeps_go_style_parse_failures_and_rust_target_boundaries() {
    let _lock = test_guard();
    let fixture = TestFixture::new("cli-load-parse-failures");
    fixture.write_workspace_file(
        "config.yml",
        r#"
server:
  read_timeout: "not-a-duration"
"#,
    );

    let duration_error = load_for_cli().unwrap_err().to_string();
    assert!(duration_error.contains("invalid duration for server.read_timeout: not-a-duration"));

    fixture.write_workspace_file(
        "config.yml",
        r#"
log:
  level: "verbose"
"#,
    );

    assert_eq!(
        load_for_cli().unwrap_err().to_string(),
        "invalid log level 'verbose'"
    );

    fixture.write_workspace_file(
        "config.yml",
        r#"
db:
  dialect: "mysql"
  dsn: "mysql://root:root@127.0.0.1:3306/axonhub"
"#,
    );

    let dialect_error = load_for_cli().unwrap_err().to_string();
    assert!(dialect_error.contains("unsupported db.dialect 'mysql'"));
}
