use super::{
    admin::{SqliteAdminService, SqliteOperationalService, StoredBackupApiKey, StoredBackupChannel, StoredBackupModel, StoredBackupPayload},
    admin_operational::{RestoreOptions, SeaOrmOperationalService},
    authz::{
        scope_strings, serialize_scope_slugs, ScopeLevel, ScopeSlug, ROLE_LEVEL_PROJECT,
        ROLE_LEVEL_SYSTEM, SCOPE_READ_CHANNELS, SCOPE_READ_REQUESTS,
        SCOPE_READ_PROMPTS, SCOPE_READ_USERS, SCOPE_READ_SETTINGS, SCOPE_WRITE_API_KEYS,
        SCOPE_WRITE_PROMPTS, SCOPE_WRITE_SETTINGS, SCOPE_WRITE_REQUESTS, SCOPE_WRITE_USERS,
    },
    circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker},
    graphql::SeaOrmAdminGraphqlService,
    graphql_sqlite_support::{SqliteAdminGraphqlService, SqliteOpenApiGraphqlService},
    identity_service::{SeaOrmIdentityService, SqliteIdentityService},
    openai_v1::{
        NewChannelRecord, NewModelRecord, NewRequestExecutionRecord, NewRequestRecord,
        NewUsageLogRecord, SeaOrmOpenAiV1Service, SqliteOpenAiV1Service,
    },
    request_context::parse_onboarding_record,
    request_context_service::{SeaOrmRequestContextService, SqliteRequestContextService},
    seaorm::SeaOrmConnectionFactory,
    shared::{
        SqliteFoundation, DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_VALUE,
        PRIMARY_DATA_STORAGE_NAME, SYSTEM_KEY_CHANNEL_SETTINGS, SYSTEM_KEY_ONBOARDED, graphql_gid,
    },
    sqlite_support::ensure_operational_tables,
    system::{ensure_identity_tables, hash_password, SeaOrmBootstrapService, SqliteBootstrapService},
};
use axonhub_http::{
    AdminCapability, AdminError, AdminGraphqlCapability, AdminGraphqlPort, AdminPort,
    AuthUserContext, GraphqlRequestPayload, HttpCorsSettings, HttpState, IdentityCapability,
    IdentityPort, InitializeSystemRequest, OpenAiRequestBody, OpenAiV1Capability,
    OpenAiV1ExecutionRequest, OpenAiV1Port, OpenAiV1Route, OpenApiGraphqlCapability,
    ProjectContext, ProviderEdgeAdminCapability, RealtimeSessionCreateRequest,
    RealtimeSessionPatchRequest, RealtimeSessionTransportRequest, RequestContextCapability,
    RequestContextPort, SignInRequest, SystemBootstrapCapability, SystemBootstrapPort,
    TraceConfig, router as http_router,
};
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::ServiceResponse;
use actix_web::http::{Method, StatusCode};
use actix_web::http::header;
use actix_web::test as actix_test;
use pg_embed::pg_enums::PgAuthMethod;
use pg_embed::pg_fetch::{PgFetchSettings, PG_V15};
use pg_embed::postgres::{PgEmbed, PgSettings};
use postgres::{Client as PostgresClient, NoTls};
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

async fn read_json_response<B>(response: ServiceResponse<B>) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn assert_graphql_status<B>(response: ServiceResponse<B>, expected_status: StatusCode) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    assert_eq!(response.status(), expected_status);
    read_json_response(response).await
}

fn assert_graphql_success_field<'a>(json: &'a Value, field: &str) -> &'a Value {
    assert!(
        json.get("errors").is_none_or(Value::is_null),
        "expected GraphQL success for `{field}`, got errors: {}",
        json.get("errors").cloned().unwrap_or(Value::Null)
    );
    &json["data"][field]
}

