use std::sync::Arc;

use axonhub_http::{
    AdminAuthError, AdminContentDownload, AdminError, AdminGraphqlPort, AdminPort,
    AnthropicModelListResponse, ApiKeyAuthError, AuthApiKeyContext, AuthUserContext,
    ContextResolveError, GeminiModelListResponse, GraphqlExecutionResult, GraphqlRequestPayload,
    IdentityPort, InitializeSystemRequest, ModelListResponse, OpenAiModel, OpenAiV1Error,
    OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Port, OpenAiV1Route,
    OpenApiGraphqlPort, ProjectContext, RealtimeSessionCreateRequest, RealtimeSessionPatchRequest,
    RealtimeSessionRecord, RequestContextPort, SignInError, SignInRequest, SignInSuccess,
    SystemBootstrapPort, SystemInitializeError, SystemQueryError, ThreadContext, TraceContext,
};

use crate::foundation::ports::{
    AdminGraphqlRepository, AdminRepository, IdentityRepository, OpenAiV1Repository,
    OpenApiGraphqlRepository, RequestContextRepository, SystemBootstrapRepository,
};

pub(crate) struct SystemBootstrapApplicationService {
    repository: Arc<dyn SystemBootstrapRepository>,
}

impl SystemBootstrapApplicationService {
    pub(crate) fn new(repository: Arc<dyn SystemBootstrapRepository>) -> Self {
        Self { repository }
    }
}

impl SystemBootstrapPort for SystemBootstrapApplicationService {
    fn is_initialized(&self) -> Result<bool, SystemQueryError> {
        self.repository.is_initialized()
    }

    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
        self.repository.initialize(request)
    }
}

pub(crate) struct IdentityApplicationService {
    repository: Arc<dyn IdentityRepository>,
}

impl IdentityApplicationService {
    pub(crate) fn new(repository: Arc<dyn IdentityRepository>) -> Self {
        Self { repository }
    }
}

impl IdentityPort for IdentityApplicationService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        self.repository.admin_signin(request)
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        self.repository.authenticate_admin_jwt(token)
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        self.repository.authenticate_api_key(key, allow_no_auth)
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        self.repository
            .authenticate_gemini_key(query_key, header_key)
    }
}

pub(crate) struct RequestContextApplicationService {
    repository: Arc<dyn RequestContextRepository>,
}

impl RequestContextApplicationService {
    pub(crate) fn new(repository: Arc<dyn RequestContextRepository>) -> Self {
        Self { repository }
    }
}

impl RequestContextPort for RequestContextApplicationService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        self.repository.resolve_project(project_id)
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        self.repository.resolve_thread(project_id, thread_id)
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        self.repository
            .resolve_trace(project_id, trace_id, thread_db_id)
    }
}

pub(crate) struct OpenAiV1ApplicationService {
    repository: Arc<dyn OpenAiV1Repository>,
}

impl OpenAiV1ApplicationService {
    pub(crate) fn new(repository: Arc<dyn OpenAiV1Repository>) -> Self {
        Self { repository }
    }
}

impl OpenAiV1Port for OpenAiV1ApplicationService {
    fn list_models(
        &self,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<ModelListResponse, OpenAiV1Error> {
        self.repository.list_models(include, api_key)
    }

    fn retrieve_model(
        &self,
        model_id: &str,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<OpenAiModel, OpenAiV1Error> {
        self.repository.retrieve_model(model_id, include, api_key)
    }

    fn retrieve_response(
        &self,
        response_id: &str,
        api_key: &AuthApiKeyContext,
    ) -> Result<Option<serde_json::Value>, OpenAiV1Error> {
        self.repository.retrieve_response(response_id, api_key)
    }

    fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
        self.repository.list_anthropic_models()
    }

    fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
        self.repository.list_gemini_models()
    }

    fn execute(
        &self,
        route: OpenAiV1Route,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        self.repository.execute(route, request)
    }

    fn execute_compatibility(
        &self,
        route: axonhub_http::CompatibilityRoute,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        self.repository.execute_compatibility(route, request)
    }

    fn create_realtime_session(
        &self,
        request: RealtimeSessionCreateRequest,
    ) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
        self.repository.create_realtime_session(request)
    }

    fn get_realtime_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        self.repository.get_realtime_session(session_id)
    }

    fn update_realtime_session(
        &self,
        session_id: &str,
        patch: RealtimeSessionPatchRequest,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        self.repository.update_realtime_session(session_id, patch)
    }

    fn delete_realtime_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        self.repository.delete_realtime_session(session_id)
    }
}

pub(crate) struct AdminApplicationService {
    repository: Arc<dyn AdminRepository>,
}

impl AdminApplicationService {
    pub(crate) fn new(repository: Arc<dyn AdminRepository>) -> Self {
        Self { repository }
    }
}

impl AdminPort for AdminApplicationService {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError> {
        self.repository
            .download_request_content(project_id, request_id, user)
    }
}

pub(crate) struct AdminGraphqlApplicationService {
    repository: Arc<dyn AdminGraphqlRepository>,
}

impl AdminGraphqlApplicationService {
    pub(crate) fn new(repository: Arc<dyn AdminGraphqlRepository>) -> Self {
        Self { repository }
    }
}

impl AdminGraphqlPort for AdminGraphqlApplicationService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = GraphqlExecutionResult> + Send>> {
        self.repository.execute_graphql(request, project_id, user)
    }
}

pub(crate) struct OpenApiGraphqlApplicationService {
    repository: Arc<dyn OpenApiGraphqlRepository>,
}

impl OpenApiGraphqlApplicationService {
    pub(crate) fn new(repository: Arc<dyn OpenApiGraphqlRepository>) -> Self {
        Self { repository }
    }
}

impl OpenApiGraphqlPort for OpenApiGraphqlApplicationService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = GraphqlExecutionResult> + Send>> {
        self.repository.execute_graphql(request, owner_api_key)
    }
}
