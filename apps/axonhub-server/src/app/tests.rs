use super::build_info::{version, BuildInfo};
use super::capabilities::{
    build_admin_capability, build_admin_graphql_capability, build_identity_capability,
    build_openai_v1_capability, build_openapi_graphql_capability,
    build_provider_edge_admin_capability, build_request_context_capability,
    build_system_bootstrap_capability,
};
use super::cli::{
    axonhub_cli_command, axonhub_config_cli_command, parse_axonhub_cli, AxonhubCliContract,
};
use super::server::startup_messages;
use crate::foundation::{
    request_context::parse_onboarding_record,
    provider_edge::PROVIDER_EDGE_REQUIRED_ENV_VARS,
    shared::{
        DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_VALUE, PRIMARY_DATA_STORAGE_NAME,
        SYSTEM_KEY_BRAND_NAME, SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_ONBOARDED,
        SYSTEM_KEY_VERSION, graphql_gid,
    },
    sqlite_support::{hash_password, SqliteBootstrapService, SqliteFoundation},
};
use axonhub_http::{
    HttpCorsSettings, HttpState, InitializeSystemRequest, ProviderEdgeAdminCapability,
    SystemBootstrapCapability, SystemBootstrapPort, SystemInitializeError, TraceConfig,
    router as http_router,
};
use axonhub_http::{
    AdminAuthError, ApiKeyAuthError, AuthApiKeyContext, AuthUserContext, ContextResolveError,
    IdentityCapability, IdentityPort, ProjectContext, RequestContextCapability, RequestContextPort,
    SignInError, SignInRequest, SignInSuccess, ThreadContext, TraceContext,
};
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::ServiceResponse;
use actix_web::http::{Method, StatusCode};
use actix_web::test as actix_test;
use clap::{error::ErrorKind, CommandFactory};
use pg_embed::pg_enums::PgAuthMethod;
use pg_embed::pg_fetch::{PgFetchSettings, PG_V15};
use pg_embed::postgres::{PgEmbed, PgSettings};
use postgres::{Client as PostgresClient, NoTls};
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn disabled_test_cors() -> HttpCorsSettings {
    HttpCorsSettings::default()
}

#[derive(Clone)]
struct TestApp {
    state: HttpState,
}

impl TestApp {
    fn new(state: HttpState) -> Self {
        Self { state }
    }

    async fn oneshot(&self, request: TestHttpRequest) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
        let app = actix_test::init_service(http_router(self.state.clone())).await;
        let mut actix_request = actix_test::TestRequest::default()
            .method(Method::from_bytes(request.method.as_bytes()).expect("valid method"))
            .uri(&request.uri);
        for (name, value) in &request.headers {
            actix_request = actix_request.insert_header((name.as_str(), value.as_str()));
        }
        Ok(actix_test::call_service(&app, actix_request.set_payload(request.body).to_request()).await)
    }
}

async fn read_body<B>(response: ServiceResponse<B>) -> Vec<u8>
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    actix_web::body::to_bytes(response.into_body())
        .await
        .unwrap()
        .to_vec()
}

fn router(state: HttpState) -> TestApp {
    TestApp::new(state)
}

struct Body;

impl Body {
    fn empty() -> Vec<u8> {
        Vec::new()
    }

    fn from(value: impl Into<Vec<u8>>) -> Vec<u8> {
        value.into()
    }
}

struct Request;

impl Request {
    fn builder() -> TestRequestBuilder {
        TestRequestBuilder::default()
    }
}

#[derive(Default)]
struct TestRequestBuilder {
    method: Option<String>,
    uri: Option<String>,
    headers: Vec<(String, String)>,
}

impl TestRequestBuilder {
    fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    fn method(mut self, method: impl ToString) -> Self {
        self.method = Some(method.to_string());
        self
    }

    fn header(mut self, name: impl ToString, value: impl ToString) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    fn body(self, body: Vec<u8>) -> Result<TestHttpRequest, Infallible> {
        Ok(TestHttpRequest {
            method: self.method.unwrap_or_else(|| "GET".to_owned()),
            uri: self.uri.unwrap_or_else(|| "/".to_owned()),
            headers: self.headers,
            body,
        })
    }
}

struct TestHttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

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
                profiles_json: None,
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
        (
            "AXONHUB_PROVIDER_EDGE_CODEX_AUTHORIZE_URL",
            "https://example.test/codex/authorize",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CODEX_TOKEN_URL",
            "https://example.test/codex/token",
        ),
        ("AXONHUB_PROVIDER_EDGE_CODEX_CLIENT_ID", "codex-client-id"),
        (
            "AXONHUB_PROVIDER_EDGE_CODEX_REDIRECT_URI",
            "http://localhost:1455/auth/callback",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CODEX_SCOPES",
            "openid profile email offline_access",
        ),
        ("AXONHUB_PROVIDER_EDGE_CODEX_USER_AGENT", "codex-test-agent"),
        (
            "AXONHUB_PROVIDER_EDGE_CLAUDECODE_AUTHORIZE_URL",
            "https://example.test/claudecode/authorize",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CLAUDECODE_TOKEN_URL",
            "https://example.test/claudecode/token",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CLAUDECODE_CLIENT_ID",
            "claudecode-client-id",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CLAUDECODE_REDIRECT_URI",
            "http://localhost:54545/callback",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CLAUDECODE_SCOPES",
            "org:create_api_key user:profile user:inference",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_CLAUDECODE_USER_AGENT",
            "claudecode-test-agent",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_AUTHORIZE_URL",
            "https://example.test/antigravity/authorize",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_TOKEN_URL",
            "https://example.test/antigravity/token",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_ID",
            "antigravity-client-id",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_SECRET",
            "antigravity-client-secret",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_REDIRECT_URI",
            "http://localhost:51121/oauth-callback",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_SCOPES",
            "scope-a scope-b",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_LOAD_ENDPOINTS",
            "https://example.test/load-a,https://example.test/load-b",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_USER_AGENT",
            "antigravity-test-agent",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_METADATA",
            r#"{"ideType":"ANTIGRAVITY"}"#,
        ),
        (
            "AXONHUB_PROVIDER_EDGE_COPILOT_DEVICE_CODE_URL",
            "https://example.test/copilot/device/code",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_COPILOT_ACCESS_TOKEN_URL",
            "https://example.test/copilot/access/token",
        ),
        (
            "AXONHUB_PROVIDER_EDGE_COPILOT_CLIENT_ID",
            "copilot-client-id",
        ),
        ("AXONHUB_PROVIDER_EDGE_COPILOT_SCOPE", "read:user"),
    ]
}

fn insert_sqlite_user(
    foundation: &Arc<SqliteFoundation>,
    email: &str,
    password: &str,
    scopes: &[&str],
) -> i64 {
    let connection = foundation.open_connection(true).unwrap();
    let hashed = hash_password(password).unwrap();
    let scopes_json = serde_json::to_string(scopes).unwrap();
    connection
        .execute(
            "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at)
             VALUES (?1, 'activated', 'en', ?2, 'Test', 'User', '', 0, ?3, 0)",
            rusqlite::params![email, hashed, scopes_json],
        )
        .unwrap();
    connection.last_insert_rowid()
}

fn insert_sqlite_project_membership(
    foundation: &Arc<SqliteFoundation>,
    user_id: i64,
    project_id: i64,
    is_owner: bool,
    scopes: &[&str],
) {
    let connection = foundation.open_connection(true).unwrap();
    let scopes_json = serde_json::to_string(scopes).unwrap();
    connection
        .execute(
            "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![user_id, project_id, is_owner, scopes_json],
        )
        .unwrap();
}

fn insert_sqlite_role(
    foundation: &Arc<SqliteFoundation>,
    name: &str,
    level: &str,
    project_id: i64,
    scopes: &[&str],
) -> i64 {
    let connection = foundation.open_connection(true).unwrap();
    let scopes_json = serde_json::to_string(scopes).unwrap();
    connection
        .execute(
            "INSERT INTO roles (name, level, project_id, scopes, deleted_at) VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![name, level, project_id, scopes_json],
        )
        .unwrap();
    connection.last_insert_rowid()
}

fn attach_sqlite_role(foundation: &Arc<SqliteFoundation>, user_id: i64, role_id: i64) {
    let connection = foundation.open_connection(true).unwrap();
    connection
        .execute(
            "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
            rusqlite::params![user_id, role_id],
        )
        .unwrap();
}

fn seed_sqlite_request_content(
    foundation: &Arc<SqliteFoundation>,
    project_id: i64,
) -> (i64, std::path::PathBuf) {
    let content_dir = temp_postgres_dir("sqlite-admin-request-content-files");
    std::fs::create_dir_all(&content_dir).unwrap();

    let connection = foundation.open_connection(true).unwrap();
    let storage_settings = serde_json::json!({
        "directory": content_dir.to_string_lossy(),
    })
    .to_string();
    connection
        .execute(
            "INSERT INTO data_storages (name, description, \"primary\", type, settings, status, deleted_at)
             VALUES (?1, ?2, 0, 'fs', ?3, 'active', 0)",
            rusqlite::params!["SQLite Request Content FS", "sqlite-admin-read-test", storage_settings],
        )
        .unwrap();
    let storage_id = connection.last_insert_rowid();

    connection
        .execute(
            "INSERT INTO requests (
                api_key_id, project_id, trace_id, data_storage_id, source, model_id, format,
                request_headers, request_body, response_body, response_chunks, channel_id,
                external_id, status, stream, client_ip, metrics_latency_ms,
                metrics_first_token_latency_ms, content_saved, content_storage_id,
                content_storage_key, content_saved_at
            ) VALUES (
                NULL, ?1, NULL, ?2, 'api', 'gpt-4o', 'openai/chat_completions',
                '{}', '{}', NULL, NULL, NULL,
                NULL, 'completed', 0, '', NULL,
                NULL, 1, ?2,
                '', '2026-03-23T00:00:00Z'
            )",
            rusqlite::params![project_id, storage_id],
        )
        .unwrap();
    let request_id = connection.last_insert_rowid();

    let content_key = format!("/{project_id}/requests/{request_id}/chat/output.json");
    connection
        .execute(
            "UPDATE requests SET content_storage_key = ?2 WHERE id = ?1",
            rusqlite::params![request_id, content_key],
        )
        .unwrap();

    let full_path = content_dir.join(format!(
        "{project_id}/requests/{request_id}/chat/output.json"
    ));
    std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
    std::fs::write(&full_path, br#"{"content":"sqlite-request-content"}"#).unwrap();

    (request_id, content_dir)
}

