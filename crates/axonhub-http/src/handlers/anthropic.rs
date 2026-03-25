use crate::errors::{compatibility_error_response, not_implemented_response};
use crate::handlers::execute_compatibility_request;
use crate::models::CompatibilityRoute;
use crate::state::{HttpState, OpenAiV1Capability};
use actix_web::http::Method;
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use std::collections::HashMap;

pub(crate) async fn list_anthropic_models(
    state: web::Data<HttpState>,
    request: HttpRequest,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response(
                "/anthropic/v1/*",
                Method::GET,
                request.uri().clone(),
                None,
            )
            .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_anthropic_models() {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(error) => compatibility_error_response(CompatibilityRoute::AnthropicMessages, error),
    }
}

pub(crate) async fn anthropic_messages(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        CompatibilityRoute::AnthropicMessages,
        HashMap::new(),
    )
    .await
}
