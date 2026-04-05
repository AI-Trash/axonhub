use crate::models::{
    ApiKeyType, CompatibilityRoute, ErrorDetail, ErrorResponse, NotImplementedResponse,
    ProjectContext, TraceConfig,
};
use crate::ports::{
    AdminAuthError, ApiKeyAuthError, ContextResolveError, OpenAiV1Error, ProviderEdgeAdminError,
};
use crate::state::{
    IdentityCapability, RequestAuthContext, RequestContextCapability, RequestContextState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

const NOT_IMPLEMENTED_MESSAGE: &str = "This endpoint is not yet supported by the Rust backend.";
const MIGRATION_STATUS: &str = "progressive cutover";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TransportHeaders {
    entries: HashMap<String, String>,
}

impl TransportHeaders {
    pub fn insert(&mut self, name: &str, value: &str) {
        self.entries
            .insert(name.to_ascii_lowercase(), value.to_owned());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErrorResponseSpec {
    pub status: u16,
    pub kind: &'static str,
    pub message: String,
}

impl ErrorResponseSpec {
    pub fn new(status: u16, kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            kind,
            message: message.into(),
        }
    }

    pub fn into_body(self) -> ErrorResponse {
        ErrorResponse {
            error: ErrorDetail {
                r#type: self.kind,
                message: self.message,
            },
        }
    }

    pub fn into_json(self) -> JsonValueResponse {
        JsonValueResponse::from_serializable(self.status, self.into_body())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JsonValueResponse {
    pub status: u16,
    pub body: Value,
}

impl JsonValueResponse {
    pub fn from_serializable<T>(status: u16, body: T) -> Self
    where
        T: Serialize,
    {
        Self {
            status,
            body: serde_json::to_value(body).expect("transport response should serialize"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotImplementedRoute {
    pub route_family: &'static str,
    pub method: String,
    pub path: String,
    pub message: String,
    pub migration_status: &'static str,
    pub legacy_go_backend_present: bool,
    pub gemini_api_version: Option<String>,
}

impl NotImplementedRoute {
    pub fn new(
        route_family: &'static str,
        method: impl Into<String>,
        path: impl Into<String>,
        gemini_api_version: Option<String>,
    ) -> Self {
        Self {
            route_family,
            method: method.into(),
            path: path.into(),
            message: NOT_IMPLEMENTED_MESSAGE.to_owned(),
            migration_status: MIGRATION_STATUS,
            legacy_go_backend_present: false,
            gemini_api_version,
        }
    }

    pub fn into_body(self) -> NotImplementedResponse {
        NotImplementedResponse {
            error: "not_implemented",
            status: 501,
            route_family: self.route_family,
            method: self.method,
            path: self.path,
            message: self.message,
            migration_status: self.migration_status,
            legacy_go_backend_present: self.legacy_go_backend_present,
            gemini_api_version: self.gemini_api_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpRejection {
    Error(ErrorResponseSpec),
    NotImplemented(NotImplementedRoute),
}

impl From<ErrorResponseSpec> for HttpRejection {
    fn from(value: ErrorResponseSpec) -> Self {
        Self::Error(value)
    }
}

impl From<NotImplementedRoute> for HttpRejection {
    fn from(value: NotImplementedRoute) -> Self {
        Self::NotImplemented(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpMetricRecord {
    pub method: String,
    pub path: String,
    pub status_code: u16,
}

impl HttpMetricRecord {
    pub fn new(method: impl Into<String>, path: impl Into<String>, status_code: u16) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            status_code,
        }
    }
}

pub(crate) fn resolve_http_metric_path(matched_path: Option<&str>, request_path: &str) -> String {
    matched_path.unwrap_or(request_path).to_owned()
}

pub(crate) fn authenticate_admin_request(
    identity: &IdentityCapability,
    headers: &TransportHeaders,
    context: RequestContextState,
) -> Result<RequestContextState, HttpRejection> {
    let token = extract_required_bearer_token(headers).map_err(HttpRejection::Error)?;

    let identity = match identity {
        IdentityCapability::Unsupported { .. } => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate token",
            )
            .into())
        }
        IdentityCapability::Available { identity } => identity,
    };

    let user = match identity.authenticate_admin_jwt(token.as_str()) {
        Ok(user) => user,
        Err(AdminAuthError::InvalidToken) => {
            return Err(ErrorResponseSpec::new(401, "Unauthorized", "Invalid token").into())
        }
        Err(AdminAuthError::Internal) => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate token",
            )
            .into())
        }
    };

    Ok(context.with_auth(RequestAuthContext::Admin(user)))
}

pub(crate) fn authenticate_api_key_request(
    identity: &IdentityCapability,
    headers: &TransportHeaders,
    allow_no_auth: bool,
    context: RequestContextState,
) -> Result<RequestContextState, HttpRejection> {
    let identity = match identity {
        IdentityCapability::Unsupported { .. } => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate API key",
            )
            .into())
        }
        IdentityCapability::Available { identity } => identity,
    };
    let header_key = extract_api_key_from_headers(headers);

    let api_key = match identity.authenticate_api_key(header_key.as_deref(), allow_no_auth) {
        Ok(api_key) => api_key,
        Err(ApiKeyAuthError::Missing | ApiKeyAuthError::Invalid) => {
            return Err(ErrorResponseSpec::new(401, "Unauthorized", "Invalid API key").into())
        }
        Err(ApiKeyAuthError::Internal) => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate API key",
            )
            .into())
        }
    };

    Ok(context.with_auth(RequestAuthContext::ApiKey(api_key)))
}