fn mock_openai_v1_runtime_server_url() -> &'static str {
    static SERVER_URL: OnceLock<String> = OnceLock::new();
    SERVER_URL
        .get_or_init(|| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                let mut request_counts = HashMap::new();
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
                    let method = request_line.split_whitespace().next().unwrap_or("GET");
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/");
                    let request_key = format!("{method} {path}");
                    let request_count = request_counts.entry(request_key).or_insert(0);
                    *request_count += 1;
                    let request_count = *request_count;
                    let raw_body = request.split("\r\n\r\n").nth(1).unwrap_or("");
                    let request_json = serde_json::from_str::<serde_json::Value>(raw_body)
                        .unwrap_or(serde_json::Value::Null);
                    let request_model = request_json
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("gpt-4o");

                    let body = if path.contains("/retry-twice-ok-pg/")
                        && path.ends_with("/chat/completions")
                    {
                        if request_count <= 2 {
                            serde_json::json!({
                                "error": {"message": "retry me later"}
                            })
                            .to_string()
                        } else {
                            serde_json::json!({
                                "id": "chatcmpl_retry_pg",
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
                        }
                    } else if path.ends_with("/chat/completions") {
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
                    } else if path.ends_with("/responses/compact") {
                        serde_json::json!({
                            "id": "resp_compact_mock",
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
                    } else if path.ends_with("/images/generations") {
                        serde_json::json!({
                            "created": 1,
                            "data": [{
                                "b64_json": "aGVsbG8=",
                                "revised_prompt": "draw a cat"
                            }],
                            "usage": {
                                "prompt_tokens": 20,
                                "completion_tokens": 30,
                                "total_tokens": 50,
                                "prompt_tokens_details": {"cached_tokens": 4},
                                "completion_tokens_details": {"reasoning_tokens": 2}
                            }
                        })
                        .to_string()
                    } else if path.ends_with("/videos/video_mock_task") {
                        if method == "GET" {
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

                    let status_line = if path.contains("/retry-twice-ok-pg/")
                        && path.ends_with("/chat/completions")
                        && request_count <= 2
                    {
                        "HTTP/1.1 503 Service Unavailable"
                    } else {
                        "HTTP/1.1 200 OK"
                    };

                    let response = format!(
                        "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status_line,
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
    assert_eq!(
        primary_storage_name.as_deref(),
        Some(PRIMARY_DATA_STORAGE_NAME)
    );

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
    let (mut embedded_pg, dsn, data_dir) = runtime.block_on(start_embedded_postgres(
        "postgres-request-context-capability",
    ));

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

    let thread = request_context
        .resolve_thread(project.id, "thread-postgres-1")
        .unwrap()
        .unwrap();
    let same_thread = request_context
        .resolve_thread(project.id, "thread-postgres-1")
        .unwrap()
        .unwrap();
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
    assert!(matches!(
        request_context.resolve_trace(project.id, "trace-postgres-missing-thread", Some(thread.id + 10_000)),
        Err(ContextResolveError::Internal)
    ));

    let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
    let thread_count: i64 = connection
        .query_one(
            "SELECT COUNT(*) FROM threads WHERE thread_id = $1",
            &[&"thread-postgres-1"],
        )
        .unwrap()
        .get(0);
    let trace_count: i64 = connection
        .query_one(
            "SELECT COUNT(*) FROM traces WHERE trace_id = $1",
            &[&"trace-postgres-1"],
        )
        .unwrap()
        .get(0);
    assert_eq!(thread_count, 1);
    assert_eq!(trace_count, 1);

    runtime.block_on(embedded_pg.stop_db()).unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[test]
fn sqlite_bootstrap_seeds_default_onboarding_baseline() {
    let db_path = temp_sqlite_path("sqlite-bootstrap-onboarding");
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

    let connection = foundation.open_connection(false).unwrap();
    let onboarding_raw: String = connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
            [SYSTEM_KEY_ONBOARDED],
            |row| row.get(0),
        )
        .unwrap();
    let onboarding = parse_onboarding_record(&onboarding_raw).unwrap();
    assert_eq!(onboarding, Default::default());

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn postgres_router_signin_and_debug_context_routes_work_for_auth_and_context() {
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-auth-context-router").await;

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
            message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
        },
        allow_no_auth: false,
        cors: disabled_test_cors(),
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
    let signin_body = read_body(signin_response).await;
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
    let admin_debug_body = read_body(admin_debug_response).await;
    let admin_debug_json = serde_json::from_slice::<serde_json::Value>(&admin_debug_body).unwrap();
    assert_eq!(admin_debug_json["auth"]["mode"], "jwt");
    assert_eq!(admin_debug_json["auth"]["user_id"], 1);
    assert_eq!(admin_debug_json["project"]["id"], 1);
    assert_eq!(
        admin_debug_json["thread"]["threadId"],
        "thread-router-postgres"
    );
    assert_eq!(
        admin_debug_json["trace"]["traceId"],
        "trace-router-postgres"
    );
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
    let openapi_debug_body = read_body(openapi_debug_response).await;
    let openapi_debug_json =
        serde_json::from_slice::<serde_json::Value>(&openapi_debug_body).unwrap();
    assert_eq!(openapi_debug_json["auth"]["mode"], "api_key");
    assert_eq!(openapi_debug_json["auth"]["api_key_id"], 2);
    assert_eq!(
        openapi_debug_json["auth"]["api_key_type"],
        "service_account"
    );
    assert_eq!(openapi_debug_json["project"]["id"], 1);
    assert_eq!(
        openapi_debug_json["thread"]["threadId"],
        "thread-service-postgres"
    );
    assert_eq!(
        openapi_debug_json["trace"]["traceId"],
        "trace-service-postgres"
    );
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
            .query_one(
                "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
                &[],
            )
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

    let full_path = content_dir.join(format!(
        "{project_id}/requests/{request_id}/video/video.mp4"
    ));
    std::fs::create_dir_all(full_path.parent().unwrap()).unwrap();
    std::fs::write(&full_path, b"postgres-video-content").unwrap();

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let response = runtime
        .block_on(
            router(state).oneshot(
                Request::builder()
                    .uri(format!("/admin/requests/{request_id}/content"))
                    .method(Method::GET.as_str())
                    .header("Authorization", format!("Bearer {token}"))
                    .header(
                        "X-Project-ID",
                        format!("gid://axonhub/project/{project_id}"),
                    )
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
        .block_on(actix_web::body::to_bytes(response.into_body()))
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
                    "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at)
                     VALUES ($1, $2, $3, 'enabled', $4, $5, FALSE, '', $6, $7, $8, '', '', 0)",
                    &[
                        &"openai",
                        &"https://models.example.test/v1",
                        &"Task12 QueryModels Channel",
                        &r#"{"apiKey":"test-upstream-key"}"#,
                        &r#"["gpt-4"]"#,
                        &r#"{"queryAllChannelModels":true}"#,
                        &"[]",
                        &100_i32,
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO systems (key, value, deleted_at) VALUES ($1, $2, 0)",
                    &[
                        &"system_channel_settings",
                        &r#"{"probe":{"enabled":true,"frequency":"FiveMinutes"},"query_all_channel_models":true}"#,
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO models (developer, model_id, type, name, icon, \"group\", remark, model_card, settings, status)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                    &[
                        &"openai",
                        &"gpt-4",
                        &"chat",
                        &"GPT-4",
                        &"icon",
                        &"openai",
                        &"Test model",
                        &"{}",
                        &"{}",
                        &"enabled",
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO models (developer, model_id, type, name, icon, \"group\", remark, model_card, settings, status)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                    &[
                        &"anthropic",
                        &"claude-3",
                        &"chat",
                        &"Claude 3",
                        &"icon",
                        &"anthropic",
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

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let body = read_body(response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    let models = json["data"]["queryModels"]
        .as_array()
        .expect("expected queryModels array");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["id"], "gid://axonhub/model/1");
    assert_eq!(models[0]["status"], "enabled");

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn postgres_openai_v1_subset_routes_execute_and_support_image_generations_truthfully() {
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-openai-v1-subset").await;

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
                (
                    "OpenAI Image Mock",
                    "openai",
                    base_url.clone(),
                    r#"["gpt-image-1"]"#,
                    80_i64,
                ),
            ];
            for (name, channel_type, base_url, supported_models, ordering_weight) in channel_rows {
                let ordering_weight_i32 = ordering_weight as i32;
                let params: [&(dyn postgres::types::ToSql + Sync); 8] = [
                    &channel_type,
                    &base_url,
                    &name,
                    &r#"{"apiKey":"test-upstream-key"}"#,
                    &supported_models,
                    &"{}",
                    &"[]",
                    &ordering_weight_i32,
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
                (
                    "openai",
                    "gpt-image-1",
                    "image",
                    "GPT Image 1",
                    "OpenAI",
                    "openai",
                    r#"{"limit":{"context":8192,"output":0},"cost":{"input":1.0,"output":2.0}}"#,
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

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
        &read_body(models_response).await,
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
            "/v1/responses/compact",
            r#"{"model":"gpt-4o","input":"hi"}"#,
            vec!["id"],
            serde_json::Value::String("resp_compact_mock".to_owned()),
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
        if response.status() != StatusCode::OK {
            let status = response.status();
            let body = String::from_utf8(read_body(response).await).unwrap_or_default();
            panic!("path {path} returned {status}: {body}");
        }
        let json = serde_json::from_slice::<serde_json::Value>(
            &read_body(response).await,
        )
        .unwrap();
        let actual = expected_json_path
            .iter()
            .fold(&json, |current, key| &current[*key]);
        assert_eq!(actual, &expected_value, "path {path}");
    }

    let image_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/images/generations")
                .method(Method::POST)
                .header("content-type", "application/json")
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .header("X-Project-ID", "gid://axonhub/project/1")
                .header("AH-Thread-Id", "thread-postgres-v1")
                .header("AH-Trace-Id", "trace-postgres-v1")
                .body(Body::from(r#"{"model":"gpt-image-1","prompt":"draw a cat"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(image_response.status(), StatusCode::OK);
    let image_json = serde_json::from_slice::<serde_json::Value>(&read_body(image_response).await).unwrap();
    assert_eq!(image_json["data"][0]["b64_json"], "aGVsbG8=");

    let unported = app
        .oneshot(
            Request::builder()
                .uri("/v1/images/edits")
                .method(Method::POST)
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unported.status(), StatusCode::BAD_REQUEST);

    let (request_statuses, request_formats, execution_statuses, usage_formats, trace_thread_count) = std::thread::spawn({
        let dsn = dsn.clone();
        move || {
            let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
            let request_statuses = connection
                .query("SELECT status FROM requests ORDER BY id ASC", &[])
                .unwrap()
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
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
            let usage_formats = connection
                .query("SELECT format FROM usage_logs ORDER BY id ASC", &[])
                .unwrap()
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>();
            let trace_thread_count: i64 = connection
                .query_one(
                    "SELECT COUNT(*) FROM traces t JOIN threads th ON th.id = t.thread_id WHERE t.trace_id = $1 AND th.thread_id = $2",
                    &[&"trace-postgres-v1", &"thread-postgres-v1"],
                )
                .unwrap()
                .get(0);
            (request_statuses, request_formats, execution_statuses, usage_formats, trace_thread_count)
        }
    })
    .join()
    .expect("postgres /v1 verification thread");

    assert_eq!(
        request_statuses,
        vec![
            "completed",
            "completed",
            "completed",
            "completed",
            "completed",
            "completed",
            "completed"
        ]
    );
    assert_eq!(
        request_formats,
        vec![
            "openai/chat_completions",
            "openai/responses",
            "openai/responses_compact",
            "openai/embeddings",
            "anthropic/message",
            "jina/rerank",
            "openai/images_generations"
        ]
    );
    assert_eq!(
        execution_statuses,
        vec![
            "completed",
            "completed",
            "completed",
            "completed",
            "completed",
            "completed",
            "completed"
        ]
    );
    assert_eq!(usage_formats.len(), 7);
    assert_eq!(usage_formats[6], "openai/images_generations");
    assert_eq!(trace_thread_count, 1);

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn postgres_openai_v1_retries_same_channel_before_failover_and_records_attempts_once() {
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-openai-v1-same-channel-retry").await;

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
                    "OpenAI Retry Primary",
                    format!("{base_url}/retry-twice-ok-pg"),
                    100_i32,
                ),
                ("OpenAI Retry Backup", format!("{base_url}/backup"), 90_i32),
            ];
            for (name, channel_base_url, ordering_weight) in channel_rows {
                let params: [&(dyn postgres::types::ToSql + Sync); 8] = [
                    &"openai",
                    &channel_base_url,
                    &name,
                    &r#"{"apiKey":"test-upstream-key"}"#,
                    &r#"["gpt-4o"]"#,
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

            let model_params: [&(dyn postgres::types::ToSql + Sync); 7] = [
                &"openai",
                &"gpt-4o",
                &"chat",
                &"GPT-4o",
                &"OpenAI",
                &"openai",
                &r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0}}"#,
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
    .expect("postgres /v1 same-channel retry seed thread");

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/chat/completions")
                .method(Method::POST)
                .header("content-type", "application/json")
                .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                .header("X-Project-ID", "gid://axonhub/project/1")
                .header("AH-Thread-Id", "thread-postgres-retry")
                .header("AH-Trace-Id", "trace-postgres-retry")
                .body(Body::from(
                    r#"{"model":"gpt-4o","messages":[{"role":"user","content":"retry then succeed"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = serde_json::from_slice::<serde_json::Value>(&read_body(response).await).unwrap();
    assert_eq!(json["id"], "chatcmpl_retry_pg");

    let (request_count, request_model, request_channel_id, execution_rows, usage_count, trace_thread_count) =
        std::thread::spawn({
            let dsn = dsn.clone();
            move || {
                let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
                let request_count: i64 = connection
                    .query_one("SELECT COUNT(*) FROM requests", &[])
                    .unwrap()
                    .get(0);
                let request_row = connection
                    .query_one(
                        "SELECT model_id, channel_id FROM requests ORDER BY id DESC LIMIT 1",
                        &[],
                    )
                    .unwrap();
                let request_model: String = request_row.get(0);
                let request_channel_id: i64 = request_row.get(1);
                let execution_rows = connection
                    .query(
                        "SELECT channel_id, status, response_status_code FROM request_executions ORDER BY id ASC",
                        &[],
                    )
                    .unwrap()
                    .into_iter()
                    .map(|row| {
                        (
                            row.get::<_, i64>(0),
                            row.get::<_, String>(1),
                            row.get::<_, Option<i64>>(2),
                        )
                    })
                    .collect::<Vec<_>>();
                let usage_count: i64 = connection
                    .query_one("SELECT COUNT(*) FROM usage_logs", &[])
                    .unwrap()
                    .get(0);
                let trace_thread_count: i64 = connection
                    .query_one(
                        "SELECT COUNT(*) FROM traces t JOIN threads th ON th.id = t.thread_id WHERE t.trace_id = $1 AND th.thread_id = $2",
                        &[&"trace-postgres-retry", &"thread-postgres-retry"],
                    )
                    .unwrap()
                    .get(0);
                (
                    request_count,
                    request_model,
                    request_channel_id,
                    execution_rows,
                    usage_count,
                    trace_thread_count,
                )
            }
        })
        .join()
        .expect("postgres /v1 same-channel retry verification thread");

    assert_eq!(request_count, 1);
    assert_eq!(request_model, "gpt-4o");
    assert_eq!(trace_thread_count, 1);
    assert_eq!(usage_count, 1);
    assert_eq!(execution_rows.len(), 3);
    assert!(execution_rows
        .iter()
        .all(|(channel_id, _, _)| *channel_id == request_channel_id));
    assert_eq!(
        execution_rows
            .iter()
            .map(|(_, status, _)| status.clone())
            .collect::<Vec<_>>(),
        vec!["failed", "failed", "completed"]
    );
    assert_eq!(execution_rows[0].2, Some(503));
    assert_eq!(execution_rows[1].2, Some(503));
    assert_eq!(execution_rows[2].2, Some(200));

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn postgres_openai_v1_video_routes_execute_and_keep_unported_images_truthful() {
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-openai-v1-videos").await;

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
            let ordering_weight_i32 = 100_i32;
            let channel_params: [&(dyn postgres::types::ToSql + Sync); 8] = [
                &"openai",
                &base_url,
                &"Doubao Video Alias Mock",
                &r#"{"apiKey":"test-upstream-key"}"#,
                &r#"["seedance-1.0"]"#,
                &"{}",
                &"[]",
                &ordering_weight_i32,
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

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
        &read_body(create_response).await,
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
        &read_body(get_response).await,
    )
    .unwrap();
    assert_eq!(get_json["id"], "video_mock_task");
    assert_eq!(
        get_json["content"]["video_url"],
        "https://example.com/generated.mp4"
    );

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
    let delete_body = read_body(delete_response).await;
    assert!(delete_body.is_empty());

    let unported_images = app
        .oneshot(
                Request::builder()
                    .uri("/v1/images/edits")
                    .method(Method::POST)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unported_images.status(), StatusCode::BAD_REQUEST);

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

    assert_eq!(
        request_formats,
        vec![
            "doubao/video_create",
            "doubao/video_get",
            "doubao/video_delete"
        ]
    );
    assert_eq!(
        execution_statuses,
        vec!["completed", "completed", "completed"]
    );
    assert_eq!(trace_thread_count, 1);

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[tokio::test]
async fn main_router_serves_fresh_status_and_initialize_for_sqlite_scope() {
    let db_path = temp_sqlite_path("main-router-live-scope");
    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let status_body = read_body(status_response).await;
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
fn clap_help_and_parsing_preserve_current_operator_facing_cli_contract() {
    let top_level_help = render_help(axonhub_cli_command());
    assert!(top_level_help.contains("AxonHub AI Gateway"));
    assert!(top_level_help.contains("config"));
    assert!(top_level_help.contains("version"));
    assert!(top_level_help.contains("help"));
    assert!(top_level_help.contains("build-info"));

    let config_help = render_help(axonhub_config_cli_command());
    assert!(config_help.contains("preview"));
    assert!(config_help.contains("validate"));
    assert!(config_help.contains("get"));

    let config_get_help = help_text_for_args(["axonhub", "config", "get", "--help"]);
    assert!(config_get_help.contains("server.port"));
    assert!(config_get_help.contains("Server port number"));
    assert!(config_get_help.contains("server.name"));
    assert!(config_get_help.contains("server.base_path"));
    assert!(config_get_help.contains("server.debug"));
    assert!(config_get_help.contains("db.dialect"));
    assert!(config_get_help.contains("db.dsn"));
    assert!(!config_get_help.contains("server.api.auth.allow_no_auth"));
    assert!(!config_get_help.contains("metrics.exporter.type"));
    assert!(!config_get_help.contains("cache.memory.expiration"));

    let config_help_via_subcommand = help_text_for_args(["axonhub", "config", "help"]);
    assert!(config_help_via_subcommand.contains("preview"));
    assert!(config_help_via_subcommand.contains("validate"));
    assert!(config_help_via_subcommand.contains("get"));

    assert!(parse_for_test(["axonhub", "config", "preview"]).is_ok());
    assert!(parse_for_test(["axonhub", "config", "preview", "--format", "json"]).is_ok());
    assert!(parse_for_test(["axonhub", "config", "preview", "--format", "yml"]).is_ok());
    assert!(parse_for_test(["axonhub", "config", "preview", "--format", "yaml"]).is_ok());
    assert!(parse_for_test(["axonhub", "config", "validate"]).is_ok());
    assert!(parse_for_test(["axonhub", "config", "get", "server.port"]).is_ok());
    assert!(parse_for_test(["axonhub", "build-info"]).is_ok());
    let config_without_subcommand =
        parse_for_test(["axonhub", "config"]).expect_err("config without subcommand should fail");
    let config_error_output = config_without_subcommand.to_string();
    assert!(config_error_output.contains("preview"));
    assert!(config_error_output.contains("validate"));
    assert!(config_error_output.contains("get"));
    assert!(matches!(
        parse_for_test(["axonhub", "--help"]),
        Err(error) if error.kind() == ErrorKind::DisplayHelp
    ));
    assert!(matches!(
        parse_for_test(["axonhub", "-h"]),
        Err(error) if error.kind() == ErrorKind::DisplayHelp
    ));
    assert!(matches!(
        parse_for_test(["axonhub", "help"]),
        Err(error) if error.kind() == ErrorKind::DisplayHelp
    ));
    assert!(matches!(
        parse_for_test(["axonhub", "--version"]),
        Err(error) if error.kind() == ErrorKind::DisplayVersion
    ));
    assert!(matches!(
        parse_for_test(["axonhub", "-v"]),
        Err(error) if error.kind() == ErrorKind::DisplayVersion
    ));
    assert!(parse_for_test(["axonhub", "serve"]).is_ok());
}

#[test]
fn clap_contract_debug_asserts_cleanly() {
    AxonhubCliContract::command().debug_assert();
    axonhub_config_cli_command().debug_assert();
}

fn render_help(mut command: clap::Command) -> String {
    let mut rendered = Vec::new();
    command.write_long_help(&mut rendered).expect("write help");
    String::from_utf8(rendered).expect("utf8 help")
}

fn help_text_for_args<I, T>(args: I) -> String
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let error = parse_for_test(args).expect_err("expected clap help display");
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    error.to_string()
}

fn parse_for_test<I, T>(args: I) -> Result<AxonhubCliContract, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let args = args
        .into_iter()
        .map(|arg| arg.into().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    parse_axonhub_cli(&args)
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
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-bootstrap-router").await;
    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let status_body = read_body(status_response).await;
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
    let initialize_body = read_body(initialize_response).await;
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
    let second_status_body = read_body(second_status_response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&second_status_body).unwrap();
    assert_eq!(json["isInitialized"], true);

    embedded_pg.stop_db().await.unwrap();
    std::fs::remove_dir_all(data_dir).ok();
}

#[test]
fn mysql_capabilities_are_rejected_by_the_rust_runtime_contract() {
    let mysql_dsn = "mysql://axonhub:axonhub_password@127.0.0.1:3306/axonhub";

    match build_system_bootstrap_capability("mysql", mysql_dsn, "v0.9.20") {
        SystemBootstrapCapability::Available { .. } => {
            panic!("expected mysql bootstrap capability to be unsupported")
        }
        SystemBootstrapCapability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }

    match build_identity_capability("mysql", mysql_dsn, false) {
        IdentityCapability::Available { .. } => {
            panic!("expected mysql identity capability to be unsupported")
        }
        IdentityCapability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }

    match build_request_context_capability("mysql", mysql_dsn, false) {
        RequestContextCapability::Available { .. } => {
            panic!("expected mysql request-context capability to be unsupported")
        }
        RequestContextCapability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }

    match build_openai_v1_capability("mysql", mysql_dsn) {
        axonhub_http::OpenAiV1Capability::Available { .. } => {
            panic!("expected mysql OpenAI v1 capability to be unsupported")
        }
        axonhub_http::OpenAiV1Capability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }

    match build_admin_capability("mysql", mysql_dsn) {
        axonhub_http::AdminCapability::Available { .. } => {
            panic!("expected mysql admin capability to be unsupported")
        }
        axonhub_http::AdminCapability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }

    match build_admin_graphql_capability("mysql", mysql_dsn) {
        axonhub_http::AdminGraphqlCapability::Available { .. } => {
            panic!("expected mysql admin graphql capability to be unsupported")
        }
        axonhub_http::AdminGraphqlCapability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }

    match build_openapi_graphql_capability("mysql", mysql_dsn) {
        axonhub_http::OpenApiGraphqlCapability::Available { .. } => {
            panic!("expected mysql openapi graphql capability to be unsupported")
        }
        axonhub_http::OpenApiGraphqlCapability::Unsupported { message } => {
            assert!(message.contains("sqlite3"));
            assert!(message.contains("postgres"));
        }
    }
}

#[test]
fn unsupported_dialect_messages_keep_mysql_out_of_public_rust_target_hint() {
    let message = match build_system_bootstrap_capability("oracle", ":memory:", "v0.9.20") {
        SystemBootstrapCapability::Available { .. } => {
            panic!("expected unsupported bootstrap capability for oracle dialect")
        }
        SystemBootstrapCapability::Unsupported { message } => message,
    };

    assert!(message.contains("sqlite3"));
    assert!(message.contains("postgres"));
    assert!(!message.contains("mysql"));
}

#[tokio::test]
async fn unsupported_dialect_keeps_provider_edge_admin_routes_truthful() {
    let db_path = temp_sqlite_path("unsupported-dialect-provider-edge");
    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let start_body = read_body(start_response).await;

    assert_eq!(start_status, StatusCode::NOT_IMPLEMENTED);
    let json = serde_json::from_slice::<serde_json::Value>(&start_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/codex/oauth/start");
    assert_eq!(json["path"], "/admin/codex/oauth/start");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes."
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
    let exchange_body = read_body(exchange_response).await;

    assert_eq!(exchange_status, StatusCode::NOT_IMPLEMENTED);
    let json = serde_json::from_slice::<serde_json::Value>(&exchange_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/codex/oauth/exchange");
    assert_eq!(json["path"], "/admin/codex/oauth/exchange");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes."
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
    let exchange_body = read_body(exchange_response).await;

    assert_eq!(exchange_status, StatusCode::NOT_IMPLEMENTED);
    let json = serde_json::from_slice::<serde_json::Value>(&exchange_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/codex/oauth/exchange");
    assert_eq!(json["path"], "/admin/codex/oauth/exchange");
    assert_eq!(json["method"], "POST");
    assert_eq!(
        json["message"],
        "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes."
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
    let state = HttpState { service_name: "AxonHub".to_owned(),
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
    provider_edge_admin: build_provider_edge_admin_capability("postgres", ":memory:"), allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let body = read_body(response).await;
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
    let state = HttpState { service_name: "AxonHub".to_owned(),
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
    provider_edge_admin: build_provider_edge_admin_capability("postgres", ":memory:"), allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let body = read_body(response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();
    assert!(json["session_id"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    let auth_url = json["auth_url"].as_str().expect("expected auth_url");
    assert!(auth_url.starts_with("https://example.test/codex/authorize?"));
    assert!(auth_url.contains("client_id=codex-client-id"));

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn unsupported_non_seaorm_dialect_keeps_graphql_routes_truthful() {
    let db_path = temp_sqlite_path("unsupported-dialect-graphql");

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "oracle",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: IdentityCapability::Available {
        identity: Arc::new(FakeIdentityService::new()),
    },
    request_context: RequestContextCapability::Available {
        request_context: Arc::new(FakeIdentityService::new()),
    },
    openai_v1: build_openai_v1_capability("oracle", &db_path.display().to_string()),
    admin: build_admin_capability("oracle", &db_path.display().to_string()),
    admin_graphql: build_admin_graphql_capability("oracle", &db_path.display().to_string()),
    openapi_graphql: build_openapi_graphql_capability("oracle", &db_path.display().to_string()),
    provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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

    assert_eq!(admin_graphql_response.status(), StatusCode::NOT_IMPLEMENTED);
    let admin_graphql_body = read_body(admin_graphql_response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&admin_graphql_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/admin/graphql");

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

    assert_eq!(
        openapi_graphql_response.status(),
        StatusCode::NOT_IMPLEMENTED
    );
    let openapi_graphql_body =
        read_body(openapi_graphql_response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&openapi_graphql_body).unwrap();
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], "/openapi/v1/graphql");

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

    let connection = foundation.open_connection(true).unwrap();
    connection
        .execute(
            "UPDATE api_keys SET scopes = ?1 WHERE key = ?2 AND deleted_at = 0",
            rusqlite::params![
                serde_json::to_string(&["write_api_keys"]).unwrap(),
                DEFAULT_SERVICE_API_KEY_VALUE
            ],
        )
        .unwrap();

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let body = read_body(response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();

    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    let data = json.get("data").expect("expected data field");
    let api_key = data
        .get("createLLMAPIKey")
        .expect("expected createLLMAPIKey field");
    assert_eq!(
        api_key.get("name").and_then(|v| v.as_str()),
        Some("SDK Key")
    );
    let scopes = api_key.get("scopes").and_then(|v| v.as_array()).unwrap();
    let scope_strs: Vec<&str> = scopes.iter().filter_map(|v| v.as_str()).collect();
    assert!(scope_strs.contains(&"read_channels"));
    assert!(scope_strs.contains(&"write_requests"));

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn postgres_openapi_graphql_route_executes_pilot_mutation() {
    let (mut embedded_pg, dsn, data_dir) =
        start_embedded_postgres("postgres-openapi-graphql-pilot").await;

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
            let scopes_json = serde_json::to_string(&["write_api_keys"]).unwrap();
            connection
                .execute(
                    "UPDATE api_keys SET scopes = $1 WHERE key = $2 AND deleted_at = 0",
                    &[&scopes_json, &DEFAULT_SERVICE_API_KEY_VALUE],
                )
                .unwrap();
        }
    })
    .join()
    .expect("postgres openapi service key scope update thread");

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let body = read_body(response).await;
    let json = serde_json::from_slice::<serde_json::Value>(&body).unwrap();

    assert!(json.get("errors").map(|v| v.is_null()).unwrap_or(true));
    let api_key = json["data"]["createLLMAPIKey"].clone();
    assert_eq!(api_key["name"], "SDK Key");
    assert!(api_key["key"]
        .as_str()
        .is_some_and(|value| value.starts_with("ah-")));
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

    let state = HttpState { service_name: "AxonHub".to_owned(),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

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
    let missing_body = read_body(missing_response).await;
    let missing_json = serde_json::from_slice::<serde_json::Value>(&missing_body).unwrap();
    assert_eq!(missing_json["error"]["type"], "Unauthorized");
    assert_eq!(missing_json["error"]["message"], "Authorization header is required");

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
    let invalid_body = read_body(invalid_response).await;
    let invalid_json = serde_json::from_slice::<serde_json::Value>(&invalid_body).unwrap();
    assert_eq!(invalid_json["error"]["type"], "Unauthorized");
    assert_eq!(invalid_json["error"]["message"], "Invalid API key");

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn sqlite_admin_graphql_route_keeps_supported_subset_and_explicit_boundaries() {
    let db_path = temp_sqlite_path("sqlite-admin-graphql-rbac");
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

    let system_user_id = insert_sqlite_user(
        &foundation,
        "system-reader@example.com",
        "password123",
        &["read_channels"],
    );
    let no_scope_user_id = insert_sqlite_user(
        &foundation,
        "no-scope@example.com",
        "password123",
        &[],
    );
    let owner_user_id = insert_sqlite_user(
        &foundation,
        "owner-ops@example.com",
        "password123",
        &[],
    );

    let connection = foundation.open_connection(true).unwrap();
    connection
        .execute("UPDATE users SET is_owner = 1 WHERE id = ?1", rusqlite::params![owner_user_id])
        .unwrap();

    insert_sqlite_project_membership(&foundation, system_user_id, 1, false, &[]);
    insert_sqlite_project_membership(&foundation, no_scope_user_id, 1, false, &[]);
    insert_sqlite_project_membership(&foundation, owner_user_id, 1, false, &[]);

    connection
        .execute(
            "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{}', 'enabled', '', 0)",
            rusqlite::params![
                "openai",
                "gpt-4o",
                "chat",
                "GPT-4o",
                "icon",
                "openai",
                "{}"
            ],
        )
        .unwrap();

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "sqlite3",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: build_identity_capability("sqlite3", &db_path.display().to_string(), false),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let app = router(state);

    let system_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "system-reader@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let no_scope_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "no-scope@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let owner_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "owner-ops@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };

    let allowed_system = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {system_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"query Models($input: QueryModelsInput!) { queryModels(input: $input) { id status } }","variables":{"input":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed_system.status(), StatusCode::OK);
    let allowed_system_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(allowed_system).await).unwrap();
    assert!(allowed_system_json["errors"].is_null() || allowed_system_json.get("errors").is_none());
    let allowed_models = allowed_system_json["data"]["queryModels"]
        .as_array()
        .expect("expected queryModels array");
    assert!(allowed_models.iter().all(|row| {
        row.get("id").and_then(|id| id.as_str()).is_some()
            && row.get("status").and_then(|status| status.as_str()).is_some()
    }));

    let denied_no_scope = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {no_scope_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"query Models($input: QueryModelsInput!) { queryModels(input: $input) { id status } }","variables":{"input":{}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_no_scope.status(), StatusCode::OK);
    let denied_no_scope_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(denied_no_scope).await).unwrap();
    assert!(denied_no_scope_json["data"]["queryModels"].is_null());
    assert_eq!(denied_no_scope_json["errors"][0]["message"], "permission denied");

    let unsupported_owner = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"{ allScopes { scope description levels } }"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsupported_owner.status(), StatusCode::NOT_IMPLEMENTED);
    let unsupported_owner_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(unsupported_owner).await).unwrap();
    assert_eq!(unsupported_owner_json["error"], "not_implemented");
    assert_eq!(unsupported_owner_json["route_family"], "/admin/graphql");
    assert!(unsupported_owner_json["message"]
        .as_str()
        .is_some_and(|message| message.contains("allScopes")));

    let model_count: i64 = foundation
        .open_connection(true)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM models", [], |row| row.get(0))
        .unwrap();
    assert_eq!(model_count, 1);

    let invalid_token = app
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", "Bearer invalid-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"{ queryModels(input: {}) { id } }"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_token.status(), StatusCode::UNAUTHORIZED);
    let invalid_token_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_token).await).unwrap();
    assert_eq!(invalid_token_json["error"]["message"], "Invalid token");

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn sqlite_admin_graphql_route_supports_storage_management_writes_and_truthful_501_boundaries() {
    let db_path = temp_sqlite_path("sqlite-admin-storage-management-writes");
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

    insert_sqlite_user(
        &foundation,
        "settings-admin@example.com",
        "password123",
        &["read_settings", "write_settings"],
    );
    insert_sqlite_user(
        &foundation,
        "settings-viewer@example.com",
        "password123",
        &[],
    );
    let owner_id = insert_sqlite_user(
        &foundation,
        "backup-owner@example.com",
        "password123",
        &[],
    );
    let connection = foundation.open_connection(true).unwrap();
    connection
        .execute("UPDATE users SET is_owner = 1 WHERE id = ?1", [owner_id])
        .unwrap();

    let backup_root = std::env::temp_dir().join(format!(
        "axonhub-task10-storage-management-{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&backup_root).unwrap();
    let storage_id = connection
        .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM data_storages", [], |row| row.get::<_, i64>(0))
        .unwrap();
    connection
        .execute(
            "INSERT INTO data_storages (id, name, description, \"primary\", type, settings, status, deleted_at)
             VALUES (?1, ?2, ?3, 0, 'fs', ?4, 'active', 0)",
            rusqlite::params![
                storage_id,
                "Task10 Backup FS",
                "task10 backup",
                serde_json::json!({"directory": backup_root.to_string_lossy()}).to_string(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at)
             VALUES ('codex', 'https://example.test/v1', 'Task10 Codex Quota Channel', 'enabled', '{}', '[]', 0, '', '{}', '[]', 100, '', 'task10 quota', 0)",
            [],
        )
        .unwrap();
    drop(connection);

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "sqlite3",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: build_identity_capability("sqlite3", &db_path.display().to_string(), false),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let app = router(state);
    let settings_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "settings-admin@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let no_scope_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "settings-viewer@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let owner_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "backup-owner@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };

        let denied_default_storage_update = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST.as_str())
                    .header("Authorization", format!("Bearer {no_scope_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"query":"mutation UpdateDefaultDataStorage($input: UpdateDefaultDataStorageInput!) {{ updateDefaultDataStorage(input: $input) }}","variables":{{"input":{{"dataStorageID":"{}"}}}}}}"#,
                        graphql_gid("DataStorage", storage_id)
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
    let denied_default_storage_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(denied_default_storage_update).await)
            .unwrap();
    assert_eq!(
        denied_default_storage_json["data"]["updateDefaultDataStorage"],
        serde_json::Value::Null
    );
    assert_eq!(denied_default_storage_json["errors"][0]["message"], "permission denied");

    let invalid_default_storage_update = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateDefaultDataStorage($input: UpdateDefaultDataStorageInput!) { updateDefaultDataStorage(input: $input) }","variables":{"input":{"dataStorageID":"gid://axonhub/user/1"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let invalid_default_storage_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_default_storage_update).await)
            .unwrap();
    assert_eq!(
        invalid_default_storage_json["data"]["updateDefaultDataStorage"],
        serde_json::Value::Null
    );
    assert_eq!(invalid_default_storage_json["errors"][0]["message"], "invalid dataStorageID");

    let missing_default_storage_update = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateDefaultDataStorage($input: UpdateDefaultDataStorageInput!) { updateDefaultDataStorage(input: $input) }","variables":{"input":{"dataStorageID":"gid://axonhub/DataStorage/999999"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let missing_default_storage_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(missing_default_storage_update).await)
            .unwrap();
    assert_eq!(
        missing_default_storage_json["data"]["updateDefaultDataStorage"],
        serde_json::Value::Null
    );
    assert_eq!(
        missing_default_storage_json["errors"][0]["message"],
        "data storage not found"
    );

        let update_default_storage = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST.as_str())
                    .header("Authorization", format!("Bearer {settings_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"query":"mutation UpdateDefaultDataStorage($input: UpdateDefaultDataStorageInput!) {{ updateDefaultDataStorage(input: $input) }}","variables":{{"input":{{"dataStorageID":"{}"}}}}}}"#,
                        graphql_gid("DataStorage", storage_id)
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(update_default_storage.status(), StatusCode::OK);
    let update_default_storage_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_default_storage).await).unwrap();
    assert_eq!(update_default_storage_json["data"]["updateDefaultDataStorage"], true);

    let default_storage_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ defaultDataStorageID }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let default_storage_query_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(default_storage_query).await).unwrap();
    assert_eq!(
        default_storage_query_json["data"]["defaultDataStorageID"],
        graphql_gid("DataStorage", storage_id)
    );

    let update_storage_policy = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateStoragePolicy($input: UpdateStoragePolicyInput!) { updateStoragePolicy(input: $input) }","variables":{"input":{"storeChunks":true,"storeRequestBody":false,"cleanupOptions":[{"resourceType":"requests","enabled":true,"cleanupDays":7}]}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_storage_policy.status(), StatusCode::OK);
    let update_storage_policy_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_storage_policy).await).unwrap();
    assert_eq!(update_storage_policy_json["data"]["updateStoragePolicy"], true);

    let storage_policy_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ storagePolicy { storeChunks storeRequestBody storeResponseBody cleanupOptions { resourceType enabled cleanupDays } } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let storage_policy_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(storage_policy_query).await).unwrap();
    assert_eq!(storage_policy_json["data"]["storagePolicy"]["storeChunks"], true);
    assert_eq!(storage_policy_json["data"]["storagePolicy"]["storeRequestBody"], false);

    let denied_storage_update = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {no_scope_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateStoragePolicy($input: UpdateStoragePolicyInput!) { updateStoragePolicy(input: $input) }","variables":{"input":{"storeChunks":false}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let denied_storage_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(denied_storage_update).await).unwrap();
    assert_eq!(
        denied_storage_json["data"]["updateStoragePolicy"],
        serde_json::Value::Null
    );
    assert_eq!(denied_storage_json["errors"][0]["message"], "permission denied");

    let invalid_auto_backup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateAutoBackupSettings($input: UpdateAutoBackupSettingsInput!) { updateAutoBackupSettings(input: $input) }","variables":{"input":{"enabled":true,"dataStorageID":0}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let invalid_auto_backup_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_auto_backup).await).unwrap();
    assert_eq!(
        invalid_auto_backup_json["data"]["updateAutoBackupSettings"],
        serde_json::Value::Null
    );
    assert_eq!(
        invalid_auto_backup_json["errors"][0]["message"],
        "dataStorageID is required when auto backup is enabled"
    );

    let update_auto_backup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation UpdateAutoBackupSettings($input: UpdateAutoBackupSettingsInput!) {{ updateAutoBackupSettings(input: $input) }}","variables":{{"input":{{"enabled":true,"frequency":"daily","dataStorageID":{},"includeChannels":true,"includeModels":false,"includeAPIKeys":false,"includeModelPrices":false,"retentionDays":2}}}}}}"#,
                    storage_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let update_auto_backup_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_auto_backup).await).unwrap();
    assert_eq!(update_auto_backup_json["data"]["updateAutoBackupSettings"], true);

    let auto_backup_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ autoBackupSettings { enabled dataStorageID retentionDays } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let auto_backup_query_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(auto_backup_query).await).unwrap();
    assert_eq!(auto_backup_query_json["data"]["autoBackupSettings"]["enabled"], true);
    assert_eq!(auto_backup_query_json["data"]["autoBackupSettings"]["dataStorageID"], storage_id);

    let update_channel_settings = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateSystemChannelSettings($input: UpdateSystemChannelSettingsInput!) { updateSystemChannelSettings(input: $input) }","variables":{"input":{"queryAllChannelModels":false,"probe":{"enabled":false,"frequency":"ONE_HOUR"}}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let update_channel_settings_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_channel_settings).await).unwrap();
    assert_eq!(update_channel_settings_json["data"]["updateSystemChannelSettings"], true);

    let channel_settings_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ systemChannelSettings { queryAllChannelModels probe { enabled frequency } } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let channel_settings_query_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(channel_settings_query).await).unwrap();
    assert_eq!(channel_settings_query_json["data"]["systemChannelSettings"]["queryAllChannelModels"], false);
    assert_eq!(channel_settings_query_json["data"]["systemChannelSettings"]["probe"]["enabled"], false);
    assert_eq!(channel_settings_query_json["data"]["systemChannelSettings"]["probe"]["frequency"], "ONE_HOUR");

    let trigger_backup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"mutation { triggerAutoBackup { success message } }"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(trigger_backup.status(), StatusCode::OK);
    let trigger_backup_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(trigger_backup).await).unwrap();
    assert_eq!(trigger_backup_json["data"]["triggerAutoBackup"]["success"], true);
    assert_eq!(
        trigger_backup_json["data"]["triggerAutoBackup"]["message"],
        "Backup completed successfully"
    );

    let check_quotas = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"mutation { checkProviderQuotas }"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(check_quotas.status(), StatusCode::OK);
    let check_quotas_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(check_quotas).await).unwrap();
    assert_eq!(check_quotas_json["data"]["checkProviderQuotas"], true);

    let trigger_gc = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"mutation { triggerGcCleanup }"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(trigger_gc.status(), StatusCode::OK);
    let trigger_gc_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(trigger_gc).await).unwrap();
    assert_eq!(trigger_gc_json["data"]["triggerGcCleanup"], true);

    let backup_files = std::fs::read_dir(&backup_root)
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert!(!backup_files.is_empty());

    let status_connection = foundation.open_connection(true).unwrap();
    let quota_status_count: i64 = status_connection
        .query_row("SELECT COUNT(*) FROM provider_quota_statuses", [], |row| row.get(0))
        .unwrap();
    assert_eq!(quota_status_count, 1);
    let completed_operational_runs: i64 = status_connection
        .query_row(
            "SELECT COUNT(*) FROM operational_runs WHERE operation_type IN ('auto_backup', 'quota_check', 'gc_cleanup') AND status = 'completed' AND finished_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let failed_operational_runs: i64 = status_connection
        .query_row(
            "SELECT COUNT(*) FROM operational_runs WHERE operation_type IN ('auto_backup', 'quota_check', 'gc_cleanup') AND status = 'failed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(completed_operational_runs, 3);
    assert_eq!(failed_operational_runs, 0);
    drop(status_connection);

    let systems_connection = foundation.open_connection(true).unwrap();
    let storage_policy_value: String = systems_connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
            ["storage_policy"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(storage_policy_value.contains("\"store_chunks\":true"));
    let auto_backup_value: String = systems_connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
            ["system_auto_backup_settings"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(auto_backup_value.contains(&format!("\"data_storage_id\":{storage_id}")));
    let default_data_storage_value: String = systems_connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
            [SYSTEM_KEY_DEFAULT_DATA_STORAGE],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(default_data_storage_value, storage_id.to_string());
    let channel_settings_value: String = systems_connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0",
            ["system_channel_settings"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(channel_settings_value.contains("OneHour"));
    assert!(channel_settings_value.contains("\"query_all_channel_models\":false"));
    systems_connection
        .execute(
            "UPDATE systems SET value = '0' WHERE key = ?1",
            [SYSTEM_KEY_DEFAULT_DATA_STORAGE],
        )
        .unwrap();
    drop(systems_connection);

    let invalid_default_storage_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ defaultDataStorageID }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let invalid_default_storage_query_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_default_storage_query).await)
            .unwrap();
    assert_eq!(
        invalid_default_storage_query_json["data"]["defaultDataStorageID"],
        serde_json::Value::Null
    );

    let systems_connection = foundation.open_connection(true).unwrap();
    systems_connection
        .execute(
            "UPDATE systems SET value = 'not-a-number' WHERE key = ?1",
            [SYSTEM_KEY_DEFAULT_DATA_STORAGE],
        )
        .unwrap();
    drop(systems_connection);

    let malformed_default_storage_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {settings_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ defaultDataStorageID }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let malformed_default_storage_query_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(malformed_default_storage_query).await)
            .unwrap();
    assert_eq!(
        malformed_default_storage_query_json["data"]["defaultDataStorageID"],
        serde_json::Value::Null
    );

    std::fs::remove_dir_all(backup_root).ok();
    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn sqlite_admin_graphql_route_supports_user_management_writes() {
    let db_path = temp_sqlite_path("sqlite-admin-user-management-writes");
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

    let admin_id = insert_sqlite_user(
        &foundation,
        "admin-users@example.com",
        "password123",
        &["write_users", "read_settings"],
    );
    let target_user_id = insert_sqlite_user(
        &foundation,
        "target-route@example.com",
        "password123",
        &[],
    );
    insert_sqlite_project_membership(&foundation, admin_id, 1, false, &[]);
    insert_sqlite_project_membership(&foundation, target_user_id, 1, false, &["read_requests"]);

    let connection = foundation.open_connection(true).unwrap();
    let old_role_id = insert_sqlite_role(&foundation, "Route Old Role", "system", 0, &["read_settings"]);
    let new_role_id = insert_sqlite_role(&foundation, "Route New Role", "system", 0, &["read_channels"]);
    attach_sqlite_role(&foundation, target_user_id, old_role_id);
    drop(connection);

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "sqlite3",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: build_identity_capability("sqlite3", &db_path.display().to_string(), false),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let app = router(state);
    let admin_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "admin-users@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let target_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "target-route@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };

    let create_user = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation CreateUser($input: CreateUserInput!) {{ createUser(input: $input) {{ id email firstName lastName preferLanguage scopes roles {{ edges {{ node {{ id name }} }} }} }} }}","variables":{{"input":{{"email":"route-create@example.com","password":"newpass123","firstName":"Route","lastName":"Create","scopes":["read_settings"],"roleIDs":["{}"]}}}}}}"#,
                    graphql_gid("role", new_role_id)
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_user.status(), StatusCode::OK);
    let create_user_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(create_user).await).unwrap();
    assert_eq!(create_user_json["data"]["createUser"]["email"], "route-create@example.com");

    let target_gid = graphql_gid("user", target_user_id);
    let new_role_gid = graphql_gid("role", new_role_id);

    let update_user_status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation UpdateUserStatus($id: ID!, $status: UserStatus!) {{ updateUserStatus(id: $id, status: $status) {{ id status }} }}","variables":{{"id":"{}","status":"deactivated"}}}}"#,
                    target_gid
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_user_status.status(), StatusCode::OK);
    let update_user_status_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_user_status).await).unwrap();
    assert_eq!(update_user_status_json["data"]["updateUserStatus"]["status"], "deactivated");

    let reactivate_user_status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation UpdateUserStatus($id: ID!, $status: UserStatus!) {{ updateUserStatus(id: $id, status: $status) {{ id status }} }}","variables":{{"id":"{}","status":"activated"}}}}"#,
                    target_gid
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reactivate_user_status.status(), StatusCode::OK);
    let reactivate_user_status_json = serde_json::from_slice::<serde_json::Value>(
        &read_body(reactivate_user_status).await,
    )
    .unwrap();
    assert_eq!(
        reactivate_user_status_json["data"]["updateUserStatus"]["status"],
        "activated"
    );

    let update_user = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation UpdateUser($id: ID!, $input: UpdateUserInput!) {{ updateUser(id: $id, input: $input) {{ id firstName preferLanguage scopes roles {{ edges {{ node {{ id name }} }} }} }} }}","variables":{{"id":"{}","input":{{"firstName":"RouteUpdated","preferLanguage":"fr","scopes":["read_channels"],"roleIDs":["{}"]}}}}}}"#,
                    target_gid, new_role_gid
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_user.status(), StatusCode::OK);
    let update_user_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_user).await).unwrap();
    assert_eq!(update_user_json["data"]["updateUser"]["firstName"], "RouteUpdated");
    assert_eq!(update_user_json["data"]["updateUser"]["preferLanguage"], "fr");
    assert_eq!(
        update_user_json["data"]["updateUser"]["roles"]["edges"][0]["node"]["id"],
        new_role_gid
    );

    let target_token_after_admin_updates = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "target-route@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };

    let update_me = app
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {target_token_after_admin_updates}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation UpdateMe($input: UpdateMeInput!) { updateMe(input: $input) { email firstName lastName preferLanguage avatar projects { projectID } } }","variables":{"input":{"firstName":"Self","lastName":"Updated","preferLanguage":"ja","avatar":"https://example.com/self.png"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_me.status(), StatusCode::OK);
    let update_me_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(update_me).await).unwrap();
    assert_eq!(update_me_json["data"]["updateMe"]["email"], "target-route@example.com");
    assert_eq!(update_me_json["data"]["updateMe"]["firstName"], "Self");
    assert_eq!(update_me_json["data"]["updateMe"]["preferLanguage"], "ja");
    assert_eq!(
        update_me_json["data"]["updateMe"]["projects"][0]["projectID"],
        graphql_gid("project", 1)
    );

    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn sqlite_admin_graphql_route_supports_broader_management_writes_for_task11() {
    let db_path = temp_sqlite_path("sqlite-admin-task11-management-writes");
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

    insert_sqlite_user(
        &foundation,
        "task11-admin@example.com",
        "password123",
        &[
            "write_projects",
            "write_roles",
            "write_api_keys",
            "write_channels",
        ],
    );
    insert_sqlite_user(&foundation, "task11-viewer@example.com", "password123", &[]);
    let owner_id = insert_sqlite_user(&foundation, "task11-owner@example.com", "password123", &[]);
    let connection = foundation.open_connection(true).unwrap();
    connection
        .execute("UPDATE users SET is_owner = 1 WHERE id = ?1", [owner_id])
        .unwrap();
    let storage_id = connection
        .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM data_storages", [], |row| row.get::<_, i64>(0))
        .unwrap();
    let backup_root = std::env::temp_dir().join(format!(
        "axonhub-task11-backup-{}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&backup_root).unwrap();
    connection
        .execute(
            "INSERT INTO data_storages (id, name, description, \"primary\", type, settings, status, deleted_at)
             VALUES (?1, ?2, ?3, 0, 'fs', ?4, 'active', 0)",
            rusqlite::params![
                storage_id,
                "Task11 Backup FS",
                "task11 backup",
                serde_json::json!({"directory": backup_root.to_string_lossy()}).to_string(),
            ],
        )
        .unwrap();
    drop(connection);

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "sqlite3",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: build_identity_capability("sqlite3", &db_path.display().to_string(), false),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let app = router(state);
    let admin_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "task11-admin@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let viewer_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "task11-viewer@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let owner_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "task11-owner@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };

    let denied_project = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {viewer_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation CreateProject($input: CreateProjectInput!) { createProject(input: $input) { id } }","variables":{"input":{"name":"Denied Project"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let denied_project_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(denied_project).await).unwrap();
    assert_eq!(denied_project_json["data"]["createProject"], serde_json::Value::Null);
    assert_eq!(denied_project_json["errors"][0]["message"], "permission denied");

    let create_project = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation CreateProject($input: CreateProjectInput!) { createProject(input: $input) { id name status } }","variables":{"input":{"name":"Task11 Project","description":"task11","status":"active"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_project_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(create_project).await).unwrap();
    let project_gid = create_project_json["data"]["createProject"]["id"].as_str().unwrap().to_owned();
    assert_eq!(create_project_json["data"]["createProject"]["name"], "Task11 Project");

    let create_role = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .header("X-Project-ID", project_gid.clone())
                .body(Body::from(format!(
                    r#"{{"query":"mutation CreateRole($input: CreateRoleInput!) {{ createRole(input: $input) {{ id name level projectID scopes }} }}","variables":{{"input":{{"name":"Task11 Project Role","level":"project","projectID":"{}","scopes":["write_api_keys"]}}}}}}"#,
                    project_gid
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_role_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(create_role).await).unwrap();
    assert_eq!(create_role_json["data"]["createRole"]["name"], "Task11 Project Role");

    let invalid_api_key = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation CreateAPIKey($input: CreateAPIKeyInput!) { createAPIKey(input: $input) { id } }","variables":{"input":{"projectID":"gid://axonhub/user/1","name":"bad-key"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let invalid_api_key_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_api_key).await).unwrap();
    assert_eq!(invalid_api_key_json["data"]["createAPIKey"], serde_json::Value::Null);

    let create_api_key = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .header("X-Project-ID", project_gid.clone())
                .body(Body::from(format!(
                    r#"{{"query":"mutation CreateAPIKey($input: CreateAPIKeyInput!) {{ createAPIKey(input: $input) {{ id projectID name keyType status scopes }} }}","variables":{{"input":{{"projectID":"{}","name":"Task11 Key","keyType":"user","status":"enabled","scopes":["read_channels"]}}}}}}"#,
                    project_gid
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_api_key_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(create_api_key).await).unwrap();
    assert_eq!(create_api_key_json["data"]["createAPIKey"]["name"], "Task11 Key");

    let create_channel = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation CreateChannel($input: CreateChannelInput!) { createChannel(input: $input) { id name channelType supportedModels } }","variables":{"input":{"name":"Task11 Channel","channelType":"openai","baseURL":"https://example.test/v1","supportedModels":["gpt-4o"],"defaultTestModel":"gpt-4o","status":"enabled","credentialsJSON":"{}","settingsJSON":"{}","tags":["task11"],"orderingWeight":90}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_channel_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(create_channel).await).unwrap();
    assert_eq!(create_channel_json["data"]["createChannel"]["name"], "Task11 Channel");

    let create_model = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {admin_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation CreateModel($input: CreateModelInput!) { createModel(input: $input) { id developer modelID modelType name } }","variables":{"input":{"developer":"openai","modelID":"gpt-4o","modelType":"chat","name":"GPT-4o","icon":"OpenAI","group":"openai","modelCardJSON":"{}","settingsJSON":"{}","status":"enabled","remark":"task11"}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_model_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(create_model).await).unwrap();
    assert_eq!(create_model_json["data"]["createModel"]["modelID"], "gpt-4o");

    let configure_backup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation UpdateAutoBackupSettings($input: UpdateAutoBackupSettingsInput!) {{ updateAutoBackupSettings(input: $input) }}","variables":{{"input":{{"enabled":true,"frequency":"daily","dataStorageID":{},"includeChannels":true,"includeModels":true,"includeAPIKeys":true,"includeModelPrices":false,"retentionDays":2}}}}}}"#,
                    storage_id
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let configure_backup_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(configure_backup).await).unwrap();
    assert_eq!(configure_backup_json["data"]["updateAutoBackupSettings"], true);

    let backup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation Backup($input: BackupInput!) { backup(input: $input) { success data message } }","variables":{"input":{"includeChannels":true,"includeModels":true,"includeAPIKeys":true,"includeModelPrices":false}}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let backup_json = serde_json::from_slice::<serde_json::Value>(&read_body(backup).await).unwrap();
    assert_eq!(backup_json["data"]["backup"]["success"], true);
    let backup_payload = backup_json["data"]["backup"]["data"].as_str().unwrap().to_owned();

    let restore = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"query":"mutation Restore($input: RestoreInput!) {{ restore(input: $input) {{ success message }} }}","variables":{{"input":{{"payload":{},"includeChannels":true,"includeModels":true,"includeAPIKeys":true,"includeModelPrices":false,"overwriteExisting":true}}}}}}"#,
                    serde_json::to_string(&backup_payload).unwrap()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let restore_json = serde_json::from_slice::<serde_json::Value>(&read_body(restore).await).unwrap();
    assert_eq!(restore_json["data"]["restore"]["success"], true);
    assert_eq!(restore_json["data"]["restore"]["message"], "Restore completed successfully");

    let verification = foundation.open_connection(true).unwrap();
    let project_count: i64 = verification
        .query_row("SELECT COUNT(*) FROM projects WHERE name = 'Task11 Project' AND deleted_at = 0", [], |row| row.get(0))
        .unwrap();
    let role_count: i64 = verification
        .query_row("SELECT COUNT(*) FROM roles WHERE name = 'Task11 Project Role' AND deleted_at = 0", [], |row| row.get(0))
        .unwrap();
    let api_key_count: i64 = verification
        .query_row("SELECT COUNT(*) FROM api_keys WHERE name = 'Task11 Key' AND deleted_at = 0", [], |row| row.get(0))
        .unwrap();
    let channel_count: i64 = verification
        .query_row("SELECT COUNT(*) FROM channels WHERE name = 'Task11 Channel' AND deleted_at = 0", [], |row| row.get(0))
        .unwrap();
    let model_count: i64 = verification
        .query_row("SELECT COUNT(*) FROM models WHERE developer = 'openai' AND model_id = 'gpt-4o' AND deleted_at = 0", [], |row| row.get(0))
        .unwrap();
    let restore_runs: i64 = verification
        .query_row("SELECT COUNT(*) FROM operational_runs WHERE operation_type = 'restore' AND status = 'completed'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(project_count, 1);
    assert_eq!(role_count, 1);
    assert_eq!(api_key_count, 1);
    assert_eq!(channel_count, 1);
    assert_eq!(model_count, 1);
    assert_eq!(restore_runs, 1);

    std::fs::remove_dir_all(backup_root).ok();
    std::fs::remove_file(db_path).ok();
}

#[tokio::test]
async fn sqlite_admin_request_content_route_enforces_project_scope_and_wrong_project_denial() {
    let db_path = temp_sqlite_path("sqlite-admin-request-content-rbac");
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

    let project_reader_id = insert_sqlite_user(
        &foundation,
        "project-reader@example.com",
        "password123",
        &[],
    );
    let outsider_id = insert_sqlite_user(
        &foundation,
        "outsider@example.com",
        "password123",
        &[],
    );
    let owner_reader_id = insert_sqlite_user(
        &foundation,
        "project-owner@example.com",
        "password123",
        &[],
    );

    insert_sqlite_project_membership(&foundation, project_reader_id, 1, false, &[]);
    insert_sqlite_project_membership(&foundation, owner_reader_id, 1, true, &[]);

    let project_role_id = insert_sqlite_role(
        &foundation,
        "Project Request Reader",
        "project",
        1,
        &["read_requests"],
    );
    attach_sqlite_role(&foundation, project_reader_id, project_role_id);

    let (request_id, content_dir) = seed_sqlite_request_content(&foundation, 1);

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "sqlite3",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: build_identity_capability("sqlite3", &db_path.display().to_string(), false),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let app = router(state);

    let project_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "project-reader@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let outsider_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "outsider@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };
    let owner_token = match build_identity_capability("sqlite3", &db_path.display().to_string(), false) {
        IdentityCapability::Available { identity } => identity
            .admin_signin(&SignInRequest {
                email: "project-owner@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap()
            .token,
        IdentityCapability::Unsupported { message } => panic!("Expected identity capability: {message}"),
    };

    let allowed_project = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/admin/requests/{request_id}/content"))
                .method(Method::GET.as_str())
                .header("Authorization", format!("Bearer {project_token}"))
                .header("X-Project-ID", "gid://axonhub/project/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed_project.status(), StatusCode::OK);
    let allowed_project_body = read_body(allowed_project).await;
    assert_eq!(allowed_project_body, br#"{"content":"sqlite-request-content"}"#.to_vec());

    let allowed_owner = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/admin/requests/{request_id}/content"))
                .method(Method::GET.as_str())
                .header("Authorization", format!("Bearer {owner_token}"))
                .header("X-Project-ID", "gid://axonhub/project/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed_owner.status(), StatusCode::OK);

    let denied_outsider = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/admin/requests/{request_id}/content"))
                .method(Method::GET.as_str())
                .header("Authorization", format!("Bearer {outsider_token}"))
                .header("X-Project-ID", "gid://axonhub/project/1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied_outsider.status(), StatusCode::FORBIDDEN);
    let denied_outsider_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(denied_outsider).await).unwrap();
    assert_eq!(denied_outsider_json["error"]["type"], "Forbidden");

    let wrong_project = app
        .oneshot(
            Request::builder()
                .uri(format!("/admin/requests/{request_id}/content"))
                .method(Method::GET.as_str())
                .header("Authorization", format!("Bearer {project_token}"))
                .header("X-Project-ID", "gid://axonhub/project/999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_project.status(), StatusCode::BAD_REQUEST);
    let wrong_project_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(wrong_project).await).unwrap();
    assert_eq!(wrong_project_json["error"]["message"], "Project ID not found in context");

    std::fs::remove_file(db_path).ok();
    std::fs::remove_dir_all(content_dir).ok();
}

#[tokio::test]
async fn sqlite_openapi_graphql_route_enforces_api_key_scope_and_service_account_context() {
    let db_path = temp_sqlite_path("sqlite-openapi-graphql-rbac");
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

    let connection = foundation.open_connection(true).unwrap();
    connection
        .execute(
            "UPDATE api_keys SET scopes = ?1 WHERE key = ?2 AND deleted_at = 0",
            rusqlite::params![
                serde_json::to_string(&["write_api_keys"]).unwrap(),
                DEFAULT_SERVICE_API_KEY_VALUE
            ],
        )
        .unwrap();

    let state = HttpState { service_name: "AxonHub".to_owned(),
    version: "v0.9.20".to_owned(),
    config_source: None,
    system_bootstrap: build_system_bootstrap_capability(
        "sqlite3",
        &db_path.display().to_string(),
        "v0.9.20",
    ),
    identity: build_identity_capability("sqlite3", &db_path.display().to_string(), false),
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
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes.".to_owned(),
    }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
        thread_header: Some("AH-Thread-Id".to_owned()),
        trace_header: Some("AH-Trace-Id".to_owned()),
        request_header: Some("X-Request-Id".to_owned()),
        extra_trace_headers: Vec::new(),
        extra_trace_body_fields: Vec::new(),
        claude_code_trace_enabled: false,
        codex_trace_enabled: false,
    },  };

    let app = router(state);

    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {DEFAULT_SERVICE_API_KEY_VALUE}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation { createLLMAPIKey(name: \"Scoped SDK Key\") { name scopes } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed_json = serde_json::from_slice::<serde_json::Value>(&read_body(allowed).await).unwrap();
    assert_eq!(allowed_json["data"]["createLLMAPIKey"]["name"], "Scoped SDK Key");

    let user_key_rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {DEFAULT_USER_API_KEY_VALUE}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation { createLLMAPIKey(name: \"Wrong Type Key\") { name } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(user_key_rejected.status(), StatusCode::UNAUTHORIZED);
    let user_key_rejected_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(user_key_rejected).await).unwrap();
    assert_eq!(user_key_rejected_json["error"]["message"], "Invalid API key");

    let invalid_bearer = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Token {DEFAULT_SERVICE_API_KEY_VALUE}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation { createLLMAPIKey(name: \"Wrong Prefix\") { name } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_bearer.status(), StatusCode::UNAUTHORIZED);
    let invalid_bearer_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_bearer).await).unwrap();
    assert_eq!(
        invalid_bearer_json["error"]["message"],
        "Invalid token: Authorization header must start with 'Bearer '"
    );

    let invalid_key = app
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", "Bearer invalid-key")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"mutation { createLLMAPIKey(name: \"Invalid Key\") { name } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_key.status(), StatusCode::UNAUTHORIZED);
    let invalid_key_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(invalid_key).await).unwrap();
    assert_eq!(invalid_key_json["error"]["message"], "Invalid API key");

    let api_key_count_before_unsupported: i64 = foundation
        .open_connection(true)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM api_keys WHERE deleted_at = 0", [], |row| row.get(0))
        .unwrap();

    let unsupported_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi/v1/graphql")
                .method(Method::POST.as_str())
                .header("Authorization", format!("Bearer {DEFAULT_SERVICE_API_KEY_VALUE}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"{ sdkCapabilities { name } }"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsupported_query.status(), StatusCode::NOT_IMPLEMENTED);
    let unsupported_query_json =
        serde_json::from_slice::<serde_json::Value>(&read_body(unsupported_query).await).unwrap();
    assert_eq!(unsupported_query_json["error"], "not_implemented");
    assert_eq!(unsupported_query_json["route_family"], "/openapi/v1/graphql");
    assert!(unsupported_query_json["message"]
        .as_str()
        .is_some_and(|message| message.contains("sdkCapabilities")));

    let api_key_count_after_unsupported: i64 = foundation
        .open_connection(true)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM api_keys WHERE deleted_at = 0", [], |row| row.get(0))
        .unwrap();
    assert_eq!(api_key_count_after_unsupported, api_key_count_before_unsupported);

    std::fs::remove_file(db_path).ok();
}
