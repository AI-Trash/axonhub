use std::future::Future;
use std::pin::Pin;

use axonhub_http::{
    AdminAuthError, AdminContentDownload, AdminError, AnthropicModelListResponse, ApiKeyAuthError,
    AuthApiKeyContext, AuthUserContext, CompatibilityRoute, ContextResolveError,
    GeminiModelListResponse, GraphqlExecutionResult, GraphqlRequestPayload,
    InitializeSystemRequest, ModelListResponse, OpenAiV1Error, OpenAiV1ExecutionRequest,
    OpenAiV1ExecutionResponse, OpenAiV1Route, ProjectContext, RealtimeSessionCreateRequest,
    RealtimeSessionPatchRequest, RealtimeSessionRecord, SignInError, SignInRequest, SignInSuccess,
    SystemInitializeError, SystemQueryError, ThreadContext, TraceContext,
};

pub(crate) trait SystemBootstrapRepository: Send + Sync {
    fn is_initialized(&self) -> Result<bool, SystemQueryError>;
    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError>;
}

pub(crate) trait IdentityRepository: Send + Sync {
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

pub(crate) trait RequestContextRepository: Send + Sync {
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

pub(crate) trait OpenAiV1Repository: Send + Sync {
    fn list_models(
        &self,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<ModelListResponse, OpenAiV1Error>;

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

pub(crate) trait AdminRepository: Send + Sync {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError>;
}

pub(crate) trait AdminGraphqlRepository: Send + Sync {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>;
}

pub(crate) trait OpenApiGraphqlRepository: Send + Sync {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>;
}
