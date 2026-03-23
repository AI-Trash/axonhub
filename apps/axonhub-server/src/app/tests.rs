use super::build_info::{version, BuildInfo};
use super::capabilities::{
    build_admin_capability, build_admin_graphql_capability, build_identity_capability,
    build_openai_v1_capability, build_openapi_graphql_capability,
    build_request_context_capability,
    build_system_bootstrap_capability,
};
use super::cli::{
    parse_config_command, parse_top_level_command, ConfigCommand, TopLevelCommand,
    CONFIG_GET_USAGE_TEXT, CONFIG_USAGE_TEXT, HELP_TEXT,
};
use crate::foundation::{
    shared::{
        SqliteFoundation, PRIMARY_DATA_STORAGE_NAME, SYSTEM_KEY_BRAND_NAME,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_VERSION,
    },
    system::SqliteBootstrapService,
};
use axonhub_http::{
    router, HttpState, InitializeSystemRequest, ProviderEdgeAdminCapability,
    SystemBootstrapPort, SystemInitializeError, TraceConfig,
};
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
        identity: build_identity_capability(
            "sqlite3",
            &db_path.display().to_string(),
            false,
        ),
        request_context: build_request_context_capability(
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

#[test]
fn top_level_cli_parser_preserves_current_operator_facing_verbs() {
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned()]),
        TopLevelCommand::StartServer
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "config".to_owned()]),
        TopLevelCommand::Config
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "version".to_owned()]),
        TopLevelCommand::Version
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "--version".to_owned()]),
        TopLevelCommand::Version
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "-v".to_owned()]),
        TopLevelCommand::Version
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "help".to_owned()]),
        TopLevelCommand::Help
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "--help".to_owned()]),
        TopLevelCommand::Help
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "-h".to_owned()]),
        TopLevelCommand::Help
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "build-info".to_owned()]),
        TopLevelCommand::BuildInfo
    );
    assert_eq!(
        parse_top_level_command(&["axonhub".to_owned(), "serve".to_owned()]),
        TopLevelCommand::StartServer
    );
}

#[test]
fn config_cli_parser_preserves_current_subcommands() {
    assert_eq!(
        parse_config_command(&[
            "axonhub".to_owned(),
            "config".to_owned(),
            "preview".to_owned(),
        ]),
        Some(ConfigCommand::Preview)
    );
    assert_eq!(
        parse_config_command(&[
            "axonhub".to_owned(),
            "config".to_owned(),
            "validate".to_owned(),
        ]),
        Some(ConfigCommand::Validate)
    );
    assert_eq!(
        parse_config_command(&[
            "axonhub".to_owned(),
            "config".to_owned(),
            "get".to_owned(),
        ]),
        Some(ConfigCommand::Get)
    );
    assert_eq!(
        parse_config_command(&["axonhub".to_owned(), "config".to_owned()]),
        None
    );
    assert_eq!(
        parse_config_command(&[
            "axonhub".to_owned(),
            "config".to_owned(),
            "set".to_owned(),
        ]),
        None
    );
}

#[test]
fn help_and_config_usage_texts_list_current_cli_contract() {
    assert!(HELP_TEXT.contains("axonhub config preview"));
    assert!(HELP_TEXT.contains("axonhub config validate"));
    assert!(HELP_TEXT.contains("axonhub config get <key>"));
    assert!(HELP_TEXT.contains("axonhub version"));
    assert!(HELP_TEXT.contains("axonhub build-info"));
    assert!(HELP_TEXT.contains("axonhub help"));

    assert_eq!(CONFIG_USAGE_TEXT, "Usage: axonhub config <preview|validate|get>\n");
    assert!(CONFIG_GET_USAGE_TEXT.contains("server.port       Server port number"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("db.dialect        Database dialect"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("db.dsn            Database DSN"));
}

#[test]
fn build_info_output_keeps_current_sections() {
    let rendered = BuildInfo::current().to_string();

    assert!(rendered.starts_with(&format!("Version: {}\n", version())));
    assert!(rendered.contains("Platform: "));
    assert!(rendered.contains("Uptime: "));
}
