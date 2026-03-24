use crate::errors::not_implemented_response;
use axum::extract::OriginalUri;
use axum::http::Method;
use axum::response::IntoResponse;


pub(crate) async fn unported_v1(method: Method, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    not_implemented_response("/v1/*", method, uri, None)
}