fn assert_graphql_error_field<'a>(json: &'a Value, field: &str, expected_message: &str) -> &'a Value {
    assert_eq!(json["data"][field], Value::Null);
    assert_eq!(json["errors"][0]["message"], expected_message);
    &json["errors"][0]
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

    fn multipart(boundary: &str, parts: &[(&str, Option<&str>, Option<&str>, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, filename, content_type, data) in parts {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"{}\r\n",
                    filename
                        .map(|value| format!("; filename=\"{value}\""))
                        .unwrap_or_default()
                )
                .as_bytes(),
            );
            if let Some(content_type) = content_type {
                body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
            }
            body.extend_from_slice(b"\r\n");
            body.extend_from_slice(data);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        body
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

    fn temp_postgres_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axonhub-{name}-{unique}"))
    }

    fn available_tcp_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
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
            timeout: Some(std::time::Duration::from_secs(60)),
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

    fn insert_api_key(
        connection: &Connection,
        user_id: i64,
        project_id: i64,
        key: &str,
        name: &str,
        key_type: &str,
        scopes: &[ScopeSlug],
    ) -> i64 {
        let scopes_json = serialize_scope_slugs(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', ?6, '{}', 0)
                 ON CONFLICT(key) DO UPDATE SET
                     user_id = excluded.user_id,
                     project_id = excluded.project_id,
                     name = excluded.name,
                     type = excluded.type,
                     status = excluded.status,
                     scopes = excluded.scopes,
                     profiles = excluded.profiles,
                     deleted_at = 0,
                     updated_at = CURRENT_TIMESTAMP",
                params![user_id, project_id, key, name, key_type, scopes_json],
            )
            .unwrap();
        connection
            .query_row(
                "SELECT id FROM api_keys WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                [key],
                |row| row.get(0),
            )
            .unwrap()
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
        })
    }

    fn bootstrap_postgres_auth_fixture(connection: &mut PostgresClient) {
        connection
            .execute(
                "INSERT INTO systems (key, value, deleted_at) VALUES ($1, $2, 0)
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                &[&crate::foundation::shared::SYSTEM_KEY_SECRET_KEY, &"task4-secret"],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO users (
                    id, created_at, updated_at, deleted_at, email, status, prefer_language,
                    password, first_name, last_name, avatar, is_owner, scopes
                ) VALUES (
                    1, NOW(), NOW(), 0, $1, 'activated', 'en', $2, 'System', 'Owner', '', TRUE, '[]'
                )
                ON CONFLICT (id) DO NOTHING",
                &[
                    &"owner@example.com",
                    &hash_password("password123").unwrap(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects (id, created_at, updated_at, deleted_at, name, description, status)
                 VALUES (1, NOW(), NOW(), 0, 'Default', '', 'active')
                 ON CONFLICT (id) DO NOTHING",
                &[],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO user_projects (id, created_at, updated_at, user_id, project_id, is_owner, scopes)
                 VALUES (1, NOW(), NOW(), 1, 1, TRUE, '[]')
                 ON CONFLICT (id) DO NOTHING",
                &[],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO api_keys (
                    id, created_at, updated_at, deleted_at, user_id, project_id, key, name, type, status, scopes, profiles
                ) VALUES
                    (1, NOW(), NOW(), 0, 1, 1, $1, 'Task4 User Key', 'user', 'enabled', $3, '{}'),
                    (2, NOW(), NOW(), 0, 1, 1, $2, 'Task4 Service Key', 'service_account', 'enabled', $3, '{}')
                ON CONFLICT (id) DO NOTHING",
                &[
                    &DEFAULT_USER_API_KEY_VALUE,
                    &DEFAULT_SERVICE_API_KEY_VALUE,
                    &serialize_scope_slugs(&[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS]).unwrap(),
                ],
            )
            .unwrap();
    }

    fn sqlite_task4_identity_and_request_context_services(
        foundation: Arc<SqliteFoundation>,
    ) -> (SqliteIdentityService, SqliteRequestContextService) {
        (
            SqliteIdentityService::new(foundation.clone(), false),
            SqliteRequestContextService::new(foundation, false),
        )
    }

    fn seaorm_task4_identity_and_request_context_services(
        dsn: &str,
    ) -> (SeaOrmIdentityService, SeaOrmRequestContextService) {
        let factory = SeaOrmConnectionFactory::postgres(dsn.to_owned());
        (
            SeaOrmIdentityService::new(factory.clone(), false),
            SeaOrmRequestContextService::new(factory, false),
        )
    }

    #[test]
    fn sqlite_identity_and_request_context_match_task4_auth_contract() {
        let db_path = temp_sqlite_path("task4-sqlite-auth-context");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap.initialize(&InitializeSystemRequest {
            owner_email: "owner@example.com".to_owned(),
            owner_password: "password123".to_owned(),
            owner_first_name: "System".to_owned(),
            owner_last_name: "Owner".to_owned(),
            brand_name: "AxonHub".to_owned(),
        }).unwrap();

        let (identity, request_context) =
            sqlite_task4_identity_and_request_context_services(foundation.clone());

        let signin = identity
            .admin_signin(&SignInRequest {
                email: "owner@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap();
        let admin = identity.authenticate_admin_jwt(&signin.token).unwrap();
        assert_eq!(admin.email, "owner@example.com");
        assert!(matches!(
            identity.authenticate_admin_jwt("invalid-token"),
            Err(axonhub_http::AdminAuthError::InvalidToken)
        ));

        let user_key = identity
            .authenticate_api_key(Some(DEFAULT_USER_API_KEY_VALUE), false)
            .unwrap();
        assert_eq!(user_key.project.id, 1);
        assert_eq!(user_key.key_type, axonhub_http::ApiKeyType::User);
        assert!(matches!(
            identity.authenticate_api_key(Some("invalid-key"), false),
            Err(axonhub_http::ApiKeyAuthError::Invalid)
        ));

        let service_key = identity
            .authenticate_api_key(Some(DEFAULT_SERVICE_API_KEY_VALUE), false)
            .unwrap();
        assert_eq!(service_key.key_type, axonhub_http::ApiKeyType::ServiceAccount);

        let gemini_query = identity
            .authenticate_gemini_key(Some(DEFAULT_USER_API_KEY_VALUE), None)
            .unwrap();
        let gemini_header = identity
            .authenticate_gemini_key(None, Some(DEFAULT_USER_API_KEY_VALUE))
            .unwrap();
        assert_eq!(gemini_query.id, gemini_header.id);

        let project = request_context.resolve_project(1).unwrap().unwrap();
        assert_eq!(project.name, "Default Project");

        let thread = request_context.resolve_thread(1, " thread-task4 ").unwrap().unwrap();
        assert_eq!(thread.thread_id, "thread-task4");
        let thread_reuse = request_context.resolve_thread(1, "thread-task4").unwrap().unwrap();
        assert_eq!(thread.id, thread_reuse.id);
        assert!(matches!(
            request_context.resolve_thread(2, "thread-task4"),
            Err(axonhub_http::ContextResolveError::Internal)
        ));

        let trace = request_context
            .resolve_trace(1, " trace-task4 ", Some(thread.id))
            .unwrap()
            .unwrap();
        assert_eq!(trace.trace_id, "trace-task4");
        assert_eq!(trace.thread_id, Some(thread.id));
        let trace_reuse = request_context
            .resolve_trace(1, "trace-task4", Some(thread.id))
            .unwrap()
            .unwrap();
        assert_eq!(trace.id, trace_reuse.id);
        let trace_without_thread = request_context
            .resolve_trace(1, "trace-task4", None)
            .unwrap()
            .unwrap();
        assert_eq!(trace.id, trace_without_thread.id);
        assert!(matches!(
            request_context.resolve_trace(1, "trace-task4", Some(thread.id + 1)),
            Err(axonhub_http::ContextResolveError::Internal)
        ));
        assert!(matches!(
            request_context.resolve_trace(1, "trace-task4-missing-thread", Some(thread.id + 10_000)),
            Err(axonhub_http::ContextResolveError::Internal)
        ));

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn sqlite_realtime_session_lifecycle_persists_linked_records() {
        let db_path = temp_sqlite_path("task9-realtime-session");
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

        {
            let connection = foundation.open_connection(true).unwrap();
            connection
                .execute(
                    "UPDATE api_keys SET scopes = ?2 WHERE key = ?1",
                    params![
                        DEFAULT_USER_API_KEY_VALUE,
                        serialize_scope_slugs(&[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS]).unwrap()
                    ],
                )
                .unwrap();
        }

        let request_context = SqliteRequestContextService::new(foundation.clone(), false);
        let project = request_context.resolve_project(1).unwrap().unwrap();
        let thread = request_context
            .resolve_thread(1, "thread-task9-realtime")
            .unwrap()
            .unwrap();
        let trace = request_context
            .resolve_trace(1, "trace-task9-realtime", Some(thread.id))
            .unwrap()
            .unwrap();

        let service = SqliteOpenAiV1Service::new(foundation.clone());
        let created = service
            .create_realtime_session(RealtimeSessionCreateRequest {
                project,
                thread: Some(thread.clone()),
                trace: Some(trace.clone()),
                api_key_id: Some(1),
                client_ip: Some("127.0.0.1".to_owned()),
                request_id: Some("req-task9-realtime".to_owned()),
                transport: RealtimeSessionTransportRequest {
                    transport: "session".to_owned(),
                    model: "gpt-4o-realtime-preview".to_owned(),
                    channel_id: None,
                    metadata: Some(serde_json::json!({"voice": "alloy"})),
                    expires_at: Some("2026-04-03T00:00:00Z".to_owned()),
                },
            })
            .unwrap();
        assert_eq!(created.status, "open");
        assert_eq!(created.thread_id.as_deref(), Some("thread-task9-realtime"));
        assert_eq!(created.trace_id.as_deref(), Some("trace-task9-realtime"));

        let updated = service
            .update_realtime_session(
                created.session_id.as_str(),
                RealtimeSessionPatchRequest {
                    status: Some("closing".to_owned()),
                    metadata: Some(serde_json::json!({"voice": "verse"})),
                    expires_at: None,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, "closing");
        assert_eq!(updated.metadata["attributes"]["voice"], "verse");

        let deleted = service
            .delete_realtime_session(created.session_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(deleted.status, "closed");
        assert!(deleted.closed_at.is_some());

        let connection = foundation.open_connection(false).unwrap();
        let session_row: (String, i64, i64, i64, String, String) = connection
            .query_row(
                "SELECT session_id, thread_id, trace_id, request_id, status, metadata FROM realtime_sessions ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .unwrap();
        assert_eq!(session_row.0, created.session_id);
        assert_eq!(session_row.1, thread.id);
        assert_eq!(session_row.2, trace.id);
        assert!(session_row.3 > 0);
        assert_eq!(session_row.4, "closed");
        assert!(session_row.5.contains("trace-task9-realtime"));

        let request_row: (String, String, String) = connection
            .query_row(
                "SELECT format, status, external_id FROM requests ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(request_row.0, "openai/realtime_session");
        assert_eq!(request_row.1, "completed");
        assert_eq!(request_row.2, created.session_id);

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn postgres_identity_and_request_context_match_task4_auth_contract() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut embedded_pg, dsn, data_dir) =
            runtime.block_on(start_embedded_postgres("task4-postgres-auth-context"));

        let factory = SeaOrmConnectionFactory::postgres(dsn.clone());
        runtime.block_on(factory.connect_migrated()).unwrap();

        let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
        bootstrap_postgres_auth_fixture(&mut connection);

        let (identity, request_context) = seaorm_task4_identity_and_request_context_services(&dsn);

        let signin = identity
            .admin_signin(&SignInRequest {
                email: "owner@example.com".to_owned(),
                password: "password123".to_owned(),
            })
            .unwrap();
        let admin = identity.authenticate_admin_jwt(&signin.token).unwrap();
        assert_eq!(admin.email, "owner@example.com");
        assert!(matches!(
            identity.authenticate_admin_jwt("invalid-token"),
            Err(axonhub_http::AdminAuthError::InvalidToken)
        ));

        let user_key = identity
            .authenticate_api_key(Some(DEFAULT_USER_API_KEY_VALUE), false)
            .unwrap();
        assert_eq!(user_key.project.id, 1);
        assert_eq!(user_key.key_type, axonhub_http::ApiKeyType::User);
        assert!(matches!(
            identity.authenticate_api_key(Some("invalid-key"), false),
            Err(axonhub_http::ApiKeyAuthError::Invalid)
        ));

        let service_key = identity
            .authenticate_api_key(Some(DEFAULT_SERVICE_API_KEY_VALUE), false)
            .unwrap();
        assert_eq!(service_key.key_type, axonhub_http::ApiKeyType::ServiceAccount);

        let gemini_query = identity
            .authenticate_gemini_key(Some(DEFAULT_USER_API_KEY_VALUE), None)
            .unwrap();
        let gemini_header = identity
            .authenticate_gemini_key(None, Some(DEFAULT_USER_API_KEY_VALUE))
            .unwrap();
        assert_eq!(gemini_query.id, gemini_header.id);

        let project = request_context.resolve_project(1).unwrap().unwrap();
        assert_eq!(project.name, "Default");

        let thread = request_context.resolve_thread(1, " thread-task4-pg ").unwrap().unwrap();
        assert_eq!(thread.thread_id, "thread-task4-pg");
        let thread_reuse = request_context
            .resolve_thread(1, "thread-task4-pg")
            .unwrap()
            .unwrap();
        assert_eq!(thread.id, thread_reuse.id);
        assert!(matches!(
            request_context.resolve_thread(2, "thread-task4-pg"),
            Err(axonhub_http::ContextResolveError::Internal)
        ));

        let trace = request_context
            .resolve_trace(1, " trace-task4-pg ", Some(thread.id))
            .unwrap()
            .unwrap();
        assert_eq!(trace.trace_id, "trace-task4-pg");
        assert_eq!(trace.thread_id, Some(thread.id));
        let trace_reuse = request_context
            .resolve_trace(1, "trace-task4-pg", Some(thread.id))
            .unwrap()
            .unwrap();
        assert_eq!(trace.id, trace_reuse.id);
        let trace_without_thread = request_context
            .resolve_trace(1, "trace-task4-pg", None)
            .unwrap()
            .unwrap();
        assert_eq!(trace.id, trace_without_thread.id);
        assert!(matches!(
            request_context.resolve_trace(1, "trace-task4-pg", Some(thread.id + 1)),
            Err(axonhub_http::ContextResolveError::Internal)
        ));
        assert!(matches!(
            request_context.resolve_trace(1, "trace-task4-pg-missing-thread", Some(thread.id + 10_000)),
            Err(axonhub_http::ContextResolveError::Internal)
        ));

        runtime.block_on(embedded_pg.stop_db()).unwrap();
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn bootstrap_seeds_default_task4_onboarding_record() {
        let db_path = temp_sqlite_path("task4-bootstrap-onboarding");
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

        let raw = foundation
            .system_settings()
            .value(SYSTEM_KEY_ONBOARDED)
            .unwrap()
            .expect("bootstrap should seed onboarding baseline");
        let onboarding = parse_onboarding_record(&raw).unwrap();
        assert_eq!(onboarding, Default::default());

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn request_context_linkage_conflict_is_fail_open_for_http_debug_context() {
        let db_path = temp_sqlite_path("task4-request-context-fail-open");
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

        let existing_thread = foundation
            .request_context_service(false)
            .resolve_thread(1, "thread-task4-existing")
            .unwrap()
            .unwrap();
        let _existing_trace = foundation
            .request_context_service(false)
            .resolve_trace(1, "trace-task4-existing", Some(existing_thread.id))
            .unwrap()
            .unwrap();
        let conflicting_thread = foundation
            .request_context_service(false)
            .resolve_thread(1, "thread-task4-conflict")
            .unwrap()
            .unwrap();
        assert!(matches!(
            foundation
                .request_context_service(false)
                .resolve_trace(1, "trace-task4-existing", Some(conflicting_thread.id)),
            Err(axonhub_http::ContextResolveError::Internal)
        ));

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::GET)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task4-conflict")
                    .header("AH-Trace-Id", "trace-task4-existing")
                    .header("X-Request-Id", "req-task4-conflict")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json_response(response).await;
        assert_eq!(json["auth"]["mode"], "api_key");
        assert_eq!(json["project"]["id"], 1);
        assert_eq!(json["requestId"], "req-task4-conflict");
        assert_eq!(json["thread"]["threadId"], "thread-task4-conflict");
        assert_eq!(json["trace"], Value::Null);

        let connection = foundation.open_connection(false).unwrap();
        let trace_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE trace_id = ?1",
                ["trace-task4-existing"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(trace_count, 1);
        let persisted_trace_thread: Option<i64> = connection
            .query_row(
                "SELECT thread_id FROM traces WHERE trace_id = ?1 LIMIT 1",
                ["trace-task4-existing"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_trace_thread, Some(existing_thread.id));

        std::fs::remove_file(db_path).ok();
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

        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let connection = foundation.open_connection(true).unwrap();
        let api_key_id = insert_api_key(
            &connection,
            1,
            project_id,
            DEFAULT_USER_API_KEY_VALUE,
            "Foundation Request Usage Key",
            "user",
            &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
        );
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
                                     "queryAllChannelModels": false,
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
        assert_eq!(
            settings.auto_sync.frequency,
            super::admin::AutoSyncFrequencySetting::OneHour
        );
        assert!(!settings.query_all_channel_models);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_updates_system_channel_auto_sync_frequency() {
        let db_path = temp_sqlite_path("task9-update-system-channel-auto-sync");
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
            &[SCOPE_WRITE_SETTINGS, SCOPE_READ_SETTINGS],
        );
        connection
            .execute(
                "UPDATE users SET is_owner = 1 WHERE email = ?1 AND deleted_at = 0",
                params!["admin@example.com"],
            )
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let update_response = app
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
                                    "autoSync": {
                                        "frequency": "SIX_HOURS"
                                    }
                                }
                           }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let update_json = read_json_response(update_response).await;
        assert_eq!(update_json["data"]["updateSystemChannelSettings"], true);

        let query_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"query":"{ systemChannelSettings { probe { enabled frequency } autoSync { frequency } } }"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let query_json = read_json_response(query_response).await;
        assert_eq!(
            query_json["data"]["systemChannelSettings"]["autoSync"]["frequency"],
            "SIX_HOURS"
        );

        let settings = SqliteOperationalService::new(foundation.clone())
            .system_channel_settings()
            .unwrap();
        assert_eq!(
            settings.auto_sync.frequency,
            super::admin::AutoSyncFrequencySetting::SixHours
        );

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_saves_and_queries_proxy_presets() {
        let db_path = temp_sqlite_path("task9-proxy-presets");
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
            &[SCOPE_WRITE_SETTINGS, SCOPE_READ_SETTINGS],
        );
        connection
            .execute(
                "UPDATE users SET is_owner = 1 WHERE email = ?1 AND deleted_at = 0",
                params!["admin@example.com"],
            )
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let save_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation SaveProxyPreset($input: SaveProxyPresetInput!) { saveProxyPreset(input: $input) }",
                            "variables": {
                                "input": {
                                    "name": "Office Proxy",
                                    "url": "http://proxy.internal",
                                    "username": "tester",
                                    "password": "secret"
                                }
                            }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let save_json = read_json_response(save_response).await;
        assert_eq!(save_json["data"]["saveProxyPreset"], true);

        let query_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"query":"{ proxyPresets { name url username password } }"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let query_json = read_json_response(query_response).await;
        assert_eq!(query_json["data"]["proxyPresets"][0]["name"], "Office Proxy");
        assert_eq!(query_json["data"]["proxyPresets"][0]["url"], "http://proxy.internal");
        assert_eq!(query_json["data"]["proxyPresets"][0]["username"], "tester");
        assert_eq!(query_json["data"]["proxyPresets"][0]["password"], "secret");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn admin_graphql_updates_and_queries_user_agent_pass_through_settings() {
        let db_path = temp_sqlite_path("task9-user-agent-pass-through");
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
            &[SCOPE_WRITE_SETTINGS, SCOPE_READ_SETTINGS],
        );
        connection
            .execute(
                "UPDATE users SET is_owner = 1 WHERE email = ?1 AND deleted_at = 0",
                params!["admin@example.com"],
            )
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let token = signin_token(foundation.clone(), "admin@example.com", "password123");

        let update_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{
                            "query": "mutation UpdateUserAgentPassThroughSettings($input: UpdateUserAgentPassThroughSettingsInput!) { updateUserAgentPassThroughSettings(input: $input) }",
                            "variables": {
                                "input": {
                                    "enabled": true
                                }
                            }
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let update_json = read_json_response(update_response).await;
        assert_eq!(update_json["data"]["updateUserAgentPassThroughSettings"], true);

        let query_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"query":"{ userAgentPassThroughSettings { enabled } }"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let query_json = read_json_response(query_response).await;
        assert_eq!(query_json["data"]["userAgentPassThroughSettings"]["enabled"], true);

        let enabled = SqliteOperationalService::new(foundation.clone())
            .user_agent_pass_through()
            .unwrap();
        assert!(enabled);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn sqlite_v1_models_returns_explicit_models_when_query_all_channel_models_disabled() {
        let db_path = temp_sqlite_path("task9-openai-model-list-setting");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let api_key = "task9-model-list-user-key";

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                api_key,
                "Task9 Model List User Key",
                "user",
                &[SCOPE_READ_CHANNELS],
            );
        }

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Alias Mock",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["actual-model"]"#,
                auto_sync_supported_models: false,
                default_test_model: "actual-model",
                settings_json: r#"{"modelMappings":[{"from":"alias-model","to":"actual-model"}]}"#,
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 9 model setting test",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "actual-model",
                model_type: "chat",
                name: "Actual Model",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 9 model setting test",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "alias-model",
                model_type: "chat",
                name: "Alias Model",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 9 model setting test",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let default_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .header("X-API-Key", api_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(default_response.status(), StatusCode::OK);
        let default_json = read_json_response(default_response).await;
        let default_ids = default_json["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(default_ids, vec!["actual-model"]);

        foundation
            .system_settings()
            .set_value(
                super::shared::SYSTEM_KEY_CHANNEL_SETTINGS,
                r#"{"probe":{"enabled":true,"frequency":"FiveMinutes"},"query_all_channel_models":false}"#,
            )
            .unwrap();

        let explicit_response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .method(Method::GET)
                    .header("X-API-Key", api_key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(explicit_response.status(), StatusCode::OK);
        let explicit_json = read_json_response(explicit_response).await;
        let explicit_ids = explicit_json["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(explicit_ids, vec!["actual-model", "alias-model"]);

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
        assert_eq!(quota_status[0].status, "available");
        assert!(quota_status[0].ready);
        assert!(quota_status[0]
            .quota_data_json
            .contains("ready for routing"));

        let reset_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation($channelID: ID!) { resetProviderQuota(channelID: $channelID) }","variables":{"channelID":"gid://axonhub/channel/1"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let reset_json = read_json_response(reset_response).await;
        assert_eq!(reset_json["data"]["resetProviderQuota"], true);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_route_uses_quota_ready_provider_channel_and_persists_ready_status() {
        let db_path = temp_sqlite_path("task16-provider-quota-ready");
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
                name: "Codex Ready Mock",
                channel_type: "codex",
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
                remark: "Task 16 quota ready test",
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
                remark: "Task 16 quota ready model",
            })
            .unwrap();

        let ready_api_key = "task16-ready-user-key";
        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                ready_api_key,
                "Task16 Ready User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
            connection.execute(
                "INSERT INTO provider_quota_statuses (channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at)
                 VALUES (1, 'codex', 'available', '{\"message\":\"manually ready\"}', NULL, 1, 0)",
                [],
            ).unwrap();
        }

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", ready_api_key)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hello ready quota"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let quota_status = SqliteOperationalService::new(foundation.clone())
            .provider_quota_statuses()
            .unwrap();
        assert_eq!(quota_status.len(), 1);
        assert_eq!(quota_status[0].status, "available");
        assert!(quota_status[0].ready);
        assert!(quota_status[0].quota_data_json.contains("ready for routing"));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_route_blocks_exhausted_provider_channel_until_reset() {
        let db_path = temp_sqlite_path("task16-provider-quota-exhausted");
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
                name: "Codex Exhausted Mock",
                channel_type: "codex",
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
                remark: "Task 16 quota exhausted test",
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
                remark: "Task 16 quota exhausted model",
            })
            .unwrap();

        let exhausted_api_key = "task16-exhausted-user-key";
        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            ensure_identity_tables(&connection).unwrap();
            let _admin_id = insert_test_user(
                &connection,
                "admin@example.com",
                "password123",
                &[SCOPE_WRITE_SETTINGS],
            );
            insert_api_key(
                &connection,
                1,
                1,
                exhausted_api_key,
                "Task16 Exhausted User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
            connection.execute(
                "INSERT INTO provider_quota_statuses (channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at)
                 VALUES (1, 'codex', 'exhausted', '{\"message\":\"quota exhausted\"}', NULL, 0, 0)",
                [],
            ).unwrap();
        }

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        admin_graphql: AdminGraphqlCapability::Available {
            graphql: Arc::new(SqliteAdminGraphqlService::new(foundation.clone())),
        },
        openapi_graphql: OpenApiGraphqlCapability::Unsupported {
            message: "test-only unsupported openapi graphql".to_owned(),
        },
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "test-only unsupported provider-edge admin".to_owned(),
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let blocked_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", exhausted_api_key)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"still blocked"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocked_response.status(), StatusCode::BAD_REQUEST);
        let blocked_json = read_json_response(blocked_response).await;
        assert_eq!(blocked_json["error"]["message"], "No enabled OpenAI channel is configured for the requested model");

        let token = signin_token(foundation.clone(), "admin@example.com", "password123");
        let reset_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation($channelID: ID!) { resetProviderQuota(channelID: $channelID) }","variables":{"channelID":"gid://axonhub/channel/1"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let reset_json = read_json_response(reset_response).await;
        assert_eq!(reset_json["data"]["resetProviderQuota"], true);

        let ready_response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", exhausted_api_key)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"reset worked"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ready_response.status(), StatusCode::OK);

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
        let denied_backup_json = assert_graphql_status(denied_backup_update, StatusCode::OK).await;
        assert_graphql_error_field(
            &denied_backup_json,
            "updateAutoBackupSettings",
            "permission denied: owner access required",
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
        let invalid_backup_json = assert_graphql_status(invalid_backup_update, StatusCode::OK).await;
        assert_graphql_error_field(
            &invalid_backup_json,
            "updateAutoBackupSettings",
            "dataStorageID is required when auto backup is enabled",
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
        let denied_trigger_backup_json = assert_graphql_status(denied_trigger_backup, StatusCode::OK).await;
        assert_graphql_error_field(
            &denied_trigger_backup_json,
            "triggerAutoBackup",
            "permission denied: owner access required",
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
        let denied_gc_json = assert_graphql_status(denied_gc, StatusCode::OK).await;
        assert_graphql_error_field(
            &denied_gc_json,
            "triggerGcCleanup",
            "permission denied: requires write:settings scope",
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
        let denied_update_me_json = assert_graphql_status(denied_update_me, StatusCode::OK).await;
        assert_graphql_error_field(&denied_update_me_json, "updateMe", "no fields to update");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn runtime_and_operational_rbac_denials_stay_consistent() {
        let db_path = temp_sqlite_path("task13-runtime-operational-rbac");
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

        let runtime_service = SqliteOpenAiV1Service::new(foundation.clone());
        let project = foundation.identities().find_project_by_id(1).unwrap();
        let runtime_error = runtime_service
            .execute(
                OpenAiV1Route::ChatCompletions,
                OpenAiV1ExecutionRequest {
                    headers: HashMap::new(),
                    body: OpenAiRequestBody::Json(serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": "hello"}]
                    })),
                    path: "/v1/chat/completions".to_owned(),
                    path_params: HashMap::new(),
                    query: HashMap::new(),
                    project: ProjectContext {
                        id: project.id,
                        name: project.name.clone(),
                        status: project.status.clone(),
                    },
                    trace: None,
                    api_key: axonhub_http::AuthApiKeyContext {
                        id: 777,
                        key: "task13-read-only-key".to_owned(),
                        name: "Task13 Read Only".to_owned(),
                        key_type: axonhub_http::ApiKeyType::User,
                        project: ProjectContext {
                            id: project.id,
                            name: project.name,
                            status: project.status,
                        },
                        scopes: vec!["read_channels".to_owned()],
                        profiles_json: None,
                    },
                    api_key_id: Some(777),
                    client_ip: None,
                    channel_hint_id: None,
                },
            )
            .unwrap_err();

        match runtime_error {
            axonhub_http::OpenAiV1Error::InvalidRequest { message } => {
                assert_eq!(message, "permission denied");
            }
            other => panic!("expected invalid request denial, got {other:?}"),
        }

        let connection = foundation.open_connection(true).unwrap();
        ensure_identity_tables(&connection).unwrap();
        let scoped_user_id = insert_test_user(
            &connection,
            "scoped@example.com",
            "password123",
            &[SCOPE_WRITE_SETTINGS],
        );
        assert!(scoped_user_id > 0);

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let scoped_token = signin_token(foundation.clone(), "scoped@example.com", "password123");
        let denied_backup = app
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
        let denied_backup_json = assert_graphql_status(denied_backup, StatusCode::OK).await;
        assert_graphql_error_field(
            &denied_backup_json,
            "triggerAutoBackup",
            "permission denied: owner access required",
        );

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn seaorm_operational_service_restores_backup_payload_and_records_completed_run() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut embedded_pg, dsn, data_dir) =
            runtime.block_on(start_embedded_postgres("task6-seaorm-restore-success"));

        let factory = SeaOrmConnectionFactory::postgres(dsn.clone());
        runtime.block_on(factory.connect_migrated()).unwrap();

        let bootstrap = SeaOrmBootstrapService::new(factory.clone().into(), "v0.9.20".to_owned());
        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
        bootstrap_postgres_auth_fixture(&mut connection);
        connection
            .execute(
                "INSERT INTO projects (id, created_at, updated_at, deleted_at, name, description, status)
                 VALUES (2, NOW(), NOW(), 0, 'Imported Project', '', 'active')
                 ON CONFLICT (id) DO NOTHING",
                &[],
            )
            .unwrap();

        let payload = StoredBackupPayload {
            version: super::shared::BACKUP_VERSION.to_owned(),
            timestamp: super::shared::current_rfc3339_timestamp(),
            channels: vec![StoredBackupChannel {
                id: 41,
                name: "Imported Channel".to_owned(),
                channel_type: "openai".to_owned(),
                base_url: "https://example.com/v1".to_owned(),
                status: "enabled".to_owned(),
                credentials: serde_json::json!({"apiKey":"secret"}),
                supported_models: serde_json::json!(["gpt-4o"]),
                default_test_model: "gpt-4o".to_owned(),
                settings: serde_json::json!({}),
                tags: serde_json::json!(["imported"]),
                ordering_weight: 100,
                error_message: String::new(),
                remark: "task6 restore".to_owned(),
            }],
            models: vec![StoredBackupModel {
                id: 51,
                developer: "openai".to_owned(),
                model_id: "gpt-4o".to_owned(),
                model_type: "chat".to_owned(),
                name: "GPT-4o".to_owned(),
                icon: "OpenAI".to_owned(),
                group: "openai".to_owned(),
                model_card: serde_json::json!({"limit":{"context":128000,"output":4096}}),
                settings: serde_json::json!({}),
                status: "enabled".to_owned(),
                remark: "task6 restore".to_owned(),
            }],
            channel_model_prices: vec![serde_json::json!({
                "channelName": "Imported Channel",
                "modelID": "gpt-4o",
                "price": {"items": []},
                "referenceID": "ref-task6"
            })],
            api_keys: vec![StoredBackupApiKey {
                id: 61,
                project_id: 2,
                project_name: "Imported Project".to_owned(),
                key: "sk-task6".to_owned(),
                name: "Imported Key".to_owned(),
                key_type: "user".to_owned(),
                status: "enabled".to_owned(),
                scopes: serde_json::json!(["read_channels"]),
            }],
        };

        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let service = SeaOrmOperationalService::new(factory.clone());
        let message = service
            .restore_backup(
                &payload_bytes,
                RestoreOptions {
                    include_channels: true,
                    include_models: true,
                    include_api_keys: true,
                    include_model_prices: true,
                    overwrite_existing: true,
                },
                Some(1),
            )
            .unwrap();
        assert_eq!(message, "Restore completed successfully");

        let channel_count: i64 = connection
            .query_one("SELECT COUNT(*) FROM channels WHERE name = 'Imported Channel'", &[])
            .unwrap()
            .get(0);
        let model_count: i64 = connection
            .query_one(
                "SELECT COUNT(*) FROM models WHERE model_id = 'gpt-4o' AND developer = 'openai'",
                &[],
            )
            .unwrap()
            .get(0);
        let api_key_count: i64 = connection
            .query_one("SELECT COUNT(*) FROM api_keys WHERE key = 'sk-task6'", &[])
            .unwrap()
            .get(0);
        let price_count: i64 = connection
            .query_one("SELECT COUNT(*) FROM channel_model_prices WHERE reference_id = 'ref-task6'", &[])
            .unwrap()
            .get(0);
        let run_row = connection
            .query_one(
                "SELECT status, error_message FROM operational_runs WHERE operation_type = 'restore' ORDER BY id DESC LIMIT 1",
                &[],
            )
            .unwrap();
        let run_status: String = run_row.get(0);
        let run_error: Option<String> = run_row.get(1);
        assert_eq!(channel_count, 1);
        assert_eq!(model_count, 1);
        assert_eq!(api_key_count, 1);
        assert_eq!(price_count, 1);
        assert_eq!(run_status, "completed");
        assert!(run_error.is_none());

        runtime.block_on(embedded_pg.stop_db()).ok();
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn seaorm_operational_service_rejects_invalid_backup_version_and_records_failed_run() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut embedded_pg, dsn, data_dir) =
            runtime.block_on(start_embedded_postgres("task6-seaorm-restore-failure"));

        let factory = SeaOrmConnectionFactory::postgres(dsn.clone());
        runtime.block_on(factory.connect_migrated()).unwrap();

        let bootstrap = SeaOrmBootstrapService::new(factory.clone().into(), "v0.9.20".to_owned());
        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
        bootstrap_postgres_auth_fixture(&mut connection);

        let payload_bytes = serde_json::to_vec(&StoredBackupPayload {
            version: "0.0".to_owned(),
            timestamp: super::shared::current_rfc3339_timestamp(),
            channels: Vec::new(),
            models: Vec::new(),
            channel_model_prices: Vec::new(),
            api_keys: Vec::new(),
        })
        .unwrap();

        let service = SeaOrmOperationalService::new(factory.clone());
        let error = service
            .restore_backup(
                &payload_bytes,
                RestoreOptions {
                    include_channels: true,
                    include_models: true,
                    include_api_keys: true,
                    include_model_prices: true,
                    overwrite_existing: true,
                },
                Some(1),
            )
            .unwrap_err();
        assert!(error.contains("backup version mismatch"));

        let run_row = connection
            .query_one(
                "SELECT status, COALESCE(error_message, '') FROM operational_runs WHERE operation_type = 'restore' ORDER BY id DESC LIMIT 1",
                &[],
            )
            .unwrap();
        let run_status: String = run_row.get(0);
        let run_error: String = run_row.get(1);
        assert_eq!(run_status, "failed");
        assert!(run_error.contains("backup version mismatch"));

        runtime.block_on(embedded_pg.stop_db()).ok();
        std::fs::remove_dir_all(data_dir).ok();
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

    #[test]
    fn admin_request_content_download_forbids_user_without_read_requests_scope() {
        let db_path = temp_sqlite_path("task13-request-content-noscope");
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
            "axonhub-task13-content-noscope-{}",
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
                    "Task13 FS NoScope",
                    "task13 noscope",
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
                format: "openai/chat_completions",
                request_headers_json: "{}",
                request_body_json: "{}",
                response_body_json: Some("{}"),
                response_chunks_json: Some("[]"),
                channel_id: None,
                external_id: None,
                status: "completed",
                stream: false,
                client_ip: "",
                metrics_latency_ms: None,
                metrics_first_token_latency_ms: None,
                content_saved: true,
                content_storage_id: Some(storage_id),
                content_storage_key: Some("/placeholder"),
                content_saved_at: Some("2026-03-23T00:00:00Z"),
            })
            .unwrap();

        let real_key = format!("/{project_id}/requests/{request_id}/test.txt");
        connection
            .execute(
                "UPDATE requests SET content_storage_key = ?2 WHERE id = ?1",
                params![request_id, real_key],
            )
            .unwrap();
        let full_path = content_dir.join(format!("{project_id}/requests/{request_id}/test.txt"));
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        fs::write(&full_path, b"test-content").unwrap();

        let user_without_read_requests = AuthUserContext {
            id: 2,
            email: "admin@example.com".to_owned(),
            first_name: "Admin".to_owned(),
            last_name: "User".to_owned(),
            is_owner: false,
            prefer_language: "en".to_owned(),
            avatar: None,
            scopes: scope_strings(&[
                SCOPE_READ_SETTINGS,
                SCOPE_READ_CHANNELS,
            ]),
            roles: Vec::new(),
            projects: Vec::new(),
        };

        let denied = admin
            .download_request_content(project_id, request_id, user_without_read_requests.clone())
            .unwrap_err();
        assert!(matches!(denied, AdminError::Forbidden { .. }));

        let wrong_project = admin
            .download_request_content(project_id + 1, request_id, user_without_read_requests.clone())
            .unwrap_err();
        assert!(matches!(wrong_project, AdminError::Forbidden { .. }));

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

        {
            let connection = foundation.open_connection(true).unwrap();
            connection
                .execute(
                    "UPDATE api_keys SET scopes = ?2 WHERE key = ?1",
                    params![
                        DEFAULT_USER_API_KEY_VALUE,
                        serialize_scope_slugs(&[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS]).unwrap()
                    ],
                )
                .unwrap();
        }

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
        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Image Mock",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-image-1"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-image-1",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 90,
                error_message: "",
                remark: "Task 13 image runtime test",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-image-1",
                model_type: "image",
                name: "GPT Image 1",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":8192,"output":0},"cost":{"input":1.0,"output":2.0}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 13 image runtime test",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

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
                "/v1/responses/compact",
                r#"{"model":"gpt-4o","input":"hi"}"#,
            ),
            (
                "/v1/embeddings",
                r#"{"model":"gpt-4o","input":"hi"}"#,
            ),
            (
                "/v1/images/generations",
                r#"{"model":"gpt-image-1","prompt":"draw a cat"}"#,
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

        let edit_boundary = "----task8-edit";
        let edit_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/images/edits")
                    .method(Method::POST)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={edit_boundary}"))
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task7")
                    .header("AH-Trace-Id", "trace-task7")
                    .body(Body::multipart(
                        edit_boundary,
                        &[
                            ("model", None, None, b"gpt-image-1"),
                            ("prompt", None, None, b"draw a cat"),
                            ("image", Some("cat.png"), Some("image/png"), b"png-bytes"),
                        ],
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(edit_response.status(), StatusCode::OK);

        let variation_boundary = "----task8-variation";
        let variation_response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/images/variations")
                    .method(Method::POST)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={variation_boundary}"),
                    )
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task7")
                    .header("AH-Trace-Id", "trace-task7")
                    .body(Body::multipart(
                        variation_boundary,
                        &[
                            ("model", None, None, b"gpt-image-1"),
                            ("image", Some("cat.png"), Some("image/png"), b"png-bytes"),
                        ],
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(variation_response.status(), StatusCode::OK);

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
        assert_eq!(
            request_statuses,
            vec![
                "completed",
                "completed",
                "completed",
                "completed",
                "completed",
                "completed",
                "completed",
            ]
        );

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
        assert_eq!(
            request_formats,
            vec![
                "openai/chat_completions",
                "openai/responses",
                "openai/responses_compact",
                "openai/embeddings",
                "openai/images_generations",
                "openai/images_edits",
                "openai/images_variations",
            ]
        );

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
        assert_eq!(request_trace_channels.len(), 7);
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
        assert_eq!(
            execution_statuses,
            vec![
                "completed",
                "completed",
                "completed",
                "completed",
                "completed",
                "completed",
                "completed",
            ]
        );

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 7);

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
        assert_eq!(usage_rows.len(), 7);
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

        let compact_usage = &usage_rows[2];
        assert_eq!(compact_usage.0, "openai/responses_compact");
        assert_eq!(compact_usage.1, 12);
        assert_eq!(compact_usage.2, 4);
        assert_eq!(compact_usage.3, 16);
        assert_eq!(compact_usage.4, 3);
        assert_eq!(compact_usage.5, 4);
        assert_eq!(compact_usage.6, 4);
        assert_eq!(compact_usage.7, 1);
        assert_eq!(compact_usage.8, 2);
        assert_eq!(compact_usage.9, 3);
        assert!((compact_usage.10 - 0.000015).abs() < 1e-12);
        assert_eq!(compact_usage.11, "price-ref-task9");
        assert!(compact_usage.12.contains("\"itemCode\":\"prompt_tokens\""));
        assert!(compact_usage.12.contains("\"itemCode\":\"prompt_write_cached_tokens\""));

        let generation_usage = &usage_rows[4];
        assert_eq!(generation_usage.0, "openai/images_generations");
        assert_eq!(generation_usage.1, 20);
        assert_eq!(generation_usage.2, 30);
        assert_eq!(generation_usage.3, 50);

        let edit_usage = &usage_rows[5];
        assert_eq!(edit_usage.0, "openai/images_edits");
        assert_eq!(edit_usage.1, 20);
        assert_eq!(edit_usage.2, 30);
        assert_eq!(edit_usage.3, 50);

        let variation_usage = &usage_rows[6];
        assert_eq!(variation_usage.0, "openai/images_variations");
        assert_eq!(variation_usage.1, 20);
        assert_eq!(variation_usage.2, 30);
        assert_eq!(variation_usage.3, 50);

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

        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                DEFAULT_USER_API_KEY_VALUE,
                "Task9 Failure User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

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

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

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

    #[test]
    fn seaorm_openai_v1_failure_persists_terminal_request_and_execution_state() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (mut embedded_pg, dsn, data_dir) =
            runtime.block_on(start_embedded_postgres("task5-postgres-openai-failure"));

        let factory = SeaOrmConnectionFactory::postgres(dsn.clone());
        runtime.block_on(factory.connect_migrated()).unwrap();

        let bootstrap = SeaOrmBootstrapService::new(factory.clone().into(), "v0.9.20".to_owned());
        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let mut connection = PostgresClient::connect(&dsn, NoTls).unwrap();
        bootstrap_postgres_auth_fixture(&mut connection);

        connection
            .execute(
                "INSERT INTO channels (
                    created_at, updated_at, deleted_at, type, base_url, name, status, credentials,
                    disabled_api_keys, supported_models, manual_models, auto_sync_supported_models,
                    auto_sync_model_pattern, tags, default_test_model, policies, settings,
                    ordering_weight, error_message, remark
                ) VALUES (
                    NOW(), NOW(), 0, $1, $2, $3, 'enabled', $4,
                    '[]', $5, '[]', FALSE,
                    '', '[]', $6, '{}', '{}',
                    100, '', 'Task 5 SeaORM failure channel'
                )",
                &[
                    &"openai",
                    &format!("{}/primary-fail", mock_openai_server_url()),
                    &"SeaORM Failure Channel",
                    &r#"{"apiKey":"test-upstream-key"}"#,
                    &r#"["gpt-4o"]"#,
                    &"gpt-4o",
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO models (
                    created_at, updated_at, deleted_at, developer, model_id, type, name, icon,
                    \"group\", model_card, settings, status, remark
                ) VALUES (
                    NOW(), NOW(), 0, $1, $2, $3, $4, $5,
                    $6, $7, '{}', 'enabled', $8
                )",
                &[
                    &"openai",
                    &"gpt-4o",
                    &"chat",
                    &"GPT-4o",
                    &"OpenAI",
                    &"openai",
                    &r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0}}"#,
                    &"Task 5 SeaORM failure model",
                ],
            )
            .unwrap();

        let project_row = connection
            .query_one(
                "SELECT id, name, status FROM projects WHERE id = 1",
                &[],
            )
            .unwrap();
        let project = ProjectContext {
            id: project_row.get(0),
            name: project_row.get(1),
            status: project_row.get(2),
        };
        let trace_id: i64 = connection
            .query_one(
                "INSERT INTO traces (created_at, updated_at, project_id, trace_id)
                 VALUES (NOW(), NOW(), 1, $1)
                 RETURNING id",
                &[&"trace-task5-seaorm-failure"],
            )
            .unwrap()
            .get(0);

        let api_key_project = project.clone();
        let service = SeaOrmOpenAiV1Service::new(factory.clone());
        let error = service
            .execute(
                OpenAiV1Route::ChatCompletions,
                OpenAiV1ExecutionRequest {
                    headers: HashMap::new(),
                    body: OpenAiRequestBody::Json(serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": "fail me"}]
                    })),
                    path: "/v1/chat/completions".to_owned(),
                    path_params: HashMap::new(),
                    query: HashMap::new(),
                    project,
                    trace: Some(axonhub_http::TraceContext {
                        id: trace_id,
                        trace_id: "trace-task5-seaorm-failure".to_owned(),
                        project_id: 1,
                        thread_id: None,
                    }),
                    api_key: axonhub_http::AuthApiKeyContext {
                        id: 1,
                        key: DEFAULT_USER_API_KEY_VALUE.to_owned(),
                        name: "Task5 User Key".to_owned(),
                        key_type: axonhub_http::ApiKeyType::User,
                        project: api_key_project,
                        scopes: vec!["read_channels".to_owned(), "write_requests".to_owned()],
                        profiles_json: None,
                    },
                    api_key_id: Some(1),
                    client_ip: Some("127.0.0.1".to_owned()),
                    channel_hint_id: None,
                },
            )
            .unwrap_err();

        match error {
            axonhub_http::OpenAiV1Error::Upstream { status, body } => {
                assert_eq!(status, 503);
                assert_eq!(body["error"]["message"], "primary unavailable");
            }
            other => panic!("expected upstream error, got {other:?}"),
        }

        let request_row = connection
            .query_one(
                "SELECT status, channel_id, response_body FROM requests ORDER BY id DESC LIMIT 1",
                &[],
            )
            .unwrap();
        let request_row: (String, Option<i64>, Option<String>) = (
            request_row.get(0),
            request_row.get(1),
            request_row.get(2),
        );
        assert_eq!(request_row.0, "failed");
        assert!(request_row.1.is_some());
        assert!(request_row.2.unwrap_or_default().contains("primary unavailable"));

        let execution_row = connection
            .query_one(
                "SELECT status, channel_id, error_message, response_status_code, response_body
                 FROM request_executions ORDER BY id DESC LIMIT 1",
                &[],
            )
            .unwrap();
        let execution_row: (String, i64, String, Option<i64>, Option<String>) = (
            execution_row.get(0),
            execution_row.get(1),
            execution_row.get(2),
            execution_row.get(3),
            execution_row.get(4),
        );
        assert_eq!(execution_row.0, "failed");
        assert_eq!(execution_row.2, "primary unavailable");
        assert_eq!(execution_row.3, Some(503));
        assert!(execution_row.4.unwrap_or_default().contains("primary unavailable"));

        let usage_count: i64 = connection
            .query_one("SELECT COUNT(*) FROM usage_logs", &[])
            .unwrap()
            .get(0);
        assert_eq!(usage_count, 0);

        runtime.block_on(embedded_pg.stop_db()).ok();
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn openai_v1_route_denies_over_quota_api_key_without_success_accounting_side_effects() {
        let db_path = temp_sqlite_path("task9-openai-quota-denial");
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
                name: "OpenAI Quota Mock",
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
                remark: "Task 9 quota denial test",
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
                remark: "Task 9 quota denial model",
            })
            .unwrap();

        let quota_profiles = serde_json::json!({
            "activeProfile": "quota-hit",
            "profiles": [
                {
                    "name": "quota-hit",
                    "quota": {
                        "requests": 1,
                        "period": {
                            "type": "all_time"
                        }
                    }
                }
            ]
        })
        .to_string();
        {
            let connection = foundation.open_connection(true).unwrap();
            let api_key_id = insert_api_key(
                &connection,
                1,
                1,
                DEFAULT_USER_API_KEY_VALUE,
                "Task9 Quota User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
            connection
                .execute(
                    "UPDATE api_keys SET profiles = ?2 WHERE key = ?1",
                    params![DEFAULT_USER_API_KEY_VALUE, quota_profiles],
                )
                .unwrap();

            let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
            connection
                .execute(
                    "INSERT INTO usage_logs (
                        created_at, updated_at,
                        request_id, api_key_id, project_id, channel_id, model_id,
                        prompt_tokens, completion_tokens, total_tokens,
                        prompt_audio_tokens, prompt_cached_tokens, prompt_write_cached_tokens,
                        prompt_write_cached_tokens_5m, prompt_write_cached_tokens_1h,
                        completion_audio_tokens, completion_reasoning_tokens,
                        completion_accepted_prediction_tokens, completion_rejected_prediction_tokens,
                        source, format, total_cost, cost_items, cost_price_reference_id, deleted_at
                    ) VALUES (
                        '2000-01-01 00:00:00', '2000-01-01 00:00:00',
                        999, ?1, ?2, NULL, 'gpt-4o',
                        0, 0, 0,
                        0, 0, 0,
                        0, 0,
                        0, 0,
                        0, 0,
                        'api', 'openai/chat_completions', 0.0, '[]', '', 0
                    )",
                    params![api_key_id, project_id],
                )
                .unwrap();
        }

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task9-quota")
                    .header("AH-Trace-Id", "trace-task9-quota")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"deny me"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let json = read_json_response(response).await;
        assert_eq!(json["error"]["type"], "quota_exceeded_error");
        assert_eq!(json["error"]["code"], "quota_exceeded");
        assert_eq!(json["error"]["message"], "requests quota exceeded: 1/1");

        let connection = foundation.open_connection(false).unwrap();
        let request_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(request_count, 0);

        let execution_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM request_executions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(execution_count, 0);

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 1);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_image_generation_rejects_invalid_request_without_persistence_side_effects() {
        let db_path = temp_sqlite_path("task13-openai-image-invalid");
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

        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                DEFAULT_USER_API_KEY_VALUE,
                "Task13 Invalid Image User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Image Invalid Mock",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-image-1"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-image-1",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 13 invalid image test",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-image-1",
                model_type: "image",
                name: "GPT Image 1",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":8192,"output":0},"cost":{"input":1.0,"output":2.0}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 13 invalid image test",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/images/generations")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(r#"{"model":"gpt-image-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = read_json_response(response).await;
        assert_eq!(json["error"]["message"], "prompt is required");

        let connection = foundation.open_connection(false).unwrap();
        let request_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        let execution_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM request_executions", [], |row| row.get(0))
            .unwrap();
        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(request_count, 0);
        assert_eq!(execution_count, 0);
        assert_eq!(usage_count, 0);

        std::fs::remove_file(db_path).ok();
    }

    pub(crate) fn openai_v1_runtime_contract_preserved_inner() {
        openai_v1_fails_over_to_backup_channel_when_primary_fails();
        openai_v1_reuses_same_channel_for_repeated_trace_when_both_healthy();
        openai_v1_does_not_pin_later_healthy_non_affinity_requests_to_prior_failover_backup();
    }

    #[tokio::test]
    async fn openai_v1_fails_over_to_backup_channel_when_primary_fails() {
        let db_path = temp_sqlite_path("task8-openai-failover");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let api_key_value = "task8-failover-user-key";

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                api_key_value,
                "Task8 Failover User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

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

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", api_key_value)
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
        assert_eq!(execution_statuses.len(), 4);
        assert_eq!(
            execution_statuses[..3],
            [
                (execution_statuses[0].0, "failed".to_owned()),
                (execution_statuses[1].0, "failed".to_owned()),
                (execution_statuses[2].0, "failed".to_owned()),
            ]
        );
        assert_eq!(execution_statuses[0].1, "failed");
        assert_eq!(execution_statuses[3], (backup_channel_id, "completed".to_owned()));

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 1);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_retries_same_channel_before_failover_and_persists_attempts_once() {
        let db_path = temp_sqlite_path("task12-openai-same-channel-retry");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let api_key_value = "task12-retry-user-key";

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                api_key_value,
                "Task12 Retry User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

        let retry_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Retry Primary",
                channel_type: "openai",
                base_url: format!("{}/retry-twice-ok", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 200,
                error_message: "",
                remark: "Task 12 retry primary",
            })
            .unwrap();
        let backup_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Retry Backup",
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
                remark: "Task 12 retry backup",
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
                remark: "Task 12 retry model",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", api_key_value)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task12-same-channel")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"retry then succeed"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json_response(response).await;
        assert_eq!(json["id"], "chatcmpl_retry_same_channel");

        let connection = foundation.open_connection(false).unwrap();
        let request_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(request_count, 1);

        let request_channel_id: i64 = connection
            .query_row("SELECT channel_id FROM requests ORDER BY id DESC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(request_channel_id, retry_channel_id);
        assert_ne!(request_channel_id, backup_channel_id);

        let execution_rows: Vec<(i64, String, Option<i64>, String)> = {
            let mut statement = connection
                .prepare(
                    "SELECT channel_id, status, response_status_code, request_body FROM request_executions ORDER BY id ASC",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(execution_rows.len(), 3);
        assert!(execution_rows
            .iter()
            .all(|(channel_id, _, _, _)| *channel_id == retry_channel_id));
        assert_eq!(
            execution_rows
                .iter()
                .map(|(_, status, _, _)| status.clone())
                .collect::<Vec<_>>(),
            vec!["failed", "failed", "completed"]
        );
        assert_eq!(execution_rows[0].2, Some(503));
        assert_eq!(execution_rows[1].2, Some(503));
        assert_eq!(execution_rows[2].2, Some(200));
        assert!(execution_rows
            .iter()
            .all(|(_, _, _, request_body)| request_body.contains("\"model\":\"gpt-4o\"")));

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 1);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_circuit_breaker_triggers_failover_after_threshold() {
        let db_path = temp_sqlite_path("task17-circuit-breaker-failover");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let api_key_value = "task17-breaker-user-key";

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                api_key_value,
                "Task17 Breaker User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

        let primary_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task17 Primary Fail",
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
                remark: "Task 17 breaker primary",
            })
            .unwrap();
        let backup_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task17 Backup Healthy",
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
                remark: "Task 17 breaker backup",
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
                remark: "Task 17 breaker model",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
            openai: Arc::new(SqliteOpenAiV1Service::new_with_circuit_breaker_policy(
                foundation.clone(),
                CircuitBreakerPolicy {
                    half_open_threshold: 2,
                    open_threshold: 3,
                    reset_window: std::time::Duration::from_secs(60),
                },
            )),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        for attempt in 0..3 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/chat/completions")
                        .method(Method::POST)
                        .header("content-type", "application/json")
                        .header("X-API-Key", api_key_value)
                        .header("X-Project-ID", "gid://axonhub/project/1")
                        .header("AH-Trace-Id", format!("trace-task17-open-{attempt}"))
                        .body(Body::from(
                            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"trip breaker"}]}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let json = read_json_response(response).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(json["id"], "chatcmpl_backup");
        }

        let connection = foundation.open_connection(false).unwrap();
        let last_request_channel_id: i64 = connection
            .query_row("SELECT channel_id FROM requests ORDER BY id DESC LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(last_request_channel_id, backup_channel_id);
        assert_ne!(last_request_channel_id, primary_channel_id);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_circuit_breaker_recovers_after_reset_window() {
        let db_path = temp_sqlite_path("task17-circuit-breaker-recovery");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let api_key_value = "task17-recovery-user-key";

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            ensure_operational_tables(&connection).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                api_key_value,
                "Task17 Recovery User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task17 Recovery Primary",
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
                remark: "Task 17 recovery primary",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task17 Recovery Backup",
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
                remark: "Task 17 recovery backup",
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
                remark: "Task 17 recovery model",
            })
            .unwrap();

        let policy = CircuitBreakerPolicy {
            half_open_threshold: 1,
            open_threshold: 2,
            reset_window: std::time::Duration::from_millis(25),
        };
        let circuit_breaker = SharedCircuitBreaker::new(policy.clone());
        let openai = Arc::new(SqliteOpenAiV1Service::new_with_circuit_breaker(
            foundation.clone(),
            circuit_breaker.clone(),
        ));

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        openai_v1: OpenAiV1Capability::Available { openai },
        admin: AdminCapability::Available {
            admin: Arc::new(SqliteAdminService::new(foundation.clone())),
        },
        admin_graphql: AdminGraphqlCapability::Available {
            graphql: Arc::new(SqliteAdminGraphqlService::new(foundation.clone())),
        },
        openapi_graphql: OpenApiGraphqlCapability::Unsupported {
            message: "test-only unsupported openapi graphql".to_owned(),
        },
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "test-only unsupported provider-edge admin".to_owned(),
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        for attempt in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/chat/completions")
                        .method(Method::POST)
                        .header("content-type", "application/json")
                        .header("X-API-Key", api_key_value)
                        .header("X-Project-ID", "gid://axonhub/project/1")
                        .header("AH-Trace-Id", format!("trace-task17-recovery-open-{attempt}"))
                        .body(Body::from(
                            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"open breaker"}]}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let _json = read_json_response(response).await;
            assert_eq!(status, StatusCode::OK);
        }

        std::thread::sleep(std::time::Duration::from_millis(40));

        let snapshot = circuit_breaker
            .current_snapshot(1, "gpt-4o")
            .expect("breaker snapshot should exist after reset window");
        assert_eq!(snapshot.state.as_str(), "half_open");
        assert_eq!(snapshot.model_id, "gpt-4o");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_outbound_stage_failure_stops_before_upstream_dispatch() {
        let db_path = temp_sqlite_path("task14-openai-outbound-stage-failure");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let api_key_value = "task14-stage-failure-user-key";

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                api_key_value,
                "Task14 Stage Failure User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

        let channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Task14 Invalid Header",
                channel_type: "openai",
                base_url: format!("{}/backup", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: "{\"apiKey\":\"bad\\nkey\"}",
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 14 invalid outbound header stage",
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
                remark: "Task 14 invalid outbound header model",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", api_key_value)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"fail before send"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = read_json_response(response).await;
        assert!(json["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Invalid upstream authorization header"));

        let connection = foundation.open_connection(false).unwrap();
        let request_row: (String, Option<i64>) = connection
            .query_row(
                "SELECT status, channel_id FROM requests ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(request_row.0, "failed");
        assert_eq!(request_row.1, Some(channel_id));

        let execution_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM request_executions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(execution_count, 0);

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 0);

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

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                DEFAULT_USER_API_KEY_VALUE,
                "Task8 Trace Affinity User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

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

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

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
        let request_project = ProjectContext {
            id: project.id,
            name: project.name.clone(),
            status: project.status.clone(),
        };
        let api_key_project = ProjectContext {
            id: project.id,
            name: project.name,
            status: project.status,
        };
        let response = service
            .execute(
                OpenAiV1Route::ChatCompletions,
                OpenAiV1ExecutionRequest {
                    headers: HashMap::new(),
                    body: OpenAiRequestBody::Json(serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": "hello"}]
                    })),
                    path: "/admin/playground/chat".to_owned(),
                    path_params: HashMap::new(),
                    query: HashMap::new(),
                    project: request_project,
                    trace: None,
                    api_key: axonhub_http::AuthApiKeyContext {
                        id: 1,
                        key: DEFAULT_USER_API_KEY_VALUE.to_owned(),
                        name: "Task8 User Key".to_owned(),
                        key_type: axonhub_http::ApiKeyType::User,
                        project: api_key_project,
                        scopes: vec!["read_channels".to_owned(), "write_requests".to_owned()],
                        profiles_json: None,
                    },
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

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                DEFAULT_USER_API_KEY_VALUE,
                "Task8 Selection Repair User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
        }

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

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

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
        let content_dir = std::env::temp_dir().join(format!(
            "axonhub-task12-video-content-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&content_dir).unwrap();

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                DEFAULT_USER_API_KEY_VALUE,
                "Task12 Gemini Doubao User Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
            connection
                .execute(
                    "INSERT INTO data_storages (id, name, description, \"primary\", type, settings, status, deleted_at)
                     VALUES (?1, ?2, ?3, 0, 'fs', ?4, 'active', 0)",
                    params![
                        200,
                        "Task12 Video Storage",
                        "task12 video storage",
                        serde_json::json!({"directory": content_dir.to_string_lossy()}).to_string(),
                    ],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO systems (key, value, deleted_at) VALUES (?1, ?2, 0)",
                    params![
                        "system_video_storage_settings",
                        serde_json::json!({
                            "enabled": true,
                            "data_storage_id": 200,
                            "scan_interval_minutes": 1,
                            "scan_limit": 50
                        })
                        .to_string(),
                    ],
                )
                .unwrap();
        }

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

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

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
        assert_eq!(
            doubao_get_json["content"]["video_url"],
            format!("{}/generated.mp4", mock_openai_server_url())
        );

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
        assert_eq!(unsupported.status(), StatusCode::NOT_FOUND);

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

        let doubao_video_request: (i64, i64, i64, Option<String>, Option<i64>, Option<String>) = connection
            .query_row(
                "SELECT id, project_id, content_saved, content_storage_key, content_storage_id, content_saved_at
                 FROM requests WHERE format = 'doubao/video_get' ORDER BY id DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(doubao_video_request.2, 0);
        assert!(doubao_video_request.4.is_none());
        assert!(doubao_video_request.3.is_none());
        assert!(doubao_video_request.5.is_none());

        fs::remove_dir_all(content_dir).ok();
        std::fs::remove_file(db_path).ok();
    }

    fn mock_openai_server_url() -> &'static str {
        static SERVER_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SERVER_URL
            .get_or_init(|| {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                thread::spawn(move || {
                    let mut request_counts: HashMap<String, usize> = HashMap::new();
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
                        let request_key = format!("{method} {path}");
                        let request_count = request_counts.entry(request_key).or_insert(0);
                        *request_count += 1;
                        let request_count = *request_count;
                        if method == "GET" && path == "/v1/generated.mp4" {
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                b"mock-video-bytes".len(),
                                String::from_utf8_lossy(b"mock-video-bytes")
                            );
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        }
                        let body = if path.contains("/primary-fail/") && path.ends_with("/chat/completions") {
                            r#"{"error":{"message":"primary unavailable"}}"#.to_owned()
                        } else if path.contains("/retry-twice-ok/") && path.ends_with("/chat/completions") {
                            if request_count <= 2 {
                                r#"{"error":{"message":"retry me later"}}"#.to_owned()
                            } else {
                                r#"{"id":"chatcmpl_retry_same_channel","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"retried"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#.to_owned()
                            }
                        } else if path.contains("/compressed/") && path.ends_with("/chat/completions") {
                            if request_lower.contains("accept-encoding: identity") {
                                r#"{"id":"chatcmpl_compressed","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"compressed"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#.to_owned()
                            } else {
                                r#"{"error":{"message":"identity encoding required"}}"#.to_owned()
                            }
                        } else if method == "GET" && path.ends_with("/videos/video_mock_task") {
                            format!(
                                "{{\"id\":\"video_mock_task\",\"model\":\"seedance-1.0\",\"status\":\"succeeded\",\"content\":{{\"video_url\":\"{}/generated.mp4\"}},\"created_at\":1,\"completed_at\":2}}",
                                mock_openai_server_url()
                            )
                        } else if method == "DELETE" && path.ends_with("/videos/video_mock_task") {
                            "{\"id\":\"video_mock_task\"}".to_owned()
                        } else if method == "POST" && path.ends_with("/videos") {
                            "{\"id\":\"video_mock_task\"}".to_owned()
                        } else if path.contains("/backup/") && path.ends_with("/chat/completions") {
                            "{\"id\":\"chatcmpl_backup\",\"object\":\"chat.completion\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"backup\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}".to_owned()
                        } else if path.contains("/affinity-a/") && path.ends_with("/chat/completions") {
                            "{\"id\":\"chatcmpl_affinity_a\",\"object\":\"chat.completion\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"affinity-a\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}".to_owned()
                        } else if path.contains("/affinity-b/") && path.ends_with("/chat/completions") {
                            "{\"id\":\"chatcmpl_affinity_b\",\"object\":\"chat.completion\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"affinity-b\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}".to_owned()
                        } else if path.ends_with("/chat/completions") {
                            "{\"id\":\"chatcmpl_mock\",\"object\":\"chat.completion\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15,\"prompt_tokens_details\":{\"cached_tokens\":2},\"completion_tokens_details\":{\"reasoning_tokens\":1}}}".to_owned()
                        } else if path.ends_with("/responses/compact") {
                            "{\"id\":\"resp_compact_mock\",\"object\":\"response\",\"created_at\":1,\"model\":\"gpt-4o\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hi\",\"annotations\":[]}],\"status\":\"completed\"}],\"usage\":{\"input_tokens\":12,\"input_tokens_details\":{\"cached_tokens\":3,\"write_cached_tokens\":4,\"write_cached_5min_tokens\":4},\"output_tokens\":4,\"output_tokens_details\":{\"reasoning_tokens\":1,\"accepted_prediction_tokens\":2,\"rejected_prediction_tokens\":3},\"total_tokens\":16}}".to_owned()
                        } else if path.ends_with("/responses") {
                            "{\"id\":\"resp_mock\",\"object\":\"response\",\"created_at\":1,\"model\":\"gpt-4o\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hi\",\"annotations\":[]}],\"status\":\"completed\"}],\"usage\":{\"input_tokens\":12,\"input_tokens_details\":{\"cached_tokens\":3,\"write_cached_tokens\":4,\"write_cached_5min_tokens\":4},\"output_tokens\":4,\"output_tokens_details\":{\"reasoning_tokens\":1,\"accepted_prediction_tokens\":2,\"rejected_prediction_tokens\":3},\"total_tokens\":16}}".to_owned()
                        } else if path.ends_with("/images/generations")
                            || path.ends_with("/images/edits")
                            || path.ends_with("/images/variations") {
                            "{\"created\":1,\"data\":[{\"b64_json\":\"aGVsbG8=\",\"revised_prompt\":\"draw a cat\"}],\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":30,\"total_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":4},\"completion_tokens_details\":{\"reasoning_tokens\":2}}}".to_owned()
                        } else {
                            "{\"object\":\"list\",\"data\":[{\"object\":\"embedding\",\"embedding\":[0.1,0.2],\"index\":0}],\"model\":\"gpt-4o\",\"usage\":{\"prompt_tokens\":8,\"total_tokens\":8}}".to_owned()
                        };
                        let status_line = if path.contains("/primary-fail/") && path.ends_with("/chat/completions") {
                            "HTTP/1.1 503 Service Unavailable"
                        } else if path.contains("/retry-twice-ok/")
                            && path.ends_with("/chat/completions")
                            && request_count <= 2
                        {
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
    async fn admin_graphql_system_status_allows_no_scope_user_but_channels_requires_scope() {
        let db_path = temp_sqlite_path("task6-system-status-no-scope");
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

        // Create a user with NO scopes at all
        let _no_scope_user_id = insert_test_user(
            &connection,
            "no_scope@example.com",
            "password123",
            &[],
        );

        let app = graphql_test_app(foundation.clone(), bootstrap);
        let no_scope_token = signin_token(foundation.clone(), "no_scope@example.com", "password123");

        // Query systemStatus - should succeed even without any scopes
        let system_status_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {no_scope_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ systemStatus { isInitialized } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let system_status_json = read_json_response(system_status_response).await;
        assert_eq!(system_status_json["data"]["systemStatus"]["isInitialized"], true);
        assert!(system_status_json.get("errors").is_none_or(|v| v.is_null()));

        // Query channels - should fail with permission denied
        let channels_response = app
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
        let channels_json = read_json_response(channels_response).await;
        assert_eq!(channels_json["data"]["channels"], Value::Null);
        assert_eq!(channels_json["errors"][0]["message"], "permission denied");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn seaorm_admin_graphql_prompt_protection_crud_and_validation() {
        let db_path = temp_sqlite_path("task15-prompt-protection-graphql");
        let db = SeaOrmConnectionFactory::sqlite(db_path.display().to_string());
        let service = SeaOrmAdminGraphqlService::new(db.clone());
        let bootstrap = SeaOrmBootstrapService::new(db.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let connection = Connection::open(&db_path).unwrap();
        let admin_id = insert_test_user(
            &connection,
            "prompt-admin@example.com",
            "password123",
            &[SCOPE_READ_PROMPTS, SCOPE_WRITE_PROMPTS],
        );

        let admin = AuthUserContext {
            id: admin_id,
            email: "prompt-admin@example.com".to_owned(),
            first_name: "Prompt".to_owned(),
            last_name: "Admin".to_owned(),
            is_owner: false,
            prefer_language: "en".to_owned(),
            avatar: Some(String::new()),
            scopes: scope_strings(&[SCOPE_READ_PROMPTS, SCOPE_WRITE_PROMPTS]),
            roles: Vec::new(),
            projects: Vec::new(),
        };

        let created = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "mutation CreatePromptProtectionRule($input: CreatePromptProtectionRuleInput!) { createPromptProtectionRule(input: $input) { id name description pattern status settings { action replacement scopes } } }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({
                        "input": {
                            "name": "Mask Secret",
                            "description": "replace secrets",
                            "pattern": "secret",
                            "settings": {
                                "action": "mask",
                                "replacement": "[MASKED]",
                                "scopes": ["user"]
                            }
                        }
                    }),
                },
                None,
                admin.clone(),
            )
            .await;
        let created_rule = assert_graphql_success_field(&created.body, "createPromptProtectionRule");
        assert_eq!(created_rule["name"], "Mask Secret");
        assert_eq!(created_rule["status"], "disabled");
        assert_eq!(created_rule["settings"]["replacement"], "[MASKED]");
        let rule_id = created_rule["id"].as_str().unwrap().to_owned();

        let updated = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "mutation UpdatePromptProtectionRule($id: ID!, $input: UpdatePromptProtectionRuleInput!) { updatePromptProtectionRule(id: $id, input: $input) { id name pattern status settings { action replacement scopes } } }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({
                        "id": rule_id,
                        "input": {
                            "status": "enabled",
                            "pattern": "secret|token"
                        }
                    }),
                },
                None,
                admin.clone(),
            )
            .await;
        let updated_rule = assert_graphql_success_field(&updated.body, "updatePromptProtectionRule");
        assert_eq!(updated_rule["status"], "enabled");
        assert_eq!(updated_rule["pattern"], "secret|token");

        let listed = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "query GetPromptProtectionRules { promptProtectionRules { edges { node { id name status settings { action replacement scopes } } } totalCount } }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({}),
                },
                None,
                admin.clone(),
            )
            .await;
        let listed_conn = assert_graphql_success_field(&listed.body, "promptProtectionRules");
        assert_eq!(listed_conn["totalCount"], 1);
        assert_eq!(listed_conn["edges"][0]["node"]["name"], "Mask Secret");

        let invalid = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "mutation CreatePromptProtectionRule($input: CreatePromptProtectionRuleInput!) { createPromptProtectionRule(input: $input) { id } }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({
                        "input": {
                            "name": "Broken Mask",
                            "pattern": "(",
                            "settings": {
                                "action": "mask",
                                "scopes": []
                            }
                        }
                    }),
                },
                None,
                admin.clone(),
            )
            .await;
        assert_eq!(invalid.body["data"]["createPromptProtectionRule"], Value::Null);
        assert!(invalid.body["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("invalid prompt protection pattern"));

        let status_updated = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "mutation UpdatePromptProtectionRuleStatus($id: ID!, $status: PromptProtectionRuleStatus!) { updatePromptProtectionRuleStatus(id: $id, status: $status) }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({"id": updated_rule["id"], "status": "disabled"}),
                },
                None,
                admin.clone(),
            )
            .await;
        assert_eq!(status_updated.body["data"]["updatePromptProtectionRuleStatus"], true);

        let bulk_enabled = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "mutation BulkEnablePromptProtectionRules($ids: [ID!]!) { bulkEnablePromptProtectionRules(ids: $ids) }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({"ids": [updated_rule["id"].as_str().unwrap()]}),
                },
                None,
                admin.clone(),
            )
            .await;
        assert_eq!(bulk_enabled.body["data"]["bulkEnablePromptProtectionRules"], true);

        let deleted = service
            .execute_graphql(
                GraphqlRequestPayload {
                    query: "mutation DeletePromptProtectionRule($id: ID!) { deletePromptProtectionRule(id: $id) }".to_owned(),
                    operation_name: None,
                    variables: serde_json::json!({"id": updated_rule["id"]}),
                },
                None,
                admin,
            )
            .await;
        assert_eq!(deleted.body["data"]["deletePromptProtectionRule"], true);

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn openai_v1_prompt_protection_masks_user_content_before_upstream_request() {
        let db_path = temp_sqlite_path("task15-prompt-protection-mask");
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

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                "task15-mask-key",
                "Task15 Mask Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
            connection
                .execute(
                    "INSERT INTO prompt_protection_rules (name, description, pattern, status, settings, deleted_at)
                     VALUES (?1, ?2, ?3, 'enabled', ?4, 0)",
                    params![
                        "Mask Rule",
                        "mask user secret",
                        "secret",
                        serde_json::json!({"action":"mask","replacement":"[MASKED]","scopes":["user"]}).to_string()
                    ],
                )
                .unwrap();
        }

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task15 Mask Channel",
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
                remark: "Task 15 mask channel",
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
                remark: "Task 15 mask model",
            })
            .unwrap();

        let service = SqliteOpenAiV1Service::new(foundation.clone());
        let response = service
            .execute(
                OpenAiV1Route::ChatCompletions,
                OpenAiV1ExecutionRequest {
                    headers: HashMap::new(),
                    body: OpenAiRequestBody::Json(serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": "my secret token"}]
                    })),
                    path: "/v1/chat/completions".to_owned(),
                    path_params: HashMap::new(),
                    query: HashMap::new(),
                    project: ProjectContext {
                        id: 1,
                        name: "Default Project".to_owned(),
                        status: "active".to_owned(),
                    },
                    trace: None,
                    api_key: axonhub_http::AuthApiKeyContext {
                        id: 11,
                        key: "task15-mask-key".to_owned(),
                        name: "Task15 Mask Key".to_owned(),
                        key_type: axonhub_http::ApiKeyType::User,
                        project: ProjectContext {
                            id: 1,
                            name: "Default Project".to_owned(),
                            status: "active".to_owned(),
                        },
                        scopes: vec!["read_channels".to_owned(), "write_requests".to_owned()],
                        profiles_json: None,
                    },
                    api_key_id: Some(11),
                    client_ip: None,
                    channel_hint_id: None,
                },
            )
            .unwrap();
        assert_eq!(response.status, StatusCode::OK.as_u16());

        let request_body: String = foundation
            .open_connection(false)
            .unwrap()
            .query_row(
                "SELECT request_body FROM request_executions ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(request_body.contains("[MASKED]"));
        assert!(!request_body.contains("my secret token"));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_prompt_protection_rejects_blocked_user_content() {
        let db_path = temp_sqlite_path("task15-prompt-protection-reject");
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

        {
            let connection = foundation.open_connection(true).unwrap();
            insert_api_key(
                &connection,
                1,
                1,
                "task15-reject-key",
                "Task15 Reject Key",
                "user",
                &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS],
            );
            connection
                .execute(
                    "INSERT INTO prompt_protection_rules (name, description, pattern, status, settings, deleted_at)
                     VALUES (?1, ?2, ?3, 'enabled', ?4, 0)",
                    params![
                        "Reject Rule",
                        "reject blocked token",
                        "blocked",
                        serde_json::json!({"action":"reject","scopes":["user"]}).to_string()
                    ],
                )
                .unwrap();
        }

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task15 Reject Channel",
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
                remark: "Task 15 reject channel",
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
                remark: "Task 15 reject model",
            })
            .unwrap();

        let service = SqliteOpenAiV1Service::new(foundation.clone());
        let error = service
            .execute(
                OpenAiV1Route::ChatCompletions,
                OpenAiV1ExecutionRequest {
                    headers: HashMap::new(),
                    body: OpenAiRequestBody::Json(serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": "this is blocked"}]
                    })),
                    path: "/v1/chat/completions".to_owned(),
                    path_params: HashMap::new(),
                    query: HashMap::new(),
                    project: ProjectContext {
                        id: 1,
                        name: "Default Project".to_owned(),
                        status: "active".to_owned(),
                    },
                    trace: None,
                    api_key: axonhub_http::AuthApiKeyContext {
                        id: 12,
                        key: "task15-reject-key".to_owned(),
                        name: "Task15 Reject Key".to_owned(),
                        key_type: axonhub_http::ApiKeyType::User,
                        project: ProjectContext {
                            id: 1,
                            name: "Default Project".to_owned(),
                            status: "active".to_owned(),
                        },
                        scopes: vec!["read_channels".to_owned(), "write_requests".to_owned()],
                        profiles_json: None,
                    },
                    api_key_id: Some(12),
                    client_ip: None,
                    channel_hint_id: None,
                },
            )
            .unwrap_err();

        match error {
            axonhub_http::OpenAiV1Error::InvalidRequest { message } => {
                assert_eq!(message, "request blocked by prompt protection policy");
            }
            other => panic!("expected invalid request, got {other:?}"),
        }

        let execution_count: i64 = foundation
            .open_connection(false)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM request_executions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(execution_count, 0);

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
        let allowed_json = assert_graphql_status(allowed, StatusCode::OK).await;
        let allowed_key = assert_graphql_success_field(&allowed_json, "createLLMAPIKey");
        assert_eq!(allowed_key["name"], "SDK Key");
        assert_eq!(
            allowed_key["scopes"][0],
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
        let denied_json = assert_graphql_status(denied, StatusCode::OK).await;
        assert_graphql_error_field(&denied_json, "createLLMAPIKey", "permission denied");

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

        connection.execute(
            "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at)
             VALUES (?1, ?2, ?3, 'enabled', ?4, ?5, 0, '', ?6, ?7, 100, '', '', 0)",
            params![
                "openai",
                "https://models.example.test/v1",
                "Task12 SQLite QueryModels Channel",
                r#"{"apiKey":"test-upstream-key"}"#,
                r#"["gpt-4"]"#,
                r#"{"queryAllChannelModels":true}"#,
                "[]",
            ],
        ).unwrap();

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

        assert!(models.len() >= 2);

        for model in models {
            assert!(model["id"].is_string());
            assert!(model["status"].is_string());
        }

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_route_rejects_missing_channel_model_association_without_persistence_side_effects() {
        let db_path = temp_sqlite_path("task12-openai-missing-channel-model-association");
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
                remark: "Task 12 missing association model",
            })
            .unwrap();

        let app = router(HttpState { service_name: "AxonHub".to_owned(),
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
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: Vec::new(),
            extra_trace_body_fields: Vec::new(),
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task12-missing-association")
                    .header("AH-Trace-Id", "trace-task12-missing-association")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hello"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = read_json_response(response).await;
        assert_eq!(json["error"]["message"], "No enabled OpenAI channel is configured for the requested model");

        let connection = foundation.open_connection(false).unwrap();
        let request_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        let execution_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM request_executions", [], |row| row.get(0))
            .unwrap();
        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(request_count, 0);
        assert_eq!(execution_count, 0);
        assert_eq!(usage_count, 0);

        std::fs::remove_file(db_path).ok();
    }
