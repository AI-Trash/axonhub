use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use actix_web::body::BoxBody;
use actix_web::dev::ServiceResponse;
use actix_web::http::Method;
use actix_web::test as actix_test;
use actix_web::{App, web};
use axonhub_http::{
    HttpCorsSettings, HttpState, InitializeSystemRequest, TraceConfig, router as http_router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::capabilities::{
    build_admin_capability, build_admin_graphql_capability, build_identity_capability,
    build_openai_v1_capability, build_openapi_graphql_capability,
    build_oauth_provider_admin_capability, build_request_context_capability,
    build_system_bootstrap_capability,
};
use crate::foundation::admin::oauth::{oauth_provider_env_test_lock, OAUTH_PROVIDER_ADMIN_REQUIRED_ENV_VARS};
use crate::foundation::shared::{DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_VALUE};

#[derive(Deserialize)]
struct OracleFixture {
    schema_version: u32,
    emitter: Option<String>,
    request: OracleRequest,
    model: Option<OracleModel>,
    handler: Option<String>,
    normalize_generated_key: Option<bool>,
}

#[derive(Deserialize)]
struct OracleRequest {
    method: String,
    path: String,
    headers: Option<BTreeMap<String, String>>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct OracleModel {
    developer: String,
    model_id: String,
    model_type: String,
    name: String,
    icon: String,
    group: String,
    remark: String,
}

#[derive(Serialize)]
struct OracleOutput {
    suite: String,
    status: u16,
    headers: BTreeMap<String, String>,
    content_type: String,
    body: Value,
}

#[derive(Clone)]
struct TestApp {
    state: HttpState,
}

impl TestApp {
    fn new(state: HttpState) -> Self {
        Self { state }
    }

    async fn oneshot(
        &self,
        request: OracleRequest,
    ) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
        let app = actix_test::init_service(http_router(self.state.clone())).await;
        let mut actix_request = actix_test::TestRequest::default()
            .method(Method::from_bytes(request.method.as_bytes()).expect("valid method"))
            .uri(&request.path);
        if let Some(headers) = &request.headers {
            for (name, value) in headers {
                let normalized = match (name.as_str(), value.as_str()) {
                    ("X-API-Key", "default-user") => DEFAULT_USER_API_KEY_VALUE,
                    ("Authorization", "Bearer valid-admin-token") => {
                        return_with_bearer_token(&self.state)
                    }
                    _ => value.as_str(),
                };
                actix_request = actix_request.insert_header((name.as_str(), normalized));
            }
        }
        let body = request.body.unwrap_or_default().into_bytes();
        Ok(actix_test::call_service(&app, actix_request.set_payload(body).to_request()).await)
    }
}

fn return_with_bearer_token(state: &HttpState) -> &'static str {
    Box::leak(format!("Bearer {}", issue_admin_token_for_fixture(state)).into_boxed_str())
}

fn issue_admin_token_for_fixture(state: &HttpState) -> String {
    match &state.identity {
        axonhub_http::IdentityCapability::Available { identity } => {
            let request = axonhub_http::SignInRequest {
                email: "owner@example.com".to_owned(),
                password: "password123".to_owned(),
            };

            match identity.admin_signin(&request) {
                Ok(success) => success.token,
                Err(axonhub_http::SignInError::InvalidCredentials)
                | Err(axonhub_http::SignInError::Internal) => {
                    bootstrap_state_for_admin_signin(state);
                    identity
                        .admin_signin(&request)
                        .expect("issue admin token for parity fixture after bootstrap")
                        .token
                }
                Err(error) => panic!("issue admin token for parity fixture: {error:?}"),
            }
        }
        axonhub_http::IdentityCapability::Unsupported { message } => {
            panic!("identity capability unavailable for parity fixture: {message}")
        }
    }
}

fn bootstrap_state_for_admin_signin(state: &HttpState) {
    match &state.system_bootstrap {
        axonhub_http::SystemBootstrapCapability::Available { system } => {
            match system.initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            }) {
                Ok(()) | Err(axonhub_http::SystemInitializeError::AlreadyInitialized) => {}
                Err(error) => {
                    panic!("failed to bootstrap parity fixture before admin signin: {error:?}")
                }
            }
        }
        axonhub_http::SystemBootstrapCapability::Unsupported { message } => {
            panic!("system bootstrap unavailable for parity fixture: {message}")
        }
    }
}

