use crate::errors::{error_response, not_implemented_response, openai_error_response};
use crate::handlers::{
    build_openai_execution_request, execute_openai_request, execute_openai_request_with_body,
    parse_openai_multipart_body, realtime_upgrade_header_present,
};
use crate::models::{
    CompatibilityRoute, OpenAiMultipartField, OpenAiRequestBody, RealtimeSessionCreateRequest,
    RealtimeSessionPatchRequest, RealtimeSessionTransportRequest,
};
use crate::state::{HttpState, ModelsQuery, OpenAiV1Capability};
use actix_multipart::Multipart;
use actix_web::HttpMessage;
use actix_web::http::{header, Method, StatusCode, Uri};
use actix_web::{HttpRequest, HttpResponse, web};
use bytes::Bytes;
use futures_util::StreamExt;
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

    let api_key = request
        .extensions()
        .get::<crate::state::RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            crate::state::RequestAuthContext::ApiKey(key) => Some(key.clone()),
            crate::state::RequestAuthContext::Admin(_) => None,
        });
    let api_key = match api_key {
        Some(api_key) => api_key,
        None => return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key"),
    };

    match openai.list_models(query.include.as_deref(), &api_key) {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(error) => openai_error_response(error),
    }
}

pub(crate) async fn retrieve_openai_model(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
    query: web::Query<ModelsQuery>,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::GET, request.uri().clone(), None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let api_key = request
        .extensions()
        .get::<crate::state::RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            crate::state::RequestAuthContext::ApiKey(key) => Some(key.clone()),
            crate::state::RequestAuthContext::Admin(_) => None,
        });
    let api_key = match api_key {
        Some(api_key) => api_key,
        None => return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid API key"),
    };

    match openai.retrieve_model(path.as_str(), query.include.as_deref(), &api_key) {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(error) => openai_error_response(error),
    }
}

pub async fn openai_chat_completions(
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

pub async fn openai_responses(
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

pub async fn openai_responses_compact(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_openai_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        crate::models::OpenAiV1Route::ResponsesCompact,
    )
    .await
}

pub async fn openai_embeddings(
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

pub async fn openai_images_generations(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    execute_openai_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        crate::models::OpenAiV1Route::ImagesGenerations,
    )
    .await
}

pub async fn openai_images_edits(
    state: web::Data<HttpState>,
    request: HttpRequest,
    multipart: Multipart,
) -> HttpResponse {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    match parse_image_multipart_request(
        multipart,
        content_type,
        crate::models::OpenAiV1Route::ImagesEdits,
    )
    .await
    {
        Ok(body) => {
            execute_openai_request_with_body(
                state.get_ref().clone(),
                request.clone(),
                request.uri().clone(),
                crate::models::OpenAiV1Route::ImagesEdits,
                body,
                None,
            )
            .await
        }
        Err(response) => response,
    }
}

pub async fn openai_images_variations(
    state: web::Data<HttpState>,
    request: HttpRequest,
    multipart: Multipart,
) -> HttpResponse {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    match parse_image_multipart_request(
        multipart,
        content_type,
        crate::models::OpenAiV1Route::ImagesVariations,
    )
    .await
    {
        Ok(body) => {
            execute_openai_request_with_body(
                state.get_ref().clone(),
                request.clone(),
                request.uri().clone(),
                crate::models::OpenAiV1Route::ImagesVariations,
                body,
                None,
            )
            .await
        }
        Err(response) => response,
    }
}

pub async fn openai_realtime(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if realtime_upgrade_header_present(&request) {
        return create_realtime_upgrade_session(state, request).await;
    }

    execute_openai_request(
        state.get_ref().clone(),
        request.clone(),
        body,
        request.uri().clone(),
        crate::models::OpenAiV1Route::Realtime,
    )
    .await
}

pub async fn create_openai_realtime_session(
    state: web::Data<HttpState>,
    request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::POST, request.uri().clone(), None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };

    let payload: RealtimeSessionTransportRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"),
    };
    let context = request
        .extensions()
        .get::<crate::state::RequestContextState>()
        .cloned()
        .unwrap_or_default();
    let Some(project) = context.project else {
        return error_response(StatusCode::BAD_REQUEST, "Bad Request", "Project ID not found in context");
    };
    let api_key_id = context.auth.as_ref().and_then(|auth| match auth {
        crate::state::RequestAuthContext::ApiKey(key) => Some(key.id),
        crate::state::RequestAuthContext::Admin(_) => None,
    });
    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    match openai.create_realtime_session(RealtimeSessionCreateRequest {
        project,
        thread: context.thread,
        trace: context.trace,
        api_key_id,
        client_ip,
        request_id: context.request_id,
        transport: payload,
    }) {
        Ok(session) => HttpResponse::Ok().json(session),
        Err(error) => openai_error_response(error),
    }
}

