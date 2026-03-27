use crate::models::{CompatibilityRoute, InitializeSystemResponse, NotImplementedResponse};
use crate::ports::{OpenAiV1Error, ProviderEdgeAdminError};
use crate::state::transport::{
    ErrorResponseSpec, JsonValueResponse, NotImplementedRoute, translate_compatibility_error,
    translate_openai_error, translate_provider_edge_admin_error,
};
use actix_web::http::{Method, StatusCode, Uri};
use actix_web::{HttpResponse, HttpResponseBuilder};
use serde::Serialize;

pub(crate) fn not_implemented_response(
    route_family: &'static str,
    method: Method,
    uri: Uri,
    gemini_api_version: Option<String>,
) -> NotImplementedJsonResponse {
    NotImplementedJsonResponse::from_route(NotImplementedRoute::new(
        route_family,
        method.to_string(),
        uri.path().to_owned(),
        gemini_api_version,
    ))
}

pub(crate) fn error_response(status: StatusCode, kind: &'static str, message: &str) -> HttpResponse {
    response_from_json(ErrorResponseSpec::new(status.as_u16(), kind, message).into_json())
}

pub(crate) fn openai_error_response(error: OpenAiV1Error) -> HttpResponse {
    response_from_json(translate_openai_error(error))
}

pub(crate) fn compatibility_bad_request_response(
    route: CompatibilityRoute,
    message: &str,
) -> HttpResponse {
    compatibility_error_response(
        route,
        OpenAiV1Error::InvalidRequest {
            message: message.to_owned(),
        },
    )
}

pub(crate) fn compatibility_internal_error_response(route: CompatibilityRoute) -> HttpResponse {
    compatibility_error_response(
        route,
        OpenAiV1Error::Internal {
            message: "Compatibility wrapper execution task failed".to_owned(),
        },
    )
}

pub(crate) fn provider_edge_admin_error_response(error: ProviderEdgeAdminError) -> HttpResponse {
    response_from_json(translate_provider_edge_admin_error(error))
}

pub(crate) fn compatibility_error_response(
    route: CompatibilityRoute,
    error: OpenAiV1Error,
) -> HttpResponse {
    response_from_json(translate_compatibility_error(route, error))
}

pub(crate) fn internal_error_response(message: String) -> HttpResponse {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error", &message)
}

pub(crate) fn invalid_initialize_request_response() -> HttpResponse {
    HttpResponse::BadRequest().json(InitializeSystemResponse {
        success: false,
        message: "Invalid request format".to_owned(),
    })
}

pub(crate) fn already_initialized_response() -> HttpResponse {
    HttpResponse::BadRequest().json(InitializeSystemResponse {
        success: false,
        message: "System is already initialized".to_owned(),
    })
}

#[derive(Debug)]
pub(crate) struct NotImplementedJsonResponse {
    pub status: StatusCode,
    pub body: NotImplementedResponse,
}

impl NotImplementedJsonResponse {
    pub fn from_route(route: NotImplementedRoute) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            body: route.into_body(),
        }
    }

    pub fn with_message(mut self, message: &str) -> HttpResponse {
        self.body.message = message.to_owned();
        self.into_response()
    }

    pub fn into_response(self) -> HttpResponse {
        HttpResponseBuilder::new(self.status).json(self.body)
    }
}

fn response_from_json(payload: JsonValueResponse) -> HttpResponse {
    let status = StatusCode::from_u16(payload.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    HttpResponseBuilder::new(status).json(payload.body)
}

pub(crate) async fn execute_provider_edge_admin_request<T, Executor>(
    provider_edge: std::sync::Arc<dyn crate::ports::ProviderEdgeAdminPort>,
    executor: Executor,
) -> HttpResponse
where
    T: Serialize + Send + 'static,
    Executor: FnOnce(
            std::sync::Arc<dyn crate::ports::ProviderEdgeAdminPort>,
        ) -> Result<T, ProviderEdgeAdminError>
        + Send
        + 'static,
{
    let execution_result = tokio::task::spawn_blocking(move || executor(provider_edge)).await;

    match execution_result {
        Ok(Ok(response)) => HttpResponse::Ok().json(response),
        Ok(Err(error)) => provider_edge_admin_error_response(error),
        Err(_) => provider_edge_admin_error_response(ProviderEdgeAdminError::Internal {
            message: "Provider-edge admin execution task failed".to_owned(),
        }),
    }
}
