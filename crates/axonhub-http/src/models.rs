use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const BUILD_COMMIT: Option<&str> = option_env!("AXONHUB_BUILD_COMMIT");
const BUILD_TIME: Option<&str> = option_env!("AXONHUB_BUILD_TIME");
const GO_VERSION: Option<&str> = option_env!("AXONHUB_BUILD_GO_VERSION");
const GO_VERSION_FALLBACK: &str = "n/a (Rust build)";

static START_TIME: OnceLock<Instant> = OnceLock::new();

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

impl AuthUserContext {
    pub fn has_system_scope(&self, scope: &str) -> bool {
        self.is_owner
            || self.scopes.iter().any(|current| current == scope)
            || self
                .roles
                .iter()
                .flat_map(|role| role.scopes.iter())
                .any(|current| current == scope)
    }

    pub fn has_project_scope(&self, project_id: i64, scope: &str) -> bool {
        if self.is_owner {
            return true;
        }

        self.projects.iter().any(|project| {
            project.project_id.id == project_id
                && (project.is_owner
                    || project.scopes.iter().any(|current| current == scope)
                    || project
                        .roles
                        .iter()
                        .flat_map(|role| role.scopes.iter())
                        .any(|current| current == scope))
        })
    }
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
    pub profiles_json: Option<String>,
}

impl AuthApiKeyContext {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|current| current == scope)
    }

    pub fn is_service_account(&self) -> bool {
        matches!(self.key_type, ApiKeyType::ServiceAccount)
    }
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
    pub user_id: Option<i64>,
    pub api_key_id: Option<i64>,
    pub api_key_type: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiV1Route {
    ChatCompletions,
    Responses,
    ResponsesCompact,
    Embeddings,
    ImagesGenerations,
    ImagesEdits,
    ImagesVariations,
    Realtime,
}

