use crate::handlers::execute_compatibility_request;
use crate::models::CompatibilityRoute;
use crate::state::HttpState;
use axum::extract::{OriginalUri, Request, State};
use axum::response::Response;
use std::collections::HashMap;

pub(crate) async fn jina_rerank(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::JinaRerank,
        HashMap::new(),
    )
    .await
}

pub(crate) async fn jina_embeddings(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    execute_compatibility_request(
        state,
        request,
        original_uri,
        CompatibilityRoute::JinaEmbeddings,
        HashMap::new(),
    )
    .await
}
