use super::*;
use crate::models::format_go_duration;
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::ServiceResponse;
use actix_web::http::header;
use actix_web::http::{Method, StatusCode};
use actix_web::test as actix_test;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

fn disabled_test_cors() -> HttpCorsSettings {
    HttpCorsSettings::default()
}

fn enabled_test_cors() -> HttpCorsSettings {
    HttpCorsSettings {
        enabled: true,
        debug: false,
        allowed_origins: vec!["https://allowed.example".to_owned()],
        allowed_methods: vec!["GET".to_owned(), "POST".to_owned(), "OPTIONS".to_owned()],
        allowed_headers: vec!["X-API-Key".to_owned(), "Content-Type".to_owned()],
        exposed_headers: vec!["X-Request-Id".to_owned()],
        allow_credentials: true,
        max_age_seconds: Some(1800),
    }
}

#[derive(Clone)]
struct TestApp {
    state: HttpState,
    http_metrics: HttpMetricsCapability,
}

impl TestApp {
    fn new(state: HttpState) -> Self {
        Self {
            state,
            http_metrics: HttpMetricsCapability::Disabled,
        }
    }

    fn with_metrics(state: HttpState, http_metrics: HttpMetricsCapability) -> Self {
        Self {
            state,
            http_metrics,
        }
    }

    async fn oneshot(&self, request: TestHttpRequest) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
        let app = actix_test::init_service(router_with_metrics_and_base_path(
            self.state.clone(),
            self.http_metrics.clone(),
            "/",
        ))
        .await;

        let mut actix_request = actix_test::TestRequest::default()
            .method(Method::from_bytes(request.method.as_bytes()).expect("valid method"))
            .uri(&request.uri);
        for (name, value) in &request.headers {
            actix_request = actix_request.insert_header((name.as_str(), value.as_str()));
        }

        Ok(actix_test::call_service(&app, actix_request.set_payload(request.body).to_request()).await)
    }
}

fn router(state: HttpState) -> TestApp {
    TestApp::new(state)
}

