use crate::errors::{
    already_initialized_response, auth_unsupported_response, error_response,
    internal_error_response, invalid_initialize_request_response, not_implemented_response,
};
use crate::handlers::{execute_openai_request_with_body, parse_json_body};
use crate::models::{
    InitializeSystemRequest, InitializeSystemResponse, SignInRequest, SignInResponse,
    SystemStatusResponse, OpenAiV1Route, ProjectContext,
};
use crate::ports::{AdminError, SignInError, SystemInitializeError, SystemQueryError};
use crate::state::{
    AdminCapability, HttpState, IdentityCapability, OpenAiV1Capability, RequestAuthContext,
    RequestContextState, SystemBootstrapCapability,
};
use axum::body;
use axum::extract::{rejection::JsonRejection, OriginalUri, Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;
use std::borrow::Cow;

pub(crate) async fn system_status(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> impl IntoResponse {
    match &state.system_bootstrap {
        SystemBootstrapCapability::Unsupported { message } => {
            not_implemented_response("/admin/system/status", Method::GET, original_uri, None)
                .with_message(message)
        }
        SystemBootstrapCapability::Available { system } => match system.is_initialized() {
            Ok(is_initialized) => (
                StatusCode::OK,
                Json(SystemStatusResponse { is_initialized }),
            )
                .into_response(),
            Err(SystemQueryError::QueryFailed) => {
                internal_error_response("Failed to check system status".to_owned())
            }
        },
    }
}

pub(crate) async fn initialize_system(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    payload: Result<Json<InitializeSystemRequest>, JsonRejection>,
) -> impl IntoResponse {
    match &state.system_bootstrap {
        SystemBootstrapCapability::Unsupported { message } => {
            not_implemented_response("/admin/system/initialize", Method::POST, original_uri, None)
                .with_message(message)
        }
        SystemBootstrapCapability::Available { system } => {
            let Json(request) = match payload {
                Ok(payload) => payload,
                Err(_) => return invalid_initialize_request_response(),
            };

            if !request.is_valid() {
                return invalid_initialize_request_response();
            }

            match system.is_initialized() {
                Ok(true) => return already_initialized_response(),
                Ok(false) => {}
                Err(SystemQueryError::QueryFailed) => {
                    return internal_error_response("Failed to check initialization status".to_owned())
                }
            }

            match system.initialize(&request) {
                Ok(()) => (
                    StatusCode::OK,
                    Json(InitializeSystemResponse {
                        success: true,
                        message: "System initialized successfully".to_owned(),
                    }),
                )
                    .into_response(),
                Err(SystemInitializeError::AlreadyInitialized) => already_initialized_response(),
                Err(SystemInitializeError::InitializeFailed(message)) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(InitializeSystemResponse {
                        success: false,
                        message: format!("Failed to initialize system: {message}"),
                    }),
                )
                    .into_response(),
            }
        }
    }
}

pub(crate) async fn sign_in(
    State(state): State<HttpState>,
    payload: Result<Json<SignInRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Invalid request format",
            )
        }
    };

    let identity = match &state.identity {
        IdentityCapability::Unsupported { message } => return auth_unsupported_response(message),
        IdentityCapability::Available { identity } => identity,
    };

    match identity.admin_signin(&request) {
        Ok(result) => (
            StatusCode::OK,
            Json(SignInResponse {
                user: result.user,
                token: result.token,
            }),
        )
            .into_response(),
        Err(SignInError::InvalidCredentials) => error_response(
            StatusCode::UNAUTHORIZED,
            "Unauthorized",
            "Invalid email or password",
        ),
        Err(SignInError::Internal) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            "Internal server error",
        ),
    }
}

pub(crate) async fn download_request_content(
    State(state): State<HttpState>,
    Path(request_id): Path<i64>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let admin = match &state.admin {
        AdminCapability::Unsupported { message } => {
            return not_implemented_response(
                "/admin/requests/:request_id/content",
                Method::GET,
                original_uri,
                None,
            )
            .with_message(message)
        }
        AdminCapability::Available { admin } => admin,
    };

    let project_id = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.project.as_ref())
        .map(|project| project.id)
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Project ID not found in context",
            )
        });
    let project_id = match project_id {
        Ok(project_id) => project_id,
        Err(response) => return response,
    };

    let user = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::Admin(user) => Some(user.clone()),
            RequestAuthContext::ApiKey(_) => None,
        });
    let user = match user {
        Some(user) => user,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Unauthorized",
                "Invalid token",
            )
        }
    };

    match admin.download_request_content(project_id, request_id, user) {
        Ok(content) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/octet-stream")
            .header(
                "Content-Disposition",
                format!("attachment; filename={:?}", content.filename),
            )
            .header("Cache-Control", "private, max-age=0, no-cache")
            .header("Content-Length", content.bytes.len().to_string())
            .body(body::Body::from(content.bytes))
            .unwrap_or_else(|_| internal_error_response("Failed to build content response".to_owned())),
        Err(AdminError::BadRequest { message }) => {
            error_response(StatusCode::BAD_REQUEST, "Bad Request", &message)
        }
        Err(AdminError::NotFound { message }) => {
            error_response(StatusCode::NOT_FOUND, "Not Found", &message)
        }
        Err(AdminError::Internal { message }) => internal_error_response(message),
    }
}

