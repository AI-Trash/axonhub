pub(crate) mod admin;
pub(crate) mod anthropic;
pub(crate) mod doubao;
pub(crate) mod gemini;
pub(crate) mod graphql;
pub(crate) mod jina;
pub(crate) mod openai_v1;
pub(crate) mod provider_edge;
pub(crate) mod static_files;

use crate::errors::{
    compatibility_bad_request_response, compatibility_error_response,
    compatibility_internal_error_response, error_response, internal_error_response,
    not_implemented_response,
};
use crate::middleware::ActixRequest;
use crate::models::{
    CompatibilityRoute, GraphqlExecutionResult, GraphqlRequestPayload, HealthBuildInfo,
    HealthResponse, OpenAiMultipartBody, OpenAiRequestBody, OpenAiV1ExecutionRequest,
    OpenAiV1Route,
    health_timestamp,
};
use crate::state::{
    HttpState, OpenAiV1Capability, RequestAuthContext, RequestContextState,
    request_context_snapshot,
};
use actix_web::body::BoxBody;
use actix_web::http::{Method, StatusCode, Uri};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use bytes::Bytes;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

const AISDK_UI_MESSAGE_STREAM_HEADER: &str = "X-Vercel-Ai-Ui-Message-Stream";
const AISDK_DATA_STREAM_HEADER: &str = "X-Vercel-AI-Data-Stream";
const AISDK_PROTOCOL_VERSION: &str = "v1";

