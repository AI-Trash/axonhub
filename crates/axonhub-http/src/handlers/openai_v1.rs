use crate::errors::{not_implemented_response, openai_error_response};
use crate::handlers::execute_openai_request;
use crate::state::{HttpState, ModelsQuery, OpenAiV1Capability};
use axum::extract::{OriginalUri, Query, Request, State};
use axum::http::Method;
use axum::response::Response;
use axum::{Json, response::IntoResponse};

pub(crate) async fn list_openai_models(
    State(state): State<HttpState>,
    Query(query): Query<ModelsQuery>,
    OriginalUri(original_uri): OriginalUri,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::GET, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_models(query.include.as_deref()) {
        Ok(response) => (axum::http::StatusCode::OK, Json(response)).into_response(),
        Err(error) => openai_error_response(error),
    }
}

pub(crate) async fn openai_chat_completions(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_openai_request(state, request, original_uri, crate::models::OpenAiV1Route::ChatCompletions).await
}

pub(crate) async fn openai_responses(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_openai_request(state, request, original_uri, crate::models::OpenAiV1Route::Responses).await
}

pub(crate) async fn openai_embeddings(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_openai_request(state, request, original_uri, crate::models::OpenAiV1Route::Embeddings).await
}
