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
use crate::middleware::ActixRequest;
use crate::models::{
    CompatibilityRoute, GraphqlExecutionResult, GraphqlRequestPayload, HealthResponse,
    OpenAiV1ExecutionRequest, OpenAiV1Route,
};
use crate::state::{
    HttpState, OpenAiV1Capability, RequestAuthContext, RequestContextState,
    request_context_snapshot,
};
use actix_web::body::BoxBody;
use actix_web::http::{Method, StatusCode, Uri};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use bytes::Bytes;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

const AISDK_PROTOCOL_NOT_IMPLEMENTED_MESSAGE: &str = "AiSDK compatibility is not supported in the Rust HTTP slice yet. Requests that opt into the Vercel AI SDK protocol via `X-Vercel-Ai-Ui-Message-Stream` or `X-Vercel-AI-Data-Stream` remain on the explicit `/v1/*` 501 boundary.";

pub(crate) async fn health(state: web::Data<HttpState>) -> web::Json<HealthResponse> {
    web::Json(HealthResponse {
        status: "ok",
        service: state.service_name.clone(),
        version: state.version.clone(),
        backend: "rust",
        migration_status: "progressive cutover",
        api_parity: "supported_scope",
        legacy_go_backend_present: false,
        config_source: state.config_source.clone(),
    })
}

pub(crate) async fn debug_context(request: ActixRequest) -> HttpResponse {
    let context = request
        .0
        .extensions()
        .get::<RequestContextState>()
        .cloned()
        .unwrap_or_default();

    let snapshot = request_context_snapshot(context);
    HttpResponse::Ok().json(snapshot)
}

pub(crate) async fn execute_openai_request(
    state: HttpState,
    request: HttpRequest,
    body: Bytes,
    original_uri: Uri,
    route: OpenAiV1Route,
) -> HttpResponse {
    if let OpenAiV1Capability::Unsupported { message } = &state.openai_v1 {
        return not_implemented_response(
            "/v1/*",
            Method::POST,
            original_uri,
            None,
        )
        .with_message(message);
    }

    if let Some(response) = aisdk_protocol_not_implemented_response(&request, original_uri.clone()) {
        return response;
    }

    let body = match parse_json_body(body) {
        Ok(body) => body,
        Err(response) => return response,
    };

    execute_openai_request_with_body(state, request, original_uri, route, body, None).await
}

pub(crate) async fn execute_openai_request_with_body(
    state: HttpState,
    request: HttpRequest,
    original_uri: Uri,
    route: OpenAiV1Route,
    body: Value,
    channel_hint_id: Option<i64>,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response(
                "/v1/*",
                Method::POST,
                original_uri,
                None,
            )
            .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let execution_request = match build_openai_execution_request(request, body, HashMap::new(), channel_hint_id) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => {
            let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
            actix_json_response(status, result.body)
        }
        Ok(Err(error)) => crate::errors::openai_error_response(error),
        Err(_) => internal_error_response("OpenAI `/v1` execution task failed".to_owned()),
    }
}

pub(crate) async fn execute_compatibility_request(
    state: HttpState,
    request: HttpRequest,
    body: Bytes,
    original_uri: Uri,
    route: CompatibilityRoute,
    path_params: HashMap<String, String>,
) -> HttpResponse {
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
        | CompatibilityRoute::DoubaoCreateTask => match parse_json_body_for_compatibility(body, route) {
            Ok(body) => body,
            Err(response) => return response,
        },
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => Value::Null,
    };

    let execution_request = match build_openai_execution_request(request, body, path_params, None) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute_compatibility(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => {
            let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
            actix_json_response(status, result.body)
        }
        Ok(Err(error)) => compatibility_error_response(route, error),
        Err(_) => compatibility_internal_error_response(route),
    }
}

pub(crate) async fn execute_graphql_request<Executor>(
    body: Bytes,
    executor: Executor,
) -> HttpResponse
where
    Executor: FnOnce(
        GraphqlRequestPayload,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>>,
{
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
    actix_json_response(status, result.body)
}

pub(crate) fn graphql_playground_html(endpoint: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>AxonHub GraphQL Playground</title></head><body><div id=\"root\"></div><script>window.GRAPHQL_ENDPOINT={endpoint:?};</script><p>GraphQL playground endpoint: <code>{endpoint}</code></p></body></html>"
    )
}

pub(crate) fn provider_edge_admin_port(
    state: &HttpState,
) -> Result<&Arc<dyn crate::ports::ProviderEdgeAdminPort>, String> {
    match &state.provider_edge_admin {
        crate::state::ProviderEdgeAdminCapability::Unsupported { message } => Err(message.clone()),
        crate::state::ProviderEdgeAdminCapability::Available { provider_edge } => Ok(provider_edge),
    }
}

pub(crate) fn build_openai_execution_request(
    request: HttpRequest,
    body: Value,
    path_params: HashMap<String, String>,
    channel_hint_id: Option<i64>,
) -> Result<OpenAiV1ExecutionRequest, HttpResponse> {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(parse_query_pairs).unwrap_or_default();
    let context = request
        .extensions()
        .get::<RequestContextState>()
        .cloned()
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
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|current| (name.as_str().to_owned(), current.to_owned()))
        })
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
        channel_hint_id,
    })
}

pub(crate) fn parse_json_body(body: Bytes) -> Result<Value, HttpResponse> {
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

fn aisdk_protocol_not_implemented_response(
    request: &HttpRequest,
    original_uri: Uri,
) -> Option<HttpResponse> {
    aisdk_protocol_header_present(request).then(|| {
        not_implemented_response(
            "/v1/*",
            Method::from(request.method().clone()),
            original_uri,
            None,
        )
        .with_message(AISDK_PROTOCOL_NOT_IMPLEMENTED_MESSAGE)
    })
}

fn aisdk_protocol_header_present(request: &HttpRequest) -> bool {
    ["X-Vercel-Ai-Ui-Message-Stream", "X-Vercel-AI-Data-Stream"]
        .into_iter()
        .any(|name| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| !value.trim().is_empty())
        })
}

fn parse_json_body_for_compatibility(
    body: Bytes,
    route: CompatibilityRoute,
) -> Result<Value, HttpResponse> {
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

pub(crate) fn actix_json_response(status: StatusCode, value: Value) -> HttpResponse<BoxBody> {
    HttpResponse::build(status).json(value)
}
