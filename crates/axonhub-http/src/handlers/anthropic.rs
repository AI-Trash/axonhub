use crate::errors::{compatibility_error_response, not_implemented_response};
use crate::handlers::execute_compatibility_request;
use crate::models::CompatibilityRoute;
use crate::state::{HttpState, OpenAiV1Capability};
use axum::extract::{OriginalUri, Request, State};
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::collections::HashMap;

pub(crate) async fn list_anthropic_models(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/anthropic/v1/*", Method::GET, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_anthropic_models() {
        Ok(response) => (axum::http::StatusCode::OK, Json(response)).into_response(),
        Err(error) => compatibility_error_response(CompatibilityRoute::AnthropicMessages, error),
    }
}

pub(crate) async fn anthropic_messages(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::AnthropicMessages,
        HashMap::new(),
    )
    .await
}