fn router_with_metrics(state: HttpState, http_metrics_capability: HttpMetricsCapability) -> TestApp {
    TestApp::with_metrics(state, http_metrics_capability)
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedHttpMetric {
    method: String,
    path: String,
    status_code: u16,
}

#[derive(Default)]
struct RecordingHttpMetrics {
    calls: Mutex<Vec<RecordedHttpMetric>>,
}

impl HttpMetricsRecorder for RecordingHttpMetrics {
    fn record_http_request(&self, method: &str, path: &str, status_code: u16, _duration: Duration) {
        self.calls.lock().unwrap().push(RecordedHttpMetric {
            method: method.to_owned(),
            path: path.to_owned(),
            status_code,
        });
    }
}

    #[derive(Default)]
    struct SharedSystemState {
        is_initialized: bool,
        query_fails: bool,
        initialize_error: Option<String>,
    }

    struct SharedSystemBootstrapPort {
        state: Mutex<SharedSystemState>,
    }

    impl SharedSystemBootstrapPort {
        fn new(state: SharedSystemState) -> Self {
            Self {
                state: Mutex::new(state),
            }
        }
    }

    impl SystemBootstrapPort for SharedSystemBootstrapPort {
        fn is_initialized(&self) -> Result<bool, SystemQueryError> {
            let state = self.state.lock().unwrap();
            if state.query_fails {
                return Err(SystemQueryError::QueryFailed);
            }
            Ok(state.is_initialized)
        }

        fn initialize(&self, _request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
            let mut state = self.state.lock().unwrap();
            if state.is_initialized {
                return Err(SystemInitializeError::AlreadyInitialized);
            }
            if let Some(message) = state.initialize_error.clone() {
                return Err(SystemInitializeError::InitializeFailed(message));
            }
            state.is_initialized = true;
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeAuthState {
        signin_internal: bool,
        jwt_internal: bool,
        api_internal: bool,
        project_internal: bool,
        thread_internal: bool,
        trace_internal: bool,
    }

    struct FakeAuthPort {
        state: Mutex<FakeAuthState>,
    }

    struct FakeOpenAiV1Port;

    struct FakeAdminPort;

    impl FakeAuthPort {
        fn new() -> Self {
            Self {
                state: Mutex::new(FakeAuthState::default()),
            }
        }
    }

    impl IdentityPort for FakeAuthPort {
        fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
            let state = self.state.lock().unwrap();
            if state.signin_internal {
                return Err(SignInError::Internal);
            }
            if request.email != "owner@example.com" || request.password != "password123" {
                return Err(SignInError::InvalidCredentials);
            }
            Ok(SignInSuccess {
                user: AuthUserContext {
                    id: 1,
                    email: request.email.clone(),
                    first_name: "System".to_owned(),
                    last_name: "Owner".to_owned(),
                    is_owner: true,
                    prefer_language: "en".to_owned(),
                    avatar: Some(String::new()),
                    scopes: vec!["write_users".to_owned()],
                    roles: vec![],
                    projects: vec![UserProjectInfo {
                        project_id: GlobalId {
                            resource_type: "project".to_owned(),
                            id: 1,
                        },
                        is_owner: true,
                        scopes: vec!["write_requests".to_owned()],
                        roles: vec![],
                    }],
                },
                token: "valid-admin-token".to_owned(),
            })
        }

        fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
            let state = self.state.lock().unwrap();
            if state.jwt_internal {
                return Err(AdminAuthError::Internal);
            }
            match token {
                "valid-admin-token" => Ok(AuthUserContext {
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
                }),
                "limited-admin-token" => Ok(AuthUserContext {
                    id: 2,
                    email: "admin@example.com".to_owned(),
                    first_name: "Admin".to_owned(),
                    last_name: "User".to_owned(),
                    is_owner: false,
                    prefer_language: "en".to_owned(),
                    avatar: Some(String::new()),
                    scopes: vec!["read_channels".to_owned()],
                    roles: vec![],
                    projects: vec![],
                }),
                _ => Err(AdminAuthError::InvalidToken),
            }
        }

        fn authenticate_api_key(
            &self,
            key: Option<&str>,
            allow_no_auth: bool,
        ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
            let state = self.state.lock().unwrap();
            if state.api_internal {
                return Err(ApiKeyAuthError::Internal);
            }
            match key {
                Some("api-key-123") => Ok(AuthApiKeyContext {
                    id: 10,
                    key: "api-key-123".to_owned(),
                    name: "User Key".to_owned(),
                    key_type: ApiKeyType::User,
                    project: ProjectContext {
                        id: 1,
                        name: "Default Project".to_owned(),
                        status: "active".to_owned(),
                    },
                    scopes: vec!["write_requests".to_owned()],
                }),
                Some("service-key-123") => Ok(AuthApiKeyContext {
                    id: 11,
                    key: "service-key-123".to_owned(),
                    name: "Service Key".to_owned(),
                    key_type: ApiKeyType::ServiceAccount,
                    project: ProjectContext {
                        id: 1,
                        name: "Default Project".to_owned(),
                        status: "active".to_owned(),
                    },
                    scopes: vec!["write_requests".to_owned()],
                }),
                Some("AXONHUB_API_KEY_NO_AUTH") => Err(ApiKeyAuthError::Invalid),
                Some(_) => Err(ApiKeyAuthError::Invalid),
                None if allow_no_auth => Ok(AuthApiKeyContext {
                    id: 12,
                    key: "AXONHUB_API_KEY_NO_AUTH".to_owned(),
                    name: "No Auth System Key".to_owned(),
                    key_type: ApiKeyType::NoAuth,
                    project: ProjectContext {
                        id: 1,
                        name: "Default Project".to_owned(),
                        status: "active".to_owned(),
                    },
                    scopes: vec!["write_requests".to_owned()],
                }),
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

    impl RequestContextPort for FakeAuthPort {
        fn resolve_project(&self, project_id: i64) -> Result<Option<ProjectContext>, ContextResolveError> {
            let state = self.state.lock().unwrap();
            if state.project_internal {
                return Err(ContextResolveError::Internal);
            }
            Ok((project_id == 1).then(|| ProjectContext {
                id: 1,
                name: "Default Project".to_owned(),
                status: "active".to_owned(),
            }))
        }

        fn resolve_thread(&self, project_id: i64, thread_id: &str) -> Result<Option<ThreadContext>, ContextResolveError> {
            let state = self.state.lock().unwrap();
            if state.thread_internal {
                return Err(ContextResolveError::Internal);
            }
            Ok((project_id == 1 && thread_id == "thread-1").then(|| ThreadContext {
                id: 100,
                thread_id: thread_id.to_owned(),
                project_id,
            }))
        }

        fn resolve_trace(
            &self,
            project_id: i64,
            trace_id: &str,
            thread_db_id: Option<i64>,
        ) -> Result<Option<TraceContext>, ContextResolveError> {
            let state = self.state.lock().unwrap();
            if state.trace_internal {
                return Err(ContextResolveError::Internal);
            }
            Ok((project_id == 1 && trace_id == "trace-1").then(|| TraceContext {
                id: 200,
                trace_id: trace_id.to_owned(),
                project_id,
                thread_id: thread_db_id,
            }))
        }
    }

    impl OpenAiV1Port for FakeOpenAiV1Port {
        fn list_models(&self, include: Option<&str>) -> Result<ModelListResponse, OpenAiV1Error> {
            let include_all = include == Some("all");
            Ok(ModelListResponse {
                object: "list",
                data: vec![OpenAiModel {
                    id: "gpt-4o".to_owned(),
                    object: "model",
                    created: 1,
                    owned_by: "openai".to_owned(),
                    name: include_all.then(|| "GPT-4o".to_owned()),
                    description: None,
                    icon: None,
                    r#type: include_all.then(|| "chat".to_owned()),
                    context_length: None,
                    max_output_tokens: None,
                    capabilities: None,
                    pricing: None,
                }],
            })
        }

        fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
            Ok(AnthropicModelListResponse {
                object: "list",
                data: vec![AnthropicModel {
                    id: "claude-3-5-sonnet-20241022".to_owned(),
                    kind: "model",
                    display_name: "Claude 3.5 Sonnet".to_owned(),
                    created: "2024-10-22T00:00:00Z".to_owned(),
                }],
                has_more: false,
                first_id: Some("claude-3-5-sonnet-20241022".to_owned()),
                last_id: Some("claude-3-5-sonnet-20241022".to_owned()),
            })
        }

        fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
            Ok(GeminiModelListResponse {
                models: vec![GeminiModel {
                    name: "models/gemini-2.5-flash".to_owned(),
                    base_model_id: "gemini-2.5-flash".to_owned(),
                    version: "gemini-2.5-flash-001".to_owned(),
                    display_name: "Gemini 2.5 Flash".to_owned(),
                    description: "Gemini 2.5 Flash".to_owned(),
                    supported_generation_methods: vec![
                        "generateContent",
                        "streamGenerateContent",
                    ],
                }],
            })
        }

        fn execute(
            &self,
            route: OpenAiV1Route,
            request: OpenAiV1ExecutionRequest,
        ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
            Ok(OpenAiV1ExecutionResponse {
                status: 200,
                    body: json!({
                        "id": match route {
                            OpenAiV1Route::ChatCompletions => "chatcmpl_rust",
                            OpenAiV1Route::Responses => "resp_rust",
                            OpenAiV1Route::ResponsesCompact => "resp_compact_rust",
                            OpenAiV1Route::Embeddings => "embed_rust",
                            OpenAiV1Route::ImagesGenerations => "imggen_rust",
                            #[allow(unreachable_patterns)]
                            _ => "other_rust",
                        },
                        "model": request.body["model"].clone(),
                        "project_id": request.project.id,
                        "channel_hint_id": request.channel_hint_id,
                        "path_params": request.path_params,
                    }),
                })
            }

        fn execute_compatibility(
            &self,
            route: CompatibilityRoute,
            request: OpenAiV1ExecutionRequest,
        ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
            let body = match route {
                CompatibilityRoute::AnthropicMessages => json!({
                    "id": "msg_rust",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "text", "text": "hello from rust"}],
                    "model": request.body["model"].clone(),
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 5,
                        "cache_creation_input_tokens": 0,
                        "cache_read_input_tokens": 0,
                        "cache_creation": {
                            "ephemeral_5m_input_tokens": 0,
                            "ephemeral_1h_input_tokens": 0
                        }
                    }
                }),
                CompatibilityRoute::JinaRerank => json!({
                    "model": request.body["model"].clone(),
                    "object": "list",
                    "results": [{"index": 0, "relevance_score": 0.99}],
                    "usage": {"prompt_tokens": 4, "total_tokens": 4}
                }),
                CompatibilityRoute::JinaEmbeddings => json!({
                    "object": "list",
                    "data": [{"object": "embedding", "embedding": [0.1, 0.2], "index": 0}],
                    "model": request.body["model"].clone(),
                    "usage": {"prompt_tokens": 4, "total_tokens": 4}
                }),
                CompatibilityRoute::GeminiGenerateContent => json!({
                    "candidates": [{
                        "content": {"role": "model", "parts": [{"text": "hello from gemini"}]},
                        "finishReason": "STOP",
                        "index": 0
                    }],
                    "modelVersion": request.body["model"].clone(),
                    "responseId": "gemini_resp",
                    "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5, "totalTokenCount": 15}
                }),
                CompatibilityRoute::GeminiStreamGenerateContent => json!({
                    "candidates": [{
                        "content": {"role": "model", "parts": [{"text": "hello from gemini stream"}]},
                        "finishReason": "STOP",
                        "index": 0
                    }],
                    "modelVersion": request.body["model"].clone(),
                    "responseId": "gemini_stream_resp",
                    "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5, "totalTokenCount": 15}
                }),
                CompatibilityRoute::DoubaoCreateTask => json!({"id": "task_rust"}),
                CompatibilityRoute::DoubaoGetTask => json!({
                    "id": "task_rust",
                    "model": "seedance-1.0",
                    "status": "succeeded",
                    "content": {"video_url": "https://example.com/video.mp4"},
                    "task_param": request.path_params.get("id").cloned(),
                    "usage": {"completion_tokens": 42, "total_tokens": 42},
                    "created_at": 1,
                    "updated_at": 2,
                    "resolution": "720p",
                    "ratio": "16:9"
                }),
                CompatibilityRoute::DoubaoDeleteTask => json!({"task_param": request.path_params.get("id").cloned()}),
            };

            Ok(OpenAiV1ExecutionResponse { status: 200, body })
        }
    }

    impl AdminPort for FakeAdminPort {
        fn download_request_content(
            &self,
            project_id: i64,
            request_id: i64,
            _user: AuthUserContext,
        ) -> Result<AdminContentDownload, AdminError> {
            if project_id != 1 || request_id != 42 {
                return Err(AdminError::NotFound {
                    message: "Request not found".to_owned(),
                });
            }

            Ok(AdminContentDownload {
                filename: "video.mp4".to_owned(),
                bytes: b"video-content".to_vec(),
            })
        }
    }

    fn test_state(system_bootstrap: SystemBootstrapCapability, allow_no_auth: bool) -> HttpState {
        HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap,
            identity: IdentityCapability::Available {
                identity: Arc::new(FakeAuthPort::new()),
            },
            request_context: RequestContextCapability::Available {
                request_context: Arc::new(FakeAuthPort::new()),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "OpenAI `/v1` inference is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(FakeAdminPort),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "DB-backed admin GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
            },
            allow_no_auth,
            cors: disabled_test_cors(),
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: vec!["Sentry-Trace".to_owned()],
                extra_trace_body_fields: vec![],
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        }
    }

    fn test_state_with_request_context(
        system_bootstrap: SystemBootstrapCapability,
        allow_no_auth: bool,
    ) -> (HttpState, Arc<FakeAuthPort>) {
        let request_context = Arc::new(FakeAuthPort::new());
        let state = HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap,
            identity: IdentityCapability::Available {
                identity: Arc::new(FakeAuthPort::new()),
            },
            request_context: RequestContextCapability::Available {
                request_context: request_context.clone(),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "OpenAI `/v1` inference is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(FakeAdminPort),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "DB-backed admin GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
            },
            allow_no_auth,
            cors: disabled_test_cors(),
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: vec!["Sentry-Trace".to_owned()],
                extra_trace_body_fields: vec![],
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        };

        (state, request_context)
    }

    fn test_state_with_openai(system_bootstrap: SystemBootstrapCapability, allow_no_auth: bool) -> HttpState {
        let mut state = test_state(system_bootstrap, allow_no_auth);
        state.openai_v1 = OpenAiV1Capability::Available {
            openai: Arc::new(FakeOpenAiV1Port),
        };
        state
    }