pub(crate) fn authenticate_service_api_key_request(
    identity: &IdentityCapability,
    headers: &TransportHeaders,
    context: RequestContextState,
) -> Result<RequestContextState, HttpRejection> {
    let identity = match identity {
        IdentityCapability::Unsupported { .. } => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate API key",
            )
            .into())
        }
        IdentityCapability::Available { identity } => identity,
    };
    let token = extract_required_bearer_token(headers).map_err(HttpRejection::Error)?;

    let api_key = match identity.authenticate_api_key(Some(token.as_str()), false) {
        Ok(api_key) if api_key.key_type == ApiKeyType::ServiceAccount => api_key,
        Ok(_) | Err(ApiKeyAuthError::Missing | ApiKeyAuthError::Invalid) => {
            return Err(ErrorResponseSpec::new(401, "Unauthorized", "Invalid API key").into())
        }
        Err(ApiKeyAuthError::Internal) => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate API key",
            )
            .into())
        }
    };

    Ok(context.with_auth(RequestAuthContext::ApiKey(api_key)))
}

pub(crate) fn authenticate_gemini_request(
    identity: &IdentityCapability,
    headers: &TransportHeaders,
    query_key: Option<&str>,
    context: RequestContextState,
) -> Result<RequestContextState, HttpRejection> {
    let identity = match identity {
        IdentityCapability::Unsupported { .. } => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate API key",
            )
            .into())
        }
        IdentityCapability::Available { identity } => identity,
    };
    let header_key = extract_api_key_from_headers(headers);

    let api_key = match identity.authenticate_gemini_key(query_key, header_key.as_deref()) {
        Ok(api_key) => api_key,
        Err(ApiKeyAuthError::Missing | ApiKeyAuthError::Invalid) => {
            return Err(ErrorResponseSpec::new(401, "Unauthorized", "Invalid API key").into())
        }
        Err(ApiKeyAuthError::Internal) => {
            return Err(ErrorResponseSpec::new(
                500,
                "Internal Server Error",
                "Failed to validate API key",
            )
            .into())
        }
    };

    Ok(context.with_auth(RequestAuthContext::ApiKey(api_key)))
}

