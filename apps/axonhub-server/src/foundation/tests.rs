use super::{
    admin::{SqliteAdminService, SqliteOperationalService},
    authz::{
        scope_strings, serialize_scope_slugs, ScopeLevel, ScopeSlug, ROLE_LEVEL_PROJECT,
        ROLE_LEVEL_SYSTEM, SCOPE_READ_CHANNELS, SCOPE_READ_REQUESTS,
        SCOPE_READ_USERS, SCOPE_READ_SETTINGS, SCOPE_WRITE_API_KEYS, SCOPE_WRITE_SETTINGS,
        SCOPE_WRITE_USERS,
    },
    graphql_sqlite_support::{SqliteAdminGraphqlService, SqliteOpenApiGraphqlService},
    identity_service::SqliteIdentityService,
    openai_v1::{
        NewChannelRecord, NewModelRecord, NewRequestExecutionRecord, NewRequestRecord,
        NewUsageLogRecord, SqliteOpenAiV1Service,
    },
    request_context_service::SqliteRequestContextService,
    shared::{
        SqliteFoundation, DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_VALUE,
        PRIMARY_DATA_STORAGE_NAME, graphql_gid,
    },
    system::{ensure_identity_tables, hash_password, SqliteBootstrapService},
};
use axonhub_http::{
    router as http_router, AdminCapability, AdminError, AdminGraphqlCapability, AdminPort,
    AuthUserContext, HttpState, IdentityCapability, IdentityPort, InitializeSystemRequest,
    OpenAiV1Capability, OpenAiV1ExecutionRequest, OpenAiV1Route, OpenApiGraphqlCapability,
    OpenAiV1Port, ProjectContext, ProviderEdgeAdminCapability, RequestContextCapability, SignInRequest,
    SystemBootstrapCapability, SystemBootstrapPort, TraceConfig,
};
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::ServiceResponse;
use actix_web::http::{Method, StatusCode};
use actix_web::test as actix_test;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

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