async fn read_json<B>(response: ServiceResponse<B>) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn assert_json_response<B>(response: ServiceResponse<B>, expected_status: StatusCode) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    assert_eq!(response.status(), expected_status);
    read_json(response).await
}

async fn assert_rest_error_response<B>(
    response: ServiceResponse<B>,
    expected_status: StatusCode,
    expected_message: &str,
) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    let json = assert_json_response(response, expected_status).await;
    assert_eq!(json["error"]["message"], expected_message);
    json
}

async fn assert_not_implemented_boundary<B>(
    response: ServiceResponse<B>,
    expected_status: StatusCode,
    expected_route_family: &str,
    expected_path: &str,
    expected_method: &str,
) -> Value
where
    B: MessageBody + 'static,
    B::Error: std::fmt::Debug,
{
    let json = assert_json_response(response, expected_status).await;
    assert_eq!(json["error"], "not_implemented");
    assert_eq!(json["route_family"], expected_route_family);
    assert_eq!(json["path"], expected_path);
    assert_eq!(json["method"], expected_method);
    json
}

    #[tokio::test]
    async fn health_route_matches_legacy_go_contract() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        let keys = json
            .as_object()
            .expect("health object")
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["version"], "v0.9.20");
        assert_eq!(
            keys,
            BTreeSet::from([
                "build".to_owned(),
                "status".to_owned(),
                "timestamp".to_owned(),
                "uptime".to_owned(),
                "version".to_owned(),
            ])
        );
        assert!(json.get("service").is_none());
        assert!(json.get("backend").is_none());
        assert!(json.get("migration_status").is_none());
        assert!(json.get("api_parity").is_none());
        assert!(json.get("legacy_go_backend_present").is_none());
        assert!(json.get("config_source").is_none());

        let timestamp = json["timestamp"].as_str().expect("timestamp string");
        assert!(humantime::parse_rfc3339(timestamp).is_ok());

        let uptime = json["uptime"].as_str().expect("uptime string");
        assert_go_duration_shape(uptime);

        let build = json["build"].as_object().expect("build object");
        assert_eq!(build.get("version"), Some(&Value::String("v0.9.20".to_owned())));
        assert!(build.contains_key("go_version"));
        assert!(build.contains_key("platform"));
        assert!(build.contains_key("uptime"));
        assert_go_duration_shape(build["uptime"].as_str().expect("build uptime string"));
        assert_eq!(build.get("uptime"), Some(&Value::String(uptime.to_owned())));
        assert!(!build.contains_key("commit"));
        assert!(!build.contains_key("build_time"));
    }

    #[test]
    fn format_go_duration_matches_expected_shapes() {
        assert_eq!(format_go_duration(Duration::ZERO), "0s");
        assert_eq!(format_go_duration(Duration::from_nanos(999)), "999ns");
        assert_eq!(format_go_duration(Duration::from_nanos(1_500)), "1.5µs");
        assert_eq!(format_go_duration(Duration::from_nanos(1_500_000)), "1.5ms");
        assert_eq!(format_go_duration(Duration::from_millis(120)), "120ms");
        assert_eq!(format_go_duration(Duration::from_millis(1_234)), "1.234s");
        assert_eq!(
            format_go_duration(Duration::from_secs(62) + Duration::from_millis(500)),
            "1m2.5s"
        );
        assert_eq!(
            format_go_duration(Duration::from_secs(3_661) + Duration::from_millis(250)),
            "1h1m1.25s"
        );
    }

     #[tokio::test]
     async fn enabled_cors_adds_expected_response_headers_for_allowed_origin() {
         let mut state = test_state_with_openai(
             SystemBootstrapCapability::Available {
                 system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
             },
             false,
         );
         state.cors = enabled_test_cors();
         let app = router(state);

         let response = app
             .oneshot(
                 Request::builder()
                     .uri("/health")
                     .method(Method::GET)
                     .header(header::ORIGIN.as_str(), "https://allowed.example")
                     .body(Body::empty())
                     .unwrap(),
             )
             .await
             .unwrap();

         assert_eq!(response.status(), StatusCode::OK);
         assert_eq!(
             response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
             "https://allowed.example"
         );
         assert_eq!(
             response.headers().get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS).unwrap(),
             "true"
         );
         let expose_headers = response.headers()
             .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
             .unwrap()
             .to_str()
             .unwrap();
         let exposed: Vec<String> = expose_headers
             .split(',')
             .map(|s| s.trim().to_ascii_lowercase())
             .collect();
         assert_eq!(exposed, vec!["x-request-id".to_owned()]);
     }

     #[tokio::test]
     async fn enabled_cors_preflight_succeeds_on_configured_route() {
         let mut state = test_state_with_openai(
             SystemBootstrapCapability::Available {
                 system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
             },
             false,
         );
         state.cors = enabled_test_cors();
         let app = router(state);

         let response = app
             .oneshot(
                 Request::builder()
                     .uri("/v1/models")
                     .method(Method::OPTIONS)
                     .header(header::ORIGIN.as_str(), "https://allowed.example")
                     .header(header::ACCESS_CONTROL_REQUEST_METHOD.as_str(), "GET")
                     .header(
                         header::ACCESS_CONTROL_REQUEST_HEADERS.as_str(),
                         "X-API-Key, Content-Type",
                     )
                     .body(Body::empty())
                     .unwrap(),
             )
             .await
             .unwrap();

         assert_eq!(response.status(), StatusCode::OK);
         assert_eq!(
             response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
             "https://allowed.example"
         );

         let allow_methods = response.headers()
             .get(header::ACCESS_CONTROL_ALLOW_METHODS)
             .unwrap()
             .to_str()
             .unwrap();
         let methods: Vec<&str> = allow_methods.split(',').map(|s| s.trim()).collect();
         assert!(methods.contains(&"GET"));
         assert!(methods.contains(&"POST"));
         assert!(methods.contains(&"OPTIONS"));

         let allow_headers = response.headers()
             .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
             .unwrap()
             .to_str()
             .unwrap();
         let headers: Vec<String> = allow_headers
             .split(',')
             .map(|s| s.trim().to_ascii_lowercase())
             .collect();
         assert!(headers.contains(&"content-type".to_owned()));
         assert!(headers.contains(&"x-api-key".to_owned()));

         assert_eq!(
             response.headers().get(header::ACCESS_CONTROL_MAX_AGE).unwrap(),
             "1800"
         );
     }

    #[tokio::test]
    async fn disabled_cors_remains_inert_for_origin_requests() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .header(header::ORIGIN.as_str(), "https://allowed.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

     #[tokio::test]
     async fn enabled_cors_rejects_disallowed_origin_preflight() {
         let mut state = test_state_with_openai(
             SystemBootstrapCapability::Available {
                 system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
             },
             false,
         );
         state.cors = enabled_test_cors();
         let app = router(state);

         let response = app
             .oneshot(
                 Request::builder()
                     .uri("/v1/models")
                     .method(Method::OPTIONS)
                     .header(header::ORIGIN.as_str(), "https://blocked.example")
                     .header(header::ACCESS_CONTROL_REQUEST_METHOD.as_str(), "GET")
                     .body(Body::empty())
                     .unwrap(),
             )
             .await
             .unwrap();

         assert_eq!(response.status(), StatusCode::BAD_REQUEST);
         assert!(response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
     }

     #[tokio::test]
     async fn enabled_cors_preflight_works_under_non_root_base_path() {
          let mut state = test_state_with_openai(
              SystemBootstrapCapability::Available {
                  system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
             },
             false,
          );
          state.cors = enabled_test_cors();
          let app = actix_test::init_service(router_with_metrics_and_base_path(
              state,
              HttpMetricsCapability::Disabled,
              "/console",
          ))
          .await;

         let request = actix_test::TestRequest::default()
             .uri("/console/v1/models")
             .method(Method::OPTIONS)
             .insert_header((header::ORIGIN.as_str(), "https://allowed.example"))
             .insert_header((header::ACCESS_CONTROL_REQUEST_METHOD.as_str(), "POST"))
             .insert_header((
                 header::ACCESS_CONTROL_REQUEST_HEADERS.as_str(),
                 "X-API-Key, Content-Type",
             ))
             .to_request();

         let response = actix_test::call_service(&app, request).await;

         assert_eq!(response.status(), StatusCode::OK);
         assert_eq!(
             response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
             "https://allowed.example"
         );
         let allow_methods = response.headers()
             .get(header::ACCESS_CONTROL_ALLOW_METHODS)
             .unwrap()
             .to_str()
             .unwrap();
         let methods: Vec<String> = allow_methods
             .split(',')
             .map(|s| s.trim().to_owned())
             .collect();
         assert!(methods.contains(&"POST".to_owned()));
         assert!(methods.contains(&"OPTIONS".to_owned()));
     }

    #[tokio::test]
    async fn request_metrics_middleware_records_method_route_and_status() {
        let recorder = Arc::new(RecordingHttpMetrics::default());
        let app = router_with_metrics(
            test_state_with_openai(
                SystemBootstrapCapability::Available {
                    system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
                },
                false,
            ),
            HttpMetricsCapability::Available {
                recorder: recorder.clone(),
            },
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let calls = recorder.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            RecordedHttpMetric {
                method: "GET".to_owned(),
                path: "/health".to_owned(),
                status_code: 200,
            }
        );
    }

    #[tokio::test]
    async fn disabled_request_metrics_leave_requests_stable() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["status"], "healthy");
    }