pub(crate) fn enrich_request_context(
    capability: &RequestContextCapability,
    headers: &TransportHeaders,
    trace_config: &TraceConfig,
    mut context: RequestContextState,
) -> Result<RequestContextState, HttpRejection> {
    let request_header = trace_request_header_name(trace_config);
    context.request_id = request_header_value(headers, &request_header).map(ToOwned::to_owned);

    let auth_project = context.auth.as_ref().and_then(RequestAuthContext::project);
    let header_project = match parse_project_header(headers) {
        Ok(Some(id)) => match capability {
            RequestContextCapability::Available { request_context } => {
                match request_context.resolve_project(id) {
                    Ok(project) => project,
                    Err(ContextResolveError::Internal) => {
                        return Err(ErrorResponseSpec::new(
                            500,
                            "Internal Server Error",
                            "Failed to resolve project context",
                        )
                        .into())
                    }
                }
            }
            RequestContextCapability::Unsupported { .. } => {
                // When request-context capability is unsupported, preserve header-derived project ID
                // without DB-backed resolution (minimal context for admin routes)
                Some(ProjectContext {
                    id,
                    name: String::new(),
                    status: String::new(),
                })
            }
        },
        Ok(None) => None,
        Err(error) => return Err(error.into()),
    };

    context.project = header_project.or(auth_project);

    // Resolve thread and trace only when request-context capability is available (requires DB)
    if let RequestContextCapability::Available { request_context } = capability {
        if let Some(project) = context.project.as_ref() {
            let thread_header = trace_thread_header_name(trace_config);
            if let Some(thread_id) = request_header_value(headers, &thread_header) {
                match request_context.resolve_thread(project.id, thread_id) {
                    Ok(thread) => {
                        context.thread = thread;
                    }
                    Err(ContextResolveError::Internal) => {}
                }
            }

            if let Some(trace_id) = extract_trace_id(headers, trace_config) {
                let thread_db_id = context.thread.as_ref().map(|thread| thread.id);
                match request_context.resolve_trace(project.id, trace_id.as_str(), thread_db_id) {
                    Ok(trace) => {
                        context.trace = trace;
                    }
                    Err(ContextResolveError::Internal) => {}
                }
            }
        }
    }

    Ok(context)
}

pub(crate) fn trace_thread_header_name(config: &TraceConfig) -> String {
    config
        .thread_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Thread-Id".to_owned())
}

pub(crate) fn trace_request_header_name(config: &TraceConfig) -> String {
    config
        .request_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Request-Id".to_owned())
}

pub(crate) fn trace_header_name(config: &TraceConfig) -> String {
    config
        .trace_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Trace-Id".to_owned())
}

pub(crate) fn request_header_value<'a>(
    headers: &'a TransportHeaders,
    header_name: &str,
) -> Option<&'a str> {
    headers.get(header_name)
}

pub(crate) fn extract_trace_id(headers: &TransportHeaders, config: &TraceConfig) -> Option<String> {
    request_header_value(headers, &trace_header_name(config))
        .map(ToOwned::to_owned)
        .or_else(|| {
            config
                .extra_trace_headers
                .iter()
                .find_map(|header| request_header_value(headers, header).map(ToOwned::to_owned))
        })
}

#[allow(dead_code)]
pub(crate) fn trace_body_inspection_required(
    config: &TraceConfig,
    request_method: &str,
    request_path: &str,
) -> bool {
    !config.extra_trace_body_fields.is_empty()
        || (config.claude_code_trace_enabled
            && is_claude_code_trace_request(request_method, request_path))
}

pub(crate) fn extract_request_trace_id(
    headers: &TransportHeaders,
    config: &TraceConfig,
    request_method: &str,
    request_path: &str,
    request_body: Option<&[u8]>,
) -> Option<String> {
    extract_trace_id(headers, config).or_else(|| {
        let parsed_body = parse_request_json_body(request_body);

        if config.claude_code_trace_enabled
            && is_claude_code_trace_request(request_method, request_path)
        {
            if let Some(trace_id) = parsed_body
                .as_ref()
                .and_then(|body| json_path_value(body, "metadata.user_id"))
                .and_then(|user_id| parse_claude_code_trace_id(user_id.as_str()))
            {
                return Some(trace_id);
            }
        }

        if config.codex_trace_enabled {
            if let Some(trace_id) = request_header_value(headers, "Session_id") {
                return Some(trace_id.to_owned());
            }
        }

        let body = parsed_body.as_ref()?;
        config
            .extra_trace_body_fields
            .iter()
            .find_map(|field| json_path_value(body, field))
    })
}

