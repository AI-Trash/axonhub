use crate::errors::not_implemented_response;
use axum::extract::{OriginalUri, Path};
use axum::http::Method;
use axum::response::IntoResponse;
use std::collections::HashMap;

pub(crate) async fn unported_admin(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/admin/*", method, uri, None)
}

pub(crate) async fn unported_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1/*", method, uri, None)
}

pub(crate) async fn unported_jina_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/jina/v1/*", method, uri, None)
}

pub(crate) async fn unported_anthropic_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/anthropic/v1/*", method, uri, None)
}

pub(crate) async fn unported_doubao_v3(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/doubao/v3/*", method, uri, None)
}

pub(crate) async fn unported_gemini(
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

pub(crate) async fn unported_v1beta(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1beta/*", method, uri, None)
}

pub(crate) async fn unported_openapi(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/openapi/*", method, uri, None)
}
