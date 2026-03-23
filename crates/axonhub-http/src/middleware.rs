use crate::errors::{auth_unsupported_response, error_response};
use crate::models::ApiKeyType;
use crate::state::{
    GeminiQueryKey, HttpState, IdentityCapability, RequestAuthContext, RequestContextCapability,
    RequestContextState,
};
use axum::extract::{Query, Request, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

pub(crate) async fn require_admin_jwt(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = match extract_required_bearer_token(request.headers()) {
        Ok(token) => token,
        Err(response) => return response,
    };

    let identity = match &state.identity {
        IdentityCapability::Unsupported { message } => return auth_unsupported_response(message),
        IdentityCapability::Available { identity } => identity,
    };

    let user = match identity.authenticate_admin_jwt(token) {
        Ok(user) => user,
        Err(crate::ports::AdminAuthError::InvalidToken) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid token")
        }
        Err(crate::ports::AdminAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate token",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::Admin(user)),
        ..context
    });

    next.run(request).await
}

pub(crate) async fn require_api_key_or_no_auth(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match &state.identity {
        IdentityCapability::Unsupported { message } => return auth_unsupported_response(message),
        IdentityCapability::Available { identity } => identity,
    };

    let header_key = extract_api_key_from_headers(request.headers());
    let api_key = match identity.authenticate_api_key(header_key.as_deref(), state.allow_no_auth) {
        Ok(api_key) => api_key,
        Err(crate::ports::ApiKeyAuthError::Missing | crate::ports::ApiKeyAuthError::Invalid) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key")
        }
        Err(crate::ports::ApiKeyAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate API key",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::ApiKey(api_key)),
        ..context
    });

    next.run(request).await
}

pub(crate) async fn require_service_api_key(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match &state.identity {
        IdentityCapability::Unsupported { message } => return auth_unsupported_response(message),
        IdentityCapability::Available { identity } => identity,
    };

    let token = match extract_required_bearer_token(request.headers()) {
        Ok(token) => token,
        Err(response) => return response,
    };

    let api_key = match identity.authenticate_api_key(Some(token), false) {
        Ok(api_key) if api_key.key_type == ApiKeyType::ServiceAccount => api_key,
        Ok(_) | Err(crate::ports::ApiKeyAuthError::Missing | crate::ports::ApiKeyAuthError::Invalid) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key")
        }
        Err(crate::ports::ApiKeyAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate API key",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::ApiKey(api_key)),
        ..context
    });

    next.run(request).await
}

pub(crate) async fn require_gemini_key(
    State(state): State<HttpState>,
    Query(query): Query<GeminiQueryKey>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match &state.identity {
        IdentityCapability::Unsupported { message } => return auth_unsupported_response(message),
        IdentityCapability::Available { identity } => identity,
    };

    let header_key = extract_api_key_from_headers(request.headers());
    let api_key = match identity.authenticate_gemini_key(query.key.as_deref(), header_key.as_deref()) {
        Ok(api_key) => api_key,
        Err(crate::ports::ApiKeyAuthError::Missing | crate::ports::ApiKeyAuthError::Invalid) => {
            return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "invalid api key")
        }
        Err(crate::ports::ApiKeyAuthError::Internal) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to validate API key",
            )
        }
    };

    let context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();
    request.extensions_mut().insert(RequestContextState {
        auth: Some(RequestAuthContext::ApiKey(api_key)),
        ..context
    });

    next.run(request).await
}

pub(crate) async fn apply_request_context(
    State(state): State<HttpState>,
    mut request: Request,
    next: Next,
) -> Response {
    let request_context_port = match &state.request_context {
        RequestContextCapability::Unsupported { message } => {
            request.extensions_mut().insert(RequestContextState::default());
            let _ = message;
            return next.run(request).await;
        }
        RequestContextCapability::Available { request_context } => request_context,
    };

    let mut context = request.extensions_mut().remove::<RequestContextState>().unwrap_or_default();

    let request_header = trace_request_header_name(&state.trace_config);
    context.request_id = request
        .headers()
        .get(&request_header)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let auth_project = context.auth.as_ref().and_then(|auth| match auth {
        RequestAuthContext::ApiKey(key) => Some(key.project.clone()),
        RequestAuthContext::Admin(_) => None,
    });

    let header_project = match parse_project_header(request.headers()) {
        Ok(Some(id)) => match request_context_port.resolve_project(id) {
            Ok(project) => project,
            Err(crate::ports::ContextResolveError::Internal) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    "Failed to resolve project context",
                )
            }
        },
        Ok(None) => None,
        Err(response) => return response,
    };

    context.project = header_project.or(auth_project);

    if let Some(project) = context.project.as_ref() {
        if let Some(thread_id) = request_header_value(request.headers(), &trace_thread_header_name(&state.trace_config)) {
            match request_context_port.resolve_thread(project.id, thread_id) {
                Ok(thread) => {
                    context.thread = thread;
                }
                Err(crate::ports::ContextResolveError::Internal) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal Server Error",
                        "Failed to resolve thread context",
                    )
                }
            }
        }

        if let Some(trace_id) = extract_trace_id(request.headers(), &state.trace_config) {
            let thread_db_id = context.thread.as_ref().map(|thread| thread.id);
            match request_context_port.resolve_trace(project.id, trace_id, thread_db_id) {
                Ok(trace) => {
                    context.trace = trace;
                }
                Err(crate::ports::ContextResolveError::Internal) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal Server Error",
                        "Failed to resolve trace context",
                    )
                }
            }
        }
    }

    request.extensions_mut().insert(context);
    next.run(request).await
}

pub(crate) fn trace_thread_header_name(config: &crate::models::TraceConfig) -> String {
    config
        .thread_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Thread-Id".to_owned())
}

pub(crate) fn trace_request_header_name(config: &crate::models::TraceConfig) -> String {
    config
        .request_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "X-Request-Id".to_owned())
}

pub(crate) fn trace_header_name(config: &crate::models::TraceConfig) -> String {
    config
        .trace_header
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "AH-Trace-Id".to_owned())
}

pub(crate) fn request_header_value<'a>(headers: &'a HeaderMap, header_name: &str) -> Option<&'a str> {
    headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn extract_trace_id<'a>(
    headers: &'a HeaderMap,
    config: &crate::models::TraceConfig,
) -> Option<&'a str> {
    request_header_value(headers, &trace_header_name(config)).or_else(|| {
        config
            .extra_trace_headers
            .iter()
            .find_map(|header| request_header_value(headers, header))
    })
}

fn parse_project_header(headers: &HeaderMap) -> Result<Option<i64>, Response> {
    let Some(raw) = request_header_value(headers, "X-Project-ID") else {
        return Ok(None);
    };

    parse_project_guid(raw)
        .map(Some)
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid project ID"))
}

fn parse_project_guid(raw: &str) -> Option<i64> {
    let value = raw.trim();
    let prefix = "gid://axonhub/project/";
    if !value.starts_with(prefix) {
        return None;
    }

    value[prefix.len()..].parse::<i64>().ok()
}

fn extract_required_bearer_token(headers: &HeaderMap) -> Result<&str, Response> {
    let value = request_header_value(headers, "Authorization").ok_or_else(|| {
        error_response(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "API key is required",
        )
    })?;

    value.strip_prefix("Bearer ").ok_or_else(|| {
        error_response(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "invalid token: Authorization header must start with 'Bearer '",
        )
    })
}

fn extract_api_key_from_headers(headers: &HeaderMap) -> Option<String> {
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
