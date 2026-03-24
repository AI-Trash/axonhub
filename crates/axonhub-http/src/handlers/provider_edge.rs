use crate::errors::{error_response, execute_provider_edge_admin_request};
use crate::handlers::provider_edge_admin_port;
use crate::state::HttpState;
use axum::extract::{rejection::JsonRejection, OriginalUri, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::Response;
use axum::Json;
use std::sync::Arc;

fn unsupported_provider_edge_response<'a>(
    state: &'a HttpState,
    route_family: &'static str,
    method: Method,
    uri: Uri,
) -> Result<&'a Arc<dyn crate::ports::ProviderEdgeAdminPort>, Response> {
    match provider_edge_admin_port(state) {
        Ok(provider_edge) => Ok(provider_edge),
        Err(message) => Err(
            crate::errors::not_implemented_response(route_family, method, uri, None)
                .with_message(&message),
        ),
    }
}

pub(crate) async fn start_codex_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::StartPkceOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/codex/oauth/start",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.start_codex_oauth(&request)
    })
    .await
}

pub(crate) async fn exchange_codex_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::ExchangeCallbackOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/codex/oauth/exchange",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.exchange_codex_oauth(&request)
    })
    .await
}

pub(crate) async fn start_claudecode_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::StartPkceOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/claudecode/oauth/start",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.start_claudecode_oauth(&request)
    })
    .await
}

pub(crate) async fn exchange_claudecode_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::ExchangeCallbackOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/claudecode/oauth/exchange",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.exchange_claudecode_oauth(&request)
    })
    .await
}

pub(crate) async fn start_antigravity_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::StartAntigravityOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/antigravity/oauth/start",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.start_antigravity_oauth(&request)
    })
    .await
}

pub(crate) async fn exchange_antigravity_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::ExchangeCallbackOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/antigravity/oauth/exchange",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.exchange_antigravity_oauth(&request)
    })
    .await
}

pub(crate) async fn start_copilot_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::StartCopilotOAuthRequest>, JsonRejection>,
) -> Response {
    let request = match payload {
        Ok(payload) => payload.0,
        Err(error) if error.body_text().contains("EOF") => crate::models::StartCopilotOAuthRequest {},
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/copilot/oauth/start",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.start_copilot_oauth(&request)
    })
    .await
}

pub(crate) async fn poll_copilot_oauth(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<crate::models::PollCopilotOAuthRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let provider_edge = match unsupported_provider_edge_response(
        &state,
        "/admin/copilot/oauth/poll",
        Method::POST,
        original_uri,
    ) {
        Ok(provider_edge) => provider_edge,
        Err(response) => return response,
    };

    execute_provider_edge_admin_request(Arc::clone(provider_edge), move |provider_edge: Arc<dyn crate::ports::ProviderEdgeAdminPort>| {
        provider_edge.poll_copilot_oauth(&request)
    })
    .await
}
