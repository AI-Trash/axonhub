use crate::errors::{not_implemented_response, openai_error_response};
use crate::handlers::{build_openai_execution_request, execute_openai_request};
use crate::models::CompatibilityRoute;
use crate::state::{HttpState, ModelsQuery, OpenAiV1Capability};
use actix_web::http::{Method, StatusCode, Uri};
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) async fn list_openai_models(
    state: web::Data<HttpState>,
    request: HttpRequest,
    query: web::Query<ModelsQuery>,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::GET, request.uri().clone(), None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    match openai.list_models(query.include.as_deref()) {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(error) => openai_error_response(error),
    }
}

pub(crate) async fn openai_chat_completions(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_openai_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        crate::models::OpenAiV1Route::ChatCompletions,
    )
    .await
}

pub(crate) async fn openai_responses(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_openai_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        crate::models::OpenAiV1Route::Responses,
    )
    .await
}

pub(crate) async fn openai_embeddings(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_openai_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        crate::models::OpenAiV1Route::Embeddings,
    )
    .await
}

pub(crate) async fn openai_videos_create(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_openai_video_compatibility(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        CompatibilityRoute::DoubaoCreateTask,
        HashMap::new(),
        true,
    )
    .await
}

pub(crate) async fn openai_videos_get(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), path.into_inner());
    execute_openai_video_compatibility(
        state.get_ref().clone(),
        request.clone(),
        Bytes::new(),
        request.uri().clone(),
        CompatibilityRoute::DoubaoGetTask,
        path_params,
        true,
    )
    .await
}

pub(crate) async fn openai_videos_delete(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let mut path_params = HashMap::new();
    path_params.insert("id".to_owned(), path.into_inner());
    execute_openai_video_compatibility(
        state.get_ref().clone(),
        request.clone(),
        Bytes::new(),
        request.uri().clone(),
        CompatibilityRoute::DoubaoDeleteTask,
        path_params,
        false,
    )
    .await
}

async fn execute_openai_video_compatibility(
    state: HttpState,
    request: HttpRequest,
    body_bytes: Bytes,
    original_uri: Uri,
    route: CompatibilityRoute,
    path_params: HashMap<String, String>,
    returns_json_body: bool,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::POST, original_uri, None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let body = match route {
        CompatibilityRoute::DoubaoCreateTask => match crate::handlers::parse_json_body(body_bytes) {
            Ok(body) => body,
            Err(response) => return response,
        },
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => Value::Null,
        _ => Value::Null,
    };

    let execution_request = match build_openai_execution_request(request, body, path_params, None) {
        Ok(payload) => payload,
        Err(response) => return response,
    };

    let openai = Arc::clone(openai);
    let execution_result =
        tokio::task::spawn_blocking(move || openai.execute_compatibility(route, execution_request))
            .await;

    match execution_result {
        Ok(Ok(result)) => {
            if returns_json_body {
                let status = StatusCode::from_u16(result.status).unwrap_or(StatusCode::OK);
                HttpResponse::build(status).json(result.body)
            } else {
                HttpResponse::NoContent().finish()
            }
        }
        Ok(Err(error)) => openai_error_response(error),
        Err(_) => {
            crate::errors::internal_error_response("OpenAI `/v1/videos*` execution task failed".to_owned())
        }
    }
}
