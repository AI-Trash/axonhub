use crate::handlers::execute_compatibility_request;
use crate::models::CompatibilityRoute;
use crate::state::HttpState;
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use std::collections::HashMap;

pub async fn doubao_create_task(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        CompatibilityRoute::DoubaoCreateTask,
        HashMap::new(),
    )
    .await
}

pub(crate) async fn doubao_get_task(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), path.into_inner());
    execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        Bytes::new(),
        request.uri().clone(),
        CompatibilityRoute::DoubaoGetTask,
        path_params,
    )
    .await
}

pub(crate) async fn doubao_delete_task(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), path.into_inner());
    execute_compatibility_request(
        state.get_ref().clone(),
        request.clone(),
        Bytes::new(),
        request.uri().clone(),
        CompatibilityRoute::DoubaoDeleteTask,
        path_params,
    )
    .await
}
