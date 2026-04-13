use crate::models::{
    AdminContentDownload, AnthropicModelListResponse, AuthApiKeyContext, AuthUserContext,
    ExchangeCallbackOAuthRequest, ExchangeOAuthResponse, GeminiModelListResponse,
    GraphqlExecutionResult, GraphqlRequestPayload, InitializeSystemRequest, ModelListResponse,
    OpenAiModel, OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Route,
    PollCopilotOAuthRequest, PollCopilotOAuthResponse, ProjectContext,
    RealtimeSessionCreateRequest, RealtimeSessionPatchRequest, RealtimeSessionRecord,
    SignInRequest, SignInSuccess, StartAntigravityOAuthRequest, StartCopilotOAuthRequest,
    StartCopilotOAuthResponse, StartPkceOAuthRequest, StartPkceOAuthResponse, ThreadContext,
    TraceContext,
};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

pub trait SystemBootstrapPort: Send + Sync {
    fn is_initialized(&self) -> Result<bool, SystemQueryError>;
    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError>;
}

pub trait IdentityPort: Send + Sync {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError>;

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
}

pub trait RequestContextPort: Send + Sync {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError>;
    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError>;
    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError>;
}

pub trait OpenAiV1Port: Send + Sync {
    fn list_models(
        &self,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<ModelListResponse, OpenAiV1Error>;

    fn retrieve_model(
        &self,
        model_id: &str,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<OpenAiModel, OpenAiV1Error>;

    fn retrieve_response(
        &self,
        response_id: &str,
        api_key: &AuthApiKeyContext,
    ) -> Result<Option<Value>, OpenAiV1Error>;

    fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error>;

    fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error>;

    fn execute(
        &self,
        route: OpenAiV1Route,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error>;

    fn execute_compatibility(
        &self,
        route: crate::models::CompatibilityRoute,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error>;

    fn create_realtime_session(
        &self,
        request: RealtimeSessionCreateRequest,
    ) -> Result<RealtimeSessionRecord, OpenAiV1Error>;

    fn get_realtime_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error>;

    fn update_realtime_session(
        &self,
        session_id: &str,
        patch: RealtimeSessionPatchRequest,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error>;

    fn delete_realtime_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error>;
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

pub trait OauthProviderAdminPort: Send + Sync {
    fn start_codex_oauth(
        &self,
        request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, OauthProviderAdminError>;

    fn exchange_codex_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, OauthProviderAdminError>;

    fn start_claudecode_oauth(
        &self,
        request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, OauthProviderAdminError>;

    fn exchange_claudecode_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, OauthProviderAdminError>;

    fn start_antigravity_oauth(
        &self,
        request: &StartAntigravityOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, OauthProviderAdminError>;

    fn exchange_antigravity_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, OauthProviderAdminError>;

    fn start_copilot_oauth(
        &self,
        request: &StartCopilotOAuthRequest,
    ) -> Result<StartCopilotOAuthResponse, OauthProviderAdminError>;

    fn poll_copilot_oauth(
        &self,
        request: &PollCopilotOAuthRequest,
    ) -> Result<PollCopilotOAuthResponse, OauthProviderAdminError>;
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
    Upstream { status: u16, body: Value },
    Internal { message: String },
}

#[derive(Debug, Clone)]
pub enum AdminError {
    BadRequest { message: String },
    Forbidden { message: String },
    NotFound { message: String },
    Internal { message: String },
}

#[derive(Debug, Clone)]
pub enum OauthProviderAdminError {
    InvalidRequest { message: String },
    BadGateway { message: String },
    Internal { message: String },
}
