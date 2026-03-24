use super::build_info::{version, BuildInfo};
use super::capabilities::{
    build_admin_capability, build_admin_graphql_capability, build_identity_capability,
    build_openai_v1_capability, build_openapi_graphql_capability,
    build_provider_edge_admin_capability, build_request_context_capability,
    build_system_bootstrap_capability,
};
use super::cli::{
    parse_config_command, parse_top_level_command, ConfigCommand, TopLevelCommand,
    CONFIG_GET_USAGE_TEXT, CONFIG_USAGE_TEXT, HELP_TEXT,
};
use super::server::startup_messages;
use crate::foundation::{
    provider_edge::PROVIDER_EDGE_REQUIRED_ENV_VARS,
    shared::{
        SqliteFoundation, DEFAULT_USER_API_KEY_VALUE, PRIMARY_DATA_STORAGE_NAME, SYSTEM_KEY_BRAND_NAME,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_VERSION,
    },
    system::SqliteBootstrapService,
};
use axonhub_http::{
    router, HttpState, InitializeSystemRequest, ProviderEdgeAdminCapability,
    SystemBootstrapCapability, SystemBootstrapPort, SystemInitializeError, TraceConfig,
};
use axonhub_http::{
    AdminAuthError, ApiKeyAuthError, ContextResolveError, IdentityCapability, IdentityPort,
    RequestContextCapability, RequestContextPort, AuthUserContext, AuthApiKeyContext,
    ProjectContext, ThreadContext, TraceContext, SignInRequest, SignInSuccess, SignInError,
};
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use pg_embed::pg_enums::PgAuthMethod;
use pg_embed::pg_fetch::{PgFetchSettings, PG_V15};
use pg_embed::postgres::{PgEmbed, PgSettings};
use postgres::{Client as PostgresClient, NoTls};
use std::io::{Read, Write};
use rusqlite::OptionalExtension;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

// Fake identity service for testing unsupported dialect with auth
#[derive(Clone)]
struct FakeIdentityService;

impl FakeIdentityService {
    fn new() -> Self {
        Self
    }
}

impl IdentityPort for FakeIdentityService {
    fn admin_signin(&self, _request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        unimplemented!()
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        if token == "valid-admin-token" {
            Ok(AuthUserContext {
                id: 1,
                email: "owner@example.com".to_owned(),
                first_name: "System".to_owned(),
                last_name: "Owner".to_owned(),
                is_owner: true,
                prefer_language: "en".to_owned(),
                avatar: Some(String::new()),
                scopes: vec![],
                roles: vec![],
                projects: vec![],
            })
        } else {
            Err(AdminAuthError::InvalidToken)
        }
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        _allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        match key {
            Some("service-key-123") => Ok(AuthApiKeyContext {
                id: 11,
                key: "service-key-123".to_owned(),
                name: "Service Key".to_owned(),
                key_type: axonhub_http::ApiKeyType::ServiceAccount,
                project: ProjectContext {
                    id: 1,
                    name: "Default Project".to_owned(),
                    status: "active".to_owned(),
                },
                scopes: vec!["write_requests".to_owned()],
            }),
            Some(_) => Err(ApiKeyAuthError::Invalid),
            None => Err(ApiKeyAuthError::Missing),
        }
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        let key = query_key.or(header_key).ok_or(ApiKeyAuthError::Missing)?;
        self.authenticate_api_key(Some(key), false)
    }
}

impl RequestContextPort for FakeIdentityService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        if project_id == 1 {
            Ok(Some(ProjectContext {
                id: 1,
                name: "Default Project".to_owned(),
                status: "active".to_owned(),
            }))
        } else {
            Ok(None)
        }
    }

    fn resolve_thread(
        &self,
        _project_id: i64,
        _thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        Ok(None)
    }

    fn resolve_trace(
        &self,
        _project_id: i64,
        _trace_id: &str,
        _thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        Ok(None)
    }
}

fn temp_sqlite_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("axonhub-{name}-{unique}.db"))
}

fn temp_postgres_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("axonhub-{name}-{unique}"))
}

fn available_tcp_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("bind ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port()
}

async fn start_embedded_postgres(name: &str) -> (PgEmbed, String, PathBuf) {
    let data_dir = temp_postgres_dir(name);
    let port = available_tcp_port();
    let settings = PgSettings {
        database_dir: data_dir.clone(),
        port,
        user: "postgres".to_owned(),
        password: "postgres".to_owned(),
        auth_method: PgAuthMethod::Plain,
        persistent: false,
        timeout: Some(Duration::from_secs(60)),
        migration_dir: None,
    };

    let mut pg = PgEmbed::new(
        settings,
        PgFetchSettings {
            version: PG_V15,
            ..Default::default()
        },
    )
    .await
    .expect("create embedded postgres");
    pg.setup().await.expect("setup embedded postgres");
    pg.start_db().await.expect("start embedded postgres");

    let dsn = pg.full_db_uri("postgres");
    (pg, dsn, data_dir)
}

