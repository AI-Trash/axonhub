use crate::errors::{compatibility_error_response, compatibility_internal_error_response, not_implemented_response};
use crate::handlers::{execute_compatibility_request, gemini_version_from_path, parse_query_pairs};
use crate::models::CompatibilityRoute;
use crate::state::{HttpState, OpenAiV1Capability};
use axum::body;
use axum::extract::{OriginalUri, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::collections::HashMap;

pub(crate) async fn list_gemini_models(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let path = original_uri.path().to_owned();
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response(
                if path.starts_with("/v1beta") {
                    "/v1beta/*"
                } else {
                    "/gemini/:gemini_api_version/*"
                },
                Method::GET,
                original_uri,
                gemini_version_from_path(path.as_str()),
            )
            .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_gemini_models() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => compatibility_error_response(CompatibilityRoute::GeminiGenerateContent, error),
    }
}

pub(crate) async fn gemini_generate_content(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let route = if original_uri.path().contains(":streamGenerateContent") {
        CompatibilityRoute::GeminiStreamGenerateContent
    } else if original_uri.path().contains(":generateContent") {
        CompatibilityRoute::GeminiGenerateContent
    } else {
        return not_implemented_response(
            if original_uri.path().starts_with("/v1beta") {
                "/v1beta/*"
            } else {
                "/gemini/:gemini_api_version/*"
            },
            Method::POST,
            original_uri,
            gemini_version_from_path(request.uri().path()),
        )
        .into_response();
    };

    let alt = request
        .uri()
        .query()
        .and_then(|query| parse_query_pairs(query).remove("alt"));

    let response = execute_compatibility_request(
        state,
        request,
        original_uri,
        route,
        HashMap::new(),
    )
    .await;
    if route != CompatibilityRoute::GeminiStreamGenerateContent || response.status() != StatusCode::OK {
        return response;
    }

    let body = body::to_bytes(response.into_body(), usize::MAX).await;
    let Ok(body) = body else {
        return compatibility_internal_error_response(route);
    };

    if alt.as_deref() == Some("sse") {
        let payload = String::from_utf8_lossy(&body);
        return (
            StatusCode::OK,
            [("content-type", "text/event-stream; charset=utf-8")],
            format!("data: {payload}\n\ndata: [DONE]\n\n"),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [("content-type", "application/json")],
        format!("[{0}]", String::from_utf8_lossy(&body)),
    )
        .into_response()
}
