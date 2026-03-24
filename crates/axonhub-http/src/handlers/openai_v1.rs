use crate::errors::{not_implemented_response, openai_error_response};
use crate::handlers::{build_openai_execution_request, execute_openai_request};
use crate::models::CompatibilityRoute;
use crate::state::{HttpState, ModelsQuery, OpenAiV1Capability};
use axum::extract::{OriginalUri, Path, Query, Request, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::Response;
use axum::{Json, response::IntoResponse};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

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

pub(crate) async fn openai_videos_create(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    mut request: Request,
) -> Response {
    execute_openai_video_compatibility(
        state,
        &mut request,
        original_uri,
        CompatibilityRoute::DoubaoCreateTask,
        HashMap::new(),
        true,
    )
    .await
}

pub(crate) async fn openai_videos_get(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    mut request: Request,
) -> Response {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), id);
    execute_openai_video_compatibility(
        state,
        &mut request,
        original_uri,
        CompatibilityRoute::DoubaoGetTask,
        path_params,
        true,
    )
    .await
}

pub(crate) async fn openai_videos_delete(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    mut request: Request,
) -> Response {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), id);
    execute_openai_video_compatibility(
        state,
        &mut request,
        original_uri,
        CompatibilityRoute::DoubaoDeleteTask,
        path_params,
        false,
    )
    .await
}

async fn execute_openai_video_compatibility(
    state: HttpState,
    request: &mut Request,
    original_uri: Uri,
    route: CompatibilityRoute,
    path_params: HashMap<String, String>,
    returns_json_body: bool,
) -> Response {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::POST, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let body = match route {
        CompatibilityRoute::DoubaoCreateTask => match crate::handlers::parse_json_body(request).await {
            Ok(body) => body,
            Err(response) => return response,
        },
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => Value::Null,
        _ => Value::Null,
    };

    let execution_request = match build_openai_execution_request(
        std::mem::take(request),
        body,
        path_params,
        None,
    ) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result = tokio::task::spawn_blocking(move || openai.execute_compatibility(route, execution_request)).await;

    match execution_result {
        Ok(Ok(result)) => {
            if returns_json_body {
                let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
                (status, Json(result.body)).into_response()
            } else {
                StatusCode::NO_CONTENT.into_response()
            }
        }
        Ok(Err(error)) => openai_error_response(error),
        Err(_) => crate::errors::internal_error_response("OpenAI `/v1/videos*` execution task failed".to_owned()),
    }
}