pub async fn get_openai_realtime_session(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    match openai_session_lookup(state, &request, path.into_inner().as_str()) {
        Ok(Some(session)) => HttpResponse::Ok().json(session),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "Not Found", "Realtime session not found"),
        Err(error) => openai_error_response(error),
    }
}

pub async fn update_openai_realtime_session(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
    body: Bytes,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::PATCH, request.uri().clone(), None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };
    let payload: RealtimeSessionPatchRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"),
    };
    match openai.update_realtime_session(path.into_inner().as_str(), payload) {
        Ok(Some(session)) => HttpResponse::Ok().json(session),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "Not Found", "Realtime session not found"),
        Err(error) => openai_error_response(error),
    }
}

pub async fn delete_openai_realtime_session(
    state: web::Data<HttpState>,
    request: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return not_implemented_response("/v1/*", Method::DELETE, request.uri().clone(), None)
                .with_message(message)
        }
        OpenAiV1Capability::Available { openai } => openai,
    };
    match openai.delete_realtime_session(path.into_inner().as_str()) {
        Ok(Some(session)) => HttpResponse::Ok().json(session),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "Not Found", "Realtime session not found"),
        Err(error) => openai_error_response(error),
    }
}

pub async fn openai_videos_create(
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

    let execution_request = match build_openai_execution_request(
        request,
        OpenAiRequestBody::Json(body),
        path_params,
        None,
    ) {
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

async fn create_realtime_upgrade_session(
    state: web::Data<HttpState>,
    request: HttpRequest,
) -> HttpResponse {
    let websocket_key = match request
        .headers()
        .get("Sec-WebSocket-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(key) => key.to_owned(),
        None => return error_response(StatusCode::BAD_REQUEST, "Bad Request", "Sec-WebSocket-Key header is required"),
    };
    let payload = RealtimeSessionTransportRequest {
        transport: "websocket".to_owned(),
        model: request
            .uri()
            .query()
            .and_then(|query| query.split('&').find_map(|pair| pair.split_once('=')))
            .and_then(|(key, value)| (key == "model").then(|| value.to_owned()))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "gpt-4o-realtime-preview".to_owned()),
        channel_id: None,
        metadata: Some(serde_json::json!({"upgrade": true})),
        expires_at: None,
    };
    let session_response = create_openai_realtime_session(
        state,
        request.clone(),
        Bytes::from(serde_json::to_vec(&payload).unwrap_or_default()),
    )
    .await;
    if !session_response.status().is_success() {
        return session_response;
    }
    let session_body = actix_web::body::to_bytes(session_response.into_body())
        .await
        .unwrap_or_default();
    let session_json: Value = match serde_json::from_slice(&session_body) {
        Ok(json) => json,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "Failed to decode realtime session response",
            )
        }
    };
    let accept = websocket_accept_key(websocket_key.as_str());
    HttpResponse::build(StatusCode::SWITCHING_PROTOCOLS)
        .insert_header((header::UPGRADE, "websocket"))
        .insert_header((header::CONNECTION, "Upgrade"))
        .insert_header(("Sec-WebSocket-Accept", accept))
        .insert_header(("X-AxonHub-Realtime-Session-Id", session_json["sessionId"].as_str().unwrap_or_default()))
        .json(session_json)
}