#[tokio::test]
async fn parity_oracle_emit_suite() {
    let suite = match env::var("AXONHUB_PARITY_SUITE") {
        Ok(value) => value,
        Err(_) => return,
    };
    let fixture_path = env::var("AXONHUB_PARITY_FIXTURE").expect("fixture path env");
    let capture_path = env::var("AXONHUB_PARITY_CAPTURE").expect("capture path env");
    let fixture = load_fixture(Path::new(&fixture_path));
    let emitter = fixture.emitter.clone().unwrap_or_else(|| suite.clone());

    let output = match emitter.as_str() {
        "admin_system_status_initial" => emit_admin_system_status_initial(&suite, fixture).await,
        "admin_signin_invalid_json" => emit_admin_signin_invalid_json(&suite, fixture).await,
        "v1_models_basic" => emit_v1_models_basic(&suite, fixture).await,
        "anthropic_models_basic" => emit_anthropic_models_basic(&suite, fixture).await,
        "gemini_models_basic" => emit_gemini_models_basic(&suite, fixture).await,
        "provider_edge_codex_start_invalid_json" => {
            emit_provider_edge_codex_start_invalid_json(&suite, fixture).await
        }
        "provider_edge_claudecode_start_invalid_json"
        | "provider_edge_antigravity_start_invalid_json"
        | "provider_edge_copilot_start_invalid_json" => {
            emit_http_handler_parity(&suite, fixture).await
        }
        "http_handler_parity" => emit_http_handler_parity(&suite, fixture).await,
        "openapi_graphql_create_llm_api_key" => {
            emit_openapi_graphql_create_llm_api_key(&suite, fixture).await
        }
        _ => panic!("unsupported parity emitter {emitter} for suite {suite}"),
    };

    fs::write(
        &capture_path,
        serde_json::to_string_pretty(&output).expect("serialize oracle output") + "\n",
    )
    .expect("write capture");
}

fn load_fixture(path: &Path) -> OracleFixture {
    let fixture =
        serde_json::from_str::<OracleFixture>(&fs::read_to_string(path).expect("read fixture"))
            .expect("parse fixture");
    assert_eq!(
        fixture.schema_version, 1,
        "unsupported parity fixture schema version"
    );
    fixture
}

struct ProviderEdgeEnvFixture {
    _guard: std::sync::MutexGuard<'static, ()>,
    previous: Vec<(&'static str, Option<String>)>,
}

impl ProviderEdgeEnvFixture {
    fn new() -> Self {
        let guard = oauth_provider_env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = OAUTH_PROVIDER_ADMIN_REQUIRED_ENV_VARS
            .iter()
            .map(|key| (*key, env::var(key).ok()))
            .collect::<Vec<_>>();

        for key in OAUTH_PROVIDER_ADMIN_REQUIRED_ENV_VARS {
            env::remove_var(key);
        }

        Self { _guard: guard, previous }
    }

    fn set_all(&self) {
        for (key, value) in provider_edge_env_values() {
            env::set_var(key, value);
        }
    }
}

impl Drop for ProviderEdgeEnvFixture {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
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

fn temp_sqlite_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    env::temp_dir().join(format!("axonhub-parity-{name}-{unique}.db"))
}

fn sqlite_state(db_path: &Path) -> HttpState {
    let dsn = db_path.display().to_string();
    HttpState {
        service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: build_system_bootstrap_capability("sqlite3", &dsn, "v0.9.20"),
        identity: build_identity_capability("sqlite3", &dsn, false),
        request_context: build_request_context_capability("sqlite3", &dsn, false),
        openai_v1: build_openai_v1_capability("sqlite3", &dsn),
        admin: build_admin_capability("sqlite3", &dsn),
        admin_graphql: build_admin_graphql_capability("sqlite3", &dsn),
        openapi_graphql: build_openapi_graphql_capability("sqlite3", &dsn),
        oauth_provider_admin: build_oauth_provider_admin_capability("sqlite3", &dsn),
        allow_no_auth: false,
        cors: HttpCorsSettings::default(),
        request_timeout: Some(std::time::Duration::from_secs(30)),
        llm_request_timeout: Some(std::time::Duration::from_secs(600)),
        trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },
    }
}

