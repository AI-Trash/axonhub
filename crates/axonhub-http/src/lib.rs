use axum::body;
use axum::extract::{rejection::JsonRejection, OriginalUri, Path, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub trait SystemBootstrapPort: Send + Sync {
    fn is_initialized(&self) -> Result<bool, SystemQueryError>;
    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError>;
}

pub trait AuthContextPort: Send + Sync {
    fn admin_signin(
        &self,
        request: &SignInRequest,
    ) -> Result<SignInSuccess, SignInError>;

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError>;

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError>;

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError>;

    fn resolve_project(&self, project_id: i64) -> Result<Option<ProjectContext>, ContextResolveError>;
    fn resolve_thread(&self, project_id: i64, thread_id: &str) -> Result<Option<ThreadContext>, ContextResolveError>;
    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError>;
}

pub trait OpenAiV1Port: Send + Sync {
    fn list_models(&self, include: Option<&str>) -> Result<ModelListResponse, OpenAiV1Error>;

    fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error>;

    fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error>;

    fn execute(
        &self,
        route: OpenAiV1Route,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error>;

    fn execute_compatibility(
        &self,
        route: CompatibilityRoute,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error>;
}

pub trait AdminPort: Send + Sync {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError>;
}

pub trait AdminGraphqlPort: Send + Sync {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>;
}

pub trait OpenApiGraphqlPort: Send + Sync {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>;
}

pub trait ProviderEdgeAdminPort: Send + Sync {
    fn start_codex_oauth(
        &self,
        request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError>;

    fn exchange_codex_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError>;

    fn start_claudecode_oauth(
        &self,
        request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError>;

    fn exchange_claudecode_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError>;

    fn start_antigravity_oauth(
        &self,
        request: &StartAntigravityOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError>;

    fn exchange_antigravity_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError>;

    fn start_copilot_oauth(
        &self,
        request: &StartCopilotOAuthRequest,
    ) -> Result<StartCopilotOAuthResponse, ProviderEdgeAdminError>;

    fn poll_copilot_oauth(
        &self,
        request: &PollCopilotOAuthRequest,
    ) -> Result<PollCopilotOAuthResponse, ProviderEdgeAdminError>;
}

#[derive(Clone)]
pub enum SystemBootstrapCapability {
    Unsupported { message: String },
    Available { system: Arc<dyn SystemBootstrapPort> },
}

#[derive(Clone)]
pub enum AuthContextCapability {
    Unsupported { message: String },
    Available { auth: Arc<dyn AuthContextPort> },
}

#[derive(Clone)]
pub enum OpenAiV1Capability {
    Unsupported { message: String },
    Available { openai: Arc<dyn OpenAiV1Port> },
}

#[derive(Clone)]
pub enum AdminCapability {
    Unsupported { message: String },
    Available { admin: Arc<dyn AdminPort> },
}

#[derive(Clone)]
pub enum AdminGraphqlCapability {
    Unsupported { message: String },
    Available { graphql: Arc<dyn AdminGraphqlPort> },
}

#[derive(Clone)]
pub enum OpenApiGraphqlCapability {
    Unsupported { message: String },
    Available { graphql: Arc<dyn OpenApiGraphqlPort> },
}

#[derive(Clone)]
pub enum ProviderEdgeAdminCapability {
    Unsupported { message: String },
    Available { provider_edge: Arc<dyn ProviderEdgeAdminPort> },
}

#[derive(Debug, Clone, Copy)]
pub enum SystemQueryError {
    QueryFailed,
}

#[derive(Debug, Clone)]
pub enum SystemInitializeError {
    AlreadyInitialized,
    InitializeFailed(String),
}

#[derive(Debug, Clone)]
pub enum SignInError {
    InvalidCredentials,
    Internal,
}

#[derive(Debug, Clone)]
pub enum AdminAuthError {
    InvalidToken,
    Internal,
}

#[derive(Debug, Clone)]
pub enum ApiKeyAuthError {
    Missing,
    Invalid,
    Internal,
}

#[derive(Debug, Clone)]
pub enum ContextResolveError {
    Internal,
}

#[derive(Debug, Clone)]
pub enum OpenAiV1Error {
    InvalidRequest { message: String },
    Upstream {
        status: u16,
        body: Value,
    },
    Internal { message: String },
}

#[derive(Debug, Clone)]
pub enum AdminError {
    BadRequest { message: String },
    NotFound { message: String },
    Internal { message: String },
}

#[derive(Debug, Clone)]
pub enum ProviderEdgeAdminError {
    InvalidRequest { message: String },
    BadGateway { message: String },
    Internal { message: String },
}

#[derive(Clone)]
pub struct HttpState {
    pub service_name: String,
    pub version: String,
    pub config_source: Option<String>,
    pub system_bootstrap: SystemBootstrapCapability,
    pub auth_context: AuthContextCapability,
    pub openai_v1: OpenAiV1Capability,
    pub admin: AdminCapability,
    pub admin_graphql: AdminGraphqlCapability,
    pub openapi_graphql: OpenApiGraphqlCapability,
    pub provider_edge_admin: ProviderEdgeAdminCapability,
    pub allow_no_auth: bool,
    pub trace_config: TraceConfig,
}

#[derive(Debug, Clone, Default)]
pub struct TraceConfig {
    pub thread_header: Option<String>,
    pub trace_header: Option<String>,
    pub request_header: Option<String>,
    pub extra_trace_headers: Vec<String>,
    pub extra_trace_body_fields: Vec<String>,
    pub claude_code_trace_enabled: bool,
    pub codex_trace_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignInRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct SignInSuccess {
    pub user: AuthUserContext,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUserContext {
    pub id: i64,
    pub email: String,
    #[serde(rename = "firstName")]
    pub first_name: String,
    #[serde(rename = "lastName")]
    pub last_name: String,
    #[serde(rename = "isOwner")]
    pub is_owner: bool,
    #[serde(rename = "preferLanguage")]
    pub prefer_language: String,
    pub avatar: Option<String>,
    pub scopes: Vec<String>,
    pub roles: Vec<RoleInfo>,
    pub projects: Vec<UserProjectInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleInfo {
    pub name: String,
    #[serde(skip, default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProjectInfo {
    #[serde(rename = "projectID")]
    pub project_id: GlobalId,
    #[serde(rename = "isOwner")]
    pub is_owner: bool,
    pub scopes: Vec<String>,
    pub roles: Vec<RoleInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalId {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub id: i64,
}

#[derive(Debug, Clone)]
pub struct AuthApiKeyContext {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub key_type: ApiKeyType,
    pub project: ProjectContext,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyType {
    User,
    ServiceAccount,
    NoAuth,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectContext {
    pub id: i64,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ThreadContext {
    pub id: i64,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "projectId")]
    pub project_id: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceContext {
    pub id: i64,
    #[serde(rename = "traceId")]
    pub trace_id: String,
    #[serde(rename = "projectId")]
    pub project_id: i64,
    #[serde(rename = "threadId")]
    pub thread_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct RequestContextSnapshot {
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
    pub auth: Option<AuthSnapshot>,
    pub project: Option<ProjectContext>,
    pub thread: Option<ThreadContext>,
    pub trace: Option<TraceContext>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthSnapshot {
    pub mode: &'static str,
    #[serde(rename = "userId")]
    pub user_id: Option<i64>,
    #[serde(rename = "apiKeyId")]
    pub api_key_id: Option<i64>,
    #[serde(rename = "apiKeyType")]
    pub api_key_type: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
struct RequestContextState {
    request_id: Option<String>,
    auth: Option<RequestAuthContext>,
    project: Option<ProjectContext>,
    thread: Option<ThreadContext>,
    trace: Option<TraceContext>,
}

#[derive(Debug, Clone)]
enum RequestAuthContext {
    Admin(AuthUserContext),
    ApiKey(AuthApiKeyContext),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiV1Route {
    ChatCompletions,
    Responses,
    Embeddings,
}

impl OpenAiV1Route {
    pub fn format(self) -> &'static str {
        match self {
            Self::ChatCompletions => "openai/chat_completions",
            Self::Responses => "openai/responses",
            Self::Embeddings => "openai/embeddings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityRoute {
    AnthropicMessages,
    JinaRerank,
    JinaEmbeddings,
    GeminiGenerateContent,
    GeminiStreamGenerateContent,
    DoubaoCreateTask,
    DoubaoGetTask,
    DoubaoDeleteTask,
}

impl CompatibilityRoute {
    pub fn format(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic/message",
            Self::JinaRerank => "jina/rerank",
            Self::JinaEmbeddings => "jina/embedding",
            Self::GeminiGenerateContent => "gemini/generate_content",
            Self::GeminiStreamGenerateContent => "gemini/stream_generate_content",
            Self::DoubaoCreateTask => "doubao/video_create",
            Self::DoubaoGetTask => "doubao/video_get",
            Self::DoubaoDeleteTask => "doubao/video_delete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiV1ExecutionRequest {
    pub headers: HashMap<String, String>,
    pub body: Value,
    pub path: String,
    pub path_params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub project: ProjectContext,
    pub trace: Option<TraceContext>,
    pub api_key_id: Option<i64>,
    pub client_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenAiV1ExecutionResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone)]
pub struct AdminContentDownload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartPkceOAuthRequest {}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StartPkceOAuthResponse {
    pub session_id: String,
    pub auth_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeCallbackOAuthRequest {
    pub session_id: String,
    pub callback_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExchangeOAuthResponse {
    pub credentials: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartAntigravityOAuthRequest {
    #[serde(default)]
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartCopilotOAuthRequest {}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StartCopilotOAuthResponse {
    pub session_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PollCopilotOAuthRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PollCopilotOAuthResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphqlRequestPayload {
    pub query: String,
    #[serde(rename = "operationName")]
    pub operation_name: Option<String>,
    #[serde(default = "default_graphql_variables")]
    pub variables: Value,
}

#[derive(Debug, Clone)]
pub struct GraphqlExecutionResult {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelListResponse {
    pub object: &'static str,
    pub data: Vec<OpenAiModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicModelListResponse {
    pub object: &'static str,
    pub data: Vec<AnthropicModel>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicModel {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub display_name: String,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GeminiModelListResponse {
    pub models: Vec<GeminiModel>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GeminiModel {
    pub name: String,
    #[serde(rename = "baseModelId")]
    pub base_model_id: String,
    pub version: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub description: String,
    #[serde(rename = "supportedGenerationMethods")]
    pub supported_generation_methods: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiModel {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    #[serde(rename = "owned_by")]
    pub owned_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<ModelPricing>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelCapabilities {
    pub vision: bool,
    #[serde(rename = "tool_call")]
    pub tool_call: bool,
    pub reasoning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelPricing {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cache_read")]
    pub cache_read: f64,
    #[serde(rename = "cache_write")]
    pub cache_write: f64,
    pub unit: &'static str,
    pub currency: &'static str,
}

#[derive(Debug, Deserialize)]
struct GeminiQueryKey {
    key: Option<String>,
}

pub fn router(state: HttpState) -> Router {
    let admin_public = Router::new()
        .route("/system/status", get(system_status))
        .route("/system/initialize", post(initialize_system))
        .route("/auth/signin", post(sign_in));

    let admin_protected = Router::new()
        .route("/debug/context", get(debug_context))
        .route("/playground", get(admin_graphql_playground))
        .route("/graphql", post(admin_graphql))
        .route("/codex/oauth/start", post(start_codex_oauth))
        .route("/codex/oauth/exchange", post(exchange_codex_oauth))
        .route("/claudecode/oauth/start", post(start_claudecode_oauth))
        .route("/claudecode/oauth/exchange", post(exchange_claudecode_oauth))
        .route("/antigravity/oauth/start", post(start_antigravity_oauth))
        .route("/antigravity/oauth/exchange", post(exchange_antigravity_oauth))
        .route("/copilot/oauth/start", post(start_copilot_oauth))
        .route("/copilot/oauth/poll", post(poll_copilot_oauth))
        .route("/requests/:request_id/content", get(download_request_content))
        .route("/", any(unported_admin))
        .fallback(unported_admin)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_admin_jwt));

    let api_base = Router::new()
        .route("/debug/context", any(debug_context))
        .route("/models", get(list_openai_models))
        .route("/chat/completions", post(openai_chat_completions))
        .route("/responses", post(openai_responses))
        .route("/embeddings", post(openai_embeddings))
        .route("/", any(unported_v1))
        .fallback(unported_v1)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth));

    let jina_base = Router::new()
        .route("/debug/context", any(debug_context))
        .route("/embeddings", post(jina_embeddings))
        .route("/rerank", post(jina_rerank))
        .route("/", any(unported_jina_v1))
        .fallback(unported_jina_v1)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth));

    let anthropic_base = Router::new()
        .route("/debug/context", any(debug_context))
        .route("/messages", post(anthropic_messages))
        .route("/models", get(list_anthropic_models))
        .route("/", any(unported_anthropic_v1))
        .fallback(unported_anthropic_v1)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth));

    let v1beta_base = Router::new()
        .route("/debug/context", any(debug_context))
        .route("/models", get(list_gemini_models))
        .route("/models/*action", post(gemini_generate_content))
        .route("/", any(unported_v1beta))
        .fallback(unported_v1beta)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_gemini_key));

    let openapi_base = Router::new()
        .route("/debug/context", any(debug_context))
        .route("/v1/playground", get(openapi_graphql_playground))
        .route("/v1/graphql", post(openapi_graphql))
        .route("/", any(unported_openapi))
        .fallback(unported_openapi)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_service_api_key));

    Router::new()
        .route("/health", get(health))
        .nest("/admin", admin_public.merge(admin_protected))
        .nest("/v1", api_base)
        .nest("/jina/v1", jina_base)
        .nest("/anthropic/v1", anthropic_base)
        .route(
            "/doubao/v3/debug/context",
            any(debug_context)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth)),
        )
        .route(
            "/doubao/v3/contents/generations/tasks",
            post(doubao_create_task)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth)),
        )
        .route(
            "/doubao/v3/contents/generations/tasks/:id",
            get(doubao_get_task)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth)),
        )
        .route(
            "/doubao/v3/contents/generations/tasks/:id",
            delete(doubao_delete_task)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth)),
        )
        .route(
            "/gemini/v1/debug/context",
            any(debug_context)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/models",
            get(list_gemini_models)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/models/*action",
            post(gemini_generate_content)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/debug/context",
            any(debug_context)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/models",
            get(list_gemini_models)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/models/*action",
            post(gemini_generate_content)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/",
            any(unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/*rest",
            any(unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/",
            any(unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/*rest",
            any(unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/doubao/v3/",
            any(unported_doubao_v3)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth)),
        )
        .route(
            "/doubao/v3/*rest",
            any(unported_doubao_v3)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_api_key_or_no_auth)),
        )
        .nest("/v1beta", v1beta_base)
        .nest("/openapi", openapi_base)
        .with_state(state)
}

async fn require_admin_jwt(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = match extract_required_bearer_token(request.headers()) {
        Ok(token) => token,
        Err(response) => return response,
    };

    let auth = match &state.auth_context {
        AuthContextCapability::Unsupported { message } => return auth_unsupported_response(message),
        AuthContextCapability::Available { auth } => auth,
    };

    let user = match auth.authenticate_admin_jwt(token) {
        Ok(user) => user,
        Err(AdminAuthError::InvalidToken) => return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid token"),
        Err(AdminAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate token",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::Admin(user)),
        ..context
    });

    next.run(request).await
}

async fn require_api_key_or_no_auth(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth = match &state.auth_context {
        AuthContextCapability::Unsupported { message } => return auth_unsupported_response(message),
        AuthContextCapability::Available { auth } => auth,
    };

    let header_key = extract_api_key_from_headers(request.headers());
    let api_key = match auth.authenticate_api_key(header_key.as_deref(), state.allow_no_auth) {
        Ok(api_key) => api_key,
        Err(ApiKeyAuthError::Missing | ApiKeyAuthError::Invalid) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key")
        }
        Err(ApiKeyAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate API key",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::ApiKey(api_key)),
        ..context
    });

    next.run(request).await
}

async fn require_service_api_key(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth = match &state.auth_context {
        AuthContextCapability::Unsupported { message } => return auth_unsupported_response(message),
        AuthContextCapability::Available { auth } => auth,
    };

    let token = match extract_required_bearer_token(request.headers()) {
        Ok(token) => token,
        Err(response) => return response,
    };

    let api_key = match auth.authenticate_api_key(Some(token), false) {
        Ok(api_key) if api_key.key_type == ApiKeyType::ServiceAccount => api_key,
        Ok(_) | Err(ApiKeyAuthError::Missing | ApiKeyAuthError::Invalid) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key")
        }
        Err(ApiKeyAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate API key",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::ApiKey(api_key)),
        ..context
    });

    next.run(request).await
}

async fn require_gemini_key(
    State(state): State<HttpState>,
    Query(query): Query<GeminiQueryKey>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth = match &state.auth_context {
        AuthContextCapability::Unsupported { message } => return auth_unsupported_response(message),
        AuthContextCapability::Available { auth } => auth,
    };

    let header_key = extract_api_key_from_headers(request.headers());
    let api_key = match auth.authenticate_gemini_key(query.key.as_deref(), header_key.as_deref()) {
        Ok(api_key) => api_key,
        Err(ApiKeyAuthError::Missing | ApiKeyAuthError::Invalid) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "invalid api key")
        }
        Err(ApiKeyAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate API key",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::ApiKey(api_key)),
        ..context
    });

    next.run(request).await
}

async fn apply_request_context(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth = match &state.auth_context {
        AuthContextCapability::Unsupported { message } => {
            request.extensions_mut().insert(RequestContextState::default());
            let _ = message;
            return next.run(request).await;
        }
        AuthContextCapability::Available { auth } => auth,
    };

    let mut context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();

    let request_header = trace_request_header_name(&state.trace_config);
    context.request_id = request
        .headers()
        .get(&request_header)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let auth_project = context.auth.as_ref().and_then(|auth| match auth {
        RequestAuthContext::ApiKey(key) => Some(key.project.clone()),
        RequestAuthContext::Admin(_) => None,
    });

    let header_project = match parse_project_header(request.headers()) {
        Ok(Some(id)) => match auth.resolve_project(id) {
            Ok(project) => project,
            Err(ContextResolveError::Internal) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    "Failed to resolve project context",
                )
            }
        },
        Ok(None) => None,
        Err(response) => return response,
    };

    context.project = header_project.or(auth_project);

    if let Some(project) = context.project.as_ref() {
        if let Some(thread_id) = request_header_value(request.headers(), &trace_thread_header_name(&state.trace_config)) {
            if let Ok(thread) = auth.resolve_thread(project.id, thread_id) {
                context.thread = thread;
            }
        }

        if let Some(trace_id) = extract_trace_id(request.headers(), &state.trace_config) {
            let thread_db_id = context.thread.as_ref().map(|thread| thread.id);
            if let Ok(trace) = auth.resolve_trace(project.id, trace_id, thread_db_id) {
                context.trace = trace;
            }
        }
    }

    request.extensions_mut().insert(context);
    next.run(request).await
}

async fn health(State(state): State<HttpState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: state.service_name,
        version: state.version,
        backend: "rust",
        migration_status: "first migration slice",
        api_parity: "partial",
        legacy_go_backend_present: true,
        config_source: state.config_source,
    })
}

async fn system_status(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> impl IntoResponse {
    match &state.system_bootstrap {
        SystemBootstrapCapability::Unsupported { message } => {
            not_implemented_response("/admin/system/status", Method::GET, original_uri, None)
                .with_message(message)
        }
        SystemBootstrapCapability::Available { system } => match system.is_initialized() {
            Ok(is_initialized) => (
                StatusCode::OK,
                Json(SystemStatusResponse { is_initialized }),
            )
                .into_response(),
            Err(SystemQueryError::QueryFailed) => {
                internal_error_response("Failed to check system status".to_owned())
            }
        },
    }
}

async fn initialize_system(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<InitializeSystemRequest>, JsonRejection>,
) -> impl IntoResponse {
    match &state.system_bootstrap {
        SystemBootstrapCapability::Unsupported { message } => {
            not_implemented_response("/admin/system/initialize", Method::POST, original_uri, None)
                .with_message(message)
        }
        SystemBootstrapCapability::Available { system } => {
            let Json(request) = match payload {
                Ok(payload) => payload,
                Err(_) => return invalid_initialize_request_response(),
            };

            if !request.is_valid() {
                return invalid_initialize_request_response();
            }

            match system.is_initialized() {
                Ok(true) => return already_initialized_response(),
                Ok(false) => {}
                Err(SystemQueryError::QueryFailed) => {
                    return internal_error_response("Failed to check initialization status".to_owned())
                }
            }

            match system.initialize(&request) {
                Ok(()) => (
                    StatusCode::OK,
                    Json(InitializeSystemResponse {
                        success: true,
                        message: "System initialized successfully".to_owned(),
                    }),
                )
                    .into_response(),
                Err(SystemInitializeError::AlreadyInitialized) => already_initialized_response(),
                Err(SystemInitializeError::InitializeFailed(message)) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(InitializeSystemResponse {
                        success: false,
                        message: format!("Failed to initialize system: {message}"),
                    }),
                )
                    .into_response(),
            }
        }
    }
}

async fn sign_in(
    State(state): State<HttpState>,
    payload: Result<Json<SignInRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Invalid request format",
            )
        }
    };

    let auth = match &state.auth_context {
        AuthContextCapability::Unsupported { message } => return auth_unsupported_response(message),
        AuthContextCapability::Available { auth } => auth,
    };

    match auth.admin_signin(&request) {
        Ok(result) => (
            StatusCode::OK,
            Json(SignInResponse {
                user: result.user,
                token: result.token,
            }),
        )
            .into_response(),
        Err(SignInError::InvalidCredentials) => error_response(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "Invalid email or password",
        ),
        Err(SignInError::Internal) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            "Internal server error",
        ),
    }
}

async fn debug_context(request: Request) -> impl IntoResponse {
    let context = request
        .extensions()
        .get::<RequestContextState>()
        .cloned()
        .unwrap_or_default();

    let snapshot = RequestContextSnapshot {
        request_id: context.request_id,
        auth: context.auth.map(|auth| match auth {
            RequestAuthContext::Admin(user) => AuthSnapshot {
                mode: "jwt",
                user_id: Some(user.id),
                api_key_id: None,
                api_key_type: None,
            },
            RequestAuthContext::ApiKey(key) => AuthSnapshot {
                mode: match key.key_type {
                    ApiKeyType::NoAuth => "noauth",
                    ApiKeyType::ServiceAccount | ApiKeyType::User => "api_key",
                },
                user_id: None,
                api_key_id: Some(key.id),
                api_key_type: Some(match key.key_type {
                    ApiKeyType::User => "user",
                    ApiKeyType::ServiceAccount => "service_account",
                    ApiKeyType::NoAuth => "noauth",
                }),
            },
        }),
        project: context.project,
        thread: context.thread,
        trace: context.trace,
    };

    (StatusCode::OK, Json(snapshot))
}

async fn admin_graphql_playground() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        graphql_playground_html("/admin/graphql"),
    )
}

async fn openapi_graphql_playground() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        graphql_playground_html("/openapi/v1/graphql"),
    )
}

async fn admin_graphql(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let graphql = match &state.admin_graphql {
        AdminGraphqlCapability::Unsupported { message } => {
            return not_implemented_response("/admin/graphql", Method::POST, original_uri, None)
                .with_message(message)
        }
        AdminGraphqlCapability::Available { graphql } => graphql,
    };

    let project_id = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.project.as_ref())
        .map(|project| project.id);

    let user = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::Admin(user) => Some(user.clone()),
            RequestAuthContext::ApiKey(_) => None,
        });
    let user = match user {
        Some(user) => user,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Unauthorized",
                "Invalid token",
            )
        }
    };

    execute_graphql_request(request, |payload| graphql.execute_graphql(payload, project_id, user)).await
}

async fn openapi_graphql(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let graphql = match &state.openapi_graphql {
        OpenApiGraphqlCapability::Unsupported { message } => {
            return not_implemented_response("/openapi/v1/graphql", Method::POST, original_uri, None)
                .with_message(message)
        }
        OpenApiGraphqlCapability::Available { graphql } => graphql,
    };

    let owner_api_key = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::ApiKey(key) => Some(key.clone()),
            RequestAuthContext::Admin(_) => None,
        });
    let owner_api_key = match owner_api_key {
        Some(owner_api_key) => owner_api_key,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Unauthorized",
                "Invalid API key",
            )
        }
    };

    execute_graphql_request(request, |payload| graphql.execute_graphql(payload, owner_api_key)).await
}

async fn execute_graphql_request<Executor>(
    mut request: Request,
    executor: Executor,
) -> Response
where
    Executor: FnOnce(GraphqlRequestPayload) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>,
{
    let body = match body::to_bytes(std::mem::take(request.body_mut()), usize::MAX).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Invalid request format",
            )
        }
    };

    if body.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Request body is empty",
        );
    }

    let payload: GraphqlRequestPayload = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Invalid request format",
            )
        }
    };

    let result = executor(payload).await;
    let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
    (status, Json(result.body)).into_response()
}

async fn download_request_content(
    State(state): State<HttpState>,
    Path(request_id): Path<i64>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let admin = match &state.admin {
        AdminCapability::Unsupported { message } => {
            return not_implemented_response(
                "/admin/requests/:request_id/content",
                Method::GET,
                original_uri,
                None,
            )
            .with_message(message)
        }
        AdminCapability::Available { admin } => admin,
    };

    let project_id = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.project.as_ref())
        .map(|project| project.id)
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Project ID not found in context",
            )
        });
    let project_id = match project_id {
        Ok(project_id) => project_id,
        Err(response) => return response,
    };

    let user = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::Admin(user) => Some(user.clone()),
            RequestAuthContext::ApiKey(_) => None,
        });
    let user = match user {
        Some(user) => user,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Unauthorized",
                "Invalid token",
            )
        }
    };

    match admin.download_request_content(project_id, request_id, user) {
        Ok(content) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/octet-stream")
            .header(
                "Content-Disposition",
                format!("attachment; filename={:?}", content.filename),
            )
            .header("Cache-Control", "private, max-age=0, no-cache")
            .header("Content-Length", content.bytes.len().to_string())
            .body(body::Body::from(content.bytes))
            .unwrap_or_else(|_| internal_error_response("Failed to build content response".to_owned())),
        Err(AdminError::BadRequest { message }) => {
            error_response(StatusCode::BAD_REQUEST, "Bad Request", &message)
        }
        Err(AdminError::NotFound { message }) => {
            error_response(StatusCode::NOT_FOUND, "Not Found", &message)
        }
        Err(AdminError::Internal { message }) => {
            internal_error_response(message)
        }
    }
}

async fn start_codex_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<StartPkceOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_codex_oauth(&request)
    })
    .await
}

async fn exchange_codex_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<ExchangeCallbackOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.exchange_codex_oauth(&request)
    })
    .await
}

async fn start_claudecode_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<StartPkceOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_claudecode_oauth(&request)
    })
    .await
}

async fn exchange_claudecode_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<ExchangeCallbackOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.exchange_claudecode_oauth(&request)
    })
    .await
}

async fn start_antigravity_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<StartAntigravityOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_antigravity_oauth(&request)
    })
    .await
}

async fn exchange_antigravity_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<ExchangeCallbackOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.exchange_antigravity_oauth(&request)
    })
    .await
}

async fn start_copilot_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<StartCopilotOAuthRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(payload) => payload.0,
        Err(error) if error.body_text().contains("EOF") => StartCopilotOAuthRequest {},
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_copilot_oauth(&request)
    })
    .await
}

async fn poll_copilot_oauth(
    State(state): State<HttpState>,
    payload: Result<Json<PollCopilotOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "invalid request format",
            )
        }
    };

    let provider_edge = match provider_edge_admin_port(&state) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.poll_copilot_oauth(&request)
    })
    .await
}

#[derive(Debug, Deserialize)]
struct ModelsQuery {
    include: Option<String>,
}

async fn list_openai_models(
    State(state): State<HttpState>,
    Query(query): Query<ModelsQuery>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::GET, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_models(query.include.as_deref()) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => openai_error_response(error),
    }
}

async fn openai_chat_completions(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_openai_request(state, request, original_uri, OpenAiV1Route::ChatCompletions).await
}

async fn openai_responses(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_openai_request(state, request, original_uri, OpenAiV1Route::Responses).await
}

async fn openai_embeddings(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_openai_request(state, request, original_uri, OpenAiV1Route::Embeddings).await
}

async fn list_anthropic_models(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/anthropic/v1/*", Method::GET, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_anthropic_models() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => compatibility_error_response(CompatibilityRoute::AnthropicMessages, error),
    }
}

async fn anthropic_messages(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::AnthropicMessages,
        HashMap::new(),
    )
    .await
}

async fn jina_rerank(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::JinaRerank,
        HashMap::new(),
    )
    .await
}

async fn jina_embeddings(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::JinaEmbeddings,
        HashMap::new(),
    )
    .await
}

async fn list_gemini_models(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let path = original_uri.path().to_owned();
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response(
                if path.starts_with("/v1beta") {
                    "/v1beta/*"
                } else {
                    "/gemini/:gemini_api_version/*"
                },
                Method::GET,
                original_uri,
                gemini_version_from_path(path.as_str()),
            )
            .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_gemini_models() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => compatibility_error_response(CompatibilityRoute::GeminiGenerateContent, error),
    }
}

async fn gemini_generate_content(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let route = if original_uri.path().contains(":streamGenerateContent") {
        CompatibilityRoute::GeminiStreamGenerateContent
    } else if original_uri.path().contains(":generateContent") {
        CompatibilityRoute::GeminiGenerateContent
    } else {
        return not_implemented_response(
            if original_uri.path().starts_with("/v1beta") {
                "/v1beta/*"
            } else {
                "/gemini/:gemini_api_version/*"
            },
            Method::POST,
            original_uri,
            gemini_version_from_path(request.uri().path()),
        )
        .into_response();
    };

    let alt = request
        .uri()
        .query()
        .and_then(|query| parse_query_pairs(query).remove("alt"));

    let response = execute_compatibility_request(
        state,
        request,
        original_uri,
        route,
        HashMap::new(),
    )
    .await;
    if route != CompatibilityRoute::GeminiStreamGenerateContent || response.status() != StatusCode::OK {
        return response;
    }

    let body = body::to_bytes(response.into_body(), usize::MAX).await;
    let Ok(body) = body else {
        return compatibility_internal_error_response(route);
    };

    if alt.as_deref() == Some("sse") {
        let payload = String::from_utf8_lossy(&body);
        return (
            StatusCode::OK,
            [("content-type", "text/event-stream; charset=utf-8")],
            format!("data: {payload}\n\ndata: [DONE]\n\n"),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [("content-type", "application/json")],
        format!("[{0}]", String::from_utf8_lossy(&body)),
    )
        .into_response()
}

async fn doubao_create_task(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::DoubaoCreateTask,
        HashMap::new(),
    )
    .await
}

async fn doubao_get_task(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), id);
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::DoubaoGetTask,
        path_params,
    )
    .await
}

async fn doubao_delete_task(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), id);
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::DoubaoDeleteTask,
        path_params,
    )
    .await
}

async fn execute_openai_request(
    state: HttpState,
    mut request: Request,
    original_uri: Uri,
    route: OpenAiV1Route,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::POST, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let body = match parse_json_body(&mut request).await {
        Ok(body) => body,
        Err(response) => return response,
    };

    let execution_request = match build_openai_execution_request(request, body, HashMap::new()) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => {
            let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
            (status, Json(result.body)).into_response()
        }
        Ok(Err(error)) => openai_error_response(error),
        Err(_) => internal_error_response("OpenAI `/v1` execution task failed".to_owned()),
    }
}

async fn execute_compatibility_request(
    state: HttpState,
    mut request: Request,
    original_uri: Uri,
    route: CompatibilityRoute,
    path_params: HashMap<String, String>,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            let route_family = match route {
                CompatibilityRoute::AnthropicMessages => "/anthropic/v1/*",
                CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => "/jina/v1/*",
                CompatibilityRoute::GeminiGenerateContent
                | CompatibilityRoute::GeminiStreamGenerateContent => {
                    if original_uri.path().starts_with("/v1beta") {
                        "/v1beta/*"
                    } else {
                        "/gemini/:gemini_api_version/*"
                    }
                }
                CompatibilityRoute::DoubaoCreateTask
                | CompatibilityRoute::DoubaoGetTask
                | CompatibilityRoute::DoubaoDeleteTask => "/doubao/v3/*",
            };
            return not_implemented_response(route_family, Method::POST, original_uri, None)
                .with_message(message);
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let body = match route {
        CompatibilityRoute::AnthropicMessages
        | CompatibilityRoute::JinaRerank
        | CompatibilityRoute::JinaEmbeddings
        | CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent
        | CompatibilityRoute::DoubaoCreateTask => {
            match parse_json_body_for_compatibility(&mut request, route).await {
                Ok(body) => body,
                Err(response) => return response,
            }
        }
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => Value::Null,
    };

    let execution_request = match build_openai_execution_request(request, body, path_params) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result =
        tokio::task::spawn_blocking(move || openai.execute_compatibility(route, execution_request))
            .await;

    match execution_result {
        Ok(Ok(result)) => {
            let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
            (status, Json(result.body)).into_response()
        }
        Ok(Err(error)) => compatibility_error_response(route, error),
        Err(_) => compatibility_internal_error_response(route),
    }
}

fn build_openai_execution_request(
    mut request: Request,
    body: Value,
    path_params: HashMap<String, String>,
) -> Result<OpenAiV1ExecutionRequest, Response> {
    let path = request.uri().path().to_owned();
    let query = request
        .uri()
        .query()
        .map(parse_query_pairs)
        .unwrap_or_default();
    let context = request
        .extensions_mut()
        .remove::<RequestContextState>()
        .unwrap_or_default();

    let project = context.project.ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Project ID not found in context",
        )
    })?;

    let api_key_id = context.auth.as_ref().and_then(|auth| match auth {
        RequestAuthContext::ApiKey(key) => Some(key.id),
        RequestAuthContext::Admin(_) => None,
    });

    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let headers = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|current| (name.as_str().to_owned(), current.to_owned()))
        })
        .collect::<HashMap<_, _>>();

    Ok(OpenAiV1ExecutionRequest {
        headers,
        body,
        path,
        path_params,
        query,
        project,
        trace: context.trace,
        api_key_id,
        client_ip,
    })
}

async fn parse_json_body(request: &mut Request) -> Result<Value, Response> {
    let body = body::to_bytes(std::mem::take(request.body_mut()), usize::MAX)
        .await
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"))?;

    if body.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Request body is empty",
        ));
    }

    serde_json::from_slice(&body)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"))
}

async fn parse_json_body_for_compatibility(
    request: &mut Request,
    route: CompatibilityRoute,
) -> Result<Value, Response> {
    let body = body::to_bytes(std::mem::take(request.body_mut()), usize::MAX)
        .await
        .map_err(|_| compatibility_bad_request_response(route, "Invalid request format"))?;

    if body.is_empty() {
        return Err(compatibility_bad_request_response(route, "Request body is empty"));
    }

    serde_json::from_slice(&body)
        .map_err(|_| compatibility_bad_request_response(route, "Invalid request format"))
}

async fn unported_admin(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/admin/*", method, uri, None)
}

async fn unported_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1/*", method, uri, None)
}

async fn unported_jina_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/jina/v1/*", method, uri, None)
}

async fn unported_anthropic_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/anthropic/v1/*", method, uri, None)
}

async fn unported_doubao_v3(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/doubao/v3/*", method, uri, None)
}

async fn unported_gemini(
    Path(params): Path<HashMap<String, String>>,
    method: Method,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    not_implemented_response(
        "/gemini/:gemini_api_version/*",
        method,
        uri,
        params.get("gemini_api_version").cloned(),
    )
}

async fn unported_v1beta(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1beta/*", method, uri, None)
}

async fn unported_openapi(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/openapi/*", method, uri, None)
}

fn trace_thread_header_name(config: &TraceConfig) -> String {
    config
        .thread_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Thread-Id".to_owned())
}

fn trace_request_header_name(config: &TraceConfig) -> String {
    config
        .request_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "X-Request-Id".to_owned())
}

fn trace_header_name(config: &TraceConfig) -> String {
    config
        .trace_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Trace-Id".to_owned())
}

fn request_header_value<'a>(headers: &'a HeaderMap, header_name: &str) -> Option<&'a str> {
    headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_trace_id<'a>(headers: &'a HeaderMap, config: &TraceConfig) -> Option<&'a str> {
    request_header_value(headers, &trace_header_name(config)).or_else(|| {
        config
            .extra_trace_headers
            .iter()
            .find_map(|header| request_header_value(headers, header))
    })
}

fn parse_project_header(headers: &HeaderMap) -> Result<Option<i64>, Response> {
    let Some(raw) = request_header_value(headers, "X-Project-ID") else {
        return Ok(None);
    };

    parse_project_guid(raw).map(Some).ok_or_else(|| {
        error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid project ID")
    })
}

fn parse_project_guid(raw: &str) -> Option<i64> {
    let value = raw.trim();
    let prefix = "gid://axonhub/project/";
    if !value.starts_with(prefix) {
        return None;
    }

    value[prefix.len()..].parse::<i64>().ok()
}

fn extract_required_bearer_token(headers: &HeaderMap) -> Result<&str, Response> {
    let value = request_header_value(headers, "Authorization").ok_or_else(|| {
        error_response(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "API key is required",
        )
    })?;

    value.strip_prefix("Bearer ").ok_or_else(|| {
        error_response(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "invalid token: Authorization header must start with 'Bearer '",
        )
    })
}

fn extract_api_key_from_headers(headers: &HeaderMap) -> Option<String> {
    const HEADER_NAMES: [&str; 7] = [
        "Authorization",
        "X-API-Key",
        "X-Api-Key",
        "API-Key",
        "Api-Key",
        "X-Goog-Api-Key",
        "X-Google-Api-Key",
    ];

    const PREFIXES: [&str; 4] = ["Bearer ", "Token ", "Api-Key ", "API-Key "];

    for header in HEADER_NAMES {
        let Some(value) = request_header_value(headers, header) else {
            continue;
        };

        let key = PREFIXES
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
            .unwrap_or(value)
            .trim();
        if !key.is_empty() {
            return Some(key.to_owned());
        }
    }

    None
}

fn not_implemented_response(
    route_family: &'static str,
    method: Method,
    uri: Uri,
    gemini_api_version: Option<String>,
) -> NotImplementedJsonResponse {
    NotImplementedJsonResponse {
        status: StatusCode::NOT_IMPLEMENTED,
        body: NotImplementedResponse {
            error: "not_implemented",
            status: StatusCode::NOT_IMPLEMENTED.as_u16(),
            route_family,
            method: method.to_string(),
            path: uri.path().to_owned(),
            message: "This surface has not been migrated to the Rust backend yet. Use the legacy Go backend for full AxonHub API coverage.".to_owned(),
            migration_status: "first migration slice",
            legacy_go_backend_present: true,
            gemini_api_version,
        },
    }
}

fn auth_unsupported_response(message: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(NotImplementedResponse {
            error: "not_implemented",
            status: StatusCode::NOT_IMPLEMENTED.as_u16(),
            route_family: "/auth/context",
            method: "UNKNOWN".to_owned(),
            path: "/".to_owned(),
            message: message.to_owned(),
            migration_status: "first migration slice",
            legacy_go_backend_present: true,
            gemini_api_version: None,
        }),
    )
        .into_response()
}

fn error_response(status: StatusCode, kind: &'static str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: ErrorDetail {
                r#type: kind,
                message: message.to_owned(),
            },
        }),
    )
        .into_response()
}

fn default_graphql_variables() -> Value {
    Value::Object(serde_json::Map::new())
}

fn openai_error_response(error: OpenAiV1Error) -> Response {
    match error {
        OpenAiV1Error::InvalidRequest { message } => {
            error_response(StatusCode::BAD_REQUEST, "Bad Request", &message)
        }
        OpenAiV1Error::Internal { message } => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error", &message)
        }
        OpenAiV1Error::Upstream { status, body } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            (status, Json(body)).into_response()
        }
    }
}

fn compatibility_bad_request_response(route: CompatibilityRoute, message: &str) -> Response {
    compatibility_error_response(
        route,
        OpenAiV1Error::InvalidRequest {
            message: message.to_owned(),
        },
    )
}

fn graphql_playground_html(endpoint: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>AxonHub GraphQL Playground</title></head><body><div id=\"root\"></div><script>window.GRAPHQL_ENDPOINT={endpoint:?};</script><p>GraphQL playground endpoint: <code>{endpoint}</code></p></body></html>"
    )
}

fn compatibility_internal_error_response(route: CompatibilityRoute) -> Response {
    compatibility_error_response(
        route,
        OpenAiV1Error::Internal {
            message: "Compatibility wrapper execution task failed".to_owned(),
        },
    )
}

fn provider_edge_admin_port(state: &HttpState) -> Result<&Arc<dyn ProviderEdgeAdminPort>, Response> {
    match &state.provider_edge_admin {
        ProviderEdgeAdminCapability::Unsupported { message } => Err(auth_unsupported_response(message)),
        ProviderEdgeAdminCapability::Available { provider_edge } => Ok(provider_edge),
    }
}

async fn execute_provider_edge_admin_request<T, Executor>(
    provider_edge: Arc<dyn ProviderEdgeAdminPort>,
    executor: Executor,
) -> Response
where
    T: Serialize + Send + 'static,
    Executor: FnOnce(Arc<dyn ProviderEdgeAdminPort>) -> Result<T, ProviderEdgeAdminError> + Send + 'static,
{
    let execution_result = tokio::task::spawn_blocking(move || executor(provider_edge)).await;

    match execution_result {
        Ok(Ok(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(Err(error)) => provider_edge_admin_error_response(error),
        Err(_) => provider_edge_admin_error_response(ProviderEdgeAdminError::Internal {
            message: "Provider-edge admin execution task failed".to_owned(),
        }),
    }
}

fn provider_edge_admin_error_response(error: ProviderEdgeAdminError) -> Response {
    match error {
        ProviderEdgeAdminError::InvalidRequest { message } => {
            error_response(StatusCode::BAD_REQUEST, "Bad Request", &message)
        }
        ProviderEdgeAdminError::BadGateway { message } => {
            error_response(StatusCode::BAD_GATEWAY, "Bad Gateway", &message)
        }
        ProviderEdgeAdminError::Internal { message } => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error", &message)
        }
    }
}

fn compatibility_error_response(route: CompatibilityRoute, error: OpenAiV1Error) -> Response {
    match route {
        CompatibilityRoute::AnthropicMessages => anthropic_error_response(error),
        CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => jina_error_response(error),
        CompatibilityRoute::GeminiGenerateContent | CompatibilityRoute::GeminiStreamGenerateContent => {
            gemini_error_response(error)
        }
        CompatibilityRoute::DoubaoCreateTask
        | CompatibilityRoute::DoubaoGetTask
        | CompatibilityRoute::DoubaoDeleteTask => doubao_error_response(error),
    }
}

fn anthropic_error_response(error: OpenAiV1Error) -> Response {
    let (status, error_type, message) = match error {
        OpenAiV1Error::InvalidRequest { message } => {
            (StatusCode::BAD_REQUEST, "invalid_request_error", message)
        }
        OpenAiV1Error::Internal { message } => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error", message)
        }
        OpenAiV1Error::Upstream { status, body } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            let error_type = match status {
                StatusCode::BAD_REQUEST => "invalid_request_error",
                StatusCode::UNAUTHORIZED => "authentication_error",
                StatusCode::FORBIDDEN => "permission_error",
                StatusCode::NOT_FOUND => "not_found_error",
                StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
                StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT => "api_error",
                _ => "api_error",
            };
            (status, error_type, extract_error_message(&body))
        }
    };

    (
        status,
        Json(serde_json::json!({
            "type": error_type,
            "request_id": "",
            "error": {
                "type": error_type,
                "message": message,
            }
        })),
    )
        .into_response()
}

fn jina_error_response(error: OpenAiV1Error) -> Response {
    let (status, message) = match error {
        OpenAiV1Error::InvalidRequest { message } => (StatusCode::BAD_REQUEST, message),
        OpenAiV1Error::Internal { message } => (StatusCode::INTERNAL_SERVER_ERROR, message),
        OpenAiV1Error::Upstream { status, body } => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            extract_error_message(&body),
        ),
    };

    (
        status,
        Json(serde_json::json!({
            "error": {
                "message": message,
                "type": "api_error",
            }
        })),
    )
        .into_response()
}

fn gemini_error_response(error: OpenAiV1Error) -> Response {
    let (status, message) = match error {
        OpenAiV1Error::InvalidRequest { message } => (StatusCode::BAD_REQUEST, message),
        OpenAiV1Error::Internal { message } => (StatusCode::INTERNAL_SERVER_ERROR, message),
        OpenAiV1Error::Upstream { status, body } => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            extract_error_message(&body),
        ),
    };

    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": status.as_u16(),
                "message": message,
                "status": match status {
                    StatusCode::BAD_REQUEST => "INVALID_ARGUMENT",
                    StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
                    StatusCode::FORBIDDEN => "PERMISSION_DENIED",
                    StatusCode::NOT_FOUND => "NOT_FOUND",
                    StatusCode::TOO_MANY_REQUESTS => "RESOURCE_EXHAUSTED",
                    StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
                    StatusCode::NOT_IMPLEMENTED => "UNIMPLEMENTED",
                    _ => "INTERNAL",
                }
            }
        })),
    )
        .into_response()
}

fn doubao_error_response(error: OpenAiV1Error) -> Response {
    openai_error_response(error)
}

fn gemini_version_from_path(path: &str) -> Option<String> {
    if path.starts_with("/v1beta") {
        return Some("v1beta".to_owned());
    }
    path.split('/')
        .collect::<Vec<_>>()
        .windows(3)
        .find_map(|window| (window[1] == "gemini").then(|| window[2].to_owned()))
}

fn parse_query_pairs(raw: &str) -> HashMap<String, String> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key.to_owned(), value.to_owned())
        })
        .collect()
}

fn extract_error_message(body: &Value) -> String {
    body.get("error")
        .and_then(|error| error.get("message").or_else(|| error.get("error").and_then(|nested| nested.get("message"))))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            body.get("errors")
                .and_then(Value::as_array)
                .and_then(|errors| errors.first())
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "Upstream request failed".to_owned())
}

fn internal_error_response(message: String) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error", &message)
}

fn invalid_initialize_request_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(InitializeSystemResponse {
            success: false,
            message: "Invalid request format".to_owned(),
        }),
    )
        .into_response()
}

fn already_initialized_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(InitializeSystemResponse {
            success: false,
            message: "System is already initialized".to_owned(),
        }),
    )
        .into_response()
}

#[derive(Debug)]
struct NotImplementedJsonResponse {
    status: StatusCode,
    body: NotImplementedResponse,
}

impl NotImplementedJsonResponse {
    fn with_message(mut self, message: &str) -> Response {
        self.body.message = message.to_owned();
        self.into_response()
    }
}

impl IntoResponse for NotImplementedJsonResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: String,
    version: String,
    backend: &'static str,
    migration_status: &'static str,
    api_parity: &'static str,
    legacy_go_backend_present: bool,
    config_source: Option<String>,
}

#[derive(Debug, Serialize)]
struct SystemStatusResponse {
    #[serde(rename = "isInitialized")]
    is_initialized: bool,
}

#[derive(Debug, Deserialize)]
pub struct InitializeSystemRequest {
    #[serde(rename = "ownerEmail")]
    pub owner_email: String,
    #[serde(rename = "ownerPassword")]
    pub owner_password: String,
    #[serde(rename = "ownerFirstName")]
    pub owner_first_name: String,
    #[serde(rename = "ownerLastName")]
    pub owner_last_name: String,
    #[serde(rename = "brandName")]
    pub brand_name: String,
}

impl InitializeSystemRequest {
    fn is_valid(&self) -> bool {
        is_valid_email(&self.owner_email)
            && self.owner_password.len() >= 6
            && !self.owner_first_name.trim().is_empty()
            && !self.owner_last_name.trim().is_empty()
            && !self.brand_name.trim().is_empty()
    }
}

fn is_valid_email(value: &str) -> bool {
    let email = value.trim();
    if email.is_empty() || email.contains(char::is_whitespace) {
        return false;
    }

    let mut parts = email.split('@');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
    )
}

#[derive(Debug, Serialize)]
struct InitializeSystemResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct SignInResponse {
    user: AuthUserContext,
    token: String,
}

#[derive(Debug, Serialize)]
struct NotImplementedResponse {
    error: &'static str,
    status: u16,
    route_family: &'static str,
    method: String,
    path: String,
    message: String,
    migration_status: &'static str,
    legacy_go_backend_present: bool,
    gemini_api_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    #[serde(rename = "type")]
    r#type: &'static str,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::json;
    use serde_json::Value;
    use std::sync::Mutex;
    use tower::util::ServiceExt;

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

    impl AuthContextPort for FakeAuthPort {
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
            if token != "valid-admin-token" {
                return Err(AdminAuthError::InvalidToken);
            }
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
                            OpenAiV1Route::Embeddings => "embed_rust",
                        },
                        "model": request.body["model"].clone(),
                        "project_id": request.project.id,
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
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(FakeAuthPort::new()),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "OpenAI `/v1` inference is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(FakeAdminPort),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "DB-backed admin GraphQL is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "Provider-edge admin OAuth helpers are not configured in this HTTP test fixture.".to_owned(),
            },
            allow_no_auth,
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

    fn test_state_with_openai(system_bootstrap: SystemBootstrapCapability, allow_no_auth: bool) -> HttpState {
        let mut state = test_state(system_bootstrap, allow_no_auth);
        state.openai_v1 = OpenAiV1Capability::Available {
            openai: Arc::new(FakeOpenAiV1Port),
        };
        state
    }

    async fn read_json(response: Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
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
    async fn admin_route_requires_valid_jwt_before_truthful_501() {
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
        assert_eq!(authorized.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(authorized).await;
        assert_eq!(json["route_family"], "/admin/*");
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
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"video-content");
    }

    #[tokio::test]
    async fn admin_request_content_neighboring_admin_surface_stays_truthful_501() {
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

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(response).await;
        assert_eq!(json["route_family"], "/admin/*");
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
        assert_eq!(json["project"]["id"], 1);
        assert_eq!(json["thread"]["threadId"], "thread-1");
        assert_eq!(json["trace"]["traceId"], "trace-1");
        assert_eq!(json["requestId"], "req-1");
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
        let body = axum::body::to_bytes(stream_response.into_body(), usize::MAX).await.unwrap();
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
        assert_eq!(json["route_family"], "/gemini/:gemini_api_version/*");
        assert_eq!(json["error"], "not_implemented");
    }

    #[tokio::test]
    async fn doubao_task_routes_succeed_while_neighboring_surface_stays_truthful_501() {
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
        assert_eq!(unsupported_response.status(), StatusCode::NOT_IMPLEMENTED);
        let unsupported_json = read_json(unsupported_response).await;
        assert_eq!(unsupported_json["route_family"], "/doubao/v3/*");
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
                "/v1/embeddings",
                r#"{"model":"gpt-4o","input":"hi"}"#,
                "embed_rust",
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
    async fn non_target_v1_routes_still_return_truthful_501_when_openai_slice_is_enabled() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/images")
                    .method(Method::POST)
                    .header("X-API-Key", "api-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let json = read_json(response).await;
        assert_eq!(json["route_family"], "/v1/*");
    }

    #[tokio::test]
    async fn residual_non_target_families_keep_structured_truthful_501_payloads() {
        let app = router(test_state_with_openai(
            SystemBootstrapCapability::Available {
                system: Arc::new(SharedSystemBootstrapPort::new(SharedSystemState::default())),
            },
            false,
        ));

        for (method, path, header_name, header_value, expected_family) in [
            (
                Method::POST,
                "/admin/unported",
                Some("Authorization"),
                Some("Bearer valid-admin-token"),
                "/admin/*",
            ),
            (
                Method::POST,
                "/v1/images",
                Some("X-API-Key"),
                Some("api-key-123"),
                "/v1/*",
            ),
            (
                Method::POST,
                "/anthropic/v1/count_tokens",
                Some("X-API-Key"),
                Some("api-key-123"),
                "/anthropic/v1/*",
            ),
            (
                Method::POST,
                "/jina/v1/classify",
                Some("X-API-Key"),
                Some("api-key-123"),
                "/jina/v1/*",
            ),
            (
                Method::POST,
                "/doubao/v3/contents/generations/status",
                Some("X-API-Key"),
                Some("api-key-123"),
                "/doubao/v3/*",
            ),
            (
                Method::GET,
                "/gemini/v1/files/123?key=api-key-123",
                None,
                None,
                "/gemini/:gemini_api_version/*",
            ),
            (
                Method::GET,
                "/v1beta/files/123?key=api-key-123",
                None,
                None,
                "/v1beta/*",
            ),
            (
                Method::POST,
                "/openapi/v1/keys",
                Some("Authorization"),
                Some("Bearer service-key-123"),
                "/openapi/*",
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
            assert_eq!(json["route_family"], expected_family, "{path}");
            assert_eq!(json["migration_status"], "first migration slice", "{path}");
            assert_eq!(json["legacy_go_backend_present"], true, "{path}");
        }
    }
}
