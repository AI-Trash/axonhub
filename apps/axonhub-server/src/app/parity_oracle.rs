use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

use super::capabilities::{
    build_admin_capability, build_admin_graphql_capability, build_identity_capability,
    build_openai_v1_capability, build_openapi_graphql_capability,
    build_provider_edge_admin_capability, build_request_context_capability,
    build_system_bootstrap_capability,
};
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
                    _ => value.as_str(),
                };
                actix_request = actix_request.insert_header((name.as_str(), normalized));
            }
        }
        let body = request.body.unwrap_or_default().into_bytes();
        Ok(actix_test::call_service(&app, actix_request.set_payload(body).to_request()).await)
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
        provider_edge_admin: build_provider_edge_admin_capability("sqlite3", &dsn),
        allow_no_auth: false,
        cors: HttpCorsSettings::default(),
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
    let state = sqlite_state(&db_path);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .service(
                web::resource("/admin/codex/oauth/start")
                    .route(web::post().to(axonhub_http::parity_start_codex_oauth)),
            ),
    )
    .await;
    let mut request = actix_test::TestRequest::post().uri(&fixture.request.path);
    if let Some(headers) = fixture.request.headers.as_ref() {
        for (name, value) in headers {
            request = request.insert_header((name.as_str(), value.as_str()));
        }
    }
    let response = actix_test::call_service(
        &app,
        request
            .set_payload(fixture.request.body.unwrap_or_default().into_bytes())
            .to_request(),
    )
    .await;
    let output = response_to_output(suite, response, false).await;
    fs::remove_file(db_path).ok();
    output
}

async fn emit_http_handler_parity(suite: &str, fixture: OracleFixture) -> OracleOutput {
    let db_path = temp_sqlite_path(&format!("handler-{}", suite.replace(':', "-")));
    if matches!(fixture.handler.as_deref(), Some("anthropic_models_basic" | "gemini_models_basic" | "v1_models_basic")) {
        bootstrap_sqlite(&db_path);
    }
    let state = sqlite_state(&db_path);
    let handler = fixture.handler.as_deref().expect("handler fixture is required");
    let app = match handler {
        "admin_initialize_invalid_json" => actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state))
                .service(web::resource("/admin/system/initialize").route(web::post().to(axonhub_http::parity_initialize_system))),
        )
        .await,
        "admin_graphql_playground" => actix_test::init_service(
            App::new().service(web::resource("/admin/playground").route(web::get().to(axonhub_http::parity_admin_graphql_playground))),
        )
        .await,
        "openapi_graphql_playground" => actix_test::init_service(
            App::new().service(web::resource("/openapi/v1/playground").route(web::get().to(axonhub_http::parity_openapi_graphql_playground))),
        )
        .await,
        "openai_chat_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/v1/chat/completions").route(web::post().to(axonhub_http::parity_openai_chat_completions))),
        )
        .await,
        "openai_responses_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/v1/responses").route(web::post().to(axonhub_http::parity_openai_responses))),
        )
        .await,
        "openai_embeddings_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/v1/embeddings").route(web::post().to(axonhub_http::parity_openai_embeddings))),
        )
        .await,
        "openai_images_generations_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/v1/images/generations").route(web::post().to(axonhub_http::parity_openai_images_generations))),
        )
        .await,
        "openai_videos_create_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/v1/videos").route(web::post().to(axonhub_http::parity_openai_videos_create))),
        )
        .await,
        "anthropic_messages_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/anthropic/v1/messages").route(web::post().to(axonhub_http::parity_anthropic_messages))),
        )
        .await,
        "jina_rerank_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/jina/v1/rerank").route(web::post().to(axonhub_http::parity_jina_rerank))),
        )
        .await,
        "jina_embeddings_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/jina/v1/embeddings").route(web::post().to(axonhub_http::parity_jina_embeddings))),
        )
        .await,
        "gemini_generate_content_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/gemini/v1/models/gemini-2.5-flash:generateContent").route(web::post().to(axonhub_http::parity_gemini_generate_content))),
        )
        .await,
        "v1beta_generate_content_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/v1beta/models/gemini-2.5-flash:generateContent").route(web::post().to(axonhub_http::parity_gemini_generate_content))),
        )
        .await,
        "doubao_create_task_empty_body" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/doubao/v3/contents/generations/tasks").route(web::post().to(axonhub_http::parity_doubao_create_task))),
        )
        .await,
        "provider_edge_claudecode_start_invalid_json" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/admin/claudecode/oauth/start").route(web::post().to(axonhub_http::parity_start_claudecode_oauth))),
        )
        .await,
        "provider_edge_antigravity_start_invalid_json" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/admin/antigravity/oauth/start").route(web::post().to(axonhub_http::parity_start_antigravity_oauth))),
        )
        .await,
        "provider_edge_copilot_start_invalid_json" => actix_test::init_service(
            App::new().app_data(web::Data::new(state)).service(web::resource("/admin/copilot/oauth/start").route(web::post().to(axonhub_http::parity_start_copilot_oauth))),
        )
        .await,
        _ => panic!("unsupported handler parity fixture {handler}"),
    };

    let mut request = actix_test::TestRequest::default()
        .method(Method::from_bytes(fixture.request.method.as_bytes()).expect("valid method"))
        .uri(&fixture.request.path);
    if let Some(headers) = fixture.request.headers.as_ref() {
        for (name, value) in headers {
            request = request.insert_header((name.as_str(), value.as_str()));
        }
    }
    let response = actix_test::call_service(
        &app,
        request
            .set_payload(fixture.request.body.unwrap_or_default().into_bytes())
            .to_request(),
    )
    .await;
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