pub(crate) async fn health(state: web::Data<HttpState>) -> web::Json<HealthResponse> {
    let build = HealthBuildInfo::current(&state.version);

    web::Json(HealthResponse {
        status: "healthy",
        timestamp: health_timestamp(),
        version: state.version.clone(),
        uptime: build.uptime.clone(),
        build,
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
    let aisdk_protocol = match detect_aisdk_protocol(&request) {
        Ok(protocol) => protocol,
        Err(response) => return response,
    };

    if let OpenAiV1Capability::Unsupported { message } = &state.openai_v1 {
        return not_implemented_response(
            "/v1/*",
            Method::POST,
            original_uri,
            None,
        )
        .with_message(message);
    }

    let body = match parse_openai_json_body(body) {
        Ok(body) => body,
        Err(response) => {
            return match aisdk_protocol {
                Some(protocol) => aisdk_json_error_response(protocol, StatusCode::BAD_REQUEST, "Invalid request format", "invalid_request"),
                None => response,
            }
        }
    };

    if let Some(protocol) = aisdk_protocol {
        return execute_aisdk_openai_request(state, request, original_uri, route, body, protocol).await;
    }

    execute_openai_request_with_body(state, request, original_uri, route, body, None).await
}

async fn execute_aisdk_openai_request(
    state: HttpState,
    request: HttpRequest,
    original_uri: Uri,
    route: OpenAiV1Route,
    body: OpenAiRequestBody,
    protocol: AiSdkProtocol,
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

    let execution_request = match build_openai_execution_request(request, body, HashMap::new(), None) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => match aisdk_success_response(protocol, route, result) {
            Ok(response) => response,
            Err(message) => aisdk_json_error_response(protocol, StatusCode::BAD_REQUEST, &message, "invalid_request"),
        },
        Ok(Err(error)) => aisdk_openai_error_response(protocol, error),
        Err(_) => aisdk_json_error_response(
            protocol,
            StatusCode::INTERNAL_SERVER_ERROR,
            "OpenAI `/v1` execution task failed",
            "internal_server_error",
        ),
    }
}

pub(crate) async fn execute_openai_request_with_body(
    state: HttpState,
    request: HttpRequest,
    original_uri: Uri,
    route: OpenAiV1Route,
    body: OpenAiRequestBody,
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

    let execution_request = match build_openai_execution_request(
        request,
        OpenAiRequestBody::Json(body),
        path_params,
        None,
    ) {
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
        "<!DOCTYPE html>\n<html>\n  <head>\n  \t<meta charset=\"utf-8\">\n  \t<title>AxonHub</title>\n\t<style>\n\t\tbody {{\n\t\t\tmargin: 0;\n\t\t}}\n\n\t\t#graphiql {{\n\t\t\theight: 100vh;\n\t\t}}\n\n\t\t.loading {{\n        \theight: 100%;\n        \tdisplay: flex;\n        \talign-items: center;\n        \tjustify-content: center;\n        \tfont-size: 4rem;\n\t\t}}\n\t</style>\n\t<script\n\t\tsrc=\"https://cdn.jsdelivr.net/npm/react@18.2.0/umd/react.production.min.js\"\n\t\tintegrity=\"sha256-S0lp&#43;k7zWUMk2ixteM6HZvu8L9Eh//OVrt&#43;ZfbCpmgY=\"\n\t\tcrossorigin=\"anonymous\"\n\t></script>\n\t<script\n\t\tsrc=\"https://cdn.jsdelivr.net/npm/react-dom@18.2.0/umd/react-dom.production.min.js\"\n\t\tintegrity=\"sha256-IXWO0ITNDjfnNXIu5POVfqlgYoop36bDzhodR6LW5Pc=\"\n\t\tcrossorigin=\"anonymous\"\n\t></script>\n\t<link\n\t\trel=\"stylesheet\"\n\t\thref=\"https://cdn.jsdelivr.net/npm/graphiql@4.1.2/graphiql.min.css\"\n\t\tintegrity=\"sha256-MEh&#43;B2NdMSpj9kexQNN3QKc8UzMrCXW/Sx/phcpuyIU=\"\n\t\tcrossorigin=\"anonymous\"\n\t/>\n  </head>\n  <body>\n    <div id=\"graphiql\">\n\t\t<div class=\"loading\">Loading…</div>\n\t</div>\n\n\t<script\n\t\tsrc=\"https://cdn.jsdelivr.net/npm/graphiql@4.1.2/graphiql.min.js\"\n\t\tintegrity=\"sha256-hnImuor1znlJkD/FOTL3jayfS/xsyNoP04abi8bFJWs=\"\n\t\tcrossorigin=\"anonymous\"\n\t></script>\n\n    <script>\n      class PrefixedStorage {{\n        constructor(prefix = '') {{\n          this.prefix = prefix;\n        }}\n\n        _addPrefix(key) {{\n          return this.prefix + key;\n        }}\n\n        _removePrefix(prefixedKey) {{\n          return prefixedKey.substring(this.prefix.length);\n        }}\n\n        setItem(key, value) {{\n          const prefixedKey = this._addPrefix(key);\n          localStorage.setItem(prefixedKey, value);\n        }}\n\n        getItem(key) {{\n          const prefixedKey = this._addPrefix(key);\n          return localStorage.getItem(prefixedKey);\n        }}\n\n        removeItem(key) {{\n          const prefixedKey = this._addPrefix(key);\n          localStorage.removeItem(prefixedKey);\n        }}\n\n        clear() {{\n          const keysToRemove = [];\n          for (let i = 0; i < localStorage.length; i++) {{\n            const key = localStorage.key(i);\n            if (key.startsWith(this.prefix)) {{\n              keysToRemove.push(key);\n            }}\n          }}\n          keysToRemove.forEach(key => localStorage.removeItem(key));\n        }}\n\n        get length() {{\n          let count = 0;\n          for (let i = 0; i < localStorage.length; i++) {{\n            const key = localStorage.key(i);\n            if (key.startsWith(this.prefix)) {{\n              count++;\n            }}\n          }}\n          return count;\n        }}\n\n        key(index) {{\n          const keys = [];\n          for (let i = 0; i < localStorage.length; i++) {{\n            const key = localStorage.key(i);\n            if (key.startsWith(this.prefix)) {{\n              keys.push(this._removePrefix(key));\n            }}\n          }}\n          return index >= 0 && index < keys.length ? keys[index] : null;\n        }}\n      }}\n      const url = location.protocol + '//' + location.host + \"{endpoint}\";\n      const wsProto = location.protocol == 'https:' ? 'wss:' : 'ws:';\n      const subscriptionUrl = wsProto + '//' + location.host + \"{endpoint}\";\n      const fetcherHeaders = undefined;\n      const uiHeaders = undefined;\n\n      let plugins = [];\n\n      const fetcher = GraphiQL.createFetcher({{ url, subscriptionUrl, headers: fetcherHeaders }});\n      ReactDOM.render(\n        React.createElement(GraphiQL, {{\n          fetcher: fetcher,\n          isHeadersEditorEnabled: true,\n          shouldPersistHeaders: true,\n\t\t  headers: JSON.stringify(uiHeaders, null, 2),\n\t\t  plugins: plugins,\n          storage: new PrefixedStorage('')\n        }}),\n        document.getElementById('graphiql'),\n      );\n    </script>\n  </body>\n</html>",
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
    body: OpenAiRequestBody,
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

    let api_key = context
        .auth
        .as_ref()
        .and_then(|auth| match auth {
            RequestAuthContext::ApiKey(key) => Some(key.clone()),
            RequestAuthContext::Admin(_) => None,
        })
        .ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key"))?;

    let api_key_id = Some(api_key.id);

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
        api_key,
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

pub(crate) fn parse_openai_json_body(body: Bytes) -> Result<OpenAiRequestBody, HttpResponse> {
    parse_json_body(body).map(OpenAiRequestBody::Json)
}

pub(crate) fn parse_openai_multipart_body(
    content_type: &str,
    fields: Vec<crate::models::OpenAiMultipartField>,
) -> Result<OpenAiRequestBody, HttpResponse> {
    if fields.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Request body is empty",
        ));
    }

    Ok(OpenAiRequestBody::Multipart(OpenAiMultipartBody {
        content_type: content_type.to_owned(),
        fields,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiSdkProtocol {
    UiMessageStream,
    DataStream,
}

fn detect_aisdk_protocol(request: &HttpRequest) -> Result<Option<AiSdkProtocol>, HttpResponse> {
    if let Some(value) = request
        .headers()
        .get(AISDK_UI_MESSAGE_STREAM_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if value.eq_ignore_ascii_case(AISDK_PROTOCOL_VERSION) {
            return Ok(Some(AiSdkProtocol::UiMessageStream));
        }

        return Err(aisdk_json_error_response(
            AiSdkProtocol::UiMessageStream,
            StatusCode::BAD_REQUEST,
            &format!(
                "Unsupported AiSDK protocol version `{value}` for `{AISDK_UI_MESSAGE_STREAM_HEADER}`"
            ),
            "invalid_request",
        ));
    }

    if let Some(value) = request
        .headers()
        .get(AISDK_DATA_STREAM_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if value.eq_ignore_ascii_case(AISDK_PROTOCOL_VERSION) {
            return Ok(Some(AiSdkProtocol::DataStream));
        }

        return Err(aisdk_json_error_response(
            AiSdkProtocol::DataStream,
            StatusCode::BAD_REQUEST,
            &format!(
                "Unsupported AiSDK protocol version `{value}` for `{AISDK_DATA_STREAM_HEADER}`"
            ),
            "invalid_request",
        ));
    }

    Ok(None)
}

fn aisdk_success_response(
    protocol: AiSdkProtocol,
    route: OpenAiV1Route,
    result: crate::models::OpenAiV1ExecutionResponse,
) -> Result<HttpResponse, String> {
    let text = extract_aisdk_text(route, &result.body).ok_or_else(|| {
        format!(
            "AiSDK compatibility requires a text-generation response body for `{}`",
            route.format()
        )
    })?;
    let message_id = result
        .body
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("msg_axonhub_aisdk");

    Ok(match protocol {
        AiSdkProtocol::UiMessageStream => aisdk_ui_message_stream_response(message_id, text),
        AiSdkProtocol::DataStream => aisdk_text_stream_response(text),
    })
}

fn aisdk_ui_message_stream_response(message_id: &str, text: String) -> HttpResponse {
    let text_id = format!("{message_id}_text");
    let payload = [
        json!({"type":"start","messageId":message_id}).to_string(),
        json!({"type":"start-step"}).to_string(),
        json!({"type":"text-start","id":text_id}).to_string(),
        json!({"type":"text-delta","id":text_id,"delta":text}).to_string(),
        json!({"type":"text-end","id":text_id}).to_string(),
        json!({"type":"finish-step"}).to_string(),
        json!({"type":"finish"}).to_string(),
        "[DONE]".to_owned(),
    ]
    .join("\n");

    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/event-stream; charset=utf-8"))
        .insert_header((AISDK_UI_MESSAGE_STREAM_HEADER, AISDK_PROTOCOL_VERSION))
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("Connection", "keep-alive"))
        .body(payload)
}

fn aisdk_text_stream_response(text: String) -> HttpResponse {
    let payload = [
        format!("0:{}", serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".to_owned())),
        json!({"finishReason":"stop"}).to_string(),
    ];

    HttpResponse::Ok()
        .insert_header(("Content-Type", "text/plain; charset=utf-8"))
        .insert_header((AISDK_DATA_STREAM_HEADER, AISDK_PROTOCOL_VERSION))
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("Connection", "keep-alive"))
        .body(format!("{}\ne:{}\n", payload[0], payload[1]))
}

fn aisdk_openai_error_response(protocol: AiSdkProtocol, error: crate::ports::OpenAiV1Error) -> HttpResponse {
    match error {
        crate::ports::OpenAiV1Error::InvalidRequest { message } => {
            aisdk_json_error_response(protocol, StatusCode::BAD_REQUEST, &message, "invalid_request")
        }
        crate::ports::OpenAiV1Error::Internal { message } => aisdk_json_error_response(
            protocol,
            StatusCode::INTERNAL_SERVER_ERROR,
            &message,
            "internal_server_error",
        ),
        crate::ports::OpenAiV1Error::Upstream { status, body } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            let message = body
                .get("error")
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
                .or_else(|| body.get("message").and_then(Value::as_str))
                .unwrap_or("Upstream request failed");
            let kind = body
                .get("error")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                .or_else(|| body.get("type").and_then(Value::as_str))
                .unwrap_or("upstream_error");
            aisdk_json_error_response(protocol, status, message, kind)
        }
    }
}

fn aisdk_json_error_response(
    protocol: AiSdkProtocol,
    status: StatusCode,
    message: &str,
    kind: &str,
) -> HttpResponse {
    let mut builder = HttpResponse::build(status);
    builder.insert_header(("Content-Type", "application/json"));

    match protocol {
        AiSdkProtocol::UiMessageStream => {
            builder.insert_header((AISDK_UI_MESSAGE_STREAM_HEADER, AISDK_PROTOCOL_VERSION));
        }
        AiSdkProtocol::DataStream => {
            builder.insert_header((AISDK_DATA_STREAM_HEADER, AISDK_PROTOCOL_VERSION));
        }
    }

    builder.json(json!({"message": message, "type": kind}))
}

fn extract_aisdk_text(route: OpenAiV1Route, body: &Value) -> Option<String> {
    match route {
        OpenAiV1Route::ChatCompletions => {
            extract_chat_completion_text(body).or_else(|| body.get("text").and_then(Value::as_str).map(ToOwned::to_owned))
        }
        OpenAiV1Route::Responses | OpenAiV1Route::ResponsesCompact | OpenAiV1Route::Realtime => {
            extract_responses_text(body)
                .or_else(|| extract_chat_completion_text(body))
                .or_else(|| body.get("text").and_then(Value::as_str).map(ToOwned::to_owned))
        }
        OpenAiV1Route::Embeddings
        | OpenAiV1Route::ImagesGenerations
        | OpenAiV1Route::ImagesEdits
        | OpenAiV1Route::ImagesVariations => None,
    }
}

fn extract_chat_completion_text(body: &Value) -> Option<String> {
    let message_content = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))?;

    match message_content {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                })
                .collect::<String>(),
        )
        .filter(|value| !value.is_empty()),
        _ => None,
    }
}

fn extract_responses_text(body: &Value) -> Option<String> {
    if let Some(output_text) = body.get("output_text").and_then(Value::as_str).filter(|value| !value.is_empty()) {
        return Some(output_text.to_owned());
    }

    body.get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flatten()
                .filter_map(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                })
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
}

pub(crate) fn realtime_upgrade_header_present(request: &HttpRequest) -> bool {
    let has_upgrade = request
        .headers()
        .get("Upgrade")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().is_empty());
    let has_connection_upgrade = request
        .headers()
        .get("Connection")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|token| token.trim().eq_ignore_ascii_case("upgrade")));
    let has_sec_websocket = request.headers().keys().any(|name| {
        name.as_str()
            .to_ascii_lowercase()
            .starts_with("sec-websocket-")
    });

    has_upgrade || has_connection_upgrade || has_sec_websocket
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

pub(crate) async fn not_found() -> HttpResponse {
    HttpResponse::NotFound().json(serde_json::json!({
        "error": "not_found",
        "status": 404,
        "message": "The requested endpoint does not exist"
    }))
}
