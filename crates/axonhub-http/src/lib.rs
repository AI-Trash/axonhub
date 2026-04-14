mod errors;
mod handlers;
mod middleware;
mod models;
mod ports;
mod routes;
mod state;

pub use models::{
    AdminContentDownload, AnthropicModel, AnthropicModelListResponse, ApiKeyType,
    AuthApiKeyContext, AuthUserContext, CompatibilityRoute, ExchangeCallbackOAuthRequest,
    ExchangeOAuthResponse, GeminiModel, GeminiModelListResponse, GlobalId,
    GraphqlExecutionResult, GraphqlRequestPayload, InitializeSystemRequest, ModelCapabilities,
    ModelListResponse, ModelPricing, OAuthProxyConfig, OAuthProxyType, OpenAiModel,
    OpenAiMultipartBody, OpenAiMultipartField, OpenAiRequestBody, OpenAiV1EventStream,
    OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Route,
    PollCopilotOAuthRequest, PollCopilotOAuthResponse, ProjectContext,
    RealtimeSessionCreateRequest, RealtimeSessionPatchRequest, RealtimeSessionRecord,
    RealtimeSessionTransportRequest, RoleInfo, SignInRequest, SignInSuccess,
    StartAntigravityOAuthRequest, StartCopilotOAuthRequest, StartCopilotOAuthResponse,
    StartPkceOAuthRequest, StartPkceOAuthResponse, ThreadContext, TraceConfig,
    TraceContext, UserProjectInfo,
};
pub use ports::{
    AdminAuthError, AdminError, AdminGraphqlPort, AdminPort, ApiKeyAuthError,
    ContextResolveError, IdentityPort, OpenAiV1Error, OpenAiV1Port, OpenApiGraphqlPort,
    OauthProviderAdminError, OauthProviderAdminPort, RequestContextPort, SignInError,
    SystemBootstrapPort, SystemInitializeError, SystemQueryError,
};
pub use routes::router;
pub use routes::router_with_metrics;
pub use routes::router_with_metrics_and_base_path;
pub use state::{
    AdminCapability, AdminGraphqlCapability, HttpCorsSettings, HttpMetricsCapability,
    HttpMetricsRecorder, HttpState, IdentityCapability, OpenAiV1Capability,
    OauthProviderAdminCapability, OpenApiGraphqlCapability, RequestContextCapability,
    SystemBootstrapCapability,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn tracing_inbound_traceparent_links_span() {
    tests::tracing_inbound_traceparent_links_span();
}

#[cfg(test)]
#[test]
fn tracing_sensitive_fields_not_recorded() {
    tests::tracing_sensitive_fields_not_recorded();
}

#[cfg(test)]
#[test]
fn trace_resolution_internal_failure_is_fail_open() {
    tests::trace_resolution_internal_failure_is_fail_open();
}
