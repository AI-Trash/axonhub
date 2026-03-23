use crate::errors::{
    already_initialized_response, auth_unsupported_response, error_response,
    internal_error_response, invalid_initialize_request_response, not_implemented_response,
};
use crate::models::{
    InitializeSystemRequest, InitializeSystemResponse, SignInRequest, SignInResponse,
    SystemStatusResponse,
};
use crate::ports::{AdminError, SignInError, SystemInitializeError, SystemQueryError};
use crate::state::{
    AdminCapability, HttpState, IdentityCapability, RequestAuthContext, RequestContextState,
    SystemBootstrapCapability,
};
use axum::body;
use axum::extract::{rejection::JsonRejection, OriginalUri, Path, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

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
