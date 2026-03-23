use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::env::env_override_names;
use crate::{config_search_paths, Config, LoadedConfig, PreviewFormat};

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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
    let _lock = test_lock().lock().unwrap();
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
    let _lock = test_lock().lock().unwrap();
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
    assert_eq!(loaded.config.cache.memory.expiration, "15m");
    assert_eq!(loaded.config.cache.memory.cleanup_interval, "45m");
    assert_eq!(
        loaded.get("server.name"),
        Some(serde_json::json!("Root Config"))
    );
}

#[test]
fn load_uses_home_config_when_workspace_config_is_missing() {
    let _lock = test_lock().lock().unwrap();
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
    let _lock = test_lock().lock().unwrap();
    let fixture = TestFixture::new("env-overrides");
    fixture.write_workspace_file(
        "config.yml",
        r#"
server:
  port: 9001
  name: "From File"
  trace:
    extra_trace_headers: ["File-Trace"]
    codex_trace_enabled: false
db:
  dsn: "file:from-file.db"
"#,
    );
    fixture.set_env("AXONHUB_SERVER_PORT", "7123");
    fixture.set_env("AXONHUB_SERVER_NAME", "From Env");
    fixture.set_env(
        "AXONHUB_SERVER_TRACE_EXTRA_TRACE_HEADERS",
        "Trace-A, Trace-B",
    );
    fixture.set_env("AXONHUB_SERVER_TRACE_CODEX_TRACE_ENABLED", "true");
    fixture.set_env("AXONHUB_DB_DSN", "file:from-env.db");

    let loaded = LoadedConfig::load().unwrap();

    assert_eq!(loaded.config.server.port, 7123);
    assert_eq!(loaded.config.server.name, "From Env");
    assert_eq!(
        loaded.config.server.trace.extra_trace_headers,
        vec!["Trace-A".to_owned(), "Trace-B".to_owned()]
    );
    assert!(loaded.config.server.trace.codex_trace_enabled);
    assert_eq!(loaded.config.db.dsn, "file:from-env.db");
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

    let json_preview = config.preview(PreviewFormat::Json).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_preview).unwrap();
    assert_eq!(json["server"]["name"], "AxonHub");
    assert_eq!(config.get("server.port"), Some(serde_json::json!(8090)));
    assert_eq!(
        config.get("server.trace.request_header"),
        Some(serde_json::json!(""))
    );
    assert_eq!(config.validation_errors(), Vec::<String>::new());

    let mut invalid = Config::default();
    invalid.server.port = 0;
    invalid.db.dsn = " ".to_owned();
    invalid.log.name = " ".to_owned();
    invalid.server.cors.enabled = true;
    invalid.server.cors.allowed_origins.clear();

    assert_eq!(
        invalid.validation_errors(),
        vec![
            "server.port must be between 1 and 65535".to_owned(),
            "db.dsn cannot be empty".to_owned(),
            "log.name cannot be empty".to_owned(),
            "server.cors.allowed_origins cannot be empty when CORS is enabled".to_owned(),
        ]
    );
}