fn bootstrap_sqlite(db_path: &Path) {
    let dsn = db_path.display().to_string();
    let capability = build_system_bootstrap_capability("sqlite3", &dsn, "v0.9.20");
    let system = match capability {
        axonhub_http::SystemBootstrapCapability::Available { system } => system,
        axonhub_http::SystemBootstrapCapability::Unsupported { message } => {
            panic!("expected bootstrap capability: {message}")
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
}

async fn emit_admin_system_status_initial(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path("admin-status");
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_admin_signin_invalid_json(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path("admin-signin-invalid");
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_v1_models_basic(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path("v1-models");
    bootstrap_sqlite(&db_path);
    if let Some(model) = fixture.model {
        let connection = rusqlite::Connection::open(&db_path).expect("open sqlite db");
        connection
            .execute(
                "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', '{}', 'enabled', ?7, 0)",
                rusqlite::params![
                    model.developer,
                    model.model_id,
                    model.model_type,
                    model.name,
                    model.icon,
                    model.group,
                    model.remark,
                ],
            )
            .expect("seed model");
    }
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_anthropic_models_basic(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path("anthropic-models");
    bootstrap_sqlite(&db_path);
    if let Some(model) = fixture.model {
        let connection = rusqlite::Connection::open(&db_path).expect("open sqlite db");
        connection
            .execute(
                "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', '{}', 'enabled', ?7, 0)",
                rusqlite::params![
                    model.developer,
                    model.model_id,
                    model.model_type,
                    model.name,
                    model.icon,
                    model.group,
                    model.remark,
                ],
            )
            .expect("seed model");
    }
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_gemini_models_basic(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path("gemini-models");
    bootstrap_sqlite(&db_path);
    if let Some(model) = fixture.model {
        let connection = rusqlite::Connection::open(&db_path).expect("open sqlite db");
        connection
            .execute(
                "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', '{}', 'enabled', ?7, 0)",
                rusqlite::params![
                    model.developer,
                    model.model_id,
                    model.model_type,
                    model.name,
                    model.icon,
                    model.group,
                    model.remark,
                ],
            )
            .expect("seed model");
    }
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_provider_edge_codex_start_invalid_json(
    suite: &str,
    fixture: OracleFixture,
) -> OracleOutput {
    let db_path = temp_sqlite_path("provider-edge-invalid-json");
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_http_handler_parity(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path(&format!("handler-{}", suite.replace(':', "-")));
    if matches!(fixture.handler.as_deref(), Some("anthropic_models_basic" | "gemini_models_basic" | "v1_models_basic")) {
        bootstrap_sqlite(&db_path);
    }
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(fixture.request)
        .await
        .unwrap();
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_openapi_graphql_create_llm_api_key(
    suite: &str,
    fixture: OracleFixture,
) -> OracleOutput {
    let db_path = temp_sqlite_path("openapi-graphql");
    bootstrap_sqlite(&db_path);
    let response = TestApp::new(sqlite_state(&db_path))
        .oneshot(OracleRequest {
            method: fixture.request.method,
            path: fixture.request.path,
            headers: Some(
                fixture
                    .request
                    .headers
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(key, value)| {
                        let normalized = if value == "service-key-123" {
                            DEFAULT_SERVICE_API_KEY_VALUE.to_owned()
                        } else {
                            value
                        };
                        (key, normalized)
                    })
                    .collect(),
            ),
            body: fixture.request.body,
        })
        .await
        .unwrap();
    let output = response_to_output(
        suite,
        response,
        fixture.normalize_generated_key.unwrap_or(false),
    )
    .await;
    fs::remove_file(db_path).ok();
    output
}

#[cfg(test)]
pub(crate) fn parity_oracle_helpers_preserve_contract_inner() {
    provider_edge_start_invalid_json_parity_fixtures_cover_all_supported_providers();
}

async fn response_to_output(
    suite: &str,
    response: ServiceResponse<BoxBody>,
    normalize_generated_key: bool,
) -> OracleOutput {
    let status = response.status().as_u16();
    let content_type = normalize_content_type(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned()
            .as_str(),
    );
    let body_bytes = actix_web::body::to_bytes(response.into_body())
        .await
        .unwrap();
    let mut body = if body_bytes.is_empty() {
        Value::String(String::new())
    } else {
        serde_json::from_slice::<Value>(&body_bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body_bytes).to_string()))
    };
    normalize_json_value(&mut body, normalize_generated_key);
    let mut headers = BTreeMap::new();
    headers.insert("content-type".to_owned(), content_type.clone());
    OracleOutput {
        suite: suite.to_owned(),
        status,
        headers,
        content_type,
        body,
    }
}

#[tokio::test]
async fn provider_edge_start_invalid_json_parity_fixtures_cover_all_supported_providers() {
    let env_fixture = ProviderEdgeEnvFixture::new();
    env_fixture.set_all();

    for (suite, path, route_family) in [
        (
            "provider_edge_claudecode_start_invalid_json",
            "/admin/claudecode/oauth/start",
            "/admin/claudecode/oauth/start",
        ),
        (
            "provider_edge_antigravity_start_invalid_json",
            "/admin/antigravity/oauth/start",
            "/admin/antigravity/oauth/start",
        ),
        (
            "provider_edge_copilot_start_invalid_json",
            "/admin/copilot/oauth/start",
            "/admin/copilot/oauth/start",
        ),
    ] {
        let output = emit_http_handler_parity(
            suite,
            OracleFixture {
                schema_version: 1,
                emitter: Some(suite.to_owned()),
                request: OracleRequest {
                    method: "POST".to_owned(),
                    path: path.to_owned(),
                    headers: Some(BTreeMap::from([
                        (
                            "Authorization".to_owned(),
                            "Bearer valid-admin-token".to_owned(),
                        ),
                        ("content-type".to_owned(), "application/json".to_owned()),
                    ])),
                    body: Some("{not-json".to_owned()),
                },
                model: None,
                handler: Some(suite.to_owned()),
                normalize_generated_key: None,
            },
        )
        .await;

        assert_eq!(output.suite, suite);
        assert_eq!(output.status, 400);
        assert_eq!(output.content_type, "application/json");
        assert_eq!(output.body["error"]["type"], "Bad Request");
        assert_eq!(output.body["error"]["message"], "invalid request format");
        assert_eq!(output.headers.get("content-type"), Some(&"application/json".to_owned()));
        assert_ne!(output.body["error"]["message"], route_family);
    }

    let codex_output = emit_provider_edge_codex_start_invalid_json(
        "provider_edge_codex_start_invalid_json",
        OracleFixture {
            schema_version: 1,
            emitter: Some("provider_edge_codex_start_invalid_json".to_owned()),
            request: OracleRequest {
                method: "POST".to_owned(),
                path: "/admin/codex/oauth/start".to_owned(),
                headers: Some(BTreeMap::from([
                    (
                        "Authorization".to_owned(),
                        "Bearer valid-admin-token".to_owned(),
                    ),
                    ("content-type".to_owned(), "application/json".to_owned()),
                ])),
                body: Some("{not-json".to_owned()),
            },
            model: None,
            handler: None,
            normalize_generated_key: None,
        },
    )
    .await;

    assert_eq!(codex_output.suite, "provider_edge_codex_start_invalid_json");
    assert_eq!(codex_output.status, 400);
    assert_eq!(codex_output.content_type, "application/json");
    assert_eq!(codex_output.body["error"]["type"], "Bad Request");
    assert_eq!(codex_output.body["error"]["message"], "invalid request format");
    assert_eq!(
        codex_output.headers.get("content-type"),
        Some(&"application/json".to_owned())
    );
}

fn normalize_content_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn normalize_json_value(value: &mut Value, normalize_generated_key: bool) {
    match value {
        Value::Object(map) => {
            for (key, current) in map.iter_mut() {
                normalize_json_value(current, normalize_generated_key);
                if key == "created" && current.is_number() {
                    *current = Value::String("<created>".to_owned());
                }
                if key == "created" && current.is_string() {
                    *current = Value::String("<created>".to_owned());
                }
                if key == "token" && current.is_string() {
                    *current = Value::String("<token>".to_owned());
                }
                if normalize_generated_key
                    && key == "key"
                    && current
                        .as_str()
                        .is_some_and(|candidate| candidate.starts_with("ah-"))
                {
                    *current = Value::String("<generated-api-key>".to_owned());
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_json_value(item, normalize_generated_key);
            }
            if items.iter().all(|item| item.as_str().is_some()) {
                items.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
            }
        }
        _ => {}
    }
}
