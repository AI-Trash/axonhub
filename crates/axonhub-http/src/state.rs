use crate::models::{
    AuthApiKeyContext, AuthUserContext, ProjectContext, RequestContextSnapshot, ThreadContext,
    TraceConfig, TraceContext,
};
use crate::ports::{
    AdminGraphqlPort, AdminPort, IdentityPort, OpenAiV1Port, OpenApiGraphqlPort,
    ProviderEdgeAdminPort, RequestContextPort, SystemBootstrapPort,
};
use std::sync::Arc;
use std::time::Duration;

#[path = "transport.rs"]
pub(crate) mod transport;

#[derive(Clone)]
pub enum SystemBootstrapCapability {
    Unsupported {
        message: String,
    },
    Available {
        system: Arc<dyn SystemBootstrapPort>,
    },
}

#[derive(Clone)]
pub enum IdentityCapability {
    Unsupported { message: String },
    Available { identity: Arc<dyn IdentityPort> },
}

#[derive(Clone)]
pub enum RequestContextCapability {
    Unsupported {
        message: String,
    },
    Available {
        request_context: Arc<dyn RequestContextPort>,
    },
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
    Unsupported {
        message: String,
    },
    Available {
        graphql: Arc<dyn OpenApiGraphqlPort>,
    },
}

#[derive(Clone)]
pub enum ProviderEdgeAdminCapability {
    Unsupported {
        message: String,
    },
    Available {
        provider_edge: Arc<dyn ProviderEdgeAdminPort>,
    },
}

pub trait HttpMetricsRecorder: Send + Sync {
    fn record_http_request(&self, method: &str, path: &str, status_code: u16, duration: Duration);
}

#[derive(Clone)]
pub enum HttpMetricsCapability {
    Disabled,
    Available {
        recorder: Arc<dyn HttpMetricsRecorder>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct HttpCorsSettings {
    pub enabled: bool,
    pub debug: bool,
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub exposed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age_seconds: Option<usize>,
}

#[derive(Clone)]
pub struct HttpState {
    pub service_name: String,
    pub version: String,
    pub config_source: Option<String>,
    pub system_bootstrap: SystemBootstrapCapability,
    pub identity: IdentityCapability,
    pub request_context: RequestContextCapability,
    pub openai_v1: OpenAiV1Capability,
    pub admin: AdminCapability,
    pub admin_graphql: AdminGraphqlCapability,
    pub openapi_graphql: OpenApiGraphqlCapability,
    pub provider_edge_admin: ProviderEdgeAdminCapability,
    pub allow_no_auth: bool,
    pub cors: HttpCorsSettings,
    pub trace_config: TraceConfig,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RequestContextState {
    pub request_id: Option<String>,
    pub auth: Option<RequestAuthContext>,
    pub project: Option<ProjectContext>,
    pub thread: Option<ThreadContext>,
    pub trace: Option<TraceContext>,
}

impl RequestContextState {
    pub(crate) fn with_auth(mut self, auth: RequestAuthContext) -> Self {
        self.auth = Some(auth);
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RequestAuthContext {
    Admin(AuthUserContext),
    ApiKey(AuthApiKeyContext),
}

impl RequestAuthContext {
    pub(crate) fn project(&self) -> Option<ProjectContext> {
        match self {
            Self::ApiKey(key) => Some(key.project.clone()),
            Self::Admin(_) => None,
        }
    }
}

pub(crate) fn request_context_snapshot(context: RequestContextState) -> RequestContextSnapshot {
    RequestContextSnapshot {
        request_id: context.request_id,
        auth: context.auth.map(|auth| match auth {
            RequestAuthContext::Admin(user) => crate::models::AuthSnapshot {
                mode: "jwt",
                user_id: Some(user.id),
                api_key_id: None,
                api_key_type: None,
            },
            RequestAuthContext::ApiKey(key) => crate::models::AuthSnapshot {
                mode: match key.key_type {
                    crate::models::ApiKeyType::NoAuth => "noauth",
                    crate::models::ApiKeyType::ServiceAccount | crate::models::ApiKeyType::User => {
                        "api_key"
                    }
                },
                user_id: None,
                api_key_id: Some(key.id),
                api_key_type: Some(match key.key_type {
                    crate::models::ApiKeyType::User => "user",
                    crate::models::ApiKeyType::ServiceAccount => "service_account",
                    crate::models::ApiKeyType::NoAuth => "noauth",
                }),
            },
        }),
        project: context.project,
        thread: context.thread,
        trace: context.trace,
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiQueryKey {
    pub key: Option<String>,
}

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ModelsQuery {
    pub include: Option<String>,
}
