mod sqlite_foundation;

use std::env;
use std::fmt::{self, Display, Formatter};
use std::process;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, Result};
use axonhub_config::{load, PreviewFormat};
use axonhub_http::{
    router, AdminCapability, AdminGraphqlCapability, AuthContextCapability, HttpState,
    OpenAiV1Capability, OpenApiGraphqlCapability, ProviderEdgeAdminCapability,
    SystemBootstrapCapability, TraceConfig,
};
use axum::Router;
use std::sync::Arc;
use sqlite_foundation::{
    SqliteAdminService, SqliteAuthContextService, SqliteBootstrapService, SqliteFoundation,
    SqliteOpenAiV1Service, SqliteProviderEdgeAdminService,
};

static START_TIME: OnceLock<Instant> = OnceLock::new();

const VERSION: &str = include_str!("../../../internal/build/VERSION");
const BUILD_COMMIT: Option<&str> = option_env!("AXONHUB_BUILD_COMMIT");
const BUILD_TIME: Option<&str> = option_env!("AXONHUB_BUILD_TIME");
const RUST_VERSION: Option<&str> = option_env!("AXONHUB_BUILD_RUST_VERSION");

const SYSTEM_BOOTSTRAP_UNSUPPORTED_MESSAGE: &str =
    "DB-backed admin system status/bootstrap is not available for the configured dialect yet. Use the legacy Go backend for this route.";

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
        system_bootstrap: build_system_bootstrap_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
            version(),
        ),
        auth_context: build_auth_context_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
            loaded.config.server.api.auth.allow_no_auth,
        ),
        openai_v1: build_openai_v1_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
        admin: build_admin_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
        admin_graphql: build_admin_graphql_capability(&loaded.config.db.dialect, &loaded.config.db.dsn),
        openapi_graphql: build_openapi_graphql_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
        ),
        provider_edge_admin: build_provider_edge_admin_capability(
            &loaded.config.db.dialect,
            &loaded.config.db.dsn,
        ),
        allow_no_auth: loaded.config.server.api.auth.allow_no_auth,
        trace_config: TraceConfig {
            thread_header: Some(loaded.config.server.trace.thread_header.clone()),
            trace_header: Some(loaded.config.server.trace.trace_header.clone()),
            request_header: Some(loaded.config.server.trace.request_header.clone()),
            extra_trace_headers: loaded.config.server.trace.extra_trace_headers.clone(),
            extra_trace_body_fields: loaded.config.server.trace.extra_trace_body_fields.clone(),
            claude_code_trace_enabled: loaded.config.server.trace.claude_code_trace_enabled,
            codex_trace_enabled: loaded.config.server.trace.codex_trace_enabled,
        },
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

fn build_system_bootstrap_capability(
    dialect: &str,
    dsn: &str,
    version: &str,
) -> SystemBootstrapCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return SystemBootstrapCapability::Available {
            system: Arc::new(SqliteBootstrapService::new(foundation, version.to_owned())),
        };
    }

    SystemBootstrapCapability::Unsupported {
        message: SYSTEM_BOOTSTRAP_UNSUPPORTED_MESSAGE.to_owned(),
    }
}

