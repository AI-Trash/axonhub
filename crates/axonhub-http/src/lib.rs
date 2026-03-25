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
    ModelListResponse, ModelPricing, OpenAiModel, OpenAiV1ExecutionRequest,
    OpenAiV1ExecutionResponse, OpenAiV1Route, PollCopilotOAuthRequest,
    PollCopilotOAuthResponse, ProjectContext, RoleInfo, SignInRequest, SignInSuccess,
    StartAntigravityOAuthRequest, StartCopilotOAuthRequest, StartCopilotOAuthResponse,
    StartPkceOAuthRequest, StartPkceOAuthResponse, ThreadContext, TraceConfig, TraceContext,
    UserProjectInfo,
};
pub use ports::{
    AdminAuthError, AdminError, AdminGraphqlPort, AdminPort, ApiKeyAuthError,
    ContextResolveError, IdentityPort, OpenAiV1Error, OpenAiV1Port, OpenApiGraphqlPort,
    ProviderEdgeAdminError, ProviderEdgeAdminPort, RequestContextPort, SignInError,
    SystemBootstrapPort, SystemInitializeError, SystemQueryError,
};
pub use routes::router;
pub use routes::router_with_metrics;
pub use routes::router_with_metrics_and_base_path;
pub use state::{
    AdminCapability, AdminGraphqlCapability, HttpMetricsCapability, HttpMetricsRecorder,
    HttpState, IdentityCapability, OpenAiV1Capability, OpenApiGraphqlCapability,
    ProviderEdgeAdminCapability, RequestContextCapability, SystemBootstrapCapability,
};

#[cfg(test)]
mod tests;