fn provider_edge_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct ProviderEdgeEnvFixture {
    _guard: MutexGuard<'static, ()>,
    previous: Vec<(&'static str, Option<String>)>,
}

impl ProviderEdgeEnvFixture {
    fn new() -> Self {
        let guard = provider_edge_env_lock()
            .lock()
            .expect("lock provider-edge env fixture");
        let previous = PROVIDER_EDGE_REQUIRED_ENV_VARS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();

        for key in PROVIDER_EDGE_REQUIRED_ENV_VARS {
            std::env::remove_var(key);
        }

        Self {
            _guard: guard,
            previous,
        }
    }

    fn set_all(&self) {
        for (key, value) in provider_edge_env_values() {
            std::env::set_var(key, value);
        }
    }
}

impl Drop for ProviderEdgeEnvFixture {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn provider_edge_env_values() -> Vec<(&'static str, &'static str)> {
    vec![
        ("AXONHUB_PROVIDER_EDGE_CODEX_AUTHORIZE_URL", "https://example.test/codex/authorize"),
        ("AXONHUB_PROVIDER_EDGE_CODEX_TOKEN_URL", "https://example.test/codex/token"),
        ("AXONHUB_PROVIDER_EDGE_CODEX_CLIENT_ID", "codex-client-id"),
        ("AXONHUB_PROVIDER_EDGE_CODEX_REDIRECT_URI", "http://localhost:1455/auth/callback"),
        ("AXONHUB_PROVIDER_EDGE_CODEX_SCOPES", "openid profile email offline_access"),
        ("AXONHUB_PROVIDER_EDGE_CODEX_USER_AGENT", "codex-test-agent"),
        ("AXONHUB_PROVIDER_EDGE_CLAUDECODE_AUTHORIZE_URL", "https://example.test/claudecode/authorize"),
        ("AXONHUB_PROVIDER_EDGE_CLAUDECODE_TOKEN_URL", "https://example.test/claudecode/token"),
        ("AXONHUB_PROVIDER_EDGE_CLAUDECODE_CLIENT_ID", "claudecode-client-id"),
        ("AXONHUB_PROVIDER_EDGE_CLAUDECODE_REDIRECT_URI", "http://localhost:54545/callback"),
        ("AXONHUB_PROVIDER_EDGE_CLAUDECODE_SCOPES", "org:create_api_key user:profile user:inference"),
        ("AXONHUB_PROVIDER_EDGE_CLAUDECODE_USER_AGENT", "claudecode-test-agent"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_AUTHORIZE_URL", "https://example.test/antigravity/authorize"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_TOKEN_URL", "https://example.test/antigravity/token"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_ID", "antigravity-client-id"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_SECRET", "antigravity-client-secret"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_REDIRECT_URI", "http://localhost:51121/oauth-callback"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_SCOPES", "scope-a scope-b"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_LOAD_ENDPOINTS", "https://example.test/load-a,https://example.test/load-b"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_USER_AGENT", "antigravity-test-agent"),
        ("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_METADATA", r#"{"ideType":"ANTIGRAVITY"}"#),
        ("AXONHUB_PROVIDER_EDGE_COPILOT_DEVICE_CODE_URL", "https://example.test/copilot/device/code"),
        ("AXONHUB_PROVIDER_EDGE_COPILOT_ACCESS_TOKEN_URL", "https://example.test/copilot/access/token"),
        ("AXONHUB_PROVIDER_EDGE_COPILOT_CLIENT_ID", "copilot-client-id"),
        ("AXONHUB_PROVIDER_EDGE_COPILOT_SCOPE", "read:user"),
    ]
}

fn mock_openai_v1_runtime_server_url() -> &'static str {
    static SERVER_URL: OnceLock<String> = OnceLock::new();
    SERVER_URL
        .get_or_init(|| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = match stream {
                        Ok(stream) => stream,
                        Err(_) => continue,
                    };
                    let mut request_bytes = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    let mut header_end = None;
                    let mut expected_body_len = None;
                    loop {
                        let size = match stream.read(&mut buffer) {
                            Ok(size) => size,
                            Err(_) => break,
                        };
                        if size == 0 {
                            break;
                        }
                        request_bytes.extend_from_slice(&buffer[..size]);

                        if header_end.is_none() {
                            header_end = request_bytes
                                .windows(4)
                                .position(|window| window == b"\r\n\r\n")
                                .map(|position| position + 4);
                            if let Some(end) = header_end {
                                let headers = String::from_utf8_lossy(&request_bytes[..end]);
                                expected_body_len = headers
                                    .lines()
                                    .find_map(|line| {
                                        line.split_once(':').and_then(|(name, value)| {
                                            name.trim()
                                                .eq_ignore_ascii_case("content-length")
                                                .then(|| value.trim().parse::<usize>().ok())
                                                .flatten()
                                        })
                                    })
                                    .or(Some(0));
                            }
                        }

                        if let (Some(end), Some(body_len)) = (header_end, expected_body_len) {
                            if request_bytes.len() >= end + body_len {
                                break;
                            }
                        }
                    }

                    let request = String::from_utf8_lossy(&request_bytes);
                    let request_line = request.lines().next().unwrap_or_default().to_owned();
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/");
                    let raw_body = request.split("\r\n\r\n").nth(1).unwrap_or("");
                    let request_json = serde_json::from_str::<serde_json::Value>(raw_body)
                        .unwrap_or(serde_json::Value::Null);
                    let request_model = request_json
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("gpt-4o");

                    let body = if path.ends_with("/chat/completions") {
                        serde_json::json!({
                            "id": format!("chatcmpl_{}", request_model.replace('-', "_")),
                            "object": "chat.completion",
                            "created": 1,
                            "model": request_model,
                            "choices": [{
                                "index": 0,
                                "message": {"role": "assistant", "content": "hi"},
                                "finish_reason": "stop"
                            }],
                            "usage": {
                                "prompt_tokens": 10,
                                "completion_tokens": 5,
                                "total_tokens": 15,
                                "prompt_tokens_details": {"cached_tokens": 2},
                                "completion_tokens_details": {"reasoning_tokens": 1}
                            }
                        })
                        .to_string()
                    } else if path.ends_with("/responses") {
                        serde_json::json!({
                            "id": "resp_mock",
                            "object": "response",
                            "created_at": 1,
                            "model": request_model,
                            "status": "completed",
                            "output": [{
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": "hi", "annotations": []}],
                                "status": "completed"
                            }],
                            "usage": {
                                "input_tokens": 12,
                                "input_tokens_details": {
                                    "cached_tokens": 3,
                                    "write_cached_tokens": 4,
                                    "write_cached_5min_tokens": 4
                                },
                                "output_tokens": 4,
                                "output_tokens_details": {
                                    "reasoning_tokens": 1,
                                    "accepted_prediction_tokens": 2,
                                    "rejected_prediction_tokens": 3
                                },
                                "total_tokens": 16
                            }
                        })
                        .to_string()
                    } else if path.ends_with("/videos/video_mock_task") {
                        if request_line.starts_with("GET ") {
                            serde_json::json!({
                                "id": "video_mock_task",
                                "model": "seedance-1.0",
                                "status": "succeeded",
                                "content": {"video_url": "https://example.com/generated.mp4"},
                                "created_at": 1,
                                "completed_at": 2
                            })
                            .to_string()
                        } else {
                            serde_json::json!({"id": "video_mock_task"}).to_string()
                        }
                    } else if path.ends_with("/videos") {
                        serde_json::json!({"id": "video_mock_task"}).to_string()
                    } else if path.ends_with("/rerank") {
                        serde_json::json!({
                            "object": "list",
                            "model": request_model,
                            "results": [{"index": 0, "relevance_score": 0.99}],
                            "usage": {"prompt_tokens": 5, "total_tokens": 5}
                        })
                        .to_string()
                    } else {
                        serde_json::json!({
                            "object": "list",
                            "data": [{"object": "embedding", "embedding": [0.1, 0.2], "index": 0}],
                            "model": request_model,
                            "usage": {"prompt_tokens": 8, "total_tokens": 8}
                        })
                        .to_string()
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
            });
            format!("http://{}/v1", address)
        })
        .as_str()
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

#[test]
fn postgres_bootstrap_capability_is_available_and_persists_initialized_state() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (mut embedded_pg, dsn, data_dir) =
        runtime.block_on(start_embedded_postgres("postgres-bootstrap-capability"));

    let capability = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match capability {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };

    assert!(!system.is_initialized().unwrap());

    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    assert!(system.is_initialized().unwrap());

    let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
    let row = connection
        .query_one(
            "SELECT value FROM systems WHERE key = $1 AND deleted_at = 0",
            &[&SYSTEM_KEY_VERSION],
        )
        .unwrap();
    let version: String = row.get(0);
    assert_eq!(version, "v0.9.20");

    runtime.block_on(embedded_pg.stop_db()).unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[test]
fn postgres_identity_capability_supports_signin_jwt_and_service_api_key_auth() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (mut embedded_pg, dsn, data_dir) =
        runtime.block_on(start_embedded_postgres("postgres-identity-capability"));

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    let capability = build_identity_capability("postgresql", &dsn, false);
    let identity = match capability {
        IdentityCapability::Available { identity } => identity,
        IdentityCapability::Unsupported { message } => {
            panic!("Expected postgres identity capability to be available: {message}");
        }
    };

    let signin = identity
        .admin_signin(&SignInRequest {
            email: "owner@example.com".to_owned(),
            password: "password123".to_owned(),
        })
        .unwrap();
    assert_eq!(signin.user.email, "owner@example.com");
    assert!(signin.user.is_owner);
    assert!(!signin.token.is_empty());

    let jwt_user = identity.authenticate_admin_jwt(&signin.token).unwrap();
    assert_eq!(jwt_user.email, "owner@example.com");
    assert!(jwt_user.is_owner);

    let api_key = identity
        .authenticate_api_key(Some("service-key-123"), false)
        .unwrap();
    assert_eq!(api_key.project.id, 1);
    assert_eq!(api_key.project.name, "Default Project");
    assert_eq!(api_key.key_type, axonhub_http::ApiKeyType::ServiceAccount);

    runtime.block_on(embedded_pg.stop_db()).unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[test]
fn postgres_request_context_capability_resolves_project_thread_and_trace() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (mut embedded_pg, dsn, data_dir) =
        runtime.block_on(start_embedded_postgres("postgres-request-context-capability"));

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    let capability = build_request_context_capability("postgres", &dsn, false);
    let request_context = match capability {
        RequestContextCapability::Available { request_context } => request_context,
        RequestContextCapability::Unsupported { message } => {
            panic!("Expected postgres request-context capability to be available: {message}");
        }
    };

    let project = request_context.resolve_project(1).unwrap().unwrap();
    assert_eq!(project.id, 1);
    assert_eq!(project.name, "Default Project");

    let thread = request_context.resolve_thread(project.id, "thread-postgres-1").unwrap().unwrap();
    let same_thread = request_context.resolve_thread(project.id, "thread-postgres-1").unwrap().unwrap();
    assert_eq!(same_thread.id, thread.id);
    assert_eq!(same_thread.thread_id, "thread-postgres-1");

    let trace = request_context
        .resolve_trace(project.id, "trace-postgres-1", Some(thread.id))
        .unwrap()
        .unwrap();
    let same_trace = request_context
        .resolve_trace(project.id, "trace-postgres-1", Some(thread.id))
        .unwrap()
        .unwrap();
    assert_eq!(same_trace.id, trace.id);
    assert_eq!(same_trace.thread_id, Some(thread.id));

    let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
    let thread_count: i64 = connection
        .query_one("SELECT COUNT(*) FROM threads WHERE thread_id = $1", &[&"thread-postgres-1"])
        .unwrap()
        .get(0);
    let trace_count: i64 = connection
        .query_one("SELECT COUNT(*) FROM traces WHERE trace_id = $1", &[&"trace-postgres-1"])
        .unwrap()
        .get(0);
    assert_eq!(thread_count, 1);
    assert_eq!(trace_count, 1);

    runtime.block_on(embedded_pg.stop_db()).unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn postgres_router_signin_and_debug_context_routes_work_for_auth_and_context() {
    let (mut embedded_pg, dsn, data_dir) = start_embedded_postgres("postgres-auth-context-router").await;

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability("postgres", &dsn, false),
        request_context: build_request_context_capability("postgres", &dsn, false),
        openai_v1: build_openai_v1_capability("postgres", &dsn),
        admin: build_admin_capability("postgres", &dsn),
        admin_graphql: build_admin_graphql_capability("postgres", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let signin_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/auth/signin")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"email":"owner@example.com","password":"password123"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(signin_response.status(), StatusCode::OK);
    let signin_body = axum::body::to_bytes(signin_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let signin_json = serde_json::from_slice::<serde_json::Value>(&signin_body).unwrap();
    let token = signin_json["token"]
        .as_str()
        .expect("expected jwt token")
        .to_owned();
    assert_eq!(signin_json["user"]["email"], "owner@example.com");

    let admin_debug_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/debug/context")
                .method(Method::GET.as_str())
                .header("Authorization", format!("Bearer {token}"))
                .header("X-Project-ID", "gid://axonhub/project/1")
                .header("AH-Thread-Id", "thread-router-postgres")
                .header("AH-Trace-Id", "trace-router-postgres")
                .header("X-Request-Id", "req-router-postgres")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_debug_response.status(), StatusCode::OK);
    let admin_debug_body = axum::body::to_bytes(admin_debug_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let admin_debug_json = serde_json::from_slice::<serde_json::Value>(&admin_debug_body).unwrap();
    assert_eq!(admin_debug_json["auth"]["mode"], "jwt");
    assert_eq!(admin_debug_json["auth"]["user_id"], 1);
    assert_eq!(admin_debug_json["project"]["id"], 1);
    assert_eq!(admin_debug_json["thread"]["threadId"], "thread-router-postgres");
    assert_eq!(admin_debug_json["trace"]["traceId"], "trace-router-postgres");
    assert_eq!(admin_debug_json["requestId"], "req-router-postgres");

    let openapi_debug_response = app
        .oneshot(
            Request::builder()
                .uri("/openapi/debug/context")
                .method(Method::GET.as_str())
                .header("Authorization", "Bearer service-key-123")
                .header("AH-Thread-Id", "thread-service-postgres")
                .header("AH-Trace-Id", "trace-service-postgres")
                .header("X-Request-Id", "req-service-postgres")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(openapi_debug_response.status(), StatusCode::OK);
    let openapi_debug_body = axum::body::to_bytes(openapi_debug_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let openapi_debug_json = serde_json::from_slice::<serde_json::Value>(&openapi_debug_body).unwrap();
    assert_eq!(openapi_debug_json["auth"]["mode"], "api_key");
    assert_eq!(openapi_debug_json["auth"]["api_key_id"], 2);
    assert_eq!(openapi_debug_json["auth"]["api_key_type"], "service_account");
    assert_eq!(openapi_debug_json["project"]["id"], 1);
    assert_eq!(openapi_debug_json["thread"]["threadId"], "thread-service-postgres");
    assert_eq!(openapi_debug_json["trace"]["traceId"], "trace-service-postgres");
    assert_eq!(openapi_debug_json["requestId"], "req-service-postgres");

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[test]
fn postgres_admin_request_content_route_downloads_seeded_content() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (mut embedded_pg, dsn, data_dir) =
        runtime.block_on(start_embedded_postgres("postgres-admin-request-content"));
    let content_dir = temp_postgres_dir("postgres-admin-request-content-files");
    std::fs::create_dir_all(&content_dir).unwrap();

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    let identity = match build_identity_capability("postgres", &dsn, false) {
        IdentityCapability::Available { identity } => identity,
        IdentityCapability::Unsupported { message } => {
            panic!("Expected postgres identity capability to be available: {message}");
        }
    };
    let token = identity
        .admin_signin(&SignInRequest {
            email: "owner@example.com".to_owned(),
            password: "password123".to_owned(),
        })
        .unwrap()
        .token;

    let (project_id, request_id) = {
        let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
        let project_id: i64 = connection
            .query_one("SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1", &[])
            .unwrap()
            .get(0);
        let storage_settings = serde_json::json!({
            "directory": content_dir.to_string_lossy(),
        })
        .to_string();
        let storage_params: [&(dyn postgres::types::ToSql + Sync); 3] = [
            &"Postgres Request Content FS",
            &"postgres-admin-read-test",
            &storage_settings,
        ];
        let storage_id: i64 = connection
            .query_one(
                "INSERT INTO data_storages (name, description, \"primary\", type, settings, status, deleted_at)
                 VALUES ($1, $2, FALSE, 'fs', $3, 'active', 0)
                 RETURNING id",
                &storage_params,
            )
            .unwrap()
            .get(0);

        let request_id: i64 = connection
            .query_one(
                "INSERT INTO requests (
                    api_key_id, project_id, trace_id, data_storage_id, source, model_id, format,
                    request_headers, request_body, response_body, response_chunks, channel_id,
                    external_id, status, stream, client_ip, metrics_latency_ms,
                    metrics_first_token_latency_ms, content_saved, content_storage_id,
                    content_storage_key, content_saved_at
                ) VALUES (
                    NULL, $1, NULL, $2, 'api', 'gpt-4o', 'openai/video',
                    '{}', '{}', NULL, NULL, NULL,
                    NULL, 'completed', FALSE, '', NULL,
                    NULL, TRUE, $2,
                    '', '2026-03-23T00:00:00Z'
                ) RETURNING id",
                &[&project_id, &storage_id],
            )
            .unwrap()
            .get(0);

        let content_key = format!("/{project_id}/requests/{request_id}/video/video.mp4");
        let update_params: [&(dyn postgres::types::ToSql + Sync); 2] = [&request_id, &content_key];
        connection
            .execute(
                "UPDATE requests SET content_storage_key = $2 WHERE id = $1",
                &update_params,
            )
            .unwrap();

        (project_id, request_id)
    };

    let full_path = content_dir.join(format!("{project_id}/requests/{request_id}/video/video.mp4"));
    std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
    std::fs::write(&full_path, b"postgres-video-content").unwrap();

    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability("postgres", &dsn, false),
        request_context: build_request_context_capability("postgres", &dsn, false),
        openai_v1: build_openai_v1_capability("postgres", &dsn),
        admin: build_admin_capability("postgresql", &dsn),
        admin_graphql: build_admin_graphql_capability("postgres", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let response = runtime
        .block_on(
            router(state).oneshot(
                Request::builder()
                    .uri(format!("/admin/requests/{request_id}/content"))
                    .method(Method::GET.as_str())
                    .header("Authorization", format!("Bearer {token}"))
                    .header("X-Project-ID", format!("gid://axonhub/project/{project_id}"))
                    .body(Body::empty())
                    .unwrap(),
            ),
        )
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("video.mp4"));
    let body = runtime
        .block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .unwrap();
    assert_eq!(body.as_ref(), b"postgres-video-content");

    runtime.block_on(embedded_pg.stop_db()).unwrap();
    std::fs::remove_dir_all(data_dir).ok();
    std::fs::remove_dir_all(content_dir).ok();
}

#[tokio::test]
async fn postgres_admin_graphql_route_executes_current_subset() {
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-admin-graphql-subset").await;

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    let identity = match build_identity_capability("postgres", &dsn, false) {
        IdentityCapability::Available { identity } => identity,
        IdentityCapability::Unsupported { message } => {
            panic!("Expected postgres identity capability to be available: {message}");
        }
    };
    let token = identity
        .admin_signin(&SignInRequest {
            email: "owner@example.com".to_owned(),
            password: "password123".to_owned(),
        })
        .unwrap()
        .token;

    std::thread::spawn({
        let dsn = dsn.clone();
        move || {
            let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
            connection
                .execute(
                    "INSERT INTO models (developer, model_id, type, name, icon, remark, model_card, settings, status)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                    &[
                        &"openai",
                        &"gpt-4",
                        &"chat",
                        &"GPT-4",
                        &"icon",
                        &"Test model",
                        &"{}",
                        &"{}",
                        &"enabled",
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO models (developer, model_id, type, name, icon, remark, model_card, settings, status)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                    &[
                        &"anthropic",
                        &"claude-3",
                        &"chat",
                        &"Claude 3",
                        &"icon",
                        &"Test model 2",
                        &"{}",
                        &"{}",
                        &"disabled",
                    ],
                )
                .unwrap();
        }
    })
    .join()
    .expect("postgres model seed thread");

    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability("postgres", &dsn, false),
        request_context: build_request_context_capability("postgres", &dsn, false),
        openai_v1: build_openai_v1_capability("postgres", &dsn),
        admin: build_admin_capability("postgres", &dsn),
        admin_graphql: build_admin_graphql_capability("postgresql", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{
                        "query": "query Models($input: QueryModelsInput!) { queryModels(input: $input) { id status } }",
                        "variables": { "input": {} }
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    let models = json["data"]["queryModels"]
        .as_array()
        .expect("expected queryModels array");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["id"], "gid://axonhub/model/1");
    assert_eq!(models[0]["status"], "enabled");
    assert_eq!(models[1]["id"], "gid://axonhub/model/2");
    assert_eq!(models[1]["status"], "disabled");

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn postgres_openai_v1_subset_routes_execute_and_keep_unported_images_truthful() {
    let (mut embedded_pg, dsn, data_dir) = start_embedded_postgres("postgres-openai-v1-subset").await;

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    std::thread::spawn({
        let dsn = dsn.clone();
        move || {
            let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
            let base_url = mock_openai_v1_runtime_server_url().to_owned();
            let channel_rows = [
                (
                    "OpenAI Mock",
                    "openai",
                    base_url.clone(),
                    r#"["gpt-4o"]"#,
                    100_i64,
                ),
                (
                    "Anthropic Alias Mock",
                    "openai",
                    base_url.clone(),
                    r#"["claude-3-5-sonnet"]"#,
                    95_i64,
                ),
                (
                    "Jina Embeddings Mock",
                    "jina",
                    base_url.clone(),
                    r#"["jina-embeddings-v3"]"#,
                    90_i64,
                ),
                (
                    "Jina Rerank Mock",
                    "jina",
                    base_url.clone(),
                    r#"["jina-reranker-v2-base-multilingual"]"#,
                    85_i64,
                ),
            ];
            for (name, channel_type, base_url, supported_models, ordering_weight) in channel_rows {
                let params: [&(dyn postgres::types::ToSql + Sync); 8] = [
                    &channel_type,
                    &base_url,
                    &name,
                    &r#"{"apiKey":"test-upstream-key"}"#,
                    &supported_models,
                    &"{}",
                    &"[]",
                    &ordering_weight,
                ];
                connection
                    .execute(
                        "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at)
                         VALUES ($1, $2, $3, 'enabled', $4, $5, FALSE, '', $6, $7, $8, '', '', 0)",
                        &params,
                    )
                    .unwrap();
            }

            let model_rows = [
                (
                    "openai",
                    "gpt-4o",
                    "chat",
                    "GPT-4o",
                    "OpenAI",
                    "openai",
                    r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0,"cacheRead":0.5,"cacheWrite":0.25,"cacheWrite5m":0.125},"vision":true,"toolCall":true,"reasoning":{"supported":true},"costPriceReferenceId":"price-ref-task9"}"#,
                ),
                (
                    "anthropic",
                    "claude-3-5-sonnet",
                    "chat",
                    "Claude 3.5 Sonnet",
                    "Anthropic",
                    "anthropic",
                    r#"{"limit":{"context":200000,"output":4096},"cost":{"input":1.0,"output":2.0}}"#,
                ),
                (
                    "jina",
                    "jina-embeddings-v3",
                    "embedding",
                    "Jina Embeddings v3",
                    "Jina",
                    "jina",
                    r#"{"limit":{"context":8192,"output":0},"cost":{"input":1.0,"output":0.0}}"#,
                ),
                (
                    "jina",
                    "jina-reranker-v2-base-multilingual",
                    "rerank",
                    "Jina Reranker",
                    "Jina",
                    "jina",
                    r#"{"limit":{"context":8192,"output":0},"cost":{"input":1.0,"output":0.0}}"#,
                ),
            ];
            for (developer, model_id, model_type, name, icon, group_name, model_card_json) in model_rows {
                let params: [&(dyn postgres::types::ToSql + Sync); 7] = [
                    &developer,
                    &model_id,
                    &model_type,
                    &name,
                    &icon,
                    &group_name,
                    &model_card_json,
                ];
                connection
                    .execute(
                        "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at)
                         VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', 'enabled', '', 0)",
                        &params,
                    )
                    .unwrap();
            }
        }
    })
    .join()
    .expect("postgres /v1 seed thread");

    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability("postgres", &dsn, false),
        request_context: build_request_context_capability("postgres", &dsn, false),
        openai_v1: build_openai_v1_capability("postgresql", &dsn),
        admin: build_admin_capability("postgres", &dsn),
        admin_graphql: build_admin_graphql_capability("postgres", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let models_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models?include=all")
                .method(Method::GET)
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(models_response.status(), StatusCode::OK);
    let models_json = serde_json::from_slice::<serde_json::Value>(
        &axum::body::to_bytes(models_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(models_json["data"][0]["id"], "gpt-4o");

    for (path, body, expected_json_path, expected_value) in [
        (
            "/v1/chat/completions",
            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
            vec!["id"],
            serde_json::Value::String("chatcmpl_gpt_4o".to_owned()),
        ),
        (
            "/v1/responses",
            r#"{"model":"gpt-4o","input":"hi"}"#,
            vec!["id"],
            serde_json::Value::String("resp_mock".to_owned()),
        ),
        (
            "/v1/embeddings",
            r#"{"model":"gpt-4o","input":"hi"}"#,
            vec!["model"],
            serde_json::Value::String("gpt-4o".to_owned()),
        ),
        (
            "/v1/messages",
            r#"{"model":"claude-3-5-sonnet","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
            vec!["type"],
            serde_json::Value::String("message".to_owned()),
        ),
        (
            "/v1/rerank",
            r#"{"model":"jina-reranker-v2-base-multilingual","query":"hello","documents":["a"]}"#,
            vec!["model"],
            serde_json::Value::String("jina-reranker-v2-base-multilingual".to_owned()),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-postgres-v1")
                    .header("AH-Trace-Id", "trace-postgres-v1")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "path {path}");
        let json = serde_json::from_slice::<serde_json::Value>(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let actual = expected_json_path.iter().fold(&json, |current, key| &current[*key]);
        assert_eq!(actual, &expected_value, "path {path}");
    }

    let unported = app
        .oneshot(
            Request::builder()
                .uri("/v1/images")
                .method(Method::POST)
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unported.status(), StatusCode::NOT_IMPLEMENTED);

    let (request_statuses, execution_statuses, usage_count, trace_thread_count) = std::thread::spawn({
        let dsn = dsn.clone();
        move || {
            let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
            let request_statuses = connection
                .query("SELECT status FROM requests ORDER BY id ASC", &[])
                .unwrap()
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
            let execution_statuses = connection
                .query("SELECT status FROM request_executions ORDER BY id ASC", &[])
                .unwrap()
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
            let usage_count: i64 = connection
                .query_one("SELECT COUNT(*) FROM usage_logs", &[])
                .unwrap()
                .get(0);
            let trace_thread_count: i64 = connection
                .query_one(
                    "SELECT COUNT(*) FROM traces t JOIN threads th ON th.id = t.thread_id WHERE t.trace_id = $1 AND th.thread_id = $2",
                    &[&"trace-postgres-v1", &"thread-postgres-v1"],
                )
                .unwrap()
                .get(0);
            (request_statuses, execution_statuses, usage_count, trace_thread_count)
        }
    })
    .join()
    .expect("postgres /v1 verification thread");

    assert_eq!(request_statuses, vec!["completed", "completed", "completed", "completed", "completed"]);
    assert_eq!(execution_statuses, vec!["completed", "completed", "completed", "completed", "completed"]);
    assert_eq!(usage_count, 5);
    assert_eq!(trace_thread_count, 1);

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn postgres_openai_v1_video_routes_execute_and_keep_unported_images_truthful() {
    let (mut embedded_pg, dsn, data_dir) = start_embedded_postgres("postgres-openai-v1-videos").await;

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    std::thread::spawn({
        let dsn = dsn.clone();
        move || {
            let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
            let base_url = mock_openai_v1_runtime_server_url().to_owned();
            let channel_params: [&(dyn postgres::types::ToSql + Sync); 8] = [
                &"openai",
                &base_url,
                &"Doubao Video Alias Mock",
                &r#"{"apiKey":"test-upstream-key"}"#,
                &r#"["seedance-1.0"]"#,
                &"{}",
                &"[]",
                &100_i64,
            ];
            connection
                .execute(
                    "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at)
                     VALUES ($1, $2, $3, 'enabled', $4, $5, FALSE, '', $6, $7, $8, '', '', 0)",
                    &channel_params,
                )
                .unwrap();

            let model_params: [&(dyn postgres::types::ToSql + Sync); 7] = [
                &"doubao",
                &"seedance-1.0",
                &"video",
                &"Seedance 1.0",
                &"Doubao",
                &"doubao",
                &r#"{"limit":{"context":8192,"output":0},"cost":{"input":1.0,"output":0.0}}"#,
            ];
            connection
                .execute(
                    "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, '{}', 'enabled', '', 0)",
                    &model_params,
                )
                .unwrap();
        }
    })
    .join()
    .expect("postgres /v1 videos seed thread");

    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability("postgres", &dsn, false),
        request_context: build_request_context_capability("postgres", &dsn, false),
        openai_v1: build_openai_v1_capability("postgres", &dsn),
        admin: build_admin_capability("postgres", &dsn),
        admin_graphql: build_admin_graphql_capability("postgres", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/videos")
                .method(Method::POST)
                .header("content-type", "application/json")
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .header("X-Project-ID", "gid://axonhub/project/1")
                .header("AH-Thread-Id", "thread-postgres-video")
                .header("AH-Trace-Id", "trace-postgres-video")
                .body(Body::from(
                    r#"{"model":"seedance-1.0","content":[{"type":"text","text":"make a trailer"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_json = serde_json::from_slice::<serde_json::Value>(
        &axum::body::to_bytes(create_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(create_json["id"], "video_mock_task");

    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/videos/video_mock_task")
                .method(Method::GET)
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .header("X-Project-ID", "gid://axonhub/project/1")
                .header("AH-Thread-Id", "thread-postgres-video")
                .header("AH-Trace-Id", "trace-postgres-video")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_json = serde_json::from_slice::<serde_json::Value>(
        &axum::body::to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(get_json["id"], "video_mock_task");
    assert_eq!(get_json["content"]["video_url"], "https://example.com/generated.mp4");

    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/videos/video_mock_task")
                .method(Method::DELETE)
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .header("X-Project-ID", "gid://axonhub/project/1")
                .header("AH-Thread-Id", "thread-postgres-video")
                .header("AH-Trace-Id", "trace-postgres-video")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
    let delete_body = axum::body::to_bytes(delete_response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(delete_body.is_empty());

    let unported_images = app
        .oneshot(
            Request::builder()
                .uri("/v1/images")
                .method(Method::POST)
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unported_images.status(), StatusCode::NOT_IMPLEMENTED);

    let (request_formats, execution_statuses, trace_thread_count) = std::thread::spawn({
        let dsn = dsn.clone();
        move || {
            let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
            let request_formats = connection
                .query("SELECT format FROM requests ORDER BY id ASC", &[])
                .unwrap()
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
            let execution_statuses = connection
                .query("SELECT status FROM request_executions ORDER BY id ASC", &[])
                .unwrap()
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
            let trace_thread_count: i64 = connection
                .query_one(
                    "SELECT COUNT(*) FROM traces t JOIN threads th ON th.id = t.thread_id WHERE t.trace_id = $1 AND th.thread_id = $2",
                    &[&"trace-postgres-video", &"thread-postgres-video"],
                )
                .unwrap()
                .get(0);
            (request_formats, execution_statuses, trace_thread_count)
        }
    })
    .join()
    .expect("postgres /v1 videos verification thread");

    assert_eq!(request_formats, vec!["doubao/video_create", "doubao/video_get", "doubao/video_delete"]);
    assert_eq!(execution_statuses, vec!["completed", "completed", "completed"]);
    assert_eq!(trace_thread_count, 1);

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
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
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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
        .clone()
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
    assert!(HELP_TEXT.contains("axonhub build-info"));
    assert!(HELP_TEXT.contains("axonhub version"));
    assert!(HELP_TEXT.contains("axonhub help"));

    assert_eq!(CONFIG_USAGE_TEXT, "Usage: axonhub config <preview|validate|get>\n");
    assert!(CONFIG_GET_USAGE_TEXT.contains("server.port    Server port number"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("server.name    Server name"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("server.base_path  Server base path"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("server.debug      Server debug mode"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("db.dialect     Database dialect"));
    assert!(CONFIG_GET_USAGE_TEXT.contains("db.dsn         Database DSN"));
}

#[test]
fn build_info_output_keeps_current_sections() {
    let rendered = BuildInfo::current().to_string();

    assert!(rendered.starts_with(&format!("Version: {}\n", version())));
    assert!(rendered.contains("Go Version: "));
    assert!(rendered.contains("Platform: "));
    assert!(rendered.contains("Uptime: "));
}

#[test]
fn startup_messages_are_truthful_for_metrics_and_identity() {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18092);

    assert_eq!(
        startup_messages("Parity Startup", address, false),
        vec!["Parity Startup listening on http://127.0.0.1:18092".to_owned()]
    );

    assert_eq!(
        startup_messages("Parity Startup", address, true),
        vec![
            "Metrics exporter initialized for Rust server runtime.".to_owned(),
            "Parity Startup listening on http://127.0.0.1:18092".to_owned(),
        ]
    );
}

#[tokio::test]
async fn postgres_bootstrap_capability_serves_fresh_status_and_initialize() {
    let (mut embedded_pg, dsn, data_dir) = start_embedded_postgres("postgres-bootstrap-router").await;
    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability(
            "postgres",
            &dsn,
            false,
        ),
        request_context: build_request_context_capability(
            "postgres",
            &dsn,
            false,
        ),
        openai_v1: build_openai_v1_capability("postgres", &dsn),
        admin: build_admin_capability("postgres", &dsn),
        admin_graphql: build_admin_graphql_capability("postgres", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
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
    let json = serde_json::from_slice::<serde_json::Value>(&status_body).unwrap();
    assert_eq!(json["isInitialized"], false);

    let initialize_response = app
        .clone()
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
    let initialize_body = axum::body::to_bytes(initialize_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&initialize_body).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "System initialized successfully");

    let second_status_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/system/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second_status_response.status(), StatusCode::OK);
    let second_status_body = axum::body::to_bytes(second_status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&second_status_body).unwrap();
    assert_eq!(json["isInitialized"], true);

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn mysql_bootstrap_routes_remain_truthful_as_unsupported() {
    let db_path = temp_sqlite_path("unsupported-dialect-truthful");
    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("mysql", &db_path.display().to_string(), "v0.9.20"),
        identity: build_identity_capability("mysql", &db_path.display().to_string(), false),
        request_context: build_request_context_capability("mysql", &db_path.display().to_string(), false),
        openai_v1: build_openai_v1_capability("mysql", &db_path.display().to_string()),
        admin: build_admin_capability("mysql", &db_path.display().to_string()),
        admin_graphql: build_admin_graphql_capability("mysql", &db_path.display().to_string()),
        openapi_graphql: build_openapi_graphql_capability("mysql", &db_path.display().to_string()),
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

    assert_eq!(status_response.status(), StatusCode::NOT_IMPLEMENTED);
    let status_body = axum::body::to_bytes(status_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&status_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/system/status");
    assert_eq!(json["path"], "/admin/system/status");
    assert_eq!(json["method"], "GET");
    assert_eq!(
        json["message"],
        "DB-backed admin system status/bootstrap is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3 and postgres."
    );

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn unsupported_dialect_keeps_provider_edge_admin_routes_truthful() {
    let db_path = temp_sqlite_path("unsupported-dialect-provider-edge");
    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability(
            "postgres",
            &db_path.display().to_string(),
            "v0.9.20",
        ),
        identity: IdentityCapability::Available {
            identity: Arc::new(FakeIdentityService::new()),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeIdentityService::new()),
        },
        openai_v1: build_openai_v1_capability("postgres", &db_path.display().to_string()),
        admin: build_admin_capability("postgres", &db_path.display().to_string()),
        admin_graphql: build_admin_graphql_capability(
            "postgres",
            &db_path.display().to_string(),
        ),
        openapi_graphql: build_openapi_graphql_capability(
            "postgres",
            &db_path.display().to_string(),
        ),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let start_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/codex/oauth/start")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer valid-admin-token")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    let start_status = start_response.status();
    let start_body = axum::body::to_bytes(start_response.into_body(), usize::MAX)
        .await
        .unwrap();

    assert_eq!(start_status, StatusCode::NOT_IMPLEMENTED);
    let json = serde_json::from_slice::<serde_json::Value>(&start_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/codex/oauth/start");
    assert_eq!(json["path"], "/admin/codex/oauth/start");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3."
    );

    let exchange_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/codex/oauth/exchange")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer valid-admin-token")
                .body(Body::from(
                    r#"{"session_id":"test-session","callback_url":"http://localhost:3000/callback"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let exchange_status = exchange_response.status();
    let exchange_body = axum::body::to_bytes(exchange_response.into_body(), usize::MAX)
        .await
        .unwrap();

    assert_eq!(exchange_status, StatusCode::NOT_IMPLEMENTED);
    let json = serde_json::from_slice::<serde_json::Value>(&exchange_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/codex/oauth/exchange");
    assert_eq!(json["path"], "/admin/codex/oauth/exchange");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3."
    );

    let exchange_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/codex/oauth/exchange")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer valid-admin-token")
                .body(Body::from(
                    r#"{"session_id":"test-session","callback_url":"http://localhost:3000/callback"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let exchange_status = exchange_response.status();
    let exchange_body = axum::body::to_bytes(exchange_response.into_body(), usize::MAX)
        .await
        .unwrap();

    assert_eq!(exchange_status, StatusCode::NOT_IMPLEMENTED);
    let json = serde_json::from_slice::<serde_json::Value>(&exchange_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/codex/oauth/exchange");
    assert_eq!(json["path"], "/admin/codex/oauth/exchange");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3."
    );

    std::fs::remove_file(db_path).ok();
}

#[test]
fn provider_edge_admin_capability_is_unsupported_without_secure_runtime_config() {
    let _env = ProviderEdgeEnvFixture::new();
    let capability = build_provider_edge_admin_capability("postgres", ":memory:");
    match capability {
        ProviderEdgeAdminCapability::Unsupported { message } => {
            assert!(message.contains("Provider-edge admin OAuth helpers"));
            assert!(message.contains("secure runtime configuration"));
            assert!(message.contains("AXONHUB_PROVIDER_EDGE_"));
        }
        ProviderEdgeAdminCapability::Available { .. } => {
            panic!("Expected Unsupported but got Available");
        }
    }
}

#[test]
fn provider_edge_admin_capability_is_available_on_postgres_with_secure_runtime_config() {
    let env = ProviderEdgeEnvFixture::new();
    env.set_all();

    let capability = build_provider_edge_admin_capability("postgres", ":memory:");
    match capability {
        ProviderEdgeAdminCapability::Available { .. } => {}
        ProviderEdgeAdminCapability::Unsupported { message } => {
            panic!("Expected Available but got Unsupported: {message}");
        }
    }
}

#[tokio::test]
async fn postgres_provider_edge_routes_remain_truthful_when_secure_runtime_config_is_absent() {
    let _env = ProviderEdgeEnvFixture::new();
    let db_path = temp_sqlite_path("postgres-provider-edge-missing-config");
    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability(
            "postgres",
            &db_path.display().to_string(),
            "v0.9.20",
        ),
        identity: IdentityCapability::Available {
            identity: Arc::new(FakeIdentityService::new()),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeIdentityService::new()),
        },
        openai_v1: build_openai_v1_capability("postgres", &db_path.display().to_string()),
        admin: build_admin_capability("postgres", &db_path.display().to_string()),
        admin_graphql: build_admin_graphql_capability("postgres", &db_path.display().to_string()),
        openapi_graphql: build_openapi_graphql_capability(
            "postgres",
            &db_path.display().to_string(),
        ),
        provider_edge_admin: build_provider_edge_admin_capability("postgres", ":memory:"),
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

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/admin/codex/oauth/start")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer valid-admin-token")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
    assert_eq!(json["route_family"], "/admin/codex/oauth/start");
    assert_eq!(json["path"], "/admin/codex/oauth/start");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes."
    );

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn postgres_provider_edge_routes_start_when_secure_runtime_config_is_present() {
    let env = ProviderEdgeEnvFixture::new();
    env.set_all();
    let db_path = temp_sqlite_path("postgres-provider-edge-secure-config");
    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability(
            "postgres",
            &db_path.display().to_string(),
            "v0.9.20",
        ),
        identity: IdentityCapability::Available {
            identity: Arc::new(FakeIdentityService::new()),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeIdentityService::new()),
        },
        openai_v1: build_openai_v1_capability("postgres", &db_path.display().to_string()),
        admin: build_admin_capability("postgres", &db_path.display().to_string()),
        admin_graphql: build_admin_graphql_capability("postgres", &db_path.display().to_string()),
        openapi_graphql: build_openapi_graphql_capability(
            "postgres",
            &db_path.display().to_string(),
        ),
        provider_edge_admin: build_provider_edge_admin_capability("postgres", ":memory:"),
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

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/admin/codex/oauth/start")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer valid-admin-token")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
    assert!(json["session_id"].as_str().is_some_and(|value| !value.is_empty()));
    let auth_url = json["auth_url"].as_str().expect("expected auth_url");
    assert!(auth_url.starts_with("https://example.test/codex/authorize?"));
    assert!(auth_url.contains("client_id=codex-client-id"));

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn unsupported_dialect_keeps_graphql_routes_truthful() {
    let db_path = temp_sqlite_path("unsupported-dialect-graphql");
    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability(
            "postgres",
            &db_path.display().to_string(),
            "v0.9.20",
        ),
        identity: IdentityCapability::Available {
            identity: Arc::new(FakeIdentityService::new()),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeIdentityService::new()),
        },
        openai_v1: build_openai_v1_capability("postgres", &db_path.display().to_string()),
        admin: build_admin_capability("postgres", &db_path.display().to_string()),
        admin_graphql: build_admin_graphql_capability(
            "postgres",
            &db_path.display().to_string(),
        ),
        openapi_graphql: build_openapi_graphql_capability(
            "postgres",
            &db_path.display().to_string(),
        ),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let admin_graphql_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer valid-admin-token")
                .body(Body::from(
                    r#"{"query":"query Models($input: QueryModelsInput!) { queryModels(input: $input) { id status } }","variables":{"input":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(admin_graphql_response.status(), StatusCode::OK);
    let admin_graphql_body = axum::body::to_bytes(admin_graphql_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&admin_graphql_body).unwrap();
    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    assert!(json["data"]["queryModels"].is_array());

    let openapi_graphql_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer service-key-123")
                .body(Body::from(
                    r#"{"query":"mutation { createLLMAPIKey(name: \"Postgres Unsupported Test Key\") { name scopes } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(openapi_graphql_response.status(), StatusCode::OK);
    let openapi_graphql_body = axum::body::to_bytes(openapi_graphql_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&openapi_graphql_body).unwrap();
    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    assert_eq!(
        json["data"]["createLLMAPIKey"]["name"],
        "Postgres Unsupported Test Key"
    );

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn sqlite_backed_openapi_graphql_route_executes_pilot_mutation() {
    let db_path = temp_sqlite_path("openapi-graphql-pilot");

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
        admin_graphql: build_admin_graphql_capability(
            "sqlite3",
            &db_path.display().to_string(),
        ),
        openapi_graphql: build_openapi_graphql_capability(
            "sqlite3",
            &db_path.display().to_string(),
        ),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer service-key-123")
                .body(Body::from(
                    r#"{"query": "mutation { createLLMAPIKey(name: \"SDK Key\") { name scopes } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();

    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    let data = json.get("data").expect("expected data field");
    let api_key = data.get("createLLMAPIKey").expect("expected createLLMAPIKey field");
    assert_eq!(api_key.get("name").and_then(|v| v.as_str()), Some("SDK Key"));
    let scopes = api_key.get("scopes").and_then(|v| v.as_array()).unwrap();
    let scope_strs: Vec<&str> = scopes.iter().filter_map(|v| v.as_str()).collect();
    assert!(scope_strs.contains(&"read_channels"));
    assert!(scope_strs.contains(&"write_requests"));

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn postgres_openapi_graphql_route_executes_pilot_mutation() {
    let (mut embedded_pg, dsn, data_dir) = start_embedded_postgres("postgres-openapi-graphql-pilot").await;

    let bootstrap = build_system_bootstrap_capability("postgres", &dsn, "v0.9.20");
    let system = match bootstrap {
        SystemBootstrapCapability::Available { system } => system,
        SystemBootstrapCapability::Unsupported { message } => {
            panic!("Expected postgres bootstrap capability to be available: {message}");
        }
    };
    system
        .initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        })
        .unwrap();

    let state = HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("postgres", &dsn, "v0.9.20"),
        identity: build_identity_capability("postgres", &dsn, false),
        request_context: build_request_context_capability("postgres", &dsn, false),
        openai_v1: build_openai_v1_capability("postgres", &dsn),
        admin: build_admin_capability("postgres", &dsn),
        admin_graphql: build_admin_graphql_capability("postgres", &dsn),
        openapi_graphql: build_openapi_graphql_capability("postgres", &dsn),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer service-key-123")
                .body(Body::from(
                    r#"{"query": "mutation { createLLMAPIKey(name: \"SDK Key\") { key name scopes } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();

    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    let api_key = json["data"]["createLLMAPIKey"].clone();
    assert_eq!(api_key["name"], "SDK Key");
    assert!(api_key["key"].as_str().is_some_and(|value| value.starts_with("ah-")));
    let scopes = api_key["scopes"].as_array().expect("expected scopes array");
    let scope_strs: Vec<&str> = scopes.iter().filter_map(|v| v.as_str()).collect();
    assert!(scope_strs.contains(&"read_channels"));
    assert!(scope_strs.contains(&"write_requests"));

    let dsn_for_query = dsn.clone();
    let key_for_query = api_key["key"]
        .as_str()
        .expect("expected api key value")
        .to_owned();
    let (stored_name, stored_type, stored_status) = std::thread::spawn(move || {
        let mut connection = PostgresClient::connect(&dsn_for_query, NoTls).unwrap();
        let row = connection
            .query_one(
                "SELECT name, type, status FROM api_keys WHERE key = $1 LIMIT 1",
                &[&key_for_query],
            )
            .unwrap();
        let stored_name: String = row.get(0);
        let stored_type: String = row.get(1);
        let stored_status: String = row.get(2);
        (stored_name, stored_type, stored_status)
    })
    .join()
    .expect("postgres verification thread");
    assert_eq!(stored_name, "SDK Key");
    assert_eq!(stored_type, "user");
    assert_eq!(stored_status, "enabled");

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn sqlite_backed_openapi_graphql_route_rejects_missing_or_invalid_service_api_key() {
    let db_path = temp_sqlite_path("openapi-graphql-auth");
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
        admin_graphql: build_admin_graphql_capability(
            "sqlite3",
            &db_path.display().to_string(),
        ),
        openapi_graphql: build_openapi_graphql_capability(
            "sqlite3",
            &db_path.display().to_string(),
        ),
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
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

    let missing_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query": "mutation { createLLMAPIKey(name: \"SDK Key\") { name scopes } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(missing_response.status(), StatusCode::UNAUTHORIZED);
    let missing_body = axum::body::to_bytes(missing_response.into_body(), usize::MAX).await.unwrap();
    let missing_json = serde_json::from_slice::<serde_json::Value>(&missing_body).unwrap();
    assert_eq!(missing_json["error"]["type"], "Unauthorized");
    assert_eq!(missing_json["error"]["message"], "API key is required");

    let invalid_response = app
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("content-type", "application/json")
                .header("Authorization", "Bearer invalid-key")
                .body(Body::from(
                    r#"{"query": "mutation { createLLMAPIKey(name: \"SDK Key\") { name scopes } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
    let invalid_body = axum::body::to_bytes(invalid_response.into_body(), usize::MAX).await.unwrap();
    let invalid_json = serde_json::from_slice::<serde_json::Value>(&invalid_body).unwrap();
    assert_eq!(invalid_json["error"]["type"], "Unauthorized");
    assert_eq!(invalid_json["error"]["message"], "Invalid API key");

    std::fs::remove_file(db_path).ok();
}