fn parse_request_json_body(request_body: Option<&[u8]>) -> Option<Value> {
    let body = request_body?;
    if body.is_empty() {
        return None;
    }

    serde_json::from_slice(body).ok()
}

fn is_claude_code_trace_request(request_method: &str, request_path: &str) -> bool {
    request_method.eq_ignore_ascii_case("POST")
        && (request_path.ends_with("/anthropic/v1/messages")
            || request_path.ends_with("/v1/messages"))
}

fn json_path_value(value: &Value, path: &str) -> Option<String> {
    let mut current = value;
    for segment in path.split('.').filter(|segment| !segment.is_empty()) {
        current = current.get(segment)?;
    }

    match current {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn parse_claude_code_trace_id(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if raw.starts_with('{') {
        let parsed: ClaudeCodeUserId = serde_json::from_str(raw).ok()?;
        let session_id = parsed.session_id.trim();
        return (!session_id.is_empty()).then(|| session_id.to_owned());
    }

    let (device_id, session_id) = raw.split_once("_account__session_")?;
    let device_id = device_id.strip_prefix("user_")?;
    if !is_hex_device_id(device_id) || !is_lower_hex_uuid(session_id) {
        return None;
    }

    Some(session_id.to_owned())
}

fn is_hex_device_id(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn is_lower_hex_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }

    for (index, character) in value.chars().enumerate() {
        let is_hyphen = matches!(index, 8 | 13 | 18 | 23);
        if is_hyphen {
            if character != '-' {
                return false;
            }
            continue;
        }

        if !matches!(character, '0'..='9' | 'a'..='f') {
            return false;
        }
    }

    true
}

#[derive(Debug, Deserialize)]
struct ClaudeCodeUserId {
    session_id: String,
}

pub(crate) fn parse_project_guid(raw: &str) -> Option<i64> {
    let value = raw.trim();
    let prefix = "gid://axonhub/project/";
    if !value.starts_with(prefix) {
        return None;
    }

    value[prefix.len()..].parse::<i64>().ok()
}

pub(crate) fn extract_required_bearer_token(
    headers: &TransportHeaders,
) -> Result<String, ErrorResponseSpec> {
    let value = request_header_value(headers, "Authorization").ok_or_else(|| {
        ErrorResponseSpec::new(401, "Unauthorized", "Authorization header is required")
    })?;

    value
        .strip_prefix("Bearer ")
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ErrorResponseSpec::new(
                401,
                "Unauthorized",
                "Invalid token: Authorization header must start with 'Bearer '",
            )
        })
}

pub(crate) fn extract_api_key_from_headers(headers: &TransportHeaders) -> Option<String> {
    const HEADER_NAMES: [&str; 7] = [
        "Authorization",
        "X-API-Key",
        "X-Api-Key",
        "API-Key",
        "Api-Key",
        "X-Goog-Api-Key",
        "X-Google-Api-Key",
    ];

    const PREFIXES: [&str; 4] = ["Bearer ", "Token ", "Api-Key ", "API-Key "];

    for header in HEADER_NAMES {
        let Some(value) = request_header_value(headers, header) else {
            continue;
        };

        let key = PREFIXES
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
            .unwrap_or(value)
            .trim();
        if !key.is_empty() {
            return Some(key.to_owned());
        }
    }

    None
}

pub(crate) fn translate_openai_error(error: OpenAiV1Error) -> JsonValueResponse {
    match error {
        OpenAiV1Error::InvalidRequest { message } => {
            ErrorResponseSpec::new(400, "Bad Request", message).into_json()
        }
        OpenAiV1Error::Internal { message } => {
            ErrorResponseSpec::new(500, "Internal Server Error", message).into_json()
        }
        OpenAiV1Error::Upstream { status, body } => JsonValueResponse {
            status: normalize_status(status, 502),
            body,
        },
    }
}