impl OpenAiV1Route {
    pub fn format(self) -> &'static str {
        match self {
            Self::ChatCompletions => "openai/chat_completions",
            Self::Responses => "openai/responses",
            Self::ResponsesCompact => "openai/responses_compact",
            Self::Embeddings => "openai/embeddings",
            Self::ImagesGenerations => "openai/images_generations",
            Self::ImagesEdits => "openai/images_edits",
            Self::ImagesVariations => "openai/images_variations",
            Self::Realtime => "openai/realtime",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiMultipartField {
    pub name: String,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiMultipartBody {
    pub content_type: String,
    pub fields: Vec<OpenAiMultipartField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAiRequestBody {
    Json(Value),
    Multipart(OpenAiMultipartBody),
}

impl OpenAiRequestBody {
    pub fn as_json(&self) -> Option<&Value> {
        match self {
            Self::Json(value) => Some(value),
            Self::Multipart(_) => None,
        }
    }

    pub fn stream_flag(&self) -> bool {
        self.as_json()
            .and_then(|value| value.get("stream"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
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
    pub body: OpenAiRequestBody,
    pub path: String,
    pub path_params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub project: ProjectContext,
    pub trace: Option<TraceContext>,
    pub api_key: AuthApiKeyContext,
    pub api_key_id: Option<i64>,
    pub client_ip: Option<String>,
    pub channel_hint_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct OpenAiV1ExecutionResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeSessionTransportRequest {
    pub transport: String,
    pub model: String,
    #[serde(rename = "channelId", skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeSessionPatchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RealtimeSessionRecord {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub transport: String,
    pub status: String,
    pub model: String,
    #[serde(rename = "projectId")]
    pub project_id: i64,
    #[serde(rename = "threadId", skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(rename = "traceId", skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    pub request_id: Option<i64>,
    #[serde(rename = "apiKeyId", skip_serializing_if = "Option::is_none")]
    pub api_key_id: Option<i64>,
    #[serde(rename = "channelId", skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<i64>,
    pub metadata: Value,
    #[serde(rename = "openedAt")]
    pub opened_at: String,
    #[serde(rename = "lastActivityAt")]
    pub last_activity_at: String,
    #[serde(rename = "closedAt", skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RealtimeSessionCreateRequest {
    pub project: ProjectContext,
    pub thread: Option<ThreadContext>,
    pub trace: Option<TraceContext>,
    pub api_key_id: Option<i64>,
    pub client_ip: Option<String>,
    pub request_id: Option<String>,
    pub transport: RealtimeSessionTransportRequest,
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
    pub timestamp: String,
    pub version: String,
    pub build: HealthBuildInfo,
    pub uptime: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct HealthBuildInfo {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(rename = "build_time", skip_serializing_if = "Option::is_none")]
    pub build_time: Option<String>,
    #[serde(rename = "go_version")]
    pub go_version: String,
    pub platform: String,
    pub uptime: String,
}

impl HealthBuildInfo {
    pub(crate) fn current(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            commit: BUILD_COMMIT
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            build_time: BUILD_TIME
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            go_version: GO_VERSION
                .filter(|value| !value.is_empty())
                .unwrap_or(GO_VERSION_FALLBACK)
                .to_owned(),
            platform: format!("{}/{}", env::consts::OS, env::consts::ARCH),
            uptime: health_uptime(),
        }
    }
}

pub(crate) fn health_timestamp() -> String {
    humantime::format_rfc3339_nanos(std::time::SystemTime::now()).to_string()
}

pub(crate) fn health_uptime() -> String {
    format_go_duration(start_time().elapsed())
}

pub(crate) fn format_go_duration(duration: Duration) -> String {
    const NANOS_PER_MICROSECOND: u128 = 1_000;
    const NANOS_PER_MILLISECOND: u128 = 1_000_000;
    const NANOS_PER_SECOND: u128 = 1_000_000_000;
    const SECONDS_PER_MINUTE: u128 = 60;
    const SECONDS_PER_HOUR: u128 = 60 * 60;

    let total_nanos = duration.as_nanos();

    if total_nanos == 0 {
        return "0s".to_owned();
    }

    if total_nanos < NANOS_PER_SECOND {
        if total_nanos < NANOS_PER_MICROSECOND {
            return format!("{total_nanos}ns");
        }

        if total_nanos < NANOS_PER_MILLISECOND {
            return format_decimal_duration(total_nanos, NANOS_PER_MICROSECOND, 3, "µs");
        }

        return format_decimal_duration(total_nanos, NANOS_PER_MILLISECOND, 6, "ms");
    }

    let total_seconds = total_nanos / NANOS_PER_SECOND;
    let hours = total_seconds / SECONDS_PER_HOUR;
    let minutes = (total_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let seconds = total_seconds % SECONDS_PER_MINUTE;
    let remaining_nanos = total_nanos % NANOS_PER_SECOND;

    let mut formatted = String::new();

    if hours > 0 {
        formatted.push_str(&hours.to_string());
        formatted.push('h');
    }

    if minutes > 0 || hours > 0 {
        formatted.push_str(&minutes.to_string());
        formatted.push('m');
    }

    if remaining_nanos == 0 {
        formatted.push_str(&seconds.to_string());
        formatted.push('s');
        return formatted;
    }

    formatted.push_str(&format_decimal_duration(
        seconds * NANOS_PER_SECOND + remaining_nanos,
        NANOS_PER_SECOND,
        9,
        "s",
    ));
    formatted
}

fn format_decimal_duration(
    total_nanos: u128,
    unit_nanos: u128,
    max_fraction_digits: usize,
    suffix: &str,
) -> String {
    let whole = total_nanos / unit_nanos;
    let fractional = total_nanos % unit_nanos;

    if fractional == 0 {
        return format!("{whole}{suffix}");
    }

    let mut fraction = format!("{:0width$}", fractional, width = max_fraction_digits);
    while fraction.ends_with('0') {
        fraction.pop();
    }

    format!("{whole}.{fraction}{suffix}")
}

fn start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
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