fn websocket_accept_key(key: &str) -> String {
    const MAGIC: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    use sha1::{Digest, Sha1};
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(MAGIC.as_bytes());
    STANDARD.encode(hasher.finalize())
}

fn openai_session_lookup(
    state: web::Data<HttpState>,
    request: &HttpRequest,
    session_id: &str,
) -> Result<Option<crate::models::RealtimeSessionRecord>, crate::ports::OpenAiV1Error> {
    let openai = match &state.openai_v1 {
        OpenAiV1Capability::Unsupported { message } => {
            return Err(crate::ports::OpenAiV1Error::Internal {
                message: message.clone(),
            })
        }
        OpenAiV1Capability::Available { openai } => openai,
    };
    let session = openai.get_realtime_session(session_id)?;
    let context = request
        .extensions()
        .get::<crate::state::RequestContextState>()
        .cloned()
        .unwrap_or_default();
    Ok(session.filter(|current| context.project.as_ref().is_none_or(|project| project.id == current.project_id)))
}

async fn parse_image_multipart_request(
    mut multipart: Multipart,
    content_type: Option<String>,
    route: crate::models::OpenAiV1Route,
) -> Result<OpenAiRequestBody, HttpResponse> {
    let content_type = content_type
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"))?;
    let mut fields = Vec::new();

    while let Some(item) = multipart.next().await {
        let mut field = item
            .map_err(|_| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid request format"))?;
        let disposition = field.content_disposition().cloned().ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid image payload")
        })?;
        let name = disposition.get_name().map(str::to_owned).ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid image payload")
        })?;
        let file_name = disposition.get_filename().map(str::to_owned);
        let content_type = field.content_type().map(ToString::to_string);
        let mut data = Vec::new();

        while let Some(chunk) = field.next().await {
            let chunk = chunk
                .map_err(|_| error_response(StatusCode::BAD_REQUEST, "Bad Request", "Invalid image payload"))?;
            data.extend_from_slice(&chunk);
        }

        fields.push(OpenAiMultipartField {
            name,
            file_name,
            content_type,
            data,
        });
    }

    validate_image_multipart_fields(route, &fields)?;

    parse_openai_multipart_body(content_type.as_str(), fields)
}

fn validate_image_multipart_fields(
    route: crate::models::OpenAiV1Route,
    fields: &[OpenAiMultipartField],
) -> Result<(), HttpResponse> {
    let has_model = fields.iter().any(|field| {
        field.name == "model"
            && std::str::from_utf8(&field.data)
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
    });
    if !has_model {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "model is required",
        ));
    }

    let image_count = fields
        .iter()
        .filter(|field| field.name == "image" || field.name == "image[]")
        .count();
    if image_count == 0 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "image is required",
        ));
    }

    match route {
        crate::models::OpenAiV1Route::ImagesEdits => {
            let has_prompt = fields.iter().any(|field| {
                field.name == "prompt"
                    && std::str::from_utf8(&field.data)
                        .ok()
                        .is_some_and(|value| !value.trim().is_empty())
            });
            if !has_prompt {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "Bad Request",
                    "prompt is required",
                ));
            }
        }
        crate::models::OpenAiV1Route::ImagesVariations => {
            if image_count != 1 {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "Bad Request",
                    "image variations require exactly one image",
                ));
            }
            let has_prompt = fields.iter().any(|field| {
                field.name == "prompt"
                    && std::str::from_utf8(&field.data)
                        .ok()
                        .is_some_and(|value| !value.trim().is_empty())
            });
            if has_prompt {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "Bad Request",
                    "prompt is not supported for image variations",
                ));
            }
        }
        _ => {}
    }

    Ok(())
}
