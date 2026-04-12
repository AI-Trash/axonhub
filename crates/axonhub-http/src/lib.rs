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
    ProviderEdgeAdminError, ProviderEdgeAdminPort, RequestContextPort, SignInError,
    SystemBootstrapPort, SystemInitializeError, SystemQueryError,
};
pub use routes::router;
pub use routes::router_with_metrics;
pub use routes::router_with_metrics_and_base_path;
pub use handlers::admin::{initialize_system as parity_initialize_system, sign_in as parity_sign_in};
pub use handlers::anthropic::{anthropic_messages as parity_anthropic_messages, list_anthropic_models as parity_list_anthropic_models};
pub use handlers::doubao::doubao_create_task as parity_doubao_create_task;
pub use handlers::gemini::{gemini_generate_content as parity_gemini_generate_content, list_gemini_models as parity_list_gemini_models};
pub use handlers::graphql::{admin_graphql_playground as parity_admin_graphql_playground, openapi_graphql_playground as parity_openapi_graphql_playground};
pub use handlers::jina::{jina_embeddings as parity_jina_embeddings, jina_rerank as parity_jina_rerank};
pub use handlers::openai_v1::{openai_chat_completions as parity_openai_chat_completions, openai_embeddings as parity_openai_embeddings, openai_images_edits as parity_openai_images_edits, openai_images_generations as parity_openai_images_generations, openai_images_variations as parity_openai_images_variations, openai_responses as parity_openai_responses, openai_videos_create as parity_openai_videos_create};
pub use handlers::provider_edge::start_codex_oauth as parity_start_codex_oauth;
pub use handlers::provider_edge::{start_antigravity_oauth as parity_start_antigravity_oauth, start_claudecode_oauth as parity_start_claudecode_oauth, start_copilot_oauth as parity_start_copilot_oauth};
pub use state::{
    AdminCapability, AdminGraphqlCapability, HttpCorsSettings, HttpMetricsCapability,
    HttpMetricsRecorder, HttpState, IdentityCapability, OpenAiV1Capability,
    OpenApiGraphqlCapability, ProviderEdgeAdminCapability, RequestContextCapability,
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
