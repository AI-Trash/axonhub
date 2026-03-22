use std::env;
use std::fmt::{self, Display, Formatter};
use std::process;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result};
use axonhub_config::{load, PreviewFormat};
use axonhub_http::{router, HttpState, SystemReadCapability, SystemReadError, SystemReadPort};
use axum::Router;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::sync::Arc;

static START_TIME: OnceLock<Instant> = OnceLock::new();

const VERSION: &str = include_str!("../../../internal/build/VERSION");
const BUILD_COMMIT: Option<&str> = option_env!("AXONHUB_BUILD_COMMIT");
const BUILD_TIME: Option<&str> = option_env!("AXONHUB_BUILD_TIME");
const RUST_VERSION: Option<&str> = option_env!("AXONHUB_BUILD_RUST_VERSION");

const SYSTEM_STATUS_UNSUPPORTED_MESSAGE: &str =
    "DB-backed admin system status is not available for the configured dialect yet. Use the legacy Go backend for this route.";

struct SqliteSystemReader {
    dsn: String,
}

impl SystemReadPort for SqliteSystemReader {
    fn is_initialized(&self) -> Result<bool, SystemReadError> {
        let flags = sqlite_open_flags(&self.dsn);
        let connection = Connection::open_with_flags(&self.dsn, flags).map_err(|_| {
            SystemReadError::QueryFailed("Failed to check system status".to_owned())
        })?;

        let value: Option<String> = connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                ["system_initialized"],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| {
                SystemReadError::QueryFailed("Failed to check system status".to_owned())
            })?;

        Ok(value
            .map(|current| current.eq_ignore_ascii_case("true"))
            .unwrap_or(false))
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if let Some(command) = args.get(1).map(String::as_str) {
        match command {
            "config" => {
                handle_config_command(&args)?;
                return Ok(());
            }
            "version" | "--version" | "-v" => {
                show_version();
                return Ok(());
            }
            "help" | "--help" | "-h" => {
                show_help();
                return Ok(());
            }
            "build-info" => {
                show_build_info();
                return Ok(());
            }
            _ => {}
        }
    }

    start_server().await
}

fn handle_config_command(args: &[String]) -> Result<()> {
    if args.len() < 3 {
        print_config_usage();
        process::exit(1);
    }

    match args[2].as_str() {
        "preview" => config_preview(args),
        "validate" => config_validate(),
        "get" => config_get(args),
        _ => {
            print_config_usage();
            process::exit(1);
        }
    }
}

fn config_preview(args: &[String]) -> Result<()> {
    let mut format = PreviewFormat::Yaml;
    let mut index = 3;

    while index < args.len() {
        if matches!(args[index].as_str(), "--format" | "-f") {
            let value = args.get(index + 1).map(String::as_str).unwrap_or_default();
            format = PreviewFormat::parse(value).unwrap_or_else(|| {
                eprintln!("Unsupported format: {value}");
                process::exit(1);
            });
            index += 2;
            continue;
        }

        index += 1;
    }

    let loaded = load().context("Failed to load config")?;
    println!("{}", loaded.preview(format)?);

    Ok(())
}

fn config_validate() -> Result<()> {
    let loaded = load().context("Failed to load config")?;
    let errors = loaded.config.validation_errors();

    if errors.is_empty() {
        println!("Configuration is valid!");
        return Ok(());
    }

    println!("Configuration validation failed:");
    for error in errors {
        println!("  - {error}");
    }

    process::exit(1);
}

fn config_get(args: &[String]) -> Result<()> {
    if args.len() < 4 {
        println!("Usage: axonhub config get <key>");
        println!();
        println!("Available keys:");
        println!("  server.port       Server port number");
        println!("  server.name       Server name");
        println!("  server.base_path  Server base path");
        println!("  server.debug      Server debug mode");
        println!("  db.dialect        Database dialect");
        println!("  db.dsn            Database DSN");
        process::exit(1);
    }

    let key = &args[3];
    let loaded = load().context("Failed to load config")?;

    if let Some(value) = loaded.get(key) {
        println!("{}", format_json_value(&value)?);
    } else {
        eprintln!("Unknown config key: {key}");
        process::exit(1);
    }

    Ok(())
}

