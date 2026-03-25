use crate::errors::{
    compatibility_error_response, compatibility_internal_error_response, not_implemented_response,
};
use crate::handlers::{execute_compatibility_request, gemini_version_from_path, parse_query_pairs};
use crate::models::CompatibilityRoute;
use crate::state::{HttpState, OpenAiV1Capability};
use actix_web::http::{Method, StatusCode};
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use std::collections::HashMap;

pub(crate) async fn list_gemini_models(
    state: web::Data<HttpState>,
    request: HttpRequest,
) -> HttpResponse {
    let path = request.uri().path().to_owned();
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response(
                if path.starts_with("/v1beta") {
                    "/v1beta/*"
                } else {
                    "/gemini/:gemini_api_version/*"
                },
                Method::GET,
                request.uri().clone(),
                gemini_version_from_path(path.as_str()),
            )
            .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_gemini_models() {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(error) => compatibility_error_response(CompatibilityRoute::GeminiGenerateContent, error),
    }
}

pub(crate) async fn gemini_generate_content(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let route = if request.uri().path().contains(":streamGenerateContent") {
        CompatibilityRoute::GeminiStreamGenerateContent
    } else if request.uri().path().contains(":generateContent") {
        CompatibilityRoute::GeminiGenerateContent
    } else {
        return not_implemented_response(
            if request.uri().path().starts_with("/v1beta") {
                "/v1beta/*"
            } else {
                "/gemini/:gemini_api_version/*"
            },
            Method::POST,
            request.uri().clone(),
            gemini_version_from_path(request.uri().path()),
        )
        .into_response();
    };

    let alt = request
        .uri()
        .query()
        .and_then(|query| parse_query_pairs(query).remove("alt"));

    let response = execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        route,
        HashMap::new(),
    )
    .await;
    if route != CompatibilityRoute::GeminiStreamGenerateContent || response.status() != StatusCode::OK {
        return response;
    }

    let body = actix_web::body::to_bytes(response.into_body()).await;
    let Ok(body) = body else {
        return compatibility_internal_error_response(route);
    };

    if alt.as_deref() == Some("sse") {
        let payload = String::from_utf8_lossy(&body);
        return HttpResponse::Ok()
            .insert_header(("content-type", "text/event-stream; charset=utf-8"))
            .body(format!("data: {payload}\n\ndata: [DONE]\n\n"));
    }

    HttpResponse::Ok()
        .insert_header(("content-type", "application/json"))
        .body(format!("[{0}]", String::from_utf8_lossy(&body)))
}
