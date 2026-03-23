use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
    pub(crate) fn is_valid(&self) -> bool {
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

fn default_graphql_variables() -> Value {
    Value::Object(serde_json::Map::new())
}

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub status: &'static str,
    pub service: String,
    pub version: String,
    pub backend: &'static str,
    pub migration_status: &'static str,
    pub api_parity: &'static str,
    pub legacy_go_backend_present: bool,
    pub config_source: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SystemStatusResponse {
    #[serde(rename = "isInitialized")]
    pub is_initialized: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct InitializeSystemResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SignInResponse {
    pub user: AuthUserContext,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct NotImplementedResponse {
    pub error: &'static str,
    pub status: u16,
    pub route_family: &'static str,
    pub method: String,
    pub path: String,
    pub message: String,
    pub migration_status: &'static str,
    pub legacy_go_backend_present: bool,
    pub gemini_api_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub(crate) struct ErrorDetail {
    #[serde(rename = "type")]
    pub r#type: &'static str,
    pub message: String,
}