fn show_help() {
    println!("AxonHub AI Gateway");
    println!();
    println!("Usage:");
    println!("  axonhub                    Start the server (default)");
    println!("  axonhub config preview     Preview configuration");
    println!("  axonhub config validate    Validate configuration");
    println!("  axonhub config get <key>   Get a specific config value");
    println!("  axonhub version            Show version");
    println!("  axonhub build-info         Show build information");
    println!("  axonhub help               Show this help message");
    println!();
    println!("Options:");
    println!("  -f, --format FORMAT        Output format for config preview (yml, json)");
}

fn print_config_usage() {
    println!("Usage: axonhub config <preview|validate|get>");
}

fn show_version() {
    println!("{}", version());
}

fn show_build_info() {
    println!("{}", BuildInfo::current());
}

async fn start_server() -> Result<()> {
    let loaded = load().context("Failed to load config")?;
    let port: u16 = loaded
        .config
        .server
        .port
        .try_into()
        .context("server.port must be between 1 and 65535")?;

    let address = format!("{}:{port}", loaded.config.server.host);
    let state = HttpState {
        service_name: loaded.config.server.name.clone(),
        version: version().to_owned(),
        config_source: loaded
            .source
            .as_ref()
            .map(|path| path.display().to_string()),
        system_read: build_system_read_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
    };

    let app = mount_base_path(router(state), &loaded.config.server.base_path);
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .with_context(|| format!("Failed to bind {address}"))?;

    println!(
        "AxonHub Rust migration slice listening on http://{}",
        listener.local_addr()?
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server exited unexpectedly")
}

fn mount_base_path(app: Router, base_path: &str) -> Router {
    let normalized = base_path.trim();
    if normalized.is_empty() || normalized == "/" {
        return app;
    }

    let prefixed = format!("/{}", normalized.trim_matches('/'));
    Router::new().nest(&prefixed, app)
}

fn build_system_read_capability(dialect: &str, dsn: &str) -> SystemReadCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        return SystemReadCapability::Available {
            reader: Arc::new(SqliteSystemReader {
                dsn: dsn.to_owned(),
            }),
        };
    }

    SystemReadCapability::Unsupported {
        message: SYSTEM_STATUS_UNSUPPORTED_MESSAGE.to_owned(),
    }
}

fn sqlite_open_flags(dsn: &str) -> OpenFlags {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
    if dsn.starts_with("file:") {
        flags |= OpenFlags::SQLITE_OPEN_URI;
    }

    flags
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn version() -> &'static str {
    VERSION.trim()
}

fn format_json_value(value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::Null => Ok("null".to_owned()),
        serde_json::Value::Bool(boolean) => Ok(boolean.to_string()),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::String(string) => Ok(string.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Ok(serde_json::to_string_pretty(value)?)
        }
    }
}

struct BuildInfo {
    version: &'static str,
    commit: Option<&'static str>,
    build_time: Option<&'static str>,
    rust_version: Option<&'static str>,
    platform: String,
    uptime: String,
}

impl BuildInfo {
    fn current() -> Self {
        Self {
            version: version(),
            commit: BUILD_COMMIT,
            build_time: BUILD_TIME,
            rust_version: RUST_VERSION,
            platform: format!("{}/{}", env::consts::OS, env::consts::ARCH),
            uptime: humantime::format_duration(start_time().elapsed()).to_string(),
        }
    }
}

fn start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
}

impl Display for BuildInfo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Version: {}", self.version)?;

        if let Some(commit) = self.commit.filter(|value| !value.is_empty()) {
            writeln!(formatter, "Commit: {commit}")?;
        }

        if let Some(build_time) = self.build_time.filter(|value| !value.is_empty()) {
            writeln!(formatter, "Build Time: {build_time}")?;
        }

        if let Some(rust_version) = self.rust_version.filter(|value| !value.is_empty()) {
            writeln!(formatter, "Rust Version: {rust_version}")?;
        }

        writeln!(formatter, "Platform: {}", self.platform)?;
        write!(formatter, "Uptime: {}", self.uptime)
    }
}