fn build_auth_context_capability(dialect: &str, dsn: &str, allow_no_auth: bool) -> AuthContextCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AuthContextCapability::Available {
            auth: Arc::new(SqliteAuthContextService::new(foundation, allow_no_auth)),
        };
    }

    AuthContextCapability::Unsupported {
        message: "DB-backed auth/context is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

fn build_openai_v1_capability(dialect: &str, dsn: &str) -> OpenAiV1Capability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return OpenAiV1Capability::Available {
            openai: Arc::new(SqliteOpenAiV1Service::new(foundation)),
        };
    }

    OpenAiV1Capability::Unsupported {
        message: "OpenAI `/v1` inference is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

fn build_admin_capability(dialect: &str, dsn: &str) -> AdminCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AdminCapability::Available {
            admin: Arc::new(SqliteAdminService::new(foundation)),
        };
    }

    AdminCapability::Unsupported {
        message: "DB-backed admin read routes are not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

fn build_admin_graphql_capability(dialect: &str, dsn: &str) -> AdminGraphqlCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AdminGraphqlCapability::Available {
            graphql: Arc::new(sqlite_foundation::SqliteAdminGraphqlService::new(foundation)),
        };
    }

    AdminGraphqlCapability::Unsupported {
        message: "DB-backed admin GraphQL is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

fn build_openapi_graphql_capability(dialect: &str, dsn: &str) -> OpenApiGraphqlCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return OpenApiGraphqlCapability::Available {
            graphql: Arc::new(sqlite_foundation::SqliteOpenApiGraphqlService::new(foundation)),
        };
    }

    OpenApiGraphqlCapability::Unsupported {
        message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

fn build_provider_edge_admin_capability(dialect: &str, _dsn: &str) -> ProviderEdgeAdminCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        return ProviderEdgeAdminCapability::Available {
            provider_edge: Arc::new(SqliteProviderEdgeAdminService::new()),
        };
    }

    ProviderEdgeAdminCapability::Unsupported {
        message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Use the legacy Go backend for these routes.".to_owned(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_foundation::{
        SqliteBootstrapService, SqliteFoundation, PRIMARY_DATA_STORAGE_NAME,
        SYSTEM_KEY_BRAND_NAME, SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_VERSION,
    };
    use axonhub_http::{InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError};
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use rusqlite::OptionalExtension;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::util::ServiceExt;

    fn temp_sqlite_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axonhub-{name}-{unique}.db"))
    }

    #[test]
    fn sqlite_bootstrap_persists_initialized_state_and_system_keys() {
        let db_path = temp_sqlite_path("bootstrap-success");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        assert!(bootstrap.is_initialized().unwrap());

        let connection = foundation.open_connection(false).unwrap();
        let brand_name: String = connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
                [SYSTEM_KEY_BRAND_NAME],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(brand_name, "AxonHub");

        let version: String = connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
                [SYSTEM_KEY_VERSION],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "v0.9.20");

        let default_storage_id: String = connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
                [SYSTEM_KEY_DEFAULT_DATA_STORAGE],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!default_storage_id.is_empty());

        let primary_storage_name: Option<String> = connection
            .query_row(
                "SELECT name FROM data_storages WHERE id = ?1 AND deleted_at = 0",
                [default_storage_id],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(primary_storage_name.as_deref(), Some(PRIMARY_DATA_STORAGE_NAME));

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn sqlite_bootstrap_returns_already_initialized_on_second_call() {
        let db_path = temp_sqlite_path("bootstrap-already-initialized");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation, "v0.9.20".to_owned());

        let request = InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        };

        bootstrap.initialize(&request).unwrap();

        let error = bootstrap.initialize(&request).unwrap_err();
        assert!(matches!(error, SystemInitializeError::AlreadyInitialized));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn main_router_serves_fresh_status_and_initialize_for_sqlite_scope() {
        let db_path = temp_sqlite_path("main-router-live-scope");
        let state = HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: build_system_bootstrap_capability(
                "sqlite3",
                &db_path.display().to_string(),
                "v0.9.20",
            ),
            auth_context: build_auth_context_capability(
                "sqlite3",
                &db_path.display().to_string(),
                false,
            ),
            openai_v1: build_openai_v1_capability("sqlite3", &db_path.display().to_string()),
            admin: build_admin_capability("sqlite3", &db_path.display().to_string()),
            admin_graphql: build_admin_graphql_capability("sqlite3", &db_path.display().to_string()),
            openapi_graphql: build_openapi_graphql_capability(
                "sqlite3",
                &db_path.display().to_string(),
            ),
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Use the legacy Go backend for these routes.".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        };

        let app = router(state);

        let status_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/system/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(status_response.status(), StatusCode::OK);
        let status_body = axum::body::to_bytes(status_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&status_body).unwrap()["isInitialized"],
            false
        );

        let initialize_response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/initialize")
                    .method(Method::POST.as_str())
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"ownerEmail":"owner@example.com","ownerPassword":"password123","ownerFirstName":"System","ownerLastName":"Owner","brandName":"AxonHub"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(initialize_response.status(), StatusCode::OK);

        std::fs::remove_file(db_path).ok();
    }
}
