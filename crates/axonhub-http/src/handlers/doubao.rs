use crate::handlers::execute_compatibility_request;
use crate::models::CompatibilityRoute;
use crate::state::HttpState;
use axum::extract::{OriginalUri, Path, Request, State};
use axum::response::Response;
use std::collections::HashMap;

pub(crate) async fn doubao_create_task(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::DoubaoCreateTask,
        HashMap::new(),
    )
    .await
}

pub(crate) async fn doubao_get_task(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), id);
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::DoubaoGetTask,
        path_params,
    )
    .await
}

pub(crate) async fn doubao_delete_task(
    State(state): State<HttpState>,
    Path(id): Path<String>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), id);
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::DoubaoDeleteTask,
        path_params,
    )
    .await
}
