use crate::errors::{error_response, execute_provider_edge_admin_request};
use crate::handlers::provider_edge_admin_port;
use crate::state::HttpState;
use actix_web::http::{Method, StatusCode, Uri};
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use std::sync::Arc;

fn unsupported_provider_edge_response<'a>(
    state: &'a HttpState,
    route_family: &'static str,
    method: Method,
    uri: Uri,
) -> Result<&'a Arc<dyn crate::ports::ProviderEdgeAdminPort>, HttpResponse> {
    match provider_edge_admin_port(state) {
        Ok(provider_edge) => Ok(provider_edge),
        Err(message) => Err(
            crate::errors::not_implemented_response(route_family, method, uri, None)
                .with_message(&message),
        ),
    }
}

pub async fn start_codex_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::StartPkceOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/codex/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_codex_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn exchange_codex_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::ExchangeCallbackOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/codex/oauth/exchange",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.exchange_codex_oauth(&request_payload)
    })
    .await
}

pub async fn start_claudecode_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::StartPkceOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/claudecode/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_claudecode_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn exchange_claudecode_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::ExchangeCallbackOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/claudecode/oauth/exchange",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.exchange_claudecode_oauth(&request_payload)
    })
    .await
}

pub async fn start_antigravity_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::StartAntigravityOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/antigravity/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_antigravity_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn exchange_antigravity_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::ExchangeCallbackOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/antigravity/oauth/exchange",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.exchange_antigravity_oauth(&request_payload)
    })
    .await
}

pub async fn start_copilot_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload = if body.is_empty() {
        crate::models::StartCopilotOAuthRequest {}
    } else {
        match serde_json::from_slice(&body) {
            Ok(payload) => payload,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "Bad Request",
                    "invalid request format",
                )
            }
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/copilot/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.start_copilot_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn poll_copilot_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let request_payload: crate::models::PollCopilotOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/copilot/oauth/poll",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge| {
        provider_edge.poll_copilot_oauth(&request_payload)
    })
    .await
}
