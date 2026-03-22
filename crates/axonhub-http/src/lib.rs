use axum::extract::{OriginalUri, Path, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{any, get};
use axum::{Json, Router};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

pub trait SystemReadPort: Send + Sync {
    fn is_initialized(&self) -> Result<bool, SystemReadError>;
}

#[derive(Clone)]
pub enum SystemReadCapability {
    Unsupported { message: String },
    Available { reader: Arc<dyn SystemReadPort> },
}

#[derive(Debug)]
pub enum SystemReadError {
    QueryFailed(String),
}

#[derive(Clone)]
pub struct HttpState {
    pub service_name: String,
    pub version: String,
    pub config_source: Option<String>,
    pub system_read: SystemReadCapability,
}

pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest(
            "/admin",
            Router::new()
                .route("/system/status", get(system_status))
                .route("/", any(unported_admin))
                .fallback(unported_admin),
        )
        .nest(
            "/v1",
            Router::new()
                .route("/", any(unported_v1))
                .fallback(unported_v1),
        )
        .nest(
            "/jina/v1",
            Router::new()
                .route("/", any(unported_jina_v1))
                .fallback(unported_jina_v1),
        )
        .nest(
            "/anthropic/v1",
            Router::new()
                .route("/", any(unported_anthropic_v1))
                .fallback(unported_anthropic_v1),
        )
        .nest(
            "/doubao/v3",
            Router::new()
                .route("/", any(unported_doubao_v3))
                .fallback(unported_doubao_v3),
        )
        .nest(
            "/gemini/{gemini_api_version}",
            Router::new()
                .route("/", any(unported_gemini))
                .fallback(unported_gemini),
        )
        .nest(
            "/v1beta",
            Router::new()
                .route("/", any(unported_v1beta))
                .fallback(unported_v1beta),
        )
        .nest(
            "/openapi",
            Router::new()
                .route("/", any(unported_openapi))
                .fallback(unported_openapi),
        )
        .with_state(state)
}

async fn health(State(state): State<HttpState>) -> Json<HealthResponse> {
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

async fn system_status(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> impl IntoResponse {
    match &state.system_read {
        SystemReadCapability::Unsupported { message } => {
            not_implemented_response("/admin/system/status", Method::GET, original_uri, None)
                .with_message(message)
        }
        SystemReadCapability::Available { reader } => match reader.is_initialized() {
            Ok(is_initialized) => (
                StatusCode::OK,
                Json(SystemStatusResponse { is_initialized }),
            )
                .into_response(),
            Err(SystemReadError::QueryFailed(message)) => internal_error_response(message),
        },
    }
}

async fn unported_admin(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/admin/*", method, uri, None)
}

async fn unported_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1/*", method, uri, None)
}

async fn unported_jina_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/jina/v1/*", method, uri, None)
}

async fn unported_anthropic_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/anthropic/v1/*", method, uri, None)
}

async fn unported_doubao_v3(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/doubao/v3/*", method, uri, None)
}

async fn unported_gemini(
    Path(params): Path<HashMap<String, String>>,
    method: Method,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    not_implemented_response(
        "/gemini/:gemini_api_version/*",
        method,
        uri,
        params.get("gemini_api_version").cloned(),
    )
}

async fn unported_v1beta(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1beta/*", method, uri, None)
}

async fn unported_openapi(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/openapi/*", method, uri, None)
}

fn not_implemented_response(
    route_family: &'static str,
    method: Method,
    uri: Uri,
    gemini_api_version: Option<String>,
) -> NotImplementedJsonResponse {
    NotImplementedJsonResponse {
        status: StatusCode::NOT_IMPLEMENTED,
        body: NotImplementedResponse {
            error: "not_implemented",
            status: StatusCode::NOT_IMPLEMENTED.as_u16(),
            route_family,
            method: method.to_string(),
            path: uri.path().to_owned(),
            message: "This surface has not been migrated to the Rust backend yet. Use the legacy Go backend for full AxonHub API coverage.".to_owned(),
            migration_status: "first migration slice",
            legacy_go_backend_present: true,
            gemini_api_version,
        },
    }
}

fn internal_error_response(message: String) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: ErrorDetail {
                r#type: "Internal Server Error",
                message,
            },
        }),
    )
        .into_response()
}

#[derive(Debug)]
struct NotImplementedJsonResponse {
    status: StatusCode,
    body: NotImplementedResponse,
}

impl NotImplementedJsonResponse {
    fn with_message(mut self, message: &str) -> axum::response::Response {
        self.body.message = message.to_owned();
        self.into_response()
    }
}

impl IntoResponse for NotImplementedJsonResponse {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: String,
    version: String,
    backend: &'static str,
    migration_status: &'static str,
    api_parity: &'static str,
    legacy_go_backend_present: bool,
    config_source: Option<String>,
}

#[derive(Debug, Serialize)]
struct SystemStatusResponse {
    #[serde(rename = "isInitialized")]
    is_initialized: bool,
}

#[derive(Debug, Serialize)]
struct NotImplementedResponse {
    error: &'static str,
    status: u16,
    route_family: &'static str,
    method: String,
    path: String,
    message: String,
    migration_status: &'static str,
    legacy_go_backend_present: bool,
    gemini_api_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    #[serde(rename = "type")]
    r#type: &'static str,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use tower::util::ServiceExt;

    struct StaticSystemReadPort {
        result: Result<bool, SystemReadError>,
    }

    impl SystemReadPort for StaticSystemReadPort {
        fn is_initialized(&self) -> Result<bool, SystemReadError> {
            match &self.result {
                Ok(value) => Ok(*value),
                Err(SystemReadError::QueryFailed(message)) => {
                    Err(SystemReadError::QueryFailed(message.clone()))
                }
            }
        }
    }

    fn test_state(system_read: SystemReadCapability) -> HttpState {
        HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_read,
        }
    }

    #[tokio::test]
    async fn system_status_route_returns_reader_value() {
        let app = router(test_state(SystemReadCapability::Available {
            reader: Arc::new(StaticSystemReadPort { result: Ok(true) }),
        }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["isInitialized"], true);
    }

    #[tokio::test]
    async fn system_status_route_returns_501_when_unsupported() {
        let app = router(test_state(SystemReadCapability::Unsupported {
            message: "DB-backed admin system status is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
        }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["route_family"], "/admin/system/status");
    }

    #[tokio::test]
    async fn system_status_route_returns_internal_error_on_query_failure() {
        let app = router(test_state(SystemReadCapability::Available {
            reader: Arc::new(StaticSystemReadPort {
                result: Err(SystemReadError::QueryFailed(
                    "Failed to check system status".to_owned(),
                )),
            }),
        }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["type"], "Internal Server Error");
        assert_eq!(json["error"]["message"], "Failed to check system status");
    }

    #[tokio::test]
    async fn unported_admin_routes_still_return_catch_all_501() {
        let app = router(test_state(SystemReadCapability::Unsupported {
            message: "unsupported".to_owned(),
        }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/system/initialize")
                    .method(Method::POST.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["route_family"], "/admin/*");
    }
}