pub(crate) fn translate_provider_edge_admin_error(
    error: ProviderEdgeAdminError,
) -> JsonValueResponse {
    match error {
        ProviderEdgeAdminError::InvalidRequest { message } => {
            ErrorResponseSpec::new(400, "Bad Request", message).into_json()
        }
        ProviderEdgeAdminError::BadGateway { message } => {
            ErrorResponseSpec::new(502, "Bad Gateway", message).into_json()
        }
        ProviderEdgeAdminError::Internal { message } => {
            ErrorResponseSpec::new(500, "Internal Server Error", message).into_json()
        }
    }
}

pub(crate) fn translate_compatibility_error(
    route: CompatibilityRoute,
    error: OpenAiV1Error,
) -> JsonValueResponse {
    match route {
        CompatibilityRoute::AnthropicMessages => anthropic_error_payload(error),
        CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => {
            jina_error_payload(error)
        }
        CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent => gemini_error_payload(error),
        CompatibilityRoute::DoubaoCreateTask
        | CompatibilityRoute::DoubaoGetTask
        | CompatibilityRoute::DoubaoDeleteTask => translate_openai_error(error),
    }
}

fn parse_project_header(headers: &TransportHeaders) -> Result<Option<i64>, ErrorResponseSpec> {
    let Some(raw) = request_header_value(headers, "X-Project-ID") else {
        return Ok(None);
    };

    parse_project_guid(raw)
        .map(Some)
        .ok_or_else(|| ErrorResponseSpec::new(400, "Bad Request", "Invalid project ID"))
}

fn anthropic_error_payload(error: OpenAiV1Error) -> JsonValueResponse {
    let (status, error_type, message) = match error {
        OpenAiV1Error::InvalidRequest { message } => (400, "invalid_request_error", message),
        OpenAiV1Error::Internal { message } => (500, "internal_server_error", message),
        OpenAiV1Error::Upstream { status, body } => {
            let status = normalize_status(status, 502);
            let error_type = match status {
                400 => "invalid_request_error",
                401 => "authentication_error",
                403 => "permission_error",
                404 => "not_found_error",
                429 => "rate_limit_error",
                500 | 502 | 503 | 504 => "api_error",
                _ => "api_error",
            };
            (status, error_type, extract_error_message(&body))
        }
    };

    JsonValueResponse {
        status,
        body: json!({
            "type": error_type,
            "request_id": "",
            "error": {
                "type": error_type,
                "message": message,
            }
        }),
    }
}

fn jina_error_payload(error: OpenAiV1Error) -> JsonValueResponse {
    let (status, message) = match error {
        OpenAiV1Error::InvalidRequest { message } => (400, message),
        OpenAiV1Error::Internal { message } => (500, message),
        OpenAiV1Error::Upstream { status, body } => {
            (normalize_status(status, 502), extract_error_message(&body))
        }
    };

    JsonValueResponse {
        status,
        body: json!({
            "error": {
                "message": message,
                "type": "api_error",
            }
        }),
    }
}

fn gemini_error_payload(error: OpenAiV1Error) -> JsonValueResponse {
    let (status, message) = match error {
        OpenAiV1Error::InvalidRequest { message } => (400, message),
        OpenAiV1Error::Internal { message } => (500, message),
        OpenAiV1Error::Upstream { status, body } => {
            (normalize_status(status, 502), extract_error_message(&body))
        }
    };

    JsonValueResponse {
        status,
        body: json!({
            "error": {
                "code": status,
                "message": message,
                "status": match status {
                    400 => "INVALID_ARGUMENT",
                    401 => "UNAUTHENTICATED",
                    403 => "PERMISSION_DENIED",
                    404 => "NOT_FOUND",
                    429 => "RESOURCE_EXHAUSTED",
                    503 => "UNAVAILABLE",
                    501 => "UNIMPLEMENTED",
                    _ => "INTERNAL",
                }
            }
        }),
    }
}

fn extract_error_message(body: &Value) -> String {
    body.get("error")
        .and_then(|error| {
            error
                .get("message")
                .or_else(|| error.get("error").and_then(|nested| nested.get("message")))
        })
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

fn normalize_status(status: u16, fallback: u16) -> u16 {
    if (100..1000).contains(&status) {
        status
    } else {
        fallback
    }
}
