use crate::models::{CompatibilityRoute, ErrorDetail, ErrorResponse, InitializeSystemResponse, NotImplementedResponse};
use crate::ports::{OpenAiV1Error, ProviderEdgeAdminError};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;

pub(crate) fn not_implemented_response(
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
            message: "This surface is not supported by the Rust backend yet. Supported AxonHub replacement scope remains SQLite-backed admin, GraphQL, CLI/config, and inference routes only.".to_owned(),
            migration_status: "progressive cutover",
            legacy_go_backend_present: false,
            gemini_api_version,
        },
    }
}

pub(crate) fn auth_unsupported_response(message: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(NotImplementedResponse {
            error: "not_implemented",
            status: StatusCode::NOT_IMPLEMENTED.as_u16(),
            route_family: "/auth/context",
            method: "UNKNOWN".to_owned(),
            path: "/".to_owned(),
            message: message.to_owned(),
            migration_status: "progressive cutover",
            legacy_go_backend_present: false,
            gemini_api_version: None,
        }),
    )
        .into_response()
}

pub(crate) fn error_response(status: StatusCode, kind: &'static str, message: &str) -> Response {
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

pub(crate) fn openai_error_response(error: OpenAiV1Error) -> Response {
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

pub(crate) fn compatibility_bad_request_response(route: CompatibilityRoute, message: &str) -> Response {
    compatibility_error_response(
        route,
        OpenAiV1Error::InvalidRequest {
            message: message.to_owned(),
        },
    )
}

pub(crate) fn compatibility_internal_error_response(route: CompatibilityRoute) -> Response {
    compatibility_error_response(
        route,
        OpenAiV1Error::Internal {
            message: "Compatibility wrapper execution task failed".to_owned(),
        },
    )
}

pub(crate) fn provider_edge_admin_error_response(error: ProviderEdgeAdminError) -> Response {
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

pub(crate) fn compatibility_error_response(route: CompatibilityRoute, error: OpenAiV1Error) -> Response {
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

pub(crate) fn internal_error_response(message: String) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error", &message)
}

pub(crate) fn invalid_initialize_request_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(InitializeSystemResponse {
            success: false,
            message: "Invalid request format".to_owned(),
        }),
    )
        .into_response()
}

pub(crate) fn already_initialized_response() -> Response {
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
pub(crate) struct NotImplementedJsonResponse {
    pub status: StatusCode,
    pub body: NotImplementedResponse,
}

impl NotImplementedJsonResponse {
    pub fn with_message(mut self, message: &str) -> Response {
        self.body.message = message.to_owned();
        self.into_response()
    }
}

impl IntoResponse for NotImplementedJsonResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub(crate) async fn execute_provider_edge_admin_request<T, Executor>(
    provider_edge: std::sync::Arc<dyn crate::ports::ProviderEdgeAdminPort>,
    executor: Executor,
) -> Response
where
    T: Serialize + Send + 'static,
    Executor: FnOnce(std::sync::Arc<dyn crate::ports::ProviderEdgeAdminPort>) -> Result<T, ProviderEdgeAdminError>
        + Send
        + 'static,
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
