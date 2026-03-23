pub(crate) mod admin;
pub(crate) mod anthropic;
pub(crate) mod doubao;
pub(crate) mod gemini;
pub(crate) mod graphql;
pub(crate) mod jina;
pub(crate) mod openai_v1;
pub(crate) mod provider_edge;
pub(crate) mod unported;

use crate::errors::{
    compatibility_bad_request_response, compatibility_error_response,
    compatibility_internal_error_response, error_response, internal_error_response,
    not_implemented_response,
};
use crate::models::{
    CompatibilityRoute, GraphqlExecutionResult, GraphqlRequestPayload, HealthResponse,
    OpenAiV1ExecutionRequest, OpenAiV1Route,
};
use crate::state::{request_context_snapshot, HttpState, OpenAiV1Capability, RequestAuthContext, RequestContextState};
use axum::body;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{extract::State, Json};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub(crate) async fn health(State(state): State<HttpState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: state.service_name,
        version: state.version,
        backend: "rust",
        migration_status: "first migration slice",
        api_parity: "partial",
        legacy_go_backend_present: true,
        config_source: state.config_source,
    })
}

pub(crate) async fn debug_context(request: axum::extract::Request) -> impl IntoResponse {
    let context = request
        .extensions()
        .get::<RequestContextState>()
        .cloned()
        .unwrap_or_default();

    let snapshot = request_context_snapshot(context);
    (StatusCode::OK, Json(snapshot))
}

pub(crate) async fn execute_openai_request(
    state: HttpState,
    mut request: axum::extract::Request,
    original_uri: Uri,
    route: OpenAiV1Route,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::POST, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let body = match parse_json_body(&mut request).await {
        Ok(body) => body,
        Err(response) => return response,
    };

    let execution_request = match build_openai_execution_request(request, body, HashMap::new()) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => {
            let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
            (status, Json(result.body)).into_response()
        }
        Ok(Err(error)) => crate::errors::openai_error_response(error),
        Err(_) => internal_error_response("OpenAI `/v1` execution task failed".to_owned()),
    }
}

pub(crate) async fn execute_compatibility_request(
    state: HttpState,
    mut request: axum::extract::Request,
    original_uri: Uri,
    route: CompatibilityRoute,
    path_params: HashMap<String, String>,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            let route_family = match route {
                CompatibilityRoute::AnthropicMessages => "/anthropic/v1/*",
                CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => "/jina/v1/*",
                CompatibilityRoute::GeminiGenerateContent
                | CompatibilityRoute::GeminiStreamGenerateContent => {
                    if original_uri.path().starts_with("/v1beta") {
                        "/v1beta/*"
                    } else {
                        "/gemini/:gemini_api_version/*"
                    }
                }
                CompatibilityRoute::DoubaoCreateTask
                | CompatibilityRoute::DoubaoGetTask
                | CompatibilityRoute::DoubaoDeleteTask => "/doubao/v3/*",
            };
            return not_implemented_response(route_family, Method::POST, original_uri, None)
                .with_message(message);
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let body = match route {
        CompatibilityRoute::AnthropicMessages
        | CompatibilityRoute::JinaRerank
        | CompatibilityRoute::JinaEmbeddings
        | CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent
        | CompatibilityRoute::DoubaoCreateTask => match parse_json_body_for_compatibility(&mut request, route).await {
            Ok(body) => body,
            Err(response) => return response,
        },
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => Value::Null,
    };

    let execution_request = match build_openai_execution_request(request, body, path_params) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute_compatibility(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => {
            let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
            (status, Json(result.body)).into_response()
        }
        Ok(Err(error)) => compatibility_error_response(route, error),
        Err(_) => compatibility_internal_error_response(route),
    }
}

pub(crate) async fn execute_graphql_request<Executor>(
    mut request: axum::extract::Request,
    executor: Executor,
) -> Response
where
    Executor: FnOnce(GraphqlRequestPayload) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>,
{
    let body = match body::to_bytes(std::mem::take(request.body_mut()), usize::MAX).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Invalid request format",
            )
        }
    };

    if body.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Request body is empty",
        );
    }

    let payload: GraphqlRequestPayload = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Invalid request format",
            )
        }
    };

    let result = executor(payload).await;
    let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
    (status, Json(result.body)).into_response()
}

pub(crate) fn graphql_playground_html(endpoint: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>AxonHub GraphQL Playground</title></head><body><div id=\"root\"></div><script>window.GRAPHQL_ENDPOINT={endpoint:?};</script><p>GraphQL playground endpoint: <code>{endpoint}</code></p></body></html>"
    )
}

pub(crate) fn provider_edge_admin_port(
    state: &HttpState,
) -> Result<&Arc<dyn crate::ports::ProviderEdgeAdminPort>, Response> {
    match &state.provider_edge_admin {
        crate::state::ProviderEdgeAdminCapability::Unsupported { message } => {
            Err(crate::errors::auth_unsupported_response(message))
        }
        crate::state::ProviderEdgeAdminCapability::Available { provider_edge } => Ok(provider_edge),
    }
}

fn build_openai_execution_request(
    mut request: axum::extract::Request,
    body: Value,
    path_params: HashMap<String, String>,
) -> Result<OpenAiV1ExecutionRequest, Response> {
    let path = request.uri().path().to_owned();
    let query = request
        .uri()
        .query()
        .map(parse_query_pairs)
        .unwrap_or_default();
    let context = request
        .extensions_mut()
        .remove::<RequestContextState>()
        .unwrap_or_default();

    let project = context.project.ok_or_else(|| {
        error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Project ID not found in context",
        )
    })?;

    let api_key_id = context.auth.as_ref().and_then(|auth| match auth {
        RequestAuthContext::ApiKey(key) => Some(key.id),
        RequestAuthContext::Admin(_) => None,
    });

    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let headers = request
        .headers()
        .iter()
        .filter_map(|(name, value)| value.to_str().ok().map(|current| (name.as_str().to_owned(), current.to_owned())))
        .collect::<HashMap<_, _>>();

    Ok(OpenAiV1ExecutionRequest {
        headers,
        body,
        path,
        path_params,
        query,
        project,
        trace: context.trace,
        api_key_id,
        client_ip,
    })
}

async fn parse_json_body(request: &mut axum::extract::Request) -> Result<Value, Response> {
    let body = body::to_bytes(std::mem::take(request.body_mut()), usize::MAX)
        .await
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"))?;

    if body.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Request body is empty",
        ));
    }

    serde_json::from_slice(&body)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"))
}

async fn parse_json_body_for_compatibility(
    request: &mut axum::extract::Request,
    route: CompatibilityRoute,
) -> Result<Value, Response> {
    let body = body::to_bytes(std::mem::take(request.body_mut()), usize::MAX)
        .await
        .map_err(|_| compatibility_bad_request_response(route, "Invalid request format"))?;

    if body.is_empty() {
        return Err(compatibility_bad_request_response(route, "Request body is empty"));
    }

    serde_json::from_slice(&body)
        .map_err(|_| compatibility_bad_request_response(route, "Invalid request format"))
}

pub(crate) fn gemini_version_from_path(path: &str) -> Option<String> {
    if path.starts_with("/v1beta") {
        return Some("v1beta".to_owned());
    }

    path.split('/')
        .collect::<Vec<_>>()
        .windows(3)
        .find_map(|window| (window[1] == "gemini").then(|| window[2].to_owned()))
}

pub(crate) fn parse_query_pairs(raw: &str) -> HashMap<String, String> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key.to_owned(), value.to_owned())
        })
        .collect()
}