fn assert_go_duration_shape(value: &str) {
    assert!(!value.is_empty(), "duration should not be empty");
    assert!(!value.contains(' '), "Go duration strings do not contain spaces: {value}");
    assert!(
        value.contains("ns")
            || value.contains("µs")
            || value.contains("ms")
            || value.contains('s')
            || value.contains('m')
            || value.contains('h'),
        "duration should contain a Go-style unit: {value}"
    );
}

    #[tokio::test]
    async fn sqlite_scoped_system_status_stays_truthful_501_when_capability_is_unsupported() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Unsupported {
                message: "DB-backed admin system status/bootstrap is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/status")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = assert_not_implemented_boundary(
            response,
            StatusCode::NOT_IMPLEMENTED,
            "/admin/system/status",
            "/admin/system/status",
            "GET",
        )
        .await;
        assert_eq!(
            json["message"],
            "DB-backed admin system status/bootstrap is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3."
        );
    }

    #[tokio::test]
    async fn signin_returns_user_and_token() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/auth/signin")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"owner@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["token"], "valid-admin-token");
        assert_eq!(json["user"]["email"], "owner@example.com");
    }

    #[tokio::test]
    async fn signin_rejects_invalid_json() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/auth/signin")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Invalid request format");
    }

    #[tokio::test]
    async fn signin_rejects_wrong_credentials() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/auth/signin")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"owner@example.com","password":"wrong"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Invalid email or password");
    }

    #[tokio::test]
    async fn signin_returns_internal_error_when_identity_unsupported() {
        let state = HttpState { service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: SystemBootstrapCapability::Available {
            system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
        },
        identity: IdentityCapability::Unsupported {
            message: "Identity service is not available in this deployment".to_owned(),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeAuthPort::new()),
        },
        openai_v1: OpenAiV1Capability::Unsupported {
            message: "OpenAI `/v1` inference is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        admin: AdminCapability::Available {
            admin: Arc::new(FakeAdminPort),
        },
        admin_graphql: AdminGraphqlCapability::Unsupported {
            message: "DB-backed admin GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        openapi_graphql: OpenApiGraphqlCapability::Unsupported {
            message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: vec!["Sentry-Trace".to_owned()],
            extra_trace_body_fields: vec![],
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  };
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/auth/signin")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"owner@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Internal server error");
    }

    #[tokio::test]
    async fn admin_route_requires_valid_jwt_before_unmatched_404() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/unported")
                    .method(Method::POST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/admin/unported")
                    .method(Method::POST)
                    .header("Authorization", "Bearer valid-admin-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::NOT_FOUND);
        let json = read_json(authorized).await;
        assert_eq!(json["error"], "not_found");
        assert_eq!(json["status"], 404);
        assert_eq!(json["message"], "The requested endpoint does not exist");
    }

    #[tokio::test]
    async fn admin_request_content_route_returns_download_when_supported() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/requests/42/content")
                    .method(Method::GET)
                    .header("Authorization", "Bearer valid-admin-token")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("video.mp4"));
        let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(body.as_ref(), b"video-content");
    }

    #[tokio::test]
    async fn admin_request_content_requires_project_context() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/requests/42/content")
                    .method(Method::GET)
                    .header("Authorization", "Bearer valid-admin-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Project ID not found in context");
    }

    #[tokio::test]
    async fn admin_request_content_rejects_missing_or_invalid_admin_context() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

         let missing_response = app
             .clone()
             .oneshot(
                 Request::builder()
                     .uri("/admin/requests/42/content")
                     .method(Method::GET)
                     .header("X-Project-ID", "gid://axonhub/project/1")
                     .body(Body::empty())
                     .unwrap(),
             )
             .await
             .unwrap();

        assert_eq!(missing_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(missing_response).await;
        assert_eq!(json["error"]["message"], "Authorization header is required");

        let invalid_response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/antigravity/oauth/start")
                    .method(Method::POST)
                    .header("Authorization", "Bearer invalid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(invalid_response).await;
        assert_eq!(json["error"]["message"], "Invalid token");
    }

    #[tokio::test]
    async fn admin_antigravity_oauth_exchange_rejects_missing_or_invalid_admin_context() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let missing_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/antigravity/oauth/exchange")
                    .method(Method::POST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(missing_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(missing_response).await;
        assert_eq!(json["error"]["message"], "Authorization header is required");

        let invalid_response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/antigravity/oauth/exchange")
                    .method(Method::POST)
                    .header("Authorization", "Bearer invalid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(invalid_response).await;
        assert_eq!(json["error"]["message"], "Invalid token");
    }

    #[tokio::test]
    async fn admin_codex_oauth_exchange_rejects_missing_or_invalid_admin_context() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let missing_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/codex/oauth/exchange")
                    .method(Method::POST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(missing_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(missing_response).await;
        assert_eq!(json["error"]["message"], "Authorization header is required");

        let invalid_response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/codex/oauth/exchange")
                    .method(Method::POST)
                    .header("Authorization", "Bearer invalid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(invalid_response).await;
        assert_eq!(json["error"]["message"], "Invalid token");
    }

    #[tokio::test]
    async fn openapi_graphql_rejects_missing_or_invalid_api_key_context() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", "Bearer invalid-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = assert_rest_error_response(
            response,
            StatusCode::UNAUTHORIZED,
            "Invalid API key",
        )
        .await;
        assert_eq!(json["error"]["message"], "Invalid API key");
    }

    #[tokio::test]
    async fn rest_assertion_helper_reports_boundary_mismatches_clearly() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/images/edits")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let json = assert_not_implemented_boundary(
            response,
            StatusCode::NOT_IMPLEMENTED,
            "/v1/*",
            "/v1/images/edits",
            "POST",
        )
        .await;
        assert_ne!(
            json["path"],
            Value::String("/v1/images/variations".to_owned()),
            "boundary helper should fail loudly if a later slice points at the wrong unsupported route"
        );
    }

    #[tokio::test]
    async fn openapi_route_rejects_malformed_bearer_header() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/playground")
                    .method(Method::GET)
                    .header("Authorization", "NotBearer abc123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(response).await;
        assert_eq!(
            json["error"]["message"],
            "Invalid token: Authorization header must start with 'Bearer '"
        );
    }

    #[tokio::test]
    async fn openapi_debug_context_preserves_service_api_key_context() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/debug/context")
                    .method(Method::GET)
                    .header("Authorization", "Bearer service-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-1")
                    .header("AH-Trace-Id", "trace-1")
                    .header("X-Request-Id", "req-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["auth"]["mode"], "api_key");
        assert_eq!(json["auth"]["api_key_id"], 11);
        assert_eq!(json["auth"]["api_key_type"], "service_account");
        assert_eq!(json["requestId"], "req-1");
        assert_eq!(json["project"]["id"], 1);
        assert_eq!(json["thread"]["threadId"], "thread-1");
        assert_eq!(json["trace"]["traceId"], "trace-1");
    }

    #[tokio::test]
    async fn openapi_debug_context_rejects_missing_or_invalid_api_key() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let missing_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi/debug/context")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(missing_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(missing_response).await;
        assert_eq!(json["error"]["message"], "Authorization header is required");

        let invalid_response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/debug/context")
                    .method(Method::GET)
                    .header("Authorization", "Bearer invalid-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(invalid_response).await;
        assert_eq!(json["error"]["message"], "Invalid API key");
    }

    #[tokio::test]
    async fn openapi_service_api_key_returns_500_when_identity_unsupported() {
        let state = HttpState { service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: SystemBootstrapCapability::Available {
            system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
        },
        identity: IdentityCapability::Unsupported {
            message: "Identity service is not available in this deployment".to_owned(),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeAuthPort::new()),
        },
        openai_v1: OpenAiV1Capability::Unsupported {
            message: "OpenAI `/v1` inference is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        admin: AdminCapability::Available {
            admin: Arc::new(FakeAdminPort),
        },
        admin_graphql: AdminGraphqlCapability::Unsupported {
            message: "DB-backed admin GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        openapi_graphql: OpenApiGraphqlCapability::Unsupported {
            message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: vec!["Sentry-Trace".to_owned()],
            extra_trace_body_fields: vec![],
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  };
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/debug/context")
                    .method(Method::GET)
                    .header("Authorization", "Bearer service-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Failed to validate API key");
    }

    #[tokio::test]
    async fn admin_request_content_neighboring_admin_surface_returns_404() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/requests/42")
                    .method(Method::GET)
                    .header("Authorization", "Bearer valid-admin-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = read_json(response).await;
        assert_eq!(json["error"], "not_found");
        assert_eq!(json["status"], 404);
        assert_eq!(json["message"], "The requested endpoint does not exist");
    }

    #[tokio::test]
    async fn api_key_no_auth_and_context_enrichment_work_on_v1_family() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            true,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::POST)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-1")
                    .header("AH-Trace-Id", "trace-1")
                    .header("X-Request-Id", "req-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["auth"]["mode"], "noauth");
        assert_eq!(json["auth"]["user_id"], Value::Null);
        assert_eq!(json["auth"]["api_key_id"], 12);
        assert_eq!(json["auth"]["api_key_type"], "noauth");
        assert_eq!(json["requestId"], "req-1");
        assert_eq!(json["project"]["id"], 1);
        assert_eq!(json["thread"]["threadId"], "thread-1");
        assert_eq!(json["trace"]["traceId"], "trace-1");
    }

    #[tokio::test]
    async fn invalid_project_header_is_rejected() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "not-a-gid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Invalid project ID");
    }

    #[tokio::test]
    async fn thread_resolution_internal_failure_is_fail_open() {
        let (state, request_context) = test_state_with_request_context(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        request_context.state.lock().unwrap().thread_internal = true;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["auth"]["mode"], "api_key");
        assert_eq!(json["project"]["id"], 1);
        assert_eq!(json["thread"], Value::Null);
        assert_eq!(json["trace"], Value::Null);
    }

    #[tokio::test]
    async fn trace_resolution_internal_failure_is_fail_open() {
        let (state, request_context) = test_state_with_request_context(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        request_context.state.lock().unwrap().trace_internal = true;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["auth"]["mode"], "api_key");
        assert_eq!(json["project"]["id"], 1);
        assert_eq!(json["thread"], Value::Null);
        assert_eq!(json["trace"], Value::Null);
    }

    #[tokio::test]
    async fn api_key_family_authenticates_before_unported_501() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(authorized).await;
        assert_eq!(json["route_family"], "/v1/*");
    }

    #[tokio::test]
    async fn generic_api_key_returns_500_when_identity_unsupported() {
        let state = HttpState { service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: SystemBootstrapCapability::Available {
            system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
        },
        identity: IdentityCapability::Unsupported {
            message: "Identity service is not available in this deployment".to_owned(),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeAuthPort::new()),
        },
        openai_v1: OpenAiV1Capability::Unsupported {
            message: "OpenAI `/v1` inference is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        admin: AdminCapability::Available {
            admin: Arc::new(FakeAdminPort),
        },
        admin_graphql: AdminGraphqlCapability::Unsupported {
            message: "DB-backed admin GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        openapi_graphql: OpenApiGraphqlCapability::Unsupported {
            message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: vec!["Sentry-Trace".to_owned()],
            extra_trace_body_fields: vec![],
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  };
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Failed to validate API key");
    }

    #[tokio::test]
    async fn gemini_query_key_authenticates_before_supported_models() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::OK);
        let json = read_json(authorized).await;
        assert_eq!(json["models"][0]["name"], "models/gemini-2.5-flash");
    }

    #[tokio::test]
    async fn gemini_versioned_generate_content_and_stream_routes_succeed() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let generate_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models/gemini-2.5-flash:generateContent?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"gemini-2.5-flash","contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(generate_response.status(), StatusCode::OK);
        let generate_json = read_json(generate_response).await;
        assert_eq!(generate_json["candidates"][0]["content"]["parts"][0]["text"], "hello from gemini");

        let stream_response = app
            .oneshot(
                Request::builder()
                    .uri("/v1beta/models/gemini-2.5-flash:streamGenerateContent?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"gemini-2.5-flash","contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stream_response.status(), StatusCode::OK);
        let body = actix_web::body::to_bytes(stream_response.into_body()).await.unwrap();
        let payload = String::from_utf8(body.to_vec()).unwrap();
        assert!(payload.contains("hello from gemini stream"));
        assert!(payload.starts_with('['));
    }

    #[tokio::test]
    async fn gemini_unsupported_count_tokens_stays_truthful_501() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models/gemini-2.5-flash:countTokens?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(response).await;
        assert_eq!(json["error"], "not_implemented");
        assert_eq!(json["route_family"], "/gemini/:gemini_api_version/*");
    }

    #[tokio::test]
    async fn gemini_auth_returns_invalid_api_key_message() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let missing_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(missing_response).await;
        assert_eq!(json["error"]["message"], "Invalid API key");

        let invalid_response = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models?key=invalid-key")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_response.status(), StatusCode::UNAUTHORIZED);
        let json = read_json(invalid_response).await;
        assert_eq!(json["error"]["message"], "Invalid API key");
    }

    #[tokio::test]
    async fn gemini_auth_returns_500_when_identity_unsupported() {
        let state = HttpState { service_name: "AxonHub".to_owned(),
        version: "v0.9.20".to_owned(),
        config_source: None,
        system_bootstrap: SystemBootstrapCapability::Available {
            system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
        },
        identity: IdentityCapability::Unsupported {
            message: "Identity service is not available in this deployment".to_owned(),
        },
        request_context: RequestContextCapability::Available {
            request_context: Arc::new(FakeAuthPort::new()),
        },
        openai_v1: OpenAiV1Capability::Unsupported {
            message: "OpenAI `/v1` inference is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        admin: AdminCapability::Available {
            admin: Arc::new(FakeAdminPort),
        },
        admin_graphql: AdminGraphqlCapability::Unsupported {
            message: "DB-backed admin GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        openapi_graphql: OpenApiGraphqlCapability::Unsupported {
            message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Rust replacement for this surface is currently supported only on sqlite3.".to_owned(),
        },
        provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
            message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
        }, allow_no_auth: false, cors: disabled_test_cors(), trace_config: TraceConfig {
            thread_header: Some("AH-Thread-Id".to_owned()),
            trace_header: Some("AH-Trace-Id".to_owned()),
            request_header: Some("X-Request-Id".to_owned()),
            extra_trace_headers: vec!["Sentry-Trace".to_owned()],
            extra_trace_body_fields: vec![],
            claude_code_trace_enabled: false,
            codex_trace_enabled: false,
        },  };
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = read_json(response).await;
        assert_eq!(json["error"]["message"], "Failed to validate API key");
    }

    #[tokio::test]
    async fn doubao_task_routes_succeed_while_neighboring_surface_stays_local_404() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .body(Body::from(r#"{"model":"seedance-1.0","content":[{"type":"text","text":"make a video"}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_json = read_json(create_response).await;
        assert_eq!(create_json["id"], "task_rust");

        let get_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks/task_rust")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_json = read_json(get_response).await;
        assert_eq!(get_json["id"], "task_rust");
        assert_eq!(get_json["content"]["video_url"], "https://example.com/video.mp4");
        assert_eq!(get_json["task_param"], "task_rust");

        let delete_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks/task_rust")
                    .method(Method::DELETE)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
        )
            .await
            .unwrap();
        assert_eq!(delete_response.status(), StatusCode::OK);
        let delete_json = read_json(delete_response).await;
        assert_eq!(delete_json["task_param"], "task_rust");

        let unsupported_response = app
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/status")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unsupported_response.status(), StatusCode::NOT_FOUND);
        let unsupported_json = read_json(unsupported_response).await;
        assert_eq!(unsupported_json["error"], "not_found");
        assert_eq!(unsupported_json["status"], 404);
        assert_eq!(unsupported_json["message"], "The requested endpoint does not exist");
    }

    #[tokio::test]
    async fn gemini_debug_context_route_matches_protected_handler() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/debug/context?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["auth"]["mode"], "api_key");
        assert_eq!(json["project"]["id"], 1);
    }

    #[tokio::test]
    async fn system_routes_keep_previous_behavior() {
        let app = router(test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let initialize_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/system/initialize")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"ownerEmail":"owner@example.com","ownerPassword":"password123","ownerFirstName":"System","ownerLastName":"Owner","brandName":"AxonHub"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initialize_response.status(), StatusCode::OK);

        let status_response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status_response.status(), StatusCode::OK);
        let json = read_json(status_response).await;
        assert_eq!(json, json!({"isInitialized": true}));
    }

    #[tokio::test]
    async fn openai_models_route_returns_real_payload_when_capability_available() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models?include=all")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["object"], "list");
        assert_eq!(json["data"][0]["id"], "gpt-4o");
        assert_eq!(json["data"][0]["name"], "GPT-4o");
    }

    #[tokio::test]
    async fn anthropic_routes_match_current_migration_slice() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let models_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/anthropic/v1/models")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(models_response.status(), StatusCode::OK);
        let models_json = read_json(models_response).await;
        assert_eq!(models_json["object"], "list");
        assert_eq!(models_json["data"][0]["id"], "claude-3-5-sonnet-20241022");

        let messages_response = app
            .oneshot(
                Request::builder()
                    .uri("/anthropic/v1/messages")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"claude-3-5-sonnet","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(messages_response.status(), StatusCode::OK);
        let messages_json = read_json(messages_response).await;
        assert_eq!(messages_json["id"], "msg_rust");
        assert_eq!(messages_json["type"], "message");
        assert_eq!(messages_json["model"], "claude-3-5-sonnet");
    }

    #[tokio::test]
    async fn openai_v1_messages_routes_use_anthropic_compatibility() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"claude-3-5-sonnet","messages":[{"role":"user","content":"hi"}],"max_tokens":16}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["id"], "msg_rust");
        assert_eq!(json["type"], "message");
        assert_eq!(json["model"], "claude-3-5-sonnet");
    }

    #[tokio::test]
    async fn anthropic_messages_extracts_claude_code_trace_from_metadata_user_id() {
        let mut state = test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        state.trace_config.claude_code_trace_enabled = true;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/anthropic/v1/messages")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .body(Body::from(
                        r#"{"model":"claude-3-5-sonnet","messages":[{"role":"user","content":"hi"}],"metadata":{"user_id":"user_aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd_account__session_7581b58b-1234-5678-9abc-def012345678"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["id"], "msg_rust");
    }

    #[tokio::test]
    async fn v1_messages_extracts_claude_code_trace_from_v2_metadata_user_id() {
        let mut state = test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        state.trace_config.claude_code_trace_enabled = true;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .body(Body::from(
                        r#"{"model":"claude-3-5-sonnet","messages":[{"role":"user","content":"hi"}],"metadata":{"user_id":"{\"device_id\":\"67bad5aabbccdd1122334455667788990011223344556677889900aabbccddee\",\"account_uuid\":\"\",\"session_id\":\"f25958b8-e75c-455d-8b40-f006d87cc2a4\"}"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["id"], "msg_rust");
    }

    #[tokio::test]
    async fn codex_session_header_extracts_trace_when_enabled() {
        let mut state = test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        state.trace_config.codex_trace_enabled = true;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("Session_id", "codex-session-123")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["id"], "chatcmpl_rust");
    }

    #[tokio::test]
    async fn debug_context_uses_extra_trace_header_before_body_fields() {
        let mut state = test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        state.trace_config.extra_trace_body_fields = vec!["trace_id".to_owned()];
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("Sentry-Trace", "trace-1")
                    .body(Body::from(r#"{"trace_id":"body-trace-1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["trace"]["traceId"], "trace-1");
    }

    #[tokio::test]
    async fn debug_context_extracts_trace_from_body_field_when_configured() {
        let mut state = test_state(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        );
        state.trace_config.extra_trace_headers = vec![];
        state.trace_config.extra_trace_body_fields = vec!["metadata.trace_id".to_owned()];
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/debug/context")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .body(Body::from(r#"{"metadata":{"trace_id":"trace-1"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["trace"]["traceId"], "trace-1");
    }

    #[tokio::test]
    async fn openai_v1_video_routes_use_existing_doubao_compatibility() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/videos")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"seedance-1.0","content":[{"type":"text","text":"make a video"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_json = read_json(create_response).await;
        assert_eq!(create_json["id"], "task_rust");

        let get_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/videos/task_rust")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_json = read_json(get_response).await;
        assert_eq!(get_json["id"], "task_rust");
        assert_eq!(get_json["content"]["video_url"], "https://example.com/video.mp4");
        assert_eq!(get_json["task_param"], "task_rust");

        let delete_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/videos/task_rust")
                    .method(Method::DELETE)
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
        let delete_body = actix_web::body::to_bytes(delete_response.into_body())
            .await
            .unwrap();
        assert!(delete_body.is_empty());
    }

    #[tokio::test]
    async fn jina_routes_match_current_migration_slice() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let embeddings_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/jina/v1/embeddings")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(r#"{"model":"jina-embeddings-v3","input":"hello"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(embeddings_response.status(), StatusCode::OK);
        let embeddings_json = read_json(embeddings_response).await;
        assert_eq!(embeddings_json["object"], "list");
        assert_eq!(embeddings_json["model"], "jina-embeddings-v3");

        let rerank_response = app
            .oneshot(
                Request::builder()
                    .uri("/jina/v1/rerank")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"jina-reranker-v2-base-multilingual","query":"hello","documents":["a"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(rerank_response.status(), StatusCode::OK);
        let rerank_json = read_json(rerank_response).await;
        assert_eq!(rerank_json["object"], "list");
        assert_eq!(rerank_json["model"], "jina-reranker-v2-base-multilingual");
        assert_eq!(rerank_json["results"][0]["relevance_score"], json!(0.99));
    }

    #[tokio::test]
    async fn v1_rerank_alias_uses_jina_compatibility() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/rerank")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", "api-key-123")
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .body(Body::from(
                        r#"{"model":"jina-reranker-v2-base-multilingual","query":"hello","documents":["a"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json(response).await;
        assert_eq!(json["object"], "list");
        assert_eq!(json["model"], "jina-reranker-v2-base-multilingual");
        assert_eq!(json["results"][0]["relevance_score"], json!(0.99));
    }

    #[tokio::test]
    async fn openai_chat_and_responses_routes_execute_after_auth_and_context() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        for (path, body, expected_id) in [
            (
                "/v1/chat/completions",
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                "chatcmpl_rust",
            ),
            (
                "/v1/responses",
                r#"{"model":"gpt-4o","input":"hi"}"#,
                "resp_rust",
            ),
            (
                "/v1/responses/compact",
                r#"{"model":"gpt-4o","input":"hi"}"#,
                "resp_compact_rust",
            ),
            (
                "/v1/embeddings",
                r#"{"model":"gpt-4o","input":"hi"}"#,
                "embed_rust",
            ),
            (
                "/v1/images/generations",
                r#"{"model":"gpt-image-1","prompt":"draw a cat"}"#,
                "imggen_rust",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .method(Method::POST)
                        .header("content-type", "application/json")
                        .header("X-API-Key", "api-key-123")
                        .header("X-Project-ID", "gid://axonhub/project/1")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            let json = read_json(response).await;
            assert_eq!(json["id"], expected_id);
            assert_eq!(json["project_id"], 1);
        }
    }

    #[tokio::test]
    async fn explicit_v1_boundary_routes_keep_501_when_openai_slice_is_enabled() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/images/edits")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(response).await;
        assert_eq!(json["error"], "not_implemented");
        assert_eq!(json["route_family"], "/v1/*");
        assert_eq!(json["path"], "/v1/images/edits");
        assert_eq!(json["method"], "POST");
    }

    #[tokio::test]
    async fn residual_non_target_v1_neighbors_keep_explicit_501_boundaries() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        for (method, path, header_name, header_value) in [
            (
                Method::POST,
                "/v1/images/edits",
                Some("X-API-Key"),
                Some("api-key-123"),
            ),
            (
                Method::POST,
                "/v1/images/variations",
                Some("X-API-Key"),
                Some("api-key-123"),
            ),
        ] {
            let mut request = Request::builder().uri(path).method(method);
            if let (Some(name), Some(value)) = (header_name, header_value) {
                request = request.header(name, value);
            }

            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
            let json = read_json(response).await;
            assert_eq!(json["error"], "not_implemented", "{path}");
            assert_eq!(json["route_family"], "/v1/*", "{path}");
            assert_eq!(json["path"], path, "{path}");
            assert_eq!(json["method"], "POST", "{path}");
        }
    }

    #[tokio::test]
    async fn aisdk_protocol_markers_keep_v1_requests_on_truthful_501_boundary() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        for (path, header_name, header_value, body) in [
            (
                "/v1/chat/completions",
                "X-Vercel-Ai-Ui-Message-Stream",
                "v1",
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
            ),
            (
                "/v1/responses",
                "X-Vercel-AI-Data-Stream",
                "v1",
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
                        .header("X-API-Key", "api-key-123")
                        .header(header_name, header_value)
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
            let json = read_json(response).await;
            assert_eq!(json["error"], "not_implemented", "{path}");
            assert_eq!(json["route_family"], "/v1/*", "{path}");
            assert_eq!(json["path"], path, "{path}");
            assert_eq!(json["method"], "POST", "{path}");
            assert_eq!(
                json["message"],
                "AiSDK compatibility is not supported in the Rust HTTP slice yet. Requests that opt into the Vercel AI SDK protocol via `X-Vercel-Ai-Ui-Message-Stream` or `X-Vercel-AI-Data-Stream` remain on the explicit `/v1/*` 501 boundary.",
                "{path}"
            );
            assert_eq!(json["migration_status"], "progressive cutover", "{path}");
            assert_eq!(json["legacy_go_backend_present"], false, "{path}");
        }
    }

    #[tokio::test]
    async fn v1_realtime_attempts_stay_on_truthful_v1_boundary() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/realtime")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(response).await;
        assert_eq!(json["error"], "not_implemented");
        assert_eq!(json["route_family"], "/v1/*");
        assert_eq!(json["method"], "GET");
        assert_eq!(json["path"], "/v1/realtime");
        assert_eq!(json["migration_status"], "progressive cutover");
        assert_eq!(json["legacy_go_backend_present"], false);
    }

    #[test]
    fn readme_unsupported_boundary_contract_matches_explicit_v1_guardrails() {
        let readme = include_str!("../../../README.md");

        for required_snippet in [
            "- **Image editing and image variants**: `POST /v1/images/edits`, `POST /v1/images/variations`, and other unmigrated image routes",
            "- **Realtime API**: no dedicated Rust realtime/WebSocket route family; `/v1/realtime` returns `501 Not Implemented`",
            "- **AiSDK compatibility**: Vercel AI SDK protocol requests remain unsupported; `/v1` requests with `X-Vercel-Ai-Ui-Message-Stream` or `X-Vercel-AI-Data-Stream` return `501 Not Implemented`",
        ] {
            assert!(
                readme.contains(required_snippet),
                "README unsupported-boundary contract missing snippet: {required_snippet}"
            );
        }

        let routes = include_str!("routes.rs");
        for required_route in [
            "web::resource(\"/images/edits\").route(web::to(explicit_v1_not_implemented_boundary))",
            "web::resource(\"/images/variations\").route(web::to(explicit_v1_not_implemented_boundary))",
            "web::resource(\"/realtime\").route(web::to(explicit_v1_not_implemented_boundary))",
        ] {
            assert!(
                routes.contains(required_route),
                "routes.rs explicit unsupported boundary missing: {required_route}"
            );
        }

        let handlers = include_str!("handlers/mod.rs");
        assert!(handlers.contains("X-Vercel-Ai-Ui-Message-Stream"));
        assert!(handlers.contains("X-Vercel-AI-Data-Stream"));
        assert!(handlers.contains("remain on the explicit `/v1/*` 501 boundary"));
    }

    #[tokio::test]
    async fn root_spa_routes_serve_embedded_index_html() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/totally-unported-surface")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "no-cache, no-store, must-revalidate"
        );
        let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let payload = String::from_utf8(body.to_vec()).unwrap();
        assert!(payload.contains("<div id=\"root\"></div>"));
        assert!(payload.contains("/assets/"));
    }

    #[tokio::test]
    async fn root_dotted_spa_routes_follow_go_fallback_behavior() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/totally-unported-surface.json")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "no-cache, no-store, must-revalidate"
        );
    }

    #[tokio::test]
    async fn root_static_assets_serve_embedded_files() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/favicon.ico")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "image/x-icon");
        let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        assert!(!body.is_empty());
    }

    #[tokio::test]
    async fn public_favicon_route_serves_embedded_icon_with_cache_headers() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/favicon")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "image/x-icon");
        assert_eq!(response.headers().get("cache-control").unwrap(), "public, max-age=3600");
    }

    #[tokio::test]
    async fn api_like_paths_do_not_fall_back_to_spa() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/unknown")
                    .method(Method::GET)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = read_json(response).await;
        assert_eq!(json["error"], "not_found");
        assert_eq!(json["status"], 404);
        assert_eq!(json["message"], "The requested endpoint does not exist");
    }

    #[tokio::test]
    async fn base_path_spa_routes_strip_prefix_before_embedded_lookup() {
        let app = actix_test::init_service(router_with_metrics_and_base_path(
            test_state_with_openai(
                SystemBootstrapCapability::Available {
                    system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
                },
                false,
            ),
            HttpMetricsCapability::Disabled,
            "/console",
        ))
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get()
                .uri("/console/workspace/settings")
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let payload = String::from_utf8(body.to_vec()).unwrap();
        assert!(payload.contains("<div id=\"root\"></div>"));
    }

    #[tokio::test]
    async fn scoped_api_families_keep_local_unmatched_boundaries() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        for (method, path, status, body_kind) in [
            (Method::GET, "/jina/v1/unknown", StatusCode::NOT_FOUND, "json"),
            (Method::GET, "/anthropic/v1/unknown", StatusCode::NOT_FOUND, "json"),
            (Method::GET, "/openapi/unknown", StatusCode::NOT_FOUND, "json"),
            (Method::GET, "/doubao/v3/unknown", StatusCode::NOT_FOUND, "json"),
        ] {
            let mut request = Request::builder().uri(path).method(method);
            if path.starts_with("/openapi") {
                request = request.header("Authorization", "Bearer service-key-123");
            } else {
                request = request.header("X-API-Key", "api-key-123");
            }

            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), status, "{path}");
            let json = read_json(response).await;
            match body_kind {
                "json" => assert_eq!(json["error"], "not_found", "{path}"),
                _ => unreachable!(),
            }
        }
    }