pub(crate) async fn playground_chat(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    mut request: Request,
) -> Response {
    if let OpenAiV1Capability::Unsupported { message } = &state.openai_v1 {
        return not_implemented_response("/admin/playground/chat", Method::POST, original_uri, None)
            .with_message(message);
    }

    let body = match parse_json_body(&mut request).await {
        Ok(body) => body,
        Err(response) => return response,
    };

    if let Some(response) = validate_playground_chat_request(&body) {
        return response;
    }

    if let Some(response) = apply_playground_project_context(&state, &mut request) {
        return response;
    }

    let channel_hint_id = match resolve_playground_channel_override(&request) {
        Ok(channel_hint_id) => channel_hint_id,
        Err(response) => return response,
    };

    execute_openai_request_with_body(
        state,
        request,
        original_uri,
        OpenAiV1Route::ChatCompletions,
        body,
        channel_hint_id,
    )
    .await
}

fn validate_playground_chat_request(body: &Value) -> Option<Response> {
    let body_object = body.as_object()?;

    if body_object
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some(error_response(
            StatusCode::NOT_IMPLEMENTED,
            "Not Implemented",
            "Streaming is not supported for /admin/playground/chat in the Rust backend yet",
        ));
    }

    None
}

fn resolve_playground_channel_override(request: &Request) -> Result<Option<i64>, Response> {
    let query_channel_id = request_query_value(request, "channel_id")
        .map(|value| {
            decode_query_component(value).map_err(|()| {
                error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid channel ID")
            })
        })
        .transpose()?;

    let Some(channel_id) = query_channel_id
        .or_else(|| request_header_value(request, "X-Channel-ID").map(Cow::Borrowed))
    else {
        return Ok(None);
    };

    parse_channel_query_value(channel_id.as_ref())
        .map(Some)
        .map_err(|()| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid channel ID"))
}

fn apply_playground_project_context(state: &HttpState, request: &mut Request) -> Option<Response> {
    if request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.project.as_ref())
        .is_some()
    {
        return None;
    }

    let Some(project_id) = request_query_value(request, "project_id") else {
        return None;
    };

    let project_id = decode_query_component(project_id).map_err(|()| {
        error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid project ID")
    });
    let project_id = match project_id {
        Ok(project_id) => project_id,
        Err(response) => return Some(response),
    };

    let project_id = parse_project_query_value(project_id.as_ref()).map_err(|()| {
        error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid project ID")
    });
    let project_id = match project_id {
        Ok(project_id) => project_id,
        Err(response) => return Some(response),
    };

    let request_context = match &state.request_context {
        crate::state::RequestContextCapability::Unsupported { message } => {
            return Some(auth_unsupported_response(message))
        }
        crate::state::RequestContextCapability::Available { request_context } => request_context,
    };

    let project = match request_context.resolve_project(project_id) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return Some(error_response(
                StatusCode::BAD_REQUEST,
                "Bad Request",
                "Project ID not found in context",
            ))
        }
        Err(crate::ports::ContextResolveError::Internal) => {
            return Some(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to resolve project context",
            ))
        }
    };

    let mut context = request
        .extensions_mut()
        .remove::<RequestContextState>()
        .unwrap_or_default();
    context.project = Some(ProjectContext {
        id: project.id,
        name: project.name,
        status: project.status,
    });
    request.extensions_mut().insert(context);

    None
}

fn request_query_value<'a>(request: &'a Request, key: &str) -> Option<&'a str> {
    request.uri().query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let (current_key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (current_key == key)
                .then_some(value.trim())
                .filter(|value| !value.is_empty())
        })
    })
}

fn request_header_value<'a>(request: &'a Request, key: &str) -> Option<&'a str> {
    request
        .headers()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_project_query_value(raw: &str) -> Result<i64, ()> {
    parse_global_query_value(raw, "project")
}

fn parse_channel_query_value(raw: &str) -> Result<i64, ()> {
    parse_global_query_value(raw, "channel")
}

fn parse_global_query_value(raw: &str, resource_type: &str) -> Result<i64, ()> {
    let prefix = format!("gid://axonhub/{resource_type}/");
    raw.strip_prefix(&prefix)
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or(())
}

fn decode_query_component(raw: &str) -> Result<Cow<'_, str>, ()> {
    if !raw.as_bytes().contains(&b'%') {
        return Ok(Cow::Borrowed(raw));
    }

    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }

            let high = decode_hex_nibble(bytes[index + 1])?;
            let low = decode_hex_nibble(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(decoded).map(Cow::Owned).map_err(|_| ())
}

fn decode_hex_nibble(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}