async fn read_json_response<B>(response: ServiceResponse<B>) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
    serde_json::from_slice(&body).unwrap()
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

    fn temp_sqlite_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axonhub-{name}-{unique}.db"))
    }

    fn test_admin_user() -> AuthUserContext {
        AuthUserContext {
            id: 1,
            email: "owner@example.com".to_owned(),
            first_name: "System".to_owned(),
            last_name: "Owner".to_owned(),
            is_owner: true,
            prefer_language: "en".to_owned(),
            avatar: Some(String::new()),
            scopes: scope_strings(&[
                SCOPE_READ_SETTINGS,
                SCOPE_READ_CHANNELS,
                SCOPE_READ_REQUESTS,
            ]),
            roles: Vec::new(),
            projects: Vec::new(),
        }
    }

    fn insert_test_user(
        connection: &Connection,
        email: &str,
        password: &str,
        scopes: &[ScopeSlug],
    ) -> i64 {
        let hashed_password = hash_password(password).unwrap();
        let scopes_json = serialize_scope_slugs(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at)
                 VALUES (?1, 'activated', 'en', ?2, 'Test', 'User', '', 0, ?3, 0)",
                params![email, hashed_password, scopes_json],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    fn insert_project_membership(
        connection: &Connection,
        user_id: i64,
        project_id: i64,
        is_owner: bool,
        scopes: &[ScopeSlug],
    ) {
        let scopes_json = serialize_scope_slugs(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO user_projects (user_id, project_id, is_owner, scopes)
                 VALUES (?1, ?2, ?3, ?4)",
                params![user_id, project_id, if is_owner { 1 } else { 0 }, scopes_json],
            )
            .unwrap();
    }

    fn insert_role(
        connection: &Connection,
        name: &str,
        level: ScopeLevel,
        project_id: i64,
        scopes: &[ScopeSlug],
    ) -> i64 {
        let scopes_json = serialize_scope_slugs(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO roles (name, level, project_id, scopes, deleted_at)
                  VALUES (?1, ?2, ?3, ?4, 0)",
                params![name, level.as_str(), project_id, scopes_json],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    fn attach_role(connection: &Connection, user_id: i64, role_id: i64) {
        connection
            .execute(
                "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                params![user_id, role_id],
            )
            .unwrap();
    }

    fn signin_token(foundation: Arc<SqliteFoundation>, email: &str, password: &str) -> String {
        let identity = SqliteIdentityService::new(foundation, false);
        identity.admin_signin(&SignInRequest {
            email: email.to_owned(),
            password: password.to_owned(),
        })
        .unwrap()
        .token
    }

    fn graphql_test_app(
        foundation: Arc<SqliteFoundation>,
        bootstrap: SqliteBootstrapService,
    ) -> TestApp {
        router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "test-only unsupported openai".to_owned(),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Available {
                graphql: Arc::new(SqliteAdminGraphqlService::new(foundation.clone())),
            },
            openapi_graphql: OpenApiGraphqlCapability::Available {
                graphql: Arc::new(SqliteOpenApiGraphqlService::new(foundation)),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        })
    }

    #[test]
    fn foundation_request_usage_and_catalog_stores_share_same_sqlite_schema() {
        let db_path = temp_sqlite_path("foundation-request-usage");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        foundation.system_settings().ensure_schema().unwrap();
        foundation.data_storages().ensure_schema().unwrap();
        foundation.identities().ensure_schema().unwrap();
        foundation.trace_contexts().ensure_schema().unwrap();
        foundation.channel_models().ensure_schema().unwrap();
        foundation.requests().ensure_schema().unwrap();
        foundation.usage_costs().ensure_schema().unwrap();

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Primary",
                channel_type: "openai",
                base_url: "https://api.openai.com/v1",
                status: "enabled",
                credentials_json: "{}",
                supported_models_json: "[\"gpt-4o\"]",
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[\"primary\"]",
                ordering_weight: 100,
                error_message: "",
                remark: "Rust migration foundation test",
            })
            .unwrap();

        let _model_row_id = foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: "{}",
                settings_json: "{}",
                status: "enabled",
                remark: "Rust migration foundation test",
            })
            .unwrap();

        let api_key_id = foundation
            .identities()
            .find_api_key_by_value(DEFAULT_USER_API_KEY_VALUE)
            .unwrap()
            .id;
        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let default_project = foundation
            .identities()
            .find_default_project_for_user(1)
            .unwrap();
        let data_storage_id = foundation
            .system_settings()
            .default_data_storage_id()
            .unwrap()
            .unwrap();
        let primary_storage = foundation
            .data_storages()
            .find_primary_active_storage()
            .unwrap()
            .unwrap();
        let trace_id = foundation
            .trace_contexts()
            .get_or_create_trace(project_id, "trace-foundation-1", None)
            .unwrap()
            .id;
        let user_context = foundation
            .identities()
            .build_user_context(foundation.identities().find_user_by_id(1).unwrap())
            .unwrap();

        let request_id = foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(api_key_id),
                project_id,
                trace_id: Some(trace_id),
                data_storage_id: Some(data_storage_id),
                source: "api",
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_headers_json: "{}",
                request_body_json: "{\"messages\":[]}",
                response_body_json: Some("{\"id\":\"resp_1\"}"),
                response_chunks_json: Some("[]"),
                channel_id: Some(channel_id),
                external_id: Some("req-ext-1"),
                status: "completed",
                stream: false,
                client_ip: "127.0.0.1",
                metrics_latency_ms: Some(120),
                metrics_first_token_latency_ms: Some(45),
                content_saved: false,
                content_storage_id: None,
                content_storage_key: None,
                content_saved_at: None,
            })
            .unwrap();

        let execution_id = foundation
            .requests()
            .create_request_execution(&NewRequestExecutionRecord {
                project_id,
                request_id,
                channel_id: Some(channel_id),
                data_storage_id: Some(data_storage_id),
                external_id: Some("exec-ext-1"),
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_body_json: "{\"provider\":true}",
                response_body_json: Some("{\"ok\":true}"),
                response_chunks_json: Some("[]"),
                error_message: "",
                response_status_code: Some(200),
                status: "completed",
                stream: false,
                metrics_latency_ms: Some(120),
                metrics_first_token_latency_ms: Some(45),
                request_headers_json: "{}",
            })
            .unwrap();

        let usage_id = foundation
            .usage_costs()
            .record_usage(&NewUsageLogRecord {
                request_id,
                api_key_id: Some(api_key_id),
                project_id,
                channel_id: Some(channel_id),
                model_id: "gpt-4o",
                prompt_tokens: 11,
                completion_tokens: 13,
                total_tokens: 24,
                prompt_audio_tokens: 0,
                prompt_cached_tokens: 0,
                prompt_write_cached_tokens: 0,
                prompt_write_cached_tokens_5m: 0,
                prompt_write_cached_tokens_1h: 0,
                completion_audio_tokens: 0,
                completion_reasoning_tokens: 0,
                completion_accepted_prediction_tokens: 0,
                completion_rejected_prediction_tokens: 0,
                source: "api",
                format: "openai/chat_completions",
                total_cost: Some(0.42),
                cost_items_json: "[{\"type\":\"input\",\"amount\":0.12}]",
                cost_price_reference_id: "price-v1",
            })
            .unwrap();

        let connection = foundation.open_connection(false).unwrap();
        assert_eq!(primary_storage.id, data_storage_id);
        assert_eq!(primary_storage.name, PRIMARY_DATA_STORAGE_NAME);
        assert_eq!(primary_storage.description, "Primary database storage");
        assert_eq!(primary_storage.storage_type, "database");
        assert_eq!(primary_storage.status, "active");
        assert_eq!(primary_storage.settings_json, "{}");
        assert_eq!(default_project.id, project_id);
        assert_eq!(user_context.id, 1);
        assert_eq!(user_context.projects[0].project_id.id, project_id);
        let primary_storage_name: String = connection
            .query_row(
                "SELECT name FROM data_storages WHERE id = ?1",
                [data_storage_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(primary_storage_name, PRIMARY_DATA_STORAGE_NAME);

        let persisted_request: (i64, String, i64) = connection
            .query_row(
                "SELECT id, model_id, channel_id FROM requests WHERE id = ?1",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(persisted_request.0, request_id);
        assert_eq!(persisted_request.1, "gpt-4o");
        assert_eq!(persisted_request.2, channel_id);

        let persisted_execution_request_id: i64 = connection
            .query_row(
                "SELECT request_id FROM request_executions WHERE id = ?1",
                [execution_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_execution_request_id, request_id);

        let persisted_usage: (i64, i64, f64) = connection
            .query_row(
                "SELECT request_id, total_tokens, total_cost FROM usage_logs WHERE id = ?1",
                [usage_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(persisted_usage.0, request_id);
        assert_eq!(persisted_usage.1, 24);
        assert_eq!(persisted_usage.2, 0.42);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_update_user_status_mutation() {
        let db_path = temp_sqlite_path("task9-update-user-status");
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
        ensure_identity_tables(&connection).unwrap();

        // Create an admin user with write_users scope
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_USERS],
        );

        // Create a target user with default activated status
        let target_user_id = insert_test_user(
            &connection,
            "target@example.com",
            "password123",
            &[],
        );

        let app = graphql_test_app(foundation.clone(), bootstrap);

        // Sign in as admin
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        // Get the target user's GraphQL ID
        let target_gid = graphql_gid("user", target_user_id);

        // Mutation: update status to deactivated
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"query": "mutation UpdateUserStatus($id: ID!, $status: UserStatus!) {{ updateUserStatus(id: $id, status: $status) {{ id createdAt updatedAt email status firstName lastName isOwner preferLanguage scopes roles {{ edges {{ node {{ id name }} }} }} }} }}", "variables": {{ "id": "{}", "status": "deactivated" }} }}"#,
                        target_gid
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;

        assert!(json["data"]["updateUserStatus"].is_object());
        let updated_user = &json["data"]["updateUserStatus"];

        // Verify returned user fields
        assert!(updated_user["id"].is_string());
        assert_eq!(updated_user["id"].as_str().unwrap(), target_gid);
        assert!(updated_user["email"].as_str().unwrap() == "target@example.com");
        assert!(updated_user["status"].as_str().unwrap() == "deactivated");
        assert!(updated_user["firstName"].as_str().unwrap() == "Test");
        assert!(updated_user["lastName"].as_str().unwrap() == "User");
        assert!(!updated_user["isOwner"].as_bool().unwrap());
        assert!(updated_user["preferLanguage"].as_str().unwrap() == "en");
        assert!(updated_user["scopes"].is_array());
        assert!(updated_user["roles"].is_object());
        let roles_conn = &updated_user["roles"];
        assert!(roles_conn["edges"].is_array());

        // Verify persisted status change in database
        let db_connection = foundation.open_connection(true).unwrap();
        let status_row: String = db_connection.query_row(
            "SELECT status FROM users WHERE id = ?1 AND deleted_at = 0",
            [target_user_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(status_row, "deactivated");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_create_user_mutation() {
        let db_path = temp_sqlite_path("task9-create-user");
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
        ensure_identity_tables(&connection).unwrap();

        // Create an admin user with write_users scope
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_USERS],
        );

        // Create a role to assign to the new user
        let _project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let role_id = insert_role(&connection, "Test Role", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_SETTINGS]);

        let app = graphql_test_app(foundation.clone(), bootstrap);

        // Sign in as admin
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        // Prepare role GID
        let role_gid = graphql_gid("role", role_id);

        // Mutation: create a new user with scopes and role
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"query": "mutation CreateUser($input: CreateUserInput!) {{ createUser(input: $input) {{ id createdAt updatedAt email status firstName lastName isOwner preferLanguage scopes roles {{ edges {{ node {{ id name }} }} }} }} }}", "variables": {{ "input": {{ "email": "newuser@example.com", "password": "newpass123", "firstName": "New", "lastName": "User", "scopes": ["read_settings"], "roleIDs": ["{}"] }} }}}}"#,
                        role_gid
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;

        assert!(json["data"]["createUser"].is_object());
        let created_user = &json["data"]["createUser"];

        // Verify returned user fields
        assert!(created_user["id"].is_string());
        assert!(created_user["email"].as_str().unwrap() == "newuser@example.com");
        assert!(created_user["status"].as_str().unwrap() == "activated");
        assert!(created_user["firstName"].as_str().unwrap() == "New");
        assert!(created_user["lastName"].as_str().unwrap() == "User");
        assert!(!created_user["isOwner"].as_bool().unwrap());
        assert!(created_user["preferLanguage"].as_str().unwrap() == "en");
        assert!(created_user["scopes"].is_array());
        let scopes: Vec<&str> = created_user["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(scopes.contains(&"read_settings"));
        assert!(created_user["roles"].is_object());
        let roles_conn = &created_user["roles"];
        assert!(roles_conn["edges"].is_array());
        let role_edges = roles_conn["edges"].as_array().unwrap();
        assert!(!role_edges.is_empty());
        for role_edge in role_edges {
            assert!(role_edge["node"].is_object());
            let role_node = &role_edge["node"];
            assert!(role_node["id"].is_string());
            assert!(role_node["name"].is_string());
        }

        // Verify persisted user in database
        let db_connection = foundation.open_connection(true).unwrap();
        let user_row: (i64, String, String, String, i64, String, String, String, String, String) = db_connection.query_row(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE email = ?1 AND deleted_at = 0",
            ["newuser@example.com"],
            |row| Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?
            )),
        ).unwrap();

        let (user_id, email, first_name, last_name, is_owner_i64, prefer_language, status, _created_at, _updated_at, scopes_json) = user_row;
        assert_eq!(email, "newuser@example.com");
        assert_eq!(first_name, "New");
        assert_eq!(last_name, "User");
        assert_eq!(is_owner_i64, 0);
        assert_eq!(prefer_language, "en");
        assert_eq!(status, "activated");
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap();
        assert_eq!(scopes, vec!["read_settings"]);

        // Verify password is hashed (not plaintext)
        let password_hash: String = db_connection.query_row(
            "SELECT password FROM users WHERE id = ?1",
            [user_id],
            |row| row.get(0),
        ).unwrap();
        assert_ne!(password_hash, "newpass123"); // Should be hashed, not plaintext

        // Verify role assignment
        let role_count: i64 = db_connection.query_row(
            "SELECT COUNT(*) FROM user_roles WHERE user_id = ?1 AND role_id = ?2",
            [user_id, role_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(role_count, 1);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_update_user_mutation() {
        let db_path = temp_sqlite_path("task9-update-user");
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
        ensure_identity_tables(&connection).unwrap();

        // Create an admin user with write_users scope
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_USERS],
        );

        // Create a target user with initial data
        let target_user_id = insert_test_user(
            &connection,
            "target@example.com",
            "password123",
            &[],
        );

        // Create two roles: one to replace, one to assign
        let old_role_id = insert_role(&connection, "Old Role", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_SETTINGS]);
        let new_role_id = insert_role(&connection, "New Role", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_CHANNELS]);

        // Assign old role to target user
        connection.execute(
            "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
            params![target_user_id, old_role_id],
        ).unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);

        // Sign in as admin
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let target_gid = graphql_gid("user", target_user_id);
        let new_role_gid = graphql_gid("role", new_role_id);

        // Mutation: update user fields, scopes, and replace role
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"query": "mutation UpdateUser($id: ID!, $input: UpdateUserInput!) {{ updateUser(id: $id, input: $input) {{ id createdAt updatedAt email status firstName lastName isOwner preferLanguage scopes roles {{ edges {{ node {{ id name }} }} }} }} }}", "variables": {{ "id": "{}", "input": {{ "firstName": "Updated", "lastName": "Name", "preferLanguage": "fr", "avatar": "https://example.com/avatar.jpg", "scopes": ["read_channels", "write_settings"], "roleIDs": ["{}"] }} }}}}"#,
                        target_gid, new_role_gid
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;

        assert!(json["data"]["updateUser"].is_object());
        let updated_user = &json["data"]["updateUser"];

        // Verify returned user fields
        assert!(updated_user["id"].is_string());
        assert_eq!(updated_user["id"].as_str().unwrap(), target_gid);
        assert!(updated_user["email"].as_str().unwrap() == "target@example.com");
        assert!(updated_user["status"].as_str().unwrap() == "activated");
        assert!(updated_user["firstName"].as_str().unwrap() == "Updated");
        assert!(updated_user["lastName"].as_str().unwrap() == "Name");
        assert!(!updated_user["isOwner"].as_bool().unwrap());
        assert!(updated_user["preferLanguage"].as_str().unwrap() == "fr");
        assert!(updated_user["scopes"].is_array());
        let scopes: Vec<&str> = updated_user["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(scopes.contains(&"read_channels"));
        assert!(scopes.contains(&"write_settings"));
        assert!(updated_user["roles"].is_object());
        let roles_conn = &updated_user["roles"];
        assert!(roles_conn["edges"].is_array());
        let role_edges = roles_conn["edges"].as_array().unwrap();
        assert_eq!(role_edges.len(), 1);
        assert_eq!(role_edges[0]["node"]["id"].as_str().unwrap(), new_role_gid);
        assert_eq!(role_edges[0]["node"]["name"].as_str().unwrap(), "New Role");

        // Verify persisted changes in database
        let db_connection = foundation.open_connection(true).unwrap();
        let user_row: (String, String, String, i64, String, String, String, String, String) = db_connection.query_row(
            "SELECT email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
            [target_user_id],
            |row| Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?
            )),
        ).unwrap();

        let (email, first_name, last_name, is_owner_i64, prefer_language, status, _created_at, _updated_at, scopes_json) = user_row;
        assert_eq!(email, "target@example.com");
        assert_eq!(first_name, "Updated");
        assert_eq!(last_name, "Name");
        assert_eq!(is_owner_i64, 0);
        assert_eq!(prefer_language, "fr");
        assert_eq!(status, "activated");
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap();
        assert_eq!(scopes, vec!["read_channels", "write_settings"]);

        // Verify role replacement: old role removed, new role present
        let role_count_old: i64 = db_connection.query_row(
            "SELECT COUNT(*) FROM user_roles WHERE user_id = ?1 AND role_id = ?2",
            [target_user_id, old_role_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(role_count_old, 0);

        let role_count_new: i64 = db_connection.query_row(
            "SELECT COUNT(*) FROM user_roles WHERE user_id = ?1 AND role_id = ?2",
            [target_user_id, new_role_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(role_count_new, 1);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_update_me_mutation() {
        let db_path = temp_sqlite_path("task9-update-me");
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
        ensure_identity_tables(&connection).unwrap();
        let user_id = insert_test_user(
            &connection,
            "testuser@example.com",
            "password123",
            &[SCOPE_READ_SETTINGS],
        );
        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        insert_project_membership(&connection, user_id, project_id, false, &[SCOPE_READ_REQUESTS]);

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "testuser@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateMe($input: UpdateMeInput!) { updateMe(input: $input) { id email firstName lastName isOwner preferLanguage avatar scopes projects { projectID } } }",
                            "variables": {
                                "input": {
                                    "firstName": "Updated",
                                    "lastName": "Profile",
                                    "preferLanguage": "fr",
                                    "avatar": "https://example.com/avatar.png"
                                }
                            }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        let updated = &json["data"]["updateMe"];
        assert_eq!(updated["email"], "testuser@example.com");
        assert_eq!(updated["firstName"], "Updated");
        assert_eq!(updated["lastName"], "Profile");
        assert_eq!(updated["preferLanguage"], "fr");
        assert_eq!(updated["avatar"], "https://example.com/avatar.png");
        assert_eq!(updated["projects"][0]["projectID"], graphql_gid("project", project_id));

        let row: (String, String, String, String) = foundation
            .open_connection(true)
            .unwrap()
            .query_row(
                "SELECT first_name, last_name, prefer_language, avatar FROM users WHERE id = ?1",
                [user_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "Updated");
        assert_eq!(row.1, "Profile");
        assert_eq!(row.2, "fr");
        assert_eq!(row.3, "https://example.com/avatar.png");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_update_storage_policy_mutation() {
        let db_path = temp_sqlite_path("task9-update-storage-policy");
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
        ensure_identity_tables(&connection).unwrap();
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateStoragePolicy($input: UpdateStoragePolicyInput!) { updateStoragePolicy(input: $input) }",
                            "variables": {
                                "input": {
                                    "storeChunks": true,
                                    "storeRequestBody": false,
                                    "cleanupOptions": [
                                        {"resourceType": "requests", "enabled": true, "cleanupDays": 7},
                                        {"resourceType": "usage_logs", "enabled": true, "cleanupDays": 14}
                                    ]
                                }
                            }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert_eq!(json["data"]["updateStoragePolicy"], true);

        let policy = SqliteOperationalService::new(foundation.clone())
            .storage_policy()
            .unwrap();
        assert!(policy.store_chunks);
        assert!(!policy.store_request_body);
        assert!(policy.store_response_body);
        assert_eq!(policy.cleanup_options.len(), 2);
        assert_eq!(policy.cleanup_options[0].resource_type, "requests");
        assert!(policy.cleanup_options[0].enabled);
        assert_eq!(policy.cleanup_options[0].cleanup_days, 7);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_update_system_channel_settings_mutation() {
        let db_path = temp_sqlite_path("task9-update-system-channel-settings");
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
        ensure_identity_tables(&connection).unwrap();
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateSystemChannelSettings($input: UpdateSystemChannelSettingsInput!) { updateSystemChannelSettings(input: $input) }",
                            "variables": {
                                "input": {
                                    "probe": {
                                        "enabled": false,
                                        "frequency": "ONE_HOUR"
                                    }
                                }
                            }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert_eq!(json["data"]["updateSystemChannelSettings"], true);

        let settings = SqliteOperationalService::new(foundation.clone())
            .system_channel_settings()
            .unwrap();
        assert!(!settings.probe.enabled);
        assert_eq!(settings.probe.frequency, super::admin::ProbeFrequencySetting::OneHour);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_check_provider_quotas_mutation() {
        let db_path = temp_sqlite_path("task9-check-provider-quotas");
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
        ensure_identity_tables(&connection).unwrap();
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Codex OAuth",
                channel_type: "codex",
                base_url: "https://example.com/v1",
                status: "enabled",
                credentials_json: "{}",
                supported_models_json: "[]",
                auto_sync_supported_models: false,
                default_test_model: "",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "quota test",
            })
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { checkProviderQuotas }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert_eq!(json["data"]["checkProviderQuotas"], true);

        let quota_status = SqliteOperationalService::new(foundation.clone())
            .provider_quota_statuses()
            .unwrap();
        assert_eq!(quota_status.len(), 1);
        assert_eq!(quota_status[0].provider_type, "codex");
        assert_eq!(quota_status[0].status, "unknown");
        assert!(!quota_status[0].ready);
        assert!(quota_status[0]
            .quota_data_json
            .contains("remain unsupported in the Rust slice"));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_trigger_gc_cleanup_mutation() {
        let db_path = temp_sqlite_path("task9-trigger-gc-cleanup");
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
        ensure_identity_tables(&connection).unwrap();
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { triggerGcCleanup }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert_eq!(json["data"]["triggerGcCleanup"], true);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_update_auto_backup_settings_and_trigger_auto_backup_mutations() {
        let db_path = temp_sqlite_path("task9-auto-backup-mutations");
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
        ensure_identity_tables(&connection).unwrap();
        let owner_id = insert_test_user(
            &connection,
            "owner2@example.com",
            "password123",
            &[],
        );
        connection
            .execute("UPDATE users SET is_owner = 1 WHERE id = ?1", [owner_id])
            .unwrap();
        let _non_owner_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );

        let backup_root = std::env::temp_dir().join(format!(
            "axonhub-task9-auto-backup-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&backup_root).unwrap();

        let storage_id = foundation
            .data_storages()
            .find_primary_active_storage()
            .unwrap()
            .unwrap()
            .id
            + 100;
        connection
            .execute(
                "INSERT INTO data_storages (id, name, description, \"primary\", type, settings, status, deleted_at)
                 VALUES (?1, ?2, ?3, 0, 'fs', ?4, 'active', 0)",
                params![
                    storage_id,
                    "Task9 Backup FS",
                    "task9 backup",
                    serde_json::json!({"directory": backup_root.to_string_lossy()}).to_string(),
                ],
            )
            .unwrap();

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Backup Channel",
                channel_type: "openai",
                base_url: "https://example.com/v1",
                status: "enabled",
                credentials_json: "{}",
                supported_models_json: "[]",
                auto_sync_supported_models: false,
                default_test_model: "",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "task9 backup",
            })
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let owner_token = signin_token(foundation.clone(), "owner2@example.com", "password123");

        let update_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {owner_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{
                            "query": "mutation UpdateAutoBackupSettings($input: UpdateAutoBackupSettingsInput!) {{ updateAutoBackupSettings(input: $input) }}",
                            "variables": {{
                                "input": {{
                                    "enabled": true,
                                    "frequency": "daily",
                                    "dataStorageID": {},
                                    "includeChannels": true,
                                    "includeModels": false,
                                    "includeAPIKeys": false,
                                    "includeModelPrices": false,
                                    "retentionDays": 2
                                }}
                            }}
                        }}"#,
                        storage_id
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        let update_json = read_json_response(update_response).await;
        assert_eq!(update_json["data"]["updateAutoBackupSettings"], true);

        let settings = SqliteOperationalService::new(foundation.clone())
            .auto_backup_settings()
            .unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.data_storage_id, storage_id);
        assert_eq!(settings.retention_days, 2);

        let trigger_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {owner_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { triggerAutoBackup { success message } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let trigger_json = read_json_response(trigger_response).await;
        assert_eq!(trigger_json["data"]["triggerAutoBackup"]["success"], true);
        assert_eq!(
            trigger_json["data"]["triggerAutoBackup"]["message"],
            "Backup completed successfully"
        );

        let files = std::fs::read_dir(&backup_root)
            .unwrap()
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        assert!(!files.is_empty());

        fs::remove_dir_all(backup_root).ok();
        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_rejects_invalid_or_unauthorized_task9_mutations_without_side_effects() {
        let db_path = temp_sqlite_path("task9-mutation-errors");
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
        ensure_identity_tables(&connection).unwrap();
        let _scoped_user_id = insert_test_user(
            &connection,
            "scoped@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );
        let _no_scope_user_id = insert_test_user(
            &connection,
            "viewer@example.com",
            "password123",
            &[],
        );
        let owner_id = insert_test_user(
            &connection,
            "owner2@example.com",
            "password123",
            &[],
        );
        connection
            .execute("UPDATE users SET is_owner = 1 WHERE id = ?1", [owner_id])
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let scoped_token = signin_token(foundation.clone(), "scoped@example.com", "password123");
        let no_scope_token = signin_token(foundation.clone(), "viewer@example.com", "password123");
        let owner_token = signin_token(foundation.clone(), "owner2@example.com", "password123");

        let denied_backup_update = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {scoped_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateAutoBackupSettings($input: UpdateAutoBackupSettingsInput!) { updateAutoBackupSettings(input: $input) }",
                            "variables": { "input": { "enabled": true, "dataStorageID": 1 } }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_backup_json = read_json_response(denied_backup_update).await;
        assert_eq!(denied_backup_json["data"]["updateAutoBackupSettings"], Value::Null);
        assert_eq!(
            denied_backup_json["errors"][0]["message"],
            "permission denied: owner access required"
        );

        let default_settings = SqliteOperationalService::new(foundation.clone())
            .auto_backup_settings()
            .unwrap();
        assert!(!default_settings.enabled);
        assert_eq!(default_settings.data_storage_id, 0);

        let invalid_backup_update = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {owner_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateAutoBackupSettings($input: UpdateAutoBackupSettingsInput!) { updateAutoBackupSettings(input: $input) }",
                            "variables": { "input": { "enabled": true, "dataStorageID": 0 } }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let invalid_backup_json = read_json_response(invalid_backup_update).await;
        assert_eq!(invalid_backup_json["data"]["updateAutoBackupSettings"], Value::Null);
        assert_eq!(
            invalid_backup_json["errors"][0]["message"],
            "dataStorageID is required when auto backup is enabled"
        );

        let denied_trigger_backup = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {scoped_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { triggerAutoBackup { success message } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_trigger_backup_json = read_json_response(denied_trigger_backup).await;
        assert_eq!(denied_trigger_backup_json["data"]["triggerAutoBackup"], Value::Null);
        assert_eq!(
            denied_trigger_backup_json["errors"][0]["message"],
            "permission denied: owner access required"
        );

        let denied_gc = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {no_scope_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { triggerGcCleanup }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_gc_json = read_json_response(denied_gc).await;
        assert_eq!(denied_gc_json["data"]["triggerGcCleanup"], Value::Null);
        assert_eq!(
            denied_gc_json["errors"][0]["message"],
            "permission denied: requires write:settings scope"
        );

        let denied_update_me = app
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {owner_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateMe($input: UpdateMeInput!) { updateMe(input: $input) { id } }",
                            "variables": { "input": {} }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_update_me_json = read_json_response(denied_update_me).await;
        assert_eq!(denied_update_me_json["data"]["updateMe"], Value::Null);
        assert_eq!(denied_update_me_json["errors"][0]["message"], "no fields to update");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_rolls_back_create_user_when_role_assignment_fails() {
        let db_path = temp_sqlite_path("task9-create-user-rollback");
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
        ensure_identity_tables(&connection).unwrap();
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_USERS],
        );
        let valid_role_id = insert_role(&connection, "Rollback Role", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_SETTINGS]);

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");
        let valid_role_gid = graphql_gid("role", valid_role_id);
        let invalid_role_gid = graphql_gid("role", valid_role_id);

        let baseline_user_count: i64 = foundation
            .open_connection(true)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM users WHERE deleted_at = 0", [], |row| row.get(0))
            .unwrap();
        let baseline_user_role_count: i64 = foundation
            .open_connection(true)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM user_roles", [], |row| row.get(0))
            .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{
                            "query": "mutation CreateUser($input: CreateUserInput!) {{ createUser(input: $input) {{ id }} }}",
                            "variables": {{
                                "input": {{
                                    "email": "rollback-create@example.com",
                                    "password": "newpass123",
                                    "firstName": "Rollback",
                                    "lastName": "Create",
                                    "roleIDs": ["{}", "{}"]
                                }}
                            }}
                        }}"#,
                        valid_role_gid, invalid_role_gid
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert_eq!(json["data"]["createUser"], Value::Null);
        assert_eq!(
            json["errors"][0]["message"],
            "failed to assign user role: UNIQUE constraint failed: user_roles.user_id, user_roles.role_id"
        );

        let verification_connection = foundation.open_connection(true).unwrap();
        let post_user_count: i64 = verification_connection
            .query_row("SELECT COUNT(*) FROM users WHERE deleted_at = 0", [], |row| row.get(0))
            .unwrap();
        let post_user_role_count: i64 = verification_connection
            .query_row("SELECT COUNT(*) FROM user_roles", [], |row| row.get(0))
            .unwrap();
        let created_user_count: i64 = verification_connection
            .query_row(
                "SELECT COUNT(*) FROM users WHERE email = ?1 AND deleted_at = 0",
                ["rollback-create@example.com"],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(post_user_count, baseline_user_count);
        assert_eq!(post_user_role_count, baseline_user_role_count);
        assert_eq!(created_user_count, 0);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_rolls_back_update_user_when_role_replacement_fails() {
        let db_path = temp_sqlite_path("task9-update-user-rollback");
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
        ensure_identity_tables(&connection).unwrap();
        let _admin_id = insert_test_user(
            &connection,
            "admin@example.com",
            "password123",
            &[SCOPE_WRITE_USERS],
        );
        let target_user_id = insert_test_user(
            &connection,
            "rollback-target@example.com",
            "password123",
            &[],
        );
        let old_role_id = insert_role(&connection, "Old Rollback Role", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_SETTINGS]);
        let new_role_id = insert_role(&connection, "New Rollback Role", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_CHANNELS]);
        attach_role(&connection, target_user_id, old_role_id);

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");
        let target_gid = graphql_gid("user", target_user_id);
        let new_role_gid = graphql_gid("role", new_role_id);

        let baseline_row: (String, String, String) = foundation
            .open_connection(true)
            .unwrap()
            .query_row(
                "SELECT first_name, prefer_language, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
                [target_user_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let baseline_role_pairs: Vec<(i64, i64)> = {
            let verification_connection = foundation.open_connection(true).unwrap();
            let mut statement = verification_connection
                .prepare("SELECT user_id, role_id FROM user_roles WHERE user_id = ?1 ORDER BY role_id ASC")
                .unwrap();
            statement
                .query_map([target_user_id], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{
                            "query": "mutation UpdateUser($id: ID!, $input: UpdateUserInput!) {{ updateUser(id: $id, input: $input) {{ id }} }}",
                            "variables": {{
                                "id": "{}",
                                "input": {{
                                    "firstName": "ShouldRollback",
                                    "preferLanguage": "fr",
                                    "scopes": ["read_channels"],
                                    "roleIDs": ["{}", "{}"]
                                }}
                            }}
                        }}"#,
                        target_gid, new_role_gid, new_role_gid
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert_eq!(json["data"]["updateUser"], Value::Null);
        assert_eq!(
            json["errors"][0]["message"],
            "failed to replace user role assignments: UNIQUE constraint failed: user_roles.user_id, user_roles.role_id"
        );

        let verification_connection = foundation.open_connection(true).unwrap();
        let post_row: (String, String, String) = verification_connection
            .query_row(
                "SELECT first_name, prefer_language, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
                [target_user_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let post_role_pairs: Vec<(i64, i64)> = {
            let mut statement = verification_connection
                .prepare("SELECT user_id, role_id FROM user_roles WHERE user_id = ?1 ORDER BY role_id ASC")
                .unwrap();
            statement
                .query_map([target_user_id], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };

        assert_eq!(post_row, baseline_row);
        assert_eq!(post_role_pairs, baseline_role_pairs);

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn admin_request_content_download_enforces_project_scope_and_path_safety() {
        let db_path = temp_sqlite_path("task13-request-content");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let admin = SqliteAdminService::new(foundation.clone());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let content_dir = std::env::temp_dir().join(format!(
            "axonhub-task13-content-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&content_dir).unwrap();

        let storage_id = foundation
            .data_storages()
            .find_primary_active_storage()
            .unwrap()
            .unwrap()
            .id
            + 100;
        let connection = foundation.open_connection(true).unwrap();
        connection
            .execute(
                "INSERT INTO data_storages (id, name, description, \"primary\", type, settings, status, deleted_at)
                 VALUES (?1, ?2, ?3, 0, 'fs', ?4, 'active', 0)",
                params![
                    storage_id,
                    "Task13 FS",
                    "task13",
                    serde_json::json!({"directory": content_dir.to_string_lossy()}).to_string(),
                ],
            )
            .unwrap();

        let request_id = foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(1),
                project_id,
                trace_id: None,
                data_storage_id: Some(storage_id),
                source: "api",
                model_id: "gpt-4o",
                format: "openai/video",
                request_headers_json: "{}",
                request_body_json: "{}",
                response_body_json: None,
                response_chunks_json: None,
                channel_id: None,
                external_id: None,
                status: "completed",
                stream: false,
                client_ip: "",
                metrics_latency_ms: None,
                metrics_first_token_latency_ms: None,
                content_saved: true,
                content_storage_id: Some(storage_id),
                content_storage_key: Some("/1/requests/1/video/video.mp4"),
                content_saved_at: Some("2026-03-23T00:00:00Z"),
            })
            .unwrap();

        let real_key = format!("/{project_id}/requests/{request_id}/video/video.mp4");
        connection
            .execute(
                "UPDATE requests SET content_storage_key = ?2 WHERE id = ?1",
                params![request_id, real_key],
            )
            .unwrap();
        let full_path = content_dir.join(format!("{project_id}/requests/{request_id}/video/video.mp4"));
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        fs::write(&full_path, b"video-content").unwrap();

        let downloaded = admin
            .download_request_content(project_id, request_id, test_admin_user())
            .unwrap();
        assert_eq!(downloaded.filename, "video.mp4");
        assert_eq!(downloaded.bytes, b"video-content");

        let wrong_project = admin
            .download_request_content(project_id + 1, request_id, test_admin_user())
            .unwrap_err();
        assert!(matches!(wrong_project, AdminError::NotFound { .. }));

        connection
            .execute(
                "UPDATE requests SET content_storage_key = '/../../etc/passwd' WHERE id = ?1",
                [request_id],
            )
            .unwrap();
        let traversal = admin
            .download_request_content(project_id, request_id, test_admin_user())
            .unwrap_err();
        assert!(matches!(traversal, AdminError::NotFound { .. }));

        fs::remove_dir_all(content_dir).ok();
        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_routes_complete_persistence_and_keep_residual_501() {
        let db_path = temp_sqlite_path("task7-openai-runtime");
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

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Mock",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 7 runtime test",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0,"cacheRead":0.5,"cacheWrite":0.25,"cacheWrite5m":0.125},"vision":true,"toolCall":true,"reasoning":{"supported":true},"costPriceReferenceId":"price-ref-task9"}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 7 runtime test",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        });

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
        let models_json = read_json_response(models_response).await;
        assert_eq!(models_json["data"][0]["id"], "gpt-4o");

        for (path, body) in [
            (
                "/v1/chat/completions",
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
            ),
            (
                "/v1/responses",
                r#"{"model":"gpt-4o","input":"hi"}"#,
            ),
            (
                "/v1/embeddings",
                r#"{"model":"gpt-4o","input":"hi"}"#,
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
                        .header("AH-Thread-Id", "thread-task7")
                        .header("AH-Trace-Id", "trace-task7")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
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

        let connection = foundation.open_connection(false).unwrap();
        let request_statuses: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT status FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_statuses, vec!["completed", "completed", "completed"]);

        let request_trace_channels: Vec<(i64, i64)> = {
            let mut statement = connection
                .prepare("SELECT trace_id, channel_id FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_trace_channels.len(), 3);
        assert!(request_trace_channels.iter().all(|(trace_id, _)| *trace_id > 0));
        let first_trace_id = request_trace_channels[0].0;
        assert!(request_trace_channels
            .iter()
            .all(|(trace_id, channel_id)| *trace_id == first_trace_id && *channel_id > 0));

        let trace_thread_link: (String, i64) = connection
            .query_row(
                "SELECT t.trace_id, t.thread_id FROM traces t ORDER BY id ASC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(trace_thread_link.0, "trace-task7");
        assert!(trace_thread_link.1 > 0);

        let thread_id: String = connection
            .query_row("SELECT thread_id FROM threads ORDER BY id ASC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(thread_id, "thread-task7");

        let execution_statuses: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT status FROM request_executions ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(execution_statuses, vec!["completed", "completed", "completed"]);

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 3);

        let usage_rows: Vec<(String, i64, i64, i64, i64, i64, i64, i64, i64, i64, f64, String, String)> = {
            let mut statement = connection
                .prepare(
                    "SELECT format, prompt_tokens, completion_tokens, total_tokens,
                            prompt_cached_tokens, prompt_write_cached_tokens,
                            prompt_write_cached_tokens_5m, completion_reasoning_tokens,
                            completion_accepted_prediction_tokens,
                            completion_rejected_prediction_tokens,
                            total_cost, cost_price_reference_id, cost_items
                     FROM usage_logs ORDER BY id ASC",
                )
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                    ))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(usage_rows.len(), 3);
        let responses_usage = &usage_rows[1];
        assert_eq!(responses_usage.1, 12);
        assert_eq!(responses_usage.2, 4);
        assert_eq!(responses_usage.3, 16);
        assert_eq!(responses_usage.4, 3);
        assert_eq!(responses_usage.5, 4);
        assert_eq!(responses_usage.6, 4);
        assert_eq!(responses_usage.7, 1);
        assert_eq!(responses_usage.8, 2);
        assert_eq!(responses_usage.9, 3);
        assert!((responses_usage.10 - 0.000015).abs() < 1e-12);
        assert_eq!(responses_usage.11, "price-ref-task9");
        assert!(responses_usage.12.contains("\"itemCode\":\"prompt_tokens\""));
        assert!(responses_usage.12.contains("\"itemCode\":\"prompt_write_cached_tokens\""));
        assert!(responses_usage
            .12
            .contains("\"promptWriteCacheVariantCode\":\"five_min\""));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_failure_persists_terminal_request_and_execution_state() {
        let db_path = temp_sqlite_path("task9-openai-failure-persistence");
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

        let failing_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Task9 Failure",
                channel_type: "openai",
                base_url: format!("{}/primary-fail", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 9 failure channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 9 failure model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task9-failure")
                    .header("AH-Trace-Id", "trace-task9-failure")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"fail me"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = read_json_response(response).await;
        assert_eq!(json["error"]["message"], "primary unavailable");

        let connection = foundation.open_connection(false).unwrap();
        let request_row: (String, i64, String) = connection
            .query_row(
                "SELECT status, channel_id, response_body FROM requests ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(request_row.0, "failed");
        assert_eq!(request_row.1, failing_channel_id);
        assert!(request_row.2.contains("primary unavailable"));

        let execution_row: (String, i64, String, i64, String) = connection
            .query_row(
                "SELECT status, channel_id, error_message, response_status_code, response_body
                 FROM request_executions ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(execution_row.0, "failed");
        assert_eq!(execution_row.1, failing_channel_id);
        assert_eq!(execution_row.2, "primary unavailable");
        assert_eq!(execution_row.3, 503);
        assert!(execution_row.4.contains("primary unavailable"));

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 0);

        let request_trace_id: i64 = connection
            .query_row("SELECT trace_id FROM requests ORDER BY id DESC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let trace_thread: (String, i64) = connection
            .query_row(
                "SELECT trace_id, thread_id FROM traces WHERE id = ?1",
                [request_trace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(trace_thread.0, "trace-task9-failure");
        assert!(trace_thread.1 > 0);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_fails_over_to_backup_channel_when_primary_fails() {
        let db_path = temp_sqlite_path("task8-openai-failover");
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

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Primary Fail",
                channel_type: "openai",
                base_url: format!("{}/primary-fail", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 200,
                error_message: "",
                remark: "Task 8 failover primary",
            })
            .unwrap();
        let backup_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Backup Healthy",
                channel_type: "openai",
                base_url: format!("{}/backup", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 failover backup",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0,"cache_read":0.5,"cache_write":0.25}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 8 failover model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task8-failover")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json_response(response).await;
        assert_eq!(json["id"], "chatcmpl_backup");

        let connection = foundation.open_connection(false).unwrap();
        let request_channel_id: i64 = connection
            .query_row("SELECT channel_id FROM requests ORDER BY id DESC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(request_channel_id, backup_channel_id);

        let execution_statuses: Vec<(i64, String)> = {
            let mut statement = connection
                .prepare("SELECT channel_id, status FROM request_executions ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(execution_statuses.len(), 2);
        assert_eq!(execution_statuses[0].1, "failed");
        assert_eq!(execution_statuses[1], (backup_channel_id, "completed".to_owned()));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_reuses_same_channel_for_repeated_trace_when_both_healthy() {
        let db_path = temp_sqlite_path("task8-openai-trace-affinity");
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

        let preferred_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Affinity A",
                channel_type: "openai",
                base_url: format!("{}/affinity-a", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 affinity preferred",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Affinity B",
                channel_type: "openai",
                base_url: format!("{}/affinity-b", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 affinity backup",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 8 affinity model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        });

        for expected_id in ["chatcmpl_affinity_a", "chatcmpl_affinity_a"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/chat/completions")
                        .method(Method::POST)
                        .header("content-type", "application/json")
                        .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                        .header("X-Project-ID", "gid://axonhub/project/1")
                        .header("AH-Trace-Id", "trace-task8-affinity")
                        .body(Body::from(
                            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hello"}]}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let json = read_json_response(response).await;
            assert_eq!(json["id"], expected_id);
        }

        let connection = foundation.open_connection(false).unwrap();
        let request_channel_ids: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT channel_id FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_channel_ids, vec![preferred_channel_id, preferred_channel_id]);

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn openai_v1_prefers_requested_channel_hint_over_higher_priority_channel() {
        let db_path = temp_sqlite_path("task16-openai-channel-hint");
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

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Priority Default",
                channel_type: "openai",
                base_url: format!("{}/affinity-a", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 200,
                error_message: "",
                remark: "Task 16 higher-priority default channel",
            })
            .unwrap();
        let requested_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Priority Requested",
                channel_type: "openai",
                base_url: format!("{}/compressed", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 16 requested channel override",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 16 requested model",
            })
            .unwrap();

        let project = foundation.identities().find_project_by_id(1).unwrap();
        let service = SqliteOpenAiV1Service::new(foundation.clone());
        let response = service
            .execute(
                OpenAiV1Route::ChatCompletions,
                OpenAiV1ExecutionRequest {
                    headers: HashMap::new(),
                    body: serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": "hello"}]
                    }),
                    path: "/admin/playground/chat".to_owned(),
                    path_params: HashMap::new(),
                    query: HashMap::new(),
                    project: ProjectContext {
                        id: project.id,
                        name: project.name,
                        status: project.status,
                    },
                    trace: None,
                    api_key_id: None,
                    client_ip: None,
                    channel_hint_id: Some(requested_channel_id),
                },
            )
            .unwrap();

        assert_eq!(response.status, StatusCode::OK.as_u16());
        assert_eq!(response.body["id"], "chatcmpl_compressed");

        let persisted_channel_id: i64 = foundation
            .open_connection(false)
            .unwrap()
            .query_row(
                "SELECT channel_id FROM requests ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_channel_id, requested_channel_id);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_does_not_pin_later_healthy_non_affinity_requests_to_prior_failover_backup() {
        let db_path = temp_sqlite_path("task8-openai-failover-selection-repair");
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

        let _failover_primary_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Primary Fail",
                channel_type: "openai",
                base_url: format!("{}/primary-fail", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 200,
                error_message: "",
                remark: "Task 8 repair failing primary",
            })
            .unwrap();
        let failover_backup_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Backup Healthy",
                channel_type: "openai",
                base_url: format!("{}/backup", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 repair failover backup",
            })
            .unwrap();
        let healthy_affinity_a_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Healthy A",
                channel_type: "openai",
                base_url: format!("{}/affinity-a", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 repair healthy affinity A",
            })
            .unwrap();
        let healthy_affinity_b_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Healthy B",
                channel_type: "openai",
                base_url: format!("{}/affinity-b", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 repair healthy affinity B",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 8 repair model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        });

        let failover_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task8-repair-failover")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"failover first"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(failover_response.status(), StatusCode::OK);
        let failover_json = read_json_response(failover_response).await;
        assert_eq!(failover_json["id"], "chatcmpl_backup");

        let healthy_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task8-repair-healthy")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"healthy later"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(healthy_response.status(), StatusCode::OK);
        let healthy_json = read_json_response(healthy_response).await;
        assert_eq!(healthy_json["id"], "chatcmpl_affinity_a");

        let connection = foundation.open_connection(false).unwrap();
        let request_channel_ids: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT channel_id FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_channel_ids, vec![failover_backup_id, healthy_affinity_a_id]);
        assert_ne!(request_channel_ids[1], failover_backup_id);
        assert_ne!(request_channel_ids[1], healthy_affinity_b_id);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn gemini_and_doubao_wrappers_use_shared_core_and_keep_neighboring_truthful_501() {
        let db_path = temp_sqlite_path("task12-gemini-doubao");
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

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task12 Shared Core",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gemini-2.5-flash","seedance-1.0"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gemini-2.5-flash",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 12 shared core channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task12 Wrong Channel",
                channel_type: "openai",
                base_url: "http://127.0.0.1:1/v1",
                status: "enabled",
                credentials_json: r#"{"apiKey":"wrong-key"}"#,
                supported_models_json: r#"["seedance-1.0"]"#,
                auto_sync_supported_models: false,
                default_test_model: "seedance-1.0",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 10,
                error_message: "",
                remark: "Task 12 wrong Doubao channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "google",
                model_id: "gemini-2.5-flash",
                model_type: "chat",
                name: "Gemini 2.5 Flash",
                icon: "Gemini",
                group: "gemini",
                model_card_json: r#"{"limit":{"context":1048576,"output":8192}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 12 Gemini model",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "bytedance",
                model_id: "seedance-1.0",
                model_type: "video",
                name: "Seedance 1.0",
                icon: "Doubao",
                group: "doubao",
                model_card_json: r#"{}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 12 Doubao model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            identity: IdentityCapability::Available {
                identity: Arc::new(SqliteIdentityService::new(foundation.clone(), false)),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(SqliteRequestContextService::new(
                    foundation.clone(),
                    false,
                )),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
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
        });

        let gemini_models = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gemini_models.status(), StatusCode::OK);
        let gemini_models_json = read_json_response(gemini_models).await;
        assert_eq!(gemini_models_json["models"][0]["name"], "models/gemini-2.5-flash");

        let gemini_generate = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models/gemini-2.5-flash:generateContent?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gemini_generate.status(), StatusCode::OK);
        let gemini_generate_json = read_json_response(gemini_generate).await;
        assert_eq!(gemini_generate_json["candidates"][0]["content"]["parts"][0]["text"], "hi");

        let gemini_stream = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1beta/models/gemini-2.5-flash:streamGenerateContent?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"contents":[{"role":"user","parts":[{"text":"stream me"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gemini_stream.status(), StatusCode::OK);

        let doubao_create = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::from(r#"{"model":"seedance-1.0","content":[{"type":"text","text":"make a trailer"}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doubao_create.status(), StatusCode::OK);
        let doubao_create_json = read_json_response(doubao_create).await;
        assert_eq!(doubao_create_json["id"], "video_mock_task");

        let doubao_get = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks/video_mock_task")
                    .method(Method::GET)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doubao_get.status(), StatusCode::OK);
        let doubao_get_json = read_json_response(doubao_get).await;
        assert_eq!(doubao_get_json["id"], "video_mock_task");
        assert_eq!(doubao_get_json["content"]["video_url"], "https://example.com/generated.mp4");

        let doubao_delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks/video_mock_task")
                    .method(Method::DELETE)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doubao_delete.status(), StatusCode::OK);

        let unsupported = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/files?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unsupported.status(), StatusCode::NOT_IMPLEMENTED);

        let connection = foundation.open_connection(false).unwrap();
        let request_formats: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT format FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert!(request_formats.contains(&"gemini/generate_content".to_owned()));
        assert!(request_formats.contains(&"gemini/stream_generate_content".to_owned()));
        assert!(request_formats.contains(&"doubao/video_create".to_owned()));
        assert!(request_formats.contains(&"doubao/video_get".to_owned()));
        assert!(request_formats.contains(&"doubao/video_delete".to_owned()));

        let failed_request_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE format IN ('doubao/video_get', 'doubao/video_delete') AND status = 'failed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_request_count, 0);

        let failed_execution_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM request_executions WHERE format IN ('doubao/video_get', 'doubao/video_delete') AND status = 'failed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_execution_count, 0);

        let doubao_channels: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT channel_id FROM requests WHERE format IN ('doubao/video_create', 'doubao/video_get', 'doubao/video_delete') ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(doubao_channels.len(), 3);
        assert!(doubao_channels.iter().all(|channel_id| *channel_id == doubao_channels[0]));

        std::fs::remove_file(db_path).ok();
    }

    fn mock_openai_server_url() -> &'static str {
        static SERVER_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SERVER_URL
            .get_or_init(|| {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                thread::spawn(move || {
                    for stream in listener.incoming() {
                        let mut stream = match stream {
                            Ok(stream) => stream,
                            Err(_) => continue,
                        };
                        let mut buffer = [0_u8; 8192];
                        let size = match stream.read(&mut buffer) {
                            Ok(size) => size,
                            Err(_) => continue,
                        };
                        let request = String::from_utf8_lossy(&buffer[..size]);
                        let request_lower = request.to_ascii_lowercase();
                        let request_line = request.lines().next().unwrap_or_default().to_owned();
                        let method = request_line
                            .split_whitespace()
                            .next()
                            .unwrap_or("GET");
                        let path = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let body = if path.contains("/primary-fail/") && path.ends_with("/chat/completions") {
                            r#"{"error":{"message":"primary unavailable"}}"#
                        } else if path.contains("/compressed/") && path.ends_with("/chat/completions") {
                            if request_lower.contains("accept-encoding: identity") {
                                r#"{"id":"chatcmpl_compressed","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"compressed"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                            } else {
                                r#"{"error":{"message":"identity encoding required"}}"#
                            }
                        } else if method == "GET" && path.ends_with("/videos/video_mock_task") {
                            r#"{"id":"video_mock_task","model":"seedance-1.0","status":"succeeded","content":{"video_url":"https://example.com/generated.mp4"},"created_at":1,"completed_at":2}"#
                        } else if method == "DELETE" && path.ends_with("/videos/video_mock_task") {
                            r#"{"id":"video_mock_task"}"#
                        } else if method == "POST" && path.ends_with("/videos") {
                            r#"{"id":"video_mock_task"}"#
                        } else if path.contains("/backup/") && path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_backup","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"backup"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                        } else if path.contains("/affinity-a/") && path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_affinity_a","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"affinity-a"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                        } else if path.contains("/affinity-b/") && path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_affinity_b","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"affinity-b"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                        } else if path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_mock","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":1}}}"#
                        } else if path.ends_with("/responses") {
                            r#"{"id":"resp_mock","object":"response","created_at":1,"model":"gpt-4o","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi","annotations":[]}],"status":"completed"}],"usage":{"input_tokens":12,"input_tokens_details":{"cached_tokens":3,"write_cached_tokens":4,"write_cached_5min_tokens":4},"output_tokens":4,"output_tokens_details":{"reasoning_tokens":1,"accepted_prediction_tokens":2,"rejected_prediction_tokens":3},"total_tokens":16}}"#
                        } else {
                            r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1,0.2],"index":0}],"model":"gpt-4o","usage":{"prompt_tokens":8,"total_tokens":8}}"#
                        };
                        let status_line = if path.contains("/primary-fail/") && path.ends_with("/chat/completions") {
                            "HTTP/1.1 503 Service Unavailable"
                        } else if path.contains("/compressed/")
                            && path.ends_with("/chat/completions")
                            && !request_lower.contains("accept-encoding: identity")
                        {
                            "HTTP/1.1 500 Internal Server Error"
                        } else if (method == "POST" && path.ends_with("/videos/video_mock_task"))
                            || ((method == "GET" || method == "DELETE") && path.ends_with("/videos"))
                        {
                            "HTTP/1.1 404 Not Found"
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

    #[tokio::test]
    async fn admin_graphql_enforces_system_and_project_scopes() {
        let db_path = temp_sqlite_path("task15-admin-graphql-rbac");
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
        ensure_identity_tables(&connection).unwrap();
        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task15 Channel",
                channel_type: "openai",
                base_url: "https://example.com/v1",
                status: "enabled",
                credentials_json: r#"{"apiKey":"key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "task15",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: "{}",
                settings_json: "{}",
                status: "enabled",
                remark: "task15",
            })
            .unwrap();
        let trace = foundation
            .trace_contexts()
            .get_or_create_trace(project_id, "trace-task15", None)
            .unwrap();
        foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(1),
                project_id,
                trace_id: Some(trace.id),
                data_storage_id: None,
                source: "api",
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_headers_json: "{}",
                request_body_json: "{}",
                response_body_json: None,
                response_chunks_json: None,
                channel_id: None,
                external_id: None,
                status: "completed",
                stream: false,
                client_ip: "",
                metrics_latency_ms: None,
                metrics_first_token_latency_ms: None,
                content_saved: false,
                content_storage_id: None,
                content_storage_key: None,
                content_saved_at: None,
            })
            .unwrap();

        let no_scope_user_id = insert_test_user(&connection, "viewer@example.com", "password123", &[]);
        insert_project_membership(&connection, no_scope_user_id, project_id, false, &[]);

        let system_reader_id = insert_test_user(&connection, "system@example.com", "password123", &[SCOPE_READ_SETTINGS, SCOPE_READ_CHANNELS]);
        insert_project_membership(&connection, system_reader_id, project_id, false, &[]);

        let project_reader_id = insert_test_user(&connection, "project@example.com", "password123", &[]);
        insert_project_membership(&connection, project_reader_id, project_id, false, &[]);
        let project_role_id = insert_role(&connection, "Request Reader", ROLE_LEVEL_PROJECT, project_id, &[SCOPE_READ_REQUESTS]);
        attach_role(&connection, project_reader_id, project_role_id);

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let no_scope_token = signin_token(foundation.clone(), "viewer@example.com", "password123");
        let denied_channels = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {no_scope_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ channels { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied_channels.status(), StatusCode::OK);
        let denied_channels_json = read_json_response(denied_channels).await;
        assert_eq!(denied_channels_json["data"]["channels"], Value::Null);
        assert_eq!(denied_channels_json["errors"][0]["message"], "permission denied");

        let system_token = signin_token(foundation.clone(), "system@example.com", "password123");
        let allowed_channels = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {system_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ systemStatus { isInitialized } channels { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed_channels_json = read_json_response(allowed_channels).await;
        assert_eq!(allowed_channels_json["data"]["systemStatus"]["isInitialized"], true);
        assert_eq!(allowed_channels_json["data"]["channels"][0]["id"], "gid://axonhub/channel/1");

        let project_token = signin_token(foundation.clone(), "project@example.com", "password123");
        let allowed_requests = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {project_token}"))
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ requests { id } traces { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed_requests_json = read_json_response(allowed_requests).await;
        assert_eq!(allowed_requests_json["data"]["requests"][0]["id"], "gid://axonhub/request/1");
        assert_eq!(allowed_requests_json["data"]["traces"][0]["id"], "gid://axonhub/trace/1");

        let denied_requests = app
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {system_token}"))
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ requests { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_requests_json = read_json_response(denied_requests).await;
        assert_eq!(denied_requests_json["data"]["requests"], Value::Null);
        assert_eq!(denied_requests_json["errors"][0]["message"], "permission denied");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openapi_create_llm_api_key_requires_write_api_keys_scope_and_service_account() {
        let db_path = temp_sqlite_path("task15-openapi-rbac");
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
                "UPDATE api_keys SET scopes = ?2 WHERE key = ?1",
                params![
                    DEFAULT_SERVICE_API_KEY_VALUE,
                    serialize_scope_slugs(&[SCOPE_WRITE_API_KEYS]).unwrap()
                ],
            )
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {DEFAULT_SERVICE_API_KEY_VALUE}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { createLLMAPIKey(name: \"SDK Key\") { name scopes } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed_json = read_json_response(allowed).await;
        assert_eq!(allowed_json["data"]["createLLMAPIKey"]["name"], "SDK Key");
        assert_eq!(
            allowed_json["data"]["createLLMAPIKey"]["scopes"][0],
            SCOPE_READ_CHANNELS.as_str()
        );

        connection
            .execute(
                "UPDATE api_keys SET scopes = ?2 WHERE key = ?1",
                params![
                    DEFAULT_SERVICE_API_KEY_VALUE,
                    serialize_scope_slugs(&[SCOPE_READ_CHANNELS]).unwrap()
                ],
            )
            .unwrap();

        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {DEFAULT_SERVICE_API_KEY_VALUE}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { createLLMAPIKey(name: \"SDK Key\") { name } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_json = read_json_response(denied).await;
        assert_eq!(denied_json["data"]["createLLMAPIKey"], Value::Null);
        assert_eq!(denied_json["errors"][0]["message"], "permission denied");

        let invalid_user_key = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {DEFAULT_USER_API_KEY_VALUE}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { createLLMAPIKey(name: \"SDK Key\") { name } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_user_key.status(), StatusCode::UNAUTHORIZED);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_users_query() {
        let db_path = temp_sqlite_path("task8-users-query");
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
        ensure_identity_tables(&connection).unwrap();

        // Create a user with read_users scope to authorize the query
        let _user_id = insert_test_user(&connection, "admin@example.com", "password123", &[SCOPE_READ_USERS]);

        // Create some additional users
        let user1_id = insert_test_user(&connection, "user1@example.com", "password123", &[SCOPE_READ_SETTINGS]);
        let user2_id = insert_test_user(&connection, "user2@example.com", "password123", &[SCOPE_READ_CHANNELS]);

        // Create roles and assign to users
        let role1_id = insert_role(&connection, "Role1", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_SETTINGS]);
        let role2_id = insert_role(&connection, "Role2", ROLE_LEVEL_SYSTEM, 0, &[SCOPE_READ_CHANNELS]);
        attach_role(&connection, user1_id, role1_id);
        attach_role(&connection, user2_id, role2_id);

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "{ users { edges { node { id createdAt updatedAt email status firstName lastName isOwner preferLanguage scopes roles { edges { node { id name } } } } } pageInfo { hasNextPage hasPreviousPage startCursor endCursor } } }"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;

        assert!(json["data"]["users"].is_object());
        let users_conn = &json["data"]["users"];

        // Verify pageInfo
        assert!(users_conn["pageInfo"].is_object());
        let page_info = &users_conn["pageInfo"];
        assert!(page_info["hasNextPage"].is_boolean());
        assert!(page_info["hasPreviousPage"].is_boolean());
        assert!(page_info["startCursor"].is_null() || page_info["startCursor"].is_string());
        assert!(page_info["endCursor"].is_null() || page_info["endCursor"].is_string());

        // Verify edges
        assert!(users_conn["edges"].is_array());
        let edges = users_conn["edges"].as_array().unwrap();
        for edge in edges {
            assert!(edge["node"].is_object());
            let node = &edge["node"];
            assert!(node["id"].is_string());
            assert!(node["createdAt"].is_string());
            assert!(node["updatedAt"].is_string());
            assert!(node["email"].is_string());
            assert!(node["status"].is_string());
            assert!(node["firstName"].is_string());
            assert!(node["lastName"].is_string());
            assert!(node["isOwner"].is_boolean());
            assert!(node["preferLanguage"].is_string());
            assert!(node["scopes"].is_array());
            assert!(node["roles"].is_object());
            let roles_conn = &node["roles"];
            assert!(roles_conn["edges"].is_array());
            let role_edges = roles_conn["edges"].as_array().unwrap();
            for role_edge in role_edges {
                assert!(role_edge["node"].is_object());
                let role_node = &role_edge["node"];
                assert!(role_node["id"].is_string());
                assert!(role_node["name"].is_string());
            }
        }

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_me_query() {
        let db_path = temp_sqlite_path("task8-me-query");
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
        ensure_identity_tables(&connection).unwrap();
        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;

        // Create a test user with scopes and project membership
        let user_id = insert_test_user(
            &connection,
            "testuser@example.com",
            "password123",
            &[SCOPE_READ_SETTINGS, SCOPE_READ_CHANNELS],
        );
        insert_project_membership(&connection, user_id, project_id, false, &[SCOPE_READ_REQUESTS]);

        // Create a role for the user at project level
        let project_role_id = insert_role(&connection, "Project Reader", ROLE_LEVEL_PROJECT, project_id, &[SCOPE_READ_REQUESTS]);
        attach_role(&connection, user_id, project_role_id);

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let token = signin_token(foundation.clone(), "testuser@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "{ me { id email firstName lastName isOwner preferLanguage avatar scopes roles { name } projects { projectID isOwner scopes roles { name } } } }"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;

        assert!(json["data"]["me"].is_object());
        let me = &json["data"]["me"];

        // Verify basic fields
        assert!(me["id"].is_string());
        assert!(me["email"].as_str().unwrap() == "testuser@example.com");
        assert!(me["firstName"].as_str().unwrap() == "Test");
        assert!(me["lastName"].as_str().unwrap() == "User");
        assert!(!me["isOwner"].as_bool().unwrap());
        assert!(me["preferLanguage"].as_str().unwrap() == "en");
        assert!(me["avatar"].is_null() || me["avatar"].is_string());

        // Verify scopes
        assert!(me["scopes"].is_array());
        let scopes: Vec<&str> = me["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(scopes.contains(&"read_settings"));
        assert!(scopes.contains(&"read_channels"));

        // Verify roles
        assert!(me["roles"].is_array());
        let roles = me["roles"].as_array().unwrap();
        assert!(!roles.is_empty());
        for role in roles {
            assert!(role["name"].is_string());
        }

        // Verify projects
        assert!(me["projects"].is_array());
        let projects = me["projects"].as_array().unwrap();
        assert!(!projects.is_empty());
        for project in projects {
            assert!(project["projectID"].is_string());
            assert!(project["isOwner"].is_boolean());
            assert!(project["scopes"].is_array());
            assert!(project["roles"].is_array());
        }

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_all_scopes_query() {
        let db_path = temp_sqlite_path("task8-all-scopes");
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
        ensure_identity_tables(&connection).unwrap();

        // Create a user with read_settings scope to authorize the query
        let _user_id = insert_test_user(&connection, "admin@example.com", "password123", &[SCOPE_READ_SETTINGS]);

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        // Query 1: allScopes without filter
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "{ allScopes { scope description levels } }"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert!(json["data"]["allScopes"].is_array());
        let all_scopes = json["data"]["allScopes"].as_array().unwrap();
        assert!(!all_scopes.is_empty());

        for scope in all_scopes {
            assert!(scope["scope"].is_string());
            assert!(scope["description"].is_string());
            assert!(scope["levels"].is_array());
        }

        // Query 2: allScopes(level: "system")
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "{ allScopes(level: \"system\") { scope description levels } }"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert!(json["data"]["allScopes"].is_array());
        let system_scopes = json["data"]["allScopes"].as_array().unwrap();
        assert!(!system_scopes.is_empty());

        for scope in system_scopes {
            assert!(scope["scope"].is_string());
            assert!(scope["description"].is_string());
            assert!(scope["levels"].is_array());
            // Verify each returned scope has "system" in its levels
            let levels = scope["levels"].as_array().unwrap();
            assert!(levels.iter().any(|l| l.as_str().unwrap() == "system"));
        }

        // Query 3: allScopes(level: "project")
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "{ allScopes(level: \"project\") { scope description levels } }"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = read_json_response(response).await;
        assert!(json["data"]["allScopes"].is_array());
        let project_scopes = json["data"]["allScopes"].as_array().unwrap();
        assert!(!project_scopes.is_empty());

        for scope in project_scopes {
            assert!(scope["scope"].is_string());
            assert!(scope["description"].is_string());
            assert!(scope["levels"].is_array());
            // Verify each returned scope has "project" in its levels
            let levels = scope["levels"].as_array().unwrap();
            assert!(levels.iter().any(|l| l.as_str().unwrap() == "project"));
        }

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_allows_query_models_query() {
        let db_path = temp_sqlite_path("task8-query-models");
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
        ensure_identity_tables(&connection).unwrap();

        // Create a user with read_channels scope to authorize the query
        let _user_id = insert_test_user(&connection, "admin@example.com", "password123", &[SCOPE_READ_CHANNELS]);

        // Insert some test models
        connection.execute(
            "INSERT INTO models (developer, model_id, type, name, icon, remark, model_card, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                "openai",
                "gpt-4",
                "chat",
                "GPT-4",
                "icon",
                "Test model",
                "{}",
                "enabled"
            ],
        ).unwrap();
        connection.execute(
            "INSERT INTO models (developer, model_id, type, name, icon, remark, model_card, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                "anthropic",
                "claude-3",
                "chat",
                "Claude 3",
                "icon",
                "Test model 2",
                "{}",
                "disabled"
            ],
        ).unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
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

        let json = read_json_response(response).await;

        assert!(json["data"]["queryModels"].is_array());
        let models = json["data"]["queryModels"].as_array().unwrap();

        // Should have at least the two models we inserted
        assert!(models.len() >= 2);

        for model in models {
            assert!(model["id"].is_string());
            assert!(model["status"].is_string());
        }

        std::fs::remove_file(db_path).ok();
    }
