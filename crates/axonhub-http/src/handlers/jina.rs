use crate::handlers::execute_compatibility_request;
use crate::models::CompatibilityRoute;
use crate::state::HttpState;
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use std::collections::HashMap;

pub async fn jina_rerank(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        CompatibilityRoute::JinaRerank,
        HashMap::new(),
    )
    .await
}

pub async fn jina_embeddings(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        CompatibilityRoute::JinaEmbeddings,
        HashMap::new(),
    )
    .await
}
