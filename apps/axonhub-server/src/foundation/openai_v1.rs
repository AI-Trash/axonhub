use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use axonhub_db_entity::realtime_sessions;
use axonhub_http::{
    AnthropicModel, AnthropicModelListResponse, AuthApiKeyContext, CompatibilityRoute,
    GeminiModel, GeminiModelListResponse, ModelCapabilities, ModelListResponse, ModelPricing,
    OpenAiModel, OpenAiMultipartBody, OpenAiRequestBody, OpenAiV1Error,
    OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Port, OpenAiV1Route,
    RealtimeSessionCreateRequest, RealtimeSessionPatchRequest, RealtimeSessionRecord,
};
use getrandom::fill as getrandom;
use hex::encode as hex_encode;
use opentelemetry::propagation::{Injector, TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{field, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use super::{
    admin_operational::{persist_provider_quota_status_seaorm, quota_exhausted_details, quota_ready_details},
    authz::{require_api_key_scope, AuthzFailure, SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS},
    circuit_breaker::{ChannelBreakerStatus, CircuitBreakerSnapshot, CircuitBreakerState, SharedCircuitBreaker},
    ports::OpenAiV1Repository,
    prompt_protection::{apply_prompt_protection, load_enabled_prompt_protection_rules_seaorm},
    repositories::openai_v1::{
        create_request_execution_seaorm, create_request_seaorm,
        default_data_storage_id_seaorm, enforce_api_key_quota_seaorm,
        list_enabled_model_records_seaorm,
        query_system_channel_settings_seaorm,
        record_usage_seaorm, select_doubao_task_targets_seaorm,
        select_inference_targets_seaorm, select_target_channels_seaorm,
        update_request_execution_result_seaorm, update_request_result_seaorm,
    },
    seaorm::SeaOrmConnectionFactory,
    shared::current_unix_timestamp,
};

#[cfg(test)]
use super::circuit_breaker::CircuitBreakerPolicy;

pub struct SeaOrmOpenAiV1Service {
    db: SeaOrmConnectionFactory,
    circuit_breaker: SharedCircuitBreaker,
}

pub(crate) const DEFAULT_MAX_CHANNEL_RETRIES: usize = 2;
pub(crate) const DEFAULT_MAX_SAME_CHANNEL_RETRIES: usize = 2;

impl SeaOrmOpenAiV1Service {
    pub fn new(db: SeaOrmConnectionFactory) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory(&db);
        Self { db, circuit_breaker }
    }

    #[cfg(test)]
    pub(crate) fn new_with_circuit_breaker(
        db: SeaOrmConnectionFactory,
        circuit_breaker: SharedCircuitBreaker,
    ) -> Self {
        Self { db, circuit_breaker }
    }

    #[cfg(test)]
    pub(crate) fn new_with_circuit_breaker_policy(
        db: SeaOrmConnectionFactory,
        policy: CircuitBreakerPolicy,
    ) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory_and_policy(&db, policy);
        Self { db, circuit_breaker }
    }
}

impl OpenAiV1Port for SeaOrmOpenAiV1Service {
    fn list_models(
        &self,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<ModelListResponse, OpenAiV1Error> {
        require_api_key_scope(api_key, SCOPE_READ_CHANNELS).map_err(authz_openai_error)?;
        let db = self.db.clone();
        let include_owned = include.map(ToOwned::to_owned);
        let profiles_json = api_key.profiles_json.clone();
        let models = db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            let include = ModelInclude::parse(include_owned.as_deref());
            let settings = query_system_channel_settings_seaorm(&connection).await?;
            let models = list_enabled_model_records_seaorm(
                &connection,
                db.backend(),
                settings.query_all_channel_models,
                profiles_json.as_deref(),
            )
                .await?
                .into_iter()
                .map(|record| record.into_openai_model(&include))
                .collect::<Vec<_>>();
            Ok(models)
        })?;

        Ok(ModelListResponse { object: "list", data: models })
    }

    fn retrieve_model(
        &self,
        model_id: &str,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<OpenAiModel, OpenAiV1Error> {
        require_api_key_scope(api_key, SCOPE_READ_CHANNELS).map_err(authz_openai_error)?;
        let db = self.db.clone();
        let model_id = model_id.to_owned();
        let include_owned = include.map(ToOwned::to_owned);
        let profiles_json = api_key.profiles_json.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            let include = ModelInclude::parse(include_owned.as_deref());
            let settings = query_system_channel_settings_seaorm(&connection).await?;
            let model = list_enabled_model_records_seaorm(
                &connection,
                db.backend(),
                settings.query_all_channel_models,
                profiles_json.as_deref(),
            )
            .await?
            .into_iter()
            .find(|record| record.model_id == model_id)
            .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                message: format!("The model `{}` does not exist or you do not have access to it.", model_id),
            })?;
            Ok(model.into_openai_model(&include))
        })
    }

    fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
        let db = self.db.clone();
        let models = db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            let settings = query_system_channel_settings_seaorm(&connection).await?;
            list_enabled_model_records_seaorm(
                &connection,
                db.backend(),
                settings.query_all_channel_models,
                None,
            )
            .await
        })?;

        let data = models
            .into_iter()
            .map(|record| AnthropicModel {
                id: record.model_id,
                kind: "model",
                display_name: record.name,
                created: sqlite_timestamp_to_rfc3339(record.created_at.as_str()),
            })
            .collect::<Vec<_>>();
        let first_id = if data.is_empty() {
            Some(String::new())
        } else {
            data.first().map(|model| model.id.clone())
        };
        let last_id = if data.is_empty() {
            Some(String::new())
        } else {
            data.last().map(|model| model.id.clone())
        };

        Ok(AnthropicModelListResponse {
            object: "list",
            data,
            has_more: false,
            first_id,
            last_id,
        })
    }

    fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
        let db = self.db.clone();
        let models = db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            let settings = query_system_channel_settings_seaorm(&connection).await?;
            list_enabled_model_records_seaorm(
                &connection,
                db.backend(),
                settings.query_all_channel_models,
                None,
            )
            .await
        })?;

        Ok(GeminiModelListResponse {
            models: models
                .into_iter()
                .enumerate()
                .map(|(index, record)| GeminiModel {
                    name: format!("models/{}", record.model_id),
                    base_model_id: record.model_id.clone(),
                    version: format!("{}-{index}", record.model_id),
                    display_name: record.name.clone(),
                    description: record.name,
                    supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
                })
                .collect(),
        })
    }

    fn execute(
        &self,
        route: OpenAiV1Route,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        let span = openai_execution_span(
            "openai_v1.execute",
            "openai_v1",
            route.format(),
            &request,
        );
        let _enter = span.enter();

        let result = (|| {
            require_api_key_scope(&request.api_key, SCOPE_WRITE_REQUESTS).map_err(authz_openai_error)?;
            validate_openai_request(route, &request.body)?;
            let route_format = route.format().to_owned();
            let request_model_id = request_model_id(&request.body)?;
            let db = self.db.clone();
            let circuit_breaker = self.circuit_breaker.clone();

            db.run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
                let backend = db.backend();
                let targets = select_target_channels_seaorm(
                    &connection,
                    backend,
                    &request,
                    route,
                    &circuit_breaker,
                    request.api_key.profiles_json.as_deref(),
                )
                .await?;
                Span::current().record("target.selected_count", targets.len() as i64);
                let data_storage_id = default_data_storage_id_seaorm(&connection, backend).await?;
                execute_shared_route_seaorm(
                    &connection,
                    backend,
                    &request,
                    request_model_id.as_str(),
                    route_format.as_str(),
                    reqwest::Method::POST,
                    targets,
                    &request.body,
                    &request.headers,
                    data_storage_id,
                    &circuit_breaker,
                    |target| target.upstream_url(route),
                    Ok,
                    |response_body| extract_usage(route, response_body),
                )
                .await
            })
        })();

        record_openai_execution_outcome(&span, &result);
        result
    }

    fn execute_compatibility(
        &self,
        route: CompatibilityRoute,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        let span = openai_execution_span(
            "openai_v1.execute_compatibility",
            "compatibility",
            route.format(),
            &request,
        );
        let _enter = span.enter();

        let result = (|| {
            let db = self.db.clone();
            let circuit_breaker = self.circuit_breaker.clone();
            db.run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
                let backend = db.backend();
                let data_storage_id = default_data_storage_id_seaorm(&connection, backend).await?;
                let prepared = prepare_compatibility_request(route, &request)?;
                let targets = if matches!(
                    route,
                    CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask
                ) {
                    select_doubao_task_targets_seaorm(&connection, backend, &request, &prepared).await?
                } else {
                    select_inference_targets_seaorm(
                        &connection,
                        backend,
                        prepared.request_model_id.as_str(),
                        request.trace.as_ref().map(|trace| trace.id),
                        DEFAULT_MAX_CHANNEL_RETRIES,
                        prepared.channel_type,
                        prepared.model_type,
                        None,
                        &circuit_breaker,
                        None,
                    )
                    .await?
                };
                Span::current().record("target.selected_count", targets.len() as i64);

                if targets.is_empty() {
                    return Err(OpenAiV1Error::InvalidRequest {
                        message: format!(
                            "No enabled {} channel is configured for the requested model",
                            prepared.channel_type
                        ),
                    });
                }

                let route_task_id = prepared.task_id.clone();
                execute_shared_route_seaorm(
                    &connection,
                    backend,
                    &request,
                    prepared.request_model_id.as_str(),
                    route.format(),
                    compatibility_upstream_method(route),
                    targets,
                    &OpenAiRequestBody::Json(prepared.upstream_body.clone()),
                    &request.headers,
                    data_storage_id,
                    &circuit_breaker,
                    move |target| compatibility_upstream_url(target, route, route_task_id.as_deref()),
                    |response_body| map_compatibility_response(route, response_body),
                    |response_body| compatibility_usage(route, response_body),
                )
                .await
            })
        })();

        record_openai_execution_outcome(&span, &result);
        result
    }

    fn create_realtime_session(
        &self,
        request: RealtimeSessionCreateRequest,
    ) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            create_realtime_session_seaorm(&connection, db.backend(), request).await
        })
    }

    fn get_realtime_session(&self, session_id: &str) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        let db = self.db.clone();
        let session_id = session_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            get_realtime_session_seaorm(&connection, &session_id).await
        })
    }

    fn update_realtime_session(
        &self,
        session_id: &str,
        patch: RealtimeSessionPatchRequest,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        let db = self.db.clone();
        let session_id = session_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            update_realtime_session_seaorm(&connection, &session_id, patch).await
        })
    }

    fn delete_realtime_session(&self, session_id: &str) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        let db = self.db.clone();
        let session_id = session_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(map_openai_db_err)?;
            delete_realtime_session_seaorm(&connection, &session_id).await
        })
    }
}

impl OpenAiV1Repository for SeaOrmOpenAiV1Service {
    fn list_models(
        &self,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<ModelListResponse, OpenAiV1Error> {
        <Self as OpenAiV1Port>::list_models(self, include, api_key)
    }

    fn retrieve_model(
        &self,
        model_id: &str,
        include: Option<&str>,
        api_key: &AuthApiKeyContext,
    ) -> Result<OpenAiModel, OpenAiV1Error> {
        <Self as OpenAiV1Port>::retrieve_model(self, model_id, include, api_key)
    }

    fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
        <Self as OpenAiV1Port>::list_anthropic_models(self)
    }

    fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
        <Self as OpenAiV1Port>::list_gemini_models(self)
    }

    fn execute(
        &self,
        route: OpenAiV1Route,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        <Self as OpenAiV1Port>::execute(self, route, request)
    }

    fn execute_compatibility(
        &self,
        route: CompatibilityRoute,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        <Self as OpenAiV1Port>::execute_compatibility(self, route, request)
    }

    fn create_realtime_session(
        &self,
        request: RealtimeSessionCreateRequest,
    ) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
        <Self as OpenAiV1Port>::create_realtime_session(self, request)
    }

    fn get_realtime_session(&self, session_id: &str) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        <Self as OpenAiV1Port>::get_realtime_session(self, session_id)
    }

    fn update_realtime_session(
        &self,
        session_id: &str,
        patch: RealtimeSessionPatchRequest,
    ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        <Self as OpenAiV1Port>::update_realtime_session(self, session_id, patch)
    }

    fn delete_realtime_session(&self, session_id: &str) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
        <Self as OpenAiV1Port>::delete_realtime_session(self, session_id)
    }
}

fn openai_execution_span(
    operation_name: &'static str,
    route_family: &'static str,
    route_name: &str,
    request: &OpenAiV1ExecutionRequest,
) -> Span {
    let span = tracing::span!(
        tracing::Level::INFO,
        "openai.v1.execution",
        operation.name = operation_name,
        route.family = route_family,
        route.name = %route_name,
        auth.mode = openai_auth_mode(&request.api_key),
        auth.subject = openai_auth_subject(&request.api_key),
        request.stream = request.body.stream_flag(),
        request.bound = header_present(request.headers.get("X-Request-Id")),
        trace.bound = request.trace.is_some(),
        thread.bound = header_present(request.headers.get("AH-Thread-Id")),
        channel.hint = request.channel_hint_id.is_some(),
        target.selected_count = field::Empty,
        retry.count = field::Empty,
        request.outcome = field::Empty,
        http.status_code = field::Empty,
    );
    span.record("target.selected_count", 0_i64);
    span.record("retry.count", 0_i64);
    span
}

fn record_openai_execution_outcome(
    span: &Span,
    result: &Result<OpenAiV1ExecutionResponse, OpenAiV1Error>,
) {
    match result {
        Ok(response) => {
            span.record("request.outcome", "success");
            span.record("http.status_code", i64::from(response.status));
        }
        Err(OpenAiV1Error::Upstream { status, .. }) => {
            span.record("request.outcome", "upstream_error");
            span.record("http.status_code", i64::from(*status));
        }
        Err(OpenAiV1Error::InvalidRequest { .. }) => {
            span.record("request.outcome", "invalid_request");
        }
        Err(OpenAiV1Error::Internal { .. }) => {
            span.record("request.outcome", "internal_error");
        }
    }
}

fn openai_auth_mode(api_key: &AuthApiKeyContext) -> &'static str {
    match api_key.key_type {
        axonhub_http::ApiKeyType::NoAuth => "noauth",
        _ => "api_key",
    }
}

fn openai_auth_subject(api_key: &AuthApiKeyContext) -> &'static str {
    match api_key.key_type {
        axonhub_http::ApiKeyType::User => "user_api_key",
        axonhub_http::ApiKeyType::ServiceAccount => "service_api_key",
        axonhub_http::ApiKeyType::NoAuth => "system_noauth",
    }
}

fn header_present(value: Option<&String>) -> bool {
    value.is_some_and(|current| !current.trim().is_empty())
}

fn authz_openai_error(error: AuthzFailure) -> OpenAiV1Error {
    OpenAiV1Error::InvalidRequest {
        message: error.message().to_owned(),
    }
}

pub(crate) async fn create_realtime_session_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request: RealtimeSessionCreateRequest,
) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
    validate_realtime_session_transport(&request)?;
    enforce_api_key_quota_seaorm(db, backend, request.api_key_id).await?;

    let session_id = generate_realtime_session_id()?;
    let request_body_json = serde_json::to_string(&request.transport).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to serialize realtime session request: {error}"),
    })?;
    let metadata_json = build_realtime_session_metadata(
        &request.transport.model,
        request.client_ip.as_deref(),
        request.request_id.as_deref(),
        request.transport.metadata.clone(),
    )?;
    let metadata_json = attach_realtime_context_metadata(
        metadata_json,
        request.thread.as_ref().map(|thread| thread.thread_id.as_str()),
        request.trace.as_ref().map(|trace| trace.trace_id.as_str()),
    )?;
    let data_storage_id = default_data_storage_id_seaorm(db, backend).await?;
    let request_row_id = create_request_seaorm(
        db,
        backend,
        &NewRequestRecord {
            api_key_id: request.api_key_id,
            project_id: request.project.id,
            trace_id: request.trace.as_ref().map(|trace| trace.id),
            data_storage_id,
            source: "api",
            model_id: request.transport.model.as_str(),
            format: realtime_route_format(request.transport.transport.as_str()),
            request_headers_json: "{}",
            request_body_json: request_body_json.as_str(),
            response_body_json: None,
            response_chunks_json: None,
            channel_id: request.transport.channel_id,
            external_id: None,
            status: "processing",
            stream: false,
            client_ip: request.client_ip.as_deref().unwrap_or(""),
            metrics_latency_ms: None,
            metrics_first_token_latency_ms: None,
            content_saved: false,
            content_storage_id: None,
            content_storage_key: None,
            content_saved_at: None,
        },
    )
    .await?;

    realtime_sessions::Entity::insert(realtime_sessions::ActiveModel {
        project_id: Set(request.project.id),
        thread_id: Set(request.thread.as_ref().map(|thread| thread.id)),
        trace_id: Set(request.trace.as_ref().map(|trace| trace.id)),
        request_id: Set(Some(request_row_id)),
        api_key_id: Set(request.api_key_id),
        channel_id: Set(request.transport.channel_id),
        session_id: Set(session_id.clone()),
        transport: Set(request.transport.transport.clone()),
        status: Set("open".to_owned()),
        metadata: Set(metadata_json),
        opened_at: Set(current_realtime_timestamp()),
        last_activity_at: Set(current_realtime_timestamp()),
        closed_at: Set(None),
        expires_at: Set(request.transport.expires_at.clone()),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(map_openai_db_err)?;

    let record = get_realtime_session_seaorm(db, &session_id)
        .await?
        .ok_or_else(|| OpenAiV1Error::Internal {
            message: "Realtime session was created but could not be reloaded".to_owned(),
        })?;
    let response_body_json = serde_json::to_string(&record).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to serialize realtime session response: {error}"),
    })?;
    update_request_result_seaorm(
        db,
        backend,
        &UpdateRequestResultRecord {
            request_id: request_row_id,
            status: "completed",
            external_id: Some(record.session_id.as_str()),
            response_body_json: Some(response_body_json.as_str()),
            channel_id: record.channel_id,
        },
    )
    .await?;

    Ok(record)
}

pub(crate) async fn get_realtime_session_seaorm(
    db: &impl ConnectionTrait,
    session_id: &str,
) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
    realtime_sessions::Entity::find()
        .filter(realtime_sessions::Column::SessionId.eq(session_id))
        .one(db)
        .await
        .map_err(map_openai_db_err)?
        .map(realtime_session_record_from_model)
        .transpose()
}

pub(crate) async fn update_realtime_session_seaorm(
    db: &impl ConnectionTrait,
    session_id: &str,
    patch: RealtimeSessionPatchRequest,
) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
    let Some(model) = realtime_sessions::Entity::find()
        .filter(realtime_sessions::Column::SessionId.eq(session_id))
        .one(db)
        .await
        .map_err(map_openai_db_err)?
    else {
        return Ok(None);
    };

    validate_realtime_session_patch(model.status.as_str(), patch.status.as_deref())?;
    let merged_metadata = merge_realtime_session_metadata(model.metadata.as_str(), patch.metadata)?;
    let next_status = patch.status.unwrap_or_else(|| model.status.clone());
    let closed_at = if realtime_session_terminal_status(next_status.as_str()) {
        Some(current_realtime_timestamp())
    } else {
        model.closed_at.clone()
    };

    let mut active: realtime_sessions::ActiveModel = model.into();
    active.status = Set(next_status);
    active.metadata = Set(merged_metadata);
    active.last_activity_at = Set(current_realtime_timestamp());
    active.closed_at = Set(closed_at);
    if let Some(expires_at) = patch.expires_at {
        active.expires_at = Set(Some(expires_at));
    }

    let updated = active.update(db).await.map_err(map_openai_db_err)?;
    Ok(Some(realtime_session_record_from_model(updated)?))
}

pub(crate) async fn delete_realtime_session_seaorm(
    db: &impl ConnectionTrait,
    session_id: &str,
) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
    update_realtime_session_seaorm(
        db,
        session_id,
        RealtimeSessionPatchRequest {
            status: Some("closed".to_owned()),
            metadata: None,
            expires_at: None,
        },
    )
    .await
}

pub(crate) fn realtime_session_record_from_model(
    model: realtime_sessions::Model,
) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
    let metadata: Value = serde_json::from_str(model.metadata.as_str()).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to decode realtime session metadata: {error}"),
    })?;
    let model_id = metadata
        .get("model")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_default();

    Ok(RealtimeSessionRecord {
        session_id: model.session_id,
        transport: model.transport,
        status: model.status,
        model: model_id,
        project_id: model.project_id,
        thread_id: metadata
            .get("threadId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        trace_id: metadata
            .get("traceId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        request_id: model.request_id,
        api_key_id: model.api_key_id,
        channel_id: model.channel_id,
        metadata,
        opened_at: model.opened_at,
        last_activity_at: model.last_activity_at,
        closed_at: model.closed_at,
        expires_at: model.expires_at,
    })
}

pub(crate) fn validate_realtime_session_transport(
    request: &RealtimeSessionCreateRequest,
) -> Result<(), OpenAiV1Error> {
    let transport = request.transport.transport.trim();
    if !matches!(transport, "websocket" | "session") {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "transport must be `websocket` or `session`".to_owned(),
        });
    }
    if request.transport.model.trim().is_empty() {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn validate_realtime_session_patch(
    current_status: &str,
    next_status: Option<&str>,
) -> Result<(), OpenAiV1Error> {
    let Some(next_status) = next_status else {
        return Ok(());
    };
    if !matches!(next_status, "open" | "closing" | "closed" | "failed") {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "status must be `open`, `closing`, `closed`, or `failed`".to_owned(),
        });
    }
    if realtime_session_terminal_status(current_status) && current_status != next_status {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "realtime session is already terminal".to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn realtime_session_terminal_status(status: &str) -> bool {
    matches!(status, "closed" | "failed")
}

pub(crate) fn realtime_route_format(transport: &str) -> &'static str {
    match transport {
        "websocket" => "openai/realtime_upgrade",
        _ => "openai/realtime_session",
    }
}

pub(crate) fn current_realtime_timestamp() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
}

pub(crate) fn generate_realtime_session_id() -> Result<String, OpenAiV1Error> {
    let mut bytes = [0_u8; 16];
    getrandom(&mut bytes).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to generate realtime session id: {error}"),
    })?;
    Ok(format!("rtsess_{}", hex_encode(bytes)))
}

pub(crate) fn build_realtime_session_metadata(
    model: &str,
    client_ip: Option<&str>,
    request_id: Option<&str>,
    user_metadata: Option<Value>,
) -> Result<String, OpenAiV1Error> {
    let mut metadata = serde_json::Map::new();
    metadata.insert("model".to_owned(), Value::String(model.to_owned()));
    if let Some(client_ip) = client_ip.filter(|value| !value.is_empty()) {
        metadata.insert("clientIp".to_owned(), Value::String(client_ip.to_owned()));
    }
    if let Some(request_id) = request_id.filter(|value| !value.is_empty()) {
        metadata.insert("requestId".to_owned(), Value::String(request_id.to_owned()));
    }
    if let Some(value) = user_metadata {
        metadata.insert("attributes".to_owned(), value);
    }
    serde_json::to_string(&Value::Object(metadata)).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to encode realtime session metadata: {error}"),
    })
}

pub(crate) fn attach_realtime_context_metadata(
    current: String,
    thread_id: Option<&str>,
    trace_id: Option<&str>,
) -> Result<String, OpenAiV1Error> {
    let mut value: Value = serde_json::from_str(current.as_str()).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to decode realtime session metadata: {error}"),
    })?;
    let object = value.as_object_mut().ok_or_else(|| OpenAiV1Error::Internal {
        message: "Realtime session metadata must be a JSON object".to_owned(),
    })?;
    if let Some(thread_id) = thread_id {
        object.insert("threadId".to_owned(), Value::String(thread_id.to_owned()));
    }
    if let Some(trace_id) = trace_id {
        object.insert("traceId".to_owned(), Value::String(trace_id.to_owned()));
    }
    serde_json::to_string(&value).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to encode realtime session metadata: {error}"),
    })
}

pub(crate) fn merge_realtime_session_metadata(
    current: &str,
    patch_metadata: Option<Value>,
) -> Result<String, OpenAiV1Error> {
    let mut value: Value = serde_json::from_str(current).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to decode realtime session metadata: {error}"),
    })?;
    if let Some(metadata) = patch_metadata {
        let object = value.as_object_mut().ok_or_else(|| OpenAiV1Error::Internal {
            message: "Realtime session metadata must be a JSON object".to_owned(),
        })?;
        object.insert("attributes".to_owned(), metadata);
    }
    serde_json::to_string(&value).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to encode realtime session metadata: {error}"),
    })
}

fn map_openai_db_err(error: sea_orm::DbErr) -> OpenAiV1Error {
    OpenAiV1Error::Internal {
        message: error.to_string(),
    }
}

async fn execute_shared_route_seaorm<UrlBuilder, ResponseMapper, UsageExtractor>(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request: &OpenAiV1ExecutionRequest,
    request_model_id: &str,
    route_format: &str,
    upstream_method: reqwest::Method,
    targets: Vec<SelectedOpenAiTarget>,
    upstream_body: &OpenAiRequestBody,
    upstream_headers: &HashMap<String, String>,
    data_storage_id: Option<i64>,
    circuit_breaker: &SharedCircuitBreaker,
    upstream_url_for_target: UrlBuilder,
    response_mapper: ResponseMapper,
    usage_extractor: UsageExtractor,
) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error>
where
    UrlBuilder: Fn(&SelectedOpenAiTarget) -> String,
    ResponseMapper: Fn(Value) -> Result<Value, OpenAiV1Error>,
    UsageExtractor: Fn(&Value) -> Option<ExtractedUsage>,
{
    enforce_api_key_quota_seaorm(db, backend, request.api_key_id).await?;
    let span = Span::current();

    let masked_request_headers = sanitize_headers_json(upstream_headers);
    let request_body_json = serde_json::to_string(&request.body).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to serialize request body: {error}"),
    })?;
    let stream = request.body.stream_flag();

    let request_id = create_request_seaorm(
        db,
        backend,
        &NewRequestRecord {
            api_key_id: request.api_key_id,
            project_id: request.project.id,
            trace_id: request.trace.as_ref().map(|trace| trace.id),
            data_storage_id,
            source: "api",
            model_id: request_model_id,
            format: route_format,
            request_headers_json: masked_request_headers.as_str(),
            request_body_json: request_body_json.as_str(),
            response_body_json: None,
            response_chunks_json: None,
            channel_id: None,
            external_id: None,
            status: "processing",
            stream,
            client_ip: request.client_ip.as_deref().unwrap_or(""),
            metrics_latency_ms: None,
            metrics_first_token_latency_ms: None,
            content_saved: false,
            content_storage_id: None,
            content_storage_key: None,
            content_saved_at: None,
        },
    )
    .await?;

    let mut last_error = None;
    let mut retry_count = 0_i64;
    for (index, target) in targets.iter().enumerate() {
        let mut same_channel_attempts = 0;
        loop {
            let prepared_attempt = match prepare_outbound_request_with_prompt_protection(
                db,
                backend,
                upstream_body,
                upstream_headers,
                target.actual_model_id.as_str(),
                target.api_key.as_str(),
            )
            .await
            {
                Ok(prepared_attempt) => prepared_attempt,
                Err(error) => {
                    mark_request_failed_seaorm(db, backend, request_id, Some(target.channel_id), None, None)
                        .await?;
                    return Err(error);
                }
            };

            update_request_result_seaorm(
                db,
                backend,
                &UpdateRequestResultRecord {
                    request_id,
                    status: "processing",
                    external_id: None,
                    response_body_json: None,
                    channel_id: Some(target.channel_id),
                },
            )
            .await?;

            let execution_id = create_request_execution_seaorm(
                db,
                backend,
                &NewRequestExecutionRecord {
                    project_id: request.project.id,
                    request_id,
                        channel_id: Some(target.channel_id),
                        data_storage_id,
                        external_id: None,
                        model_id: target.actual_model_id.as_str(),
                        format: route_format,
                        request_body_json: prepared_attempt.body_json.as_str(),
                        response_body_json: None,
                        response_chunks_json: None,
                        error_message: "",
                    response_status_code: None,
                    status: "processing",
                    stream,
                    metrics_latency_ms: None,
                    metrics_first_token_latency_ms: None,
                    request_headers_json: masked_request_headers.as_str(),
                },
            )
            .await?;

            let attempt_result = async {
                let client_http = reqwest::Client::new();
                let mut upstream_request = client_http
                    .request(upstream_method.clone(), upstream_url_for_target(target).as_str())
                    .headers(prepared_attempt.headers.clone());
                if matches!(upstream_method, reqwest::Method::POST) {
                    upstream_request = apply_upstream_request_body(upstream_request, &prepared_attempt.body)?;
                }
                let upstream_response = upstream_request.send().await.map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to execute upstream request: {error}"),
                })?;
                let status = upstream_response.status().as_u16();
                let response_text = upstream_response.text().await.map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to read upstream response: {error}"),
                })?;
                let raw_response_body: Value = serde_json::from_str(&response_text).map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to decode upstream response: {error}"),
                })?;

                if (200..300).contains(&status) {
                    let usage = usage_extractor(&raw_response_body);
                    let response_body = response_mapper(raw_response_body)?;
                    complete_execution_seaorm(
                        db,
                        backend,
                        request,
                        route_format,
                        request_id,
                        execution_id,
                        target,
                        status,
                        response_body,
                        usage,
                        circuit_breaker,
                    )
                    .await
                } else {
                    Err(OpenAiV1Error::Upstream {
                        status,
                        body: raw_response_body,
                    })
                }
            }
            .await;

            match attempt_result {
                Ok(response) => return Ok(response),
                Err(error) => {
                    maybe_persist_provider_quota_error_seaorm(db, target, &error).await?;
                    if should_retry_openai_error(&error) {
                        circuit_breaker.record_failure(target.channel_id, target.actual_model_id.as_str());
                    }
                    let (response_body, response_status_code, external_id) = match &error {
                        OpenAiV1Error::Upstream { status, body } => (
                            Some(body),
                            Some(*status),
                            body.get("id").and_then(Value::as_str),
                        ),
                        OpenAiV1Error::Internal { .. } | OpenAiV1Error::InvalidRequest { .. } => {
                            (None, None, None)
                        }
                    };
                    mark_execution_failed_seaorm(
                        db,
                        backend,
                        execution_id,
                        openai_error_message(&error).as_str(),
                        response_body,
                        response_status_code,
                        external_id,
                    )
                    .await?;

                    let retryable = should_retry_openai_error(&error);
                    if retryable && same_channel_attempts < DEFAULT_MAX_SAME_CHANNEL_RETRIES {
                        same_channel_attempts += 1;
                        retry_count += 1;
                        span.record("retry.count", retry_count);
                        continue;
                    }

                    let is_last = index + 1 == targets.len();
                    if retryable && !is_last {
                        retry_count += 1;
                        span.record("retry.count", retry_count);
                        last_error = Some(error);
                        break;
                    }

                    mark_request_failed_seaorm(
                        db,
                        backend,
                        request_id,
                        Some(target.channel_id),
                        response_body,
                        external_id,
                    )
                    .await?;
                    return Err(error);
                }
            }
        }
    }

    let terminal_error = last_error.unwrap_or_else(|| OpenAiV1Error::Internal {
        message: "No upstream channel attempt was executed".to_owned(),
    });
    mark_request_failed_seaorm(db, backend, request_id, None, None, None).await?;
    Err(terminal_error)
}

async fn complete_execution_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request: &OpenAiV1ExecutionRequest,
    route_format: &str,
    request_id: i64,
    execution_id: i64,
    target: &SelectedOpenAiTarget,
    status: u16,
    response_body: Value,
    usage: Option<ExtractedUsage>,
    circuit_breaker: &SharedCircuitBreaker,
) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
    let response_body_json = serde_json::to_string(&response_body).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to serialize upstream response: {error}"),
    })?;
    let external_id = response_body.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
    if let Some(usage) = usage {
        let usage_cost = compute_usage_cost(&target.model, &usage);
        let cost_items_json = serde_json::to_string(&usage_cost.cost_items).map_err(|error| {
            OpenAiV1Error::Internal {
                message: format!("Failed to serialize usage cost items: {error}"),
            }
        })?;
        record_usage_seaorm(
            db,
            backend,
            &NewUsageLogRecord {
                request_id,
                api_key_id: request.api_key_id,
                project_id: request.project.id,
                channel_id: Some(target.channel_id),
                model_id: target.actual_model_id.as_str(),
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                prompt_audio_tokens: usage.prompt_audio_tokens,
                prompt_cached_tokens: usage.prompt_cached_tokens,
                prompt_write_cached_tokens: usage.prompt_write_cached_tokens,
                prompt_write_cached_tokens_5m: usage.prompt_write_cached_tokens_5m,
                prompt_write_cached_tokens_1h: usage.prompt_write_cached_tokens_1h,
                completion_audio_tokens: usage.completion_audio_tokens,
                completion_reasoning_tokens: usage.completion_reasoning_tokens,
                completion_accepted_prediction_tokens: usage.completion_accepted_prediction_tokens,
                completion_rejected_prediction_tokens: usage.completion_rejected_prediction_tokens,
                source: "api",
                format: route_format,
                total_cost: usage_cost.total_cost,
                cost_items_json: cost_items_json.as_str(),
                cost_price_reference_id: usage_cost.price_reference_id.as_deref().unwrap_or(""),
            },
        )
        .await?;
    }

    mark_provider_quota_ready_seaorm(db, backend, target).await?;

    update_request_result_seaorm(
        db,
        backend,
        &UpdateRequestResultRecord {
            request_id,
            status: "completed",
            external_id: external_id.as_deref(),
            response_body_json: Some(response_body_json.as_str()),
            channel_id: Some(target.channel_id),
        },
    )
    .await?;
    update_request_execution_result_seaorm(
        db,
        backend,
        &UpdateRequestExecutionResultRecord {
            execution_id,
            status: "completed",
            external_id: external_id.as_deref(),
            response_body_json: Some(response_body_json.as_str()),
            response_status_code: Some(status as i64),
            error_message: None,
        },
    )
    .await?;
    circuit_breaker.record_success(target.channel_id, target.actual_model_id.as_str());

    Ok(OpenAiV1ExecutionResponse { status, body: response_body })
}

pub(crate) async fn mark_provider_quota_ready_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    target: &SelectedOpenAiTarget,
) -> Result<(), OpenAiV1Error> {
    let Some(provider_type) = target.provider_type.as_deref() else {
        return Ok(());
    };
    persist_provider_quota_status_seaorm(
        db,
        target.channel_id,
        provider_type,
        "available",
        true,
        None,
        current_unix_timestamp(),
        serde_json::json!({
            "message": quota_ready_details(provider_type, target.channel_id),
            "source": "runtime_success",
            "channelId": target.channel_id,
        })
        .to_string(),
    )
    .await
    .map_err(|message| OpenAiV1Error::Internal { message })
}

pub(crate) async fn maybe_persist_provider_quota_error_seaorm(
    db: &impl ConnectionTrait,
    target: &SelectedOpenAiTarget,
    error: &OpenAiV1Error,
) -> Result<(), OpenAiV1Error> {
    let Some(provider_type) = target.provider_type.as_deref() else {
        return Ok(());
    };
    let status = match error {
        OpenAiV1Error::Upstream { status, .. } if *status == 429 => *status,
        _ => return Ok(()),
    };
    let message = openai_error_message(error);
    persist_provider_quota_status_seaorm(
        db,
        target.channel_id,
        provider_type,
        "exhausted",
        false,
        None,
        current_unix_timestamp(),
        serde_json::json!({
            "message": quota_exhausted_details(provider_type, target.channel_id, message.as_str()),
            "source": "runtime_error",
            "channelId": target.channel_id,
            "statusCode": status,
        })
        .to_string(),
    )
    .await
    .map_err(|message| OpenAiV1Error::Internal { message })
}

async fn mark_request_failed_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request_id: i64,
    channel_id: Option<i64>,
    response_body: Option<&Value>,
    external_id: Option<&str>,
) -> Result<(), OpenAiV1Error> {
    let response_body_json = response_body
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| OpenAiV1Error::Internal {
            message: format!("Failed to serialize failed upstream response: {error}"),
        })?;
    update_request_result_seaorm(
        db,
        backend,
        &UpdateRequestResultRecord {
            request_id,
            status: "failed",
            external_id,
            response_body_json: response_body_json.as_deref(),
            channel_id,
        },
    )
    .await
}

async fn mark_execution_failed_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    execution_id: i64,
    error_message: &str,
    response_body: Option<&Value>,
    response_status_code: Option<u16>,
    external_id: Option<&str>,
) -> Result<(), OpenAiV1Error> {
    let response_body_json = response_body
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| OpenAiV1Error::Internal {
            message: format!("Failed to serialize failed upstream response: {error}"),
        })?;
    update_request_execution_result_seaorm(
        db,
        backend,
        &UpdateRequestExecutionResultRecord {
            execution_id,
            status: "failed",
            external_id,
            response_body_json: response_body_json.as_deref(),
            response_status_code: response_status_code.map(i64::from),
            error_message: Some(error_message),
        },
    )
    .await
}


fn should_retry_openai_error(error: &OpenAiV1Error) -> bool {
    match error {
        OpenAiV1Error::Internal { .. } => true,
        OpenAiV1Error::Upstream { status, .. } => {
            *status == 408 || *status == 409 || *status == 429 || *status >= 500
        }
        OpenAiV1Error::InvalidRequest { .. } => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredChannelSummary {
    pub id: i64,
    pub name: String,
    pub channel_type: String,
    pub base_url: String,
    pub status: String,
    pub supported_models: Vec<String>,
    pub ordering_weight: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRequestSummary {
    pub id: i64,
    pub project_id: i64,
    pub trace_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub model_id: String,
    pub format: String,
    pub status: String,
    pub source: String,
    pub external_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredModelRecord {
    pub id: i64,
    pub created_at: String,
    pub developer: String,
    pub model_id: String,
    pub model_type: String,
    pub name: String,
    pub icon: String,
    pub remark: String,
    pub model_card_json: String,
}

#[derive(Debug, Clone)]
pub struct SelectedOpenAiTarget {
    pub channel_id: i64,
    pub base_url: String,
    pub api_key: String,
    pub actual_model_id: String,
    pub provider_type: Option<String>,
    pub ordering_weight: i64,
    pub trace_affinity: bool,
    pub circuit_breaker: Option<CircuitBreakerSnapshot>,
    pub routing_stats: ChannelRoutingStats,
    pub model: StoredModelRecord,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelRoutingStats {
    pub selection_count: i64,
    pub processing_count: i64,
    pub consecutive_failures: i64,
    pub last_status_failed: bool,
}

#[allow(dead_code)]
pub struct NewChannelRecord<'a> {
    pub name: &'a str,
    pub channel_type: &'a str,
    pub base_url: &'a str,
    pub status: &'a str,
    pub credentials_json: &'a str,
    pub supported_models_json: &'a str,
    pub auto_sync_supported_models: bool,
    pub default_test_model: &'a str,
    pub settings_json: &'a str,
    pub tags_json: &'a str,
    pub ordering_weight: i64,
    pub error_message: &'a str,
    pub remark: &'a str,
}

#[allow(dead_code)]
pub struct NewModelRecord<'a> {
    pub developer: &'a str,
    pub model_id: &'a str,
    pub model_type: &'a str,
    pub name: &'a str,
    pub icon: &'a str,
    pub group: &'a str,
    pub model_card_json: &'a str,
    pub settings_json: &'a str,
    pub status: &'a str,
    pub remark: &'a str,
}

pub struct NewRequestRecord<'a> {
    pub api_key_id: Option<i64>,
    pub project_id: i64,
    pub trace_id: Option<i64>,
    pub data_storage_id: Option<i64>,
    pub source: &'a str,
    pub model_id: &'a str,
    pub format: &'a str,
    pub request_headers_json: &'a str,
    pub request_body_json: &'a str,
    pub response_body_json: Option<&'a str>,
    pub response_chunks_json: Option<&'a str>,
    pub channel_id: Option<i64>,
    pub external_id: Option<&'a str>,
    pub status: &'a str,
    pub stream: bool,
    pub client_ip: &'a str,
    pub metrics_latency_ms: Option<i64>,
    pub metrics_first_token_latency_ms: Option<i64>,
    pub content_saved: bool,
    pub content_storage_id: Option<i64>,
    pub content_storage_key: Option<&'a str>,
    pub content_saved_at: Option<&'a str>,
}

pub struct NewRequestExecutionRecord<'a> {
    pub project_id: i64,
    pub request_id: i64,
    pub channel_id: Option<i64>,
    pub data_storage_id: Option<i64>,
    pub external_id: Option<&'a str>,
    pub model_id: &'a str,
    pub format: &'a str,
    pub request_body_json: &'a str,
    pub response_body_json: Option<&'a str>,
    pub response_chunks_json: Option<&'a str>,
    pub error_message: &'a str,
    pub response_status_code: Option<i64>,
    pub status: &'a str,
    pub stream: bool,
    pub metrics_latency_ms: Option<i64>,
    pub metrics_first_token_latency_ms: Option<i64>,
    pub request_headers_json: &'a str,
}

pub struct NewUsageLogRecord<'a> {
    pub request_id: i64,
    pub api_key_id: Option<i64>,
    pub project_id: i64,
    pub channel_id: Option<i64>,
    pub model_id: &'a str,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub prompt_audio_tokens: i64,
    pub prompt_cached_tokens: i64,
    pub prompt_write_cached_tokens: i64,
    pub prompt_write_cached_tokens_5m: i64,
    pub prompt_write_cached_tokens_1h: i64,
    pub completion_audio_tokens: i64,
    pub completion_reasoning_tokens: i64,
    pub completion_accepted_prediction_tokens: i64,
    pub completion_rejected_prediction_tokens: i64,
    pub source: &'a str,
    pub format: &'a str,
    pub total_cost: Option<f64>,
    pub cost_items_json: &'a str,
    pub cost_price_reference_id: &'a str,
}

pub struct UpdateRequestResultRecord<'a> {
    pub request_id: i64,
    pub status: &'a str,
    pub external_id: Option<&'a str>,
    pub response_body_json: Option<&'a str>,
    pub channel_id: Option<i64>,
}

pub struct UpdateRequestExecutionResultRecord<'a> {
    pub execution_id: i64,
    pub status: &'a str,
    pub external_id: Option<&'a str>,
    pub response_body_json: Option<&'a str>,
    pub response_status_code: Option<i64>,
    pub error_message: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ModelInclude {
    pub(crate) all: bool,
    pub(crate) fields: Vec<String>,
}

impl ModelInclude {
    pub(crate) fn parse(include: Option<&str>) -> Self {
        match include.map(str::trim).filter(|value| !value.is_empty()) {
            None => Self::default(),
            Some("all") => Self {
                all: true,
                fields: Vec::new(),
            },
            Some(raw) => Self {
                all: false,
                fields: raw
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
            },
        }
    }

    fn includes(&self, field: &str) -> bool {
        self.all || self.fields.iter().any(|current| current == field)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedModelCard {
    pub(crate) context_length: Option<i64>,
    pub(crate) max_output_tokens: Option<i64>,
    pub(crate) capabilities: Option<ModelCapabilities>,
    pub(crate) pricing: Option<ParsedModelPricing>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedModelPricing {
    pub(crate) input: f64,
    pub(crate) output: f64,
    pub(crate) cache_read: f64,
    pub(crate) cache_write: f64,
    pub(crate) cache_write_5m: Option<f64>,
    pub(crate) cache_write_1h: Option<f64>,
    pub(crate) price_reference_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DerivedChannelModelEntry {
    pub(crate) actual_model_id: String,
    pub(crate) source: DerivedChannelModelSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerivedChannelModelSource {
    Direct,
    Prefix,
    AutoTrim,
    Mapping,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ParsedChannelSettings {
    #[serde(rename = "extraModelPrefix")]
    pub(crate) extra_model_prefix: String,
    #[serde(rename = "autoTrimedModelPrefixes")]
    pub(crate) auto_trimed_model_prefixes: Vec<String>,
    #[serde(rename = "modelMappings")]
    pub(crate) model_mappings: Vec<ParsedChannelModelMapping>,
    #[serde(rename = "hideOriginalModels")]
    pub(crate) hide_original_models: bool,
    #[serde(rename = "hideMappedModels")]
    pub(crate) hide_mapped_models: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ParsedChannelModelMapping {
    pub(crate) from: String,
    pub(crate) to: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExtractedUsage {
    pub(crate) prompt_tokens: i64,
    pub(crate) completion_tokens: i64,
    pub(crate) total_tokens: i64,
    pub(crate) prompt_audio_tokens: i64,
    pub(crate) prompt_cached_tokens: i64,
    pub(crate) prompt_write_cached_tokens: i64,
    pub(crate) prompt_write_cached_tokens_5m: i64,
    pub(crate) prompt_write_cached_tokens_1h: i64,
    pub(crate) completion_audio_tokens: i64,
    pub(crate) completion_reasoning_tokens: i64,
    pub(crate) completion_accepted_prediction_tokens: i64,
    pub(crate) completion_rejected_prediction_tokens: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct StoredCostTier {
    #[serde(rename = "upTo", skip_serializing_if = "Option::is_none")]
    pub(crate) up_to: Option<i64>,
    pub(crate) units: i64,
    pub(crate) subtotal: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct StoredCostItem {
    #[serde(rename = "itemCode")]
    pub(crate) item_code: String,
    #[serde(
        rename = "promptWriteCacheVariantCode",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) prompt_write_cache_variant_code: Option<String>,
    pub(crate) quantity: i64,
    #[serde(rename = "tierBreakdown", skip_serializing_if = "Vec::is_empty")]
    pub(crate) tier_breakdown: Vec<StoredCostTier>,
    pub(crate) subtotal: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ComputedUsageCost {
    pub(crate) total_cost: Option<f64>,
    pub(crate) cost_items: Vec<StoredCostItem>,
    pub(crate) price_reference_id: Option<String>,
}

impl StoredModelRecord {
    pub(crate) fn into_openai_model(self, include: &ModelInclude) -> OpenAiModel {
        let parsed = parse_model_card(self.model_card_json.as_str());
        let created = parse_created_at_to_unix(self.created_at.as_str());

        OpenAiModel {
            id: self.model_id,
            object: "model",
            created,
            owned_by: self.developer,
            name: include.includes("name").then_some(self.name),
            description: include.includes("description").then_some(self.remark),
            icon: include.includes("icon").then_some(self.icon),
            r#type: include.includes("type").then_some(self.model_type),
            context_length: include
                .includes("context_length")
                .then_some(parsed.context_length)
                .flatten(),
            max_output_tokens: include
                .includes("max_output_tokens")
                .then_some(parsed.max_output_tokens)
                .flatten(),
            capabilities: include
                .includes("capabilities")
                .then_some(parsed.capabilities)
                .flatten(),
            pricing: include
                .includes("pricing")
                .then_some(parsed.pricing.map(|pricing| ModelPricing {
                    input: pricing.input,
                    output: pricing.output,
                    cache_read: pricing.cache_read,
                    cache_write: pricing.cache_write,
                    unit: "per_1m_tokens",
                    currency: "USD",
                }))
                .flatten(),
        }
    }
}

impl SelectedOpenAiTarget {
    pub(crate) fn upstream_url(&self, route: OpenAiV1Route) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        match route {
            OpenAiV1Route::ChatCompletions => format!("{trimmed}/chat/completions"),
            OpenAiV1Route::Responses => format!("{trimmed}/responses"),
            OpenAiV1Route::ResponsesCompact => format!("{trimmed}/responses/compact"),
            OpenAiV1Route::Embeddings => format!("{trimmed}/embeddings"),
            OpenAiV1Route::ImagesGenerations => format!("{trimmed}/images/generations"),
            OpenAiV1Route::ImagesEdits => format!("{trimmed}/images/edits"),
            OpenAiV1Route::ImagesVariations => format!("{trimmed}/images/variations"),
            OpenAiV1Route::Realtime => format!("{trimmed}/realtime"),
        }
    }

    fn base_routing_priority_key(&self) -> (i64, i64, i64, i64) {
        (
            if self.trace_affinity { 0 } else { 1 },
            self.circuit_breaker_penalty(),
            if self.routing_stats.last_status_failed {
                1
            } else {
                0
            },
            self.routing_stats.consecutive_failures,
        )
    }

    fn circuit_breaker_penalty(&self) -> i64 {
        self.circuit_breaker
            .as_ref()
            .map(|snapshot| match snapshot.state {
                CircuitBreakerState::Closed => 0,
                CircuitBreakerState::HalfOpen => 1,
                CircuitBreakerState::Open => 2,
            })
            .unwrap_or(0)
    }
}

#[allow(dead_code)]
pub(crate) fn stored_channel_breaker_status(
    status: ChannelBreakerStatus,
) -> Option<(String, String, i32, Option<i64>)> {
    let snapshot = status.active?;
    Some((
        snapshot.model_id,
        snapshot.state.as_str().to_owned(),
        i32::try_from(snapshot.consecutive_failures).unwrap_or(i32::MAX),
        snapshot.next_probe_in_seconds,
    ))
}

pub(crate) fn validate_openai_request(
    route: OpenAiV1Route,
    body: &OpenAiRequestBody,
) -> Result<(), OpenAiV1Error> {
    if matches!(route, OpenAiV1Route::ImagesEdits | OpenAiV1Route::ImagesVariations) {
        let _ = request_model_id(body)?;
        return validate_openai_image_multipart_request(route, body);
    }

    let body = body.as_json().ok_or_else(|| OpenAiV1Error::InvalidRequest {
        message: "Invalid request format".to_owned(),
    })?;
    let object = body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "Invalid request format".to_owned(),
        })?;

    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;

    let _ = model;

    match route {
        OpenAiV1Route::ChatCompletions => {
            if !object.get("messages").is_some_and(Value::is_array) {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "messages is required".to_owned(),
                });
            }
        }
        OpenAiV1Route::Responses => {
            if !object.contains_key("input") {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "input is required".to_owned(),
                });
            }
        }
        OpenAiV1Route::ResponsesCompact => {
            if !object.contains_key("input") {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "input is required".to_owned(),
                });
            }
        }
        OpenAiV1Route::Embeddings => {
            if !object.contains_key("input") {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "input is required".to_owned(),
                });
            }
        }
        OpenAiV1Route::Realtime => {
            if object
                .get("stream")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "realtime JSON POST does not support streaming".to_owned(),
                });
            }
        }
        OpenAiV1Route::ImagesGenerations => {
            if !object
                .get("prompt")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
            {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "prompt is required".to_owned(),
                });
            }

            if object
                .get("stream")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "image generation does not support streaming".to_owned(),
                });
            }
        }
        OpenAiV1Route::ImagesEdits | OpenAiV1Route::ImagesVariations => unreachable!(),
    }

    Ok(())
}

pub(crate) fn rewrite_model(body: &Value, actual_model_id: &str) -> Value {
    let mut rewritten = body.clone();
    if let Some(object) = rewritten.as_object_mut() {
        object.insert(
            "model".to_owned(),
            Value::String(actual_model_id.to_owned()),
        );
    }
    rewritten
}

pub(crate) fn rewrite_request_body(body: &OpenAiRequestBody, actual_model_id: &str) -> OpenAiRequestBody {
    match body {
        OpenAiRequestBody::Json(value) => OpenAiRequestBody::Json(rewrite_model(value, actual_model_id)),
        OpenAiRequestBody::Multipart(multipart) => {
            let fields = multipart
                .fields
                .iter()
                .map(|field| {
                    let mut rewritten = field.clone();
                    if rewritten.name == "model" {
                        rewritten.data = actual_model_id.as_bytes().to_vec();
                    }
                    rewritten
                })
                .collect::<Vec<_>>();
            OpenAiRequestBody::Multipart(OpenAiMultipartBody {
                content_type: multipart.content_type.clone(),
                fields,
            })
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedOutboundRequest {
    pub(crate) body: OpenAiRequestBody,
    pub(crate) body_json: String,
    pub(crate) headers: HeaderMap,
}

pub(crate) fn prepare_outbound_request(
    original_body: &OpenAiRequestBody,
    original_headers: &HashMap<String, String>,
    actual_model_id: &str,
    api_key: &str,
) -> Result<PreparedOutboundRequest, OpenAiV1Error> {
    let body = rewrite_request_body(original_body, actual_model_id);
    let body_json = serde_json::to_string(&body).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to serialize upstream request body: {error}"),
    })?;
    let headers = build_upstream_headers(original_headers, api_key)?;

    Ok(PreparedOutboundRequest {
        body,
        body_json,
        headers,
    })
}

pub(crate) async fn prepare_outbound_request_with_prompt_protection(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    original_body: &OpenAiRequestBody,
    original_headers: &HashMap<String, String>,
    actual_model_id: &str,
    api_key: &str,
) -> Result<PreparedOutboundRequest, OpenAiV1Error> {
    let rewritten = rewrite_request_body(original_body, actual_model_id);
    let rules = load_enabled_prompt_protection_rules_seaorm(db, backend).await?;
    let protected = apply_prompt_protection(&rewritten, &rules)?;
    prepare_outbound_request(&protected, original_headers, actual_model_id, api_key)
}

pub(crate) fn sanitize_headers_json(headers: &HashMap<String, String>) -> String {
    let mut sanitized = BTreeMap::new();
    for (key, value) in headers {
        let is_sensitive = matches!(
            key.to_ascii_lowercase().as_str(),
            "authorization" | "x-api-key" | "api-key" | "x-goog-api-key" | "x-google-api-key"
        );
        sanitized.insert(
            key.clone(),
            if is_sensitive {
                "[REDACTED]".to_owned()
            } else {
                value.clone()
            },
        );
    }

    serde_json::to_string(&sanitized).unwrap_or_else(|_| "{}".to_owned())
}

pub(crate) fn build_upstream_headers(
    original_headers: &HashMap<String, String>,
    api_key: &str,
) -> Result<HeaderMap, OpenAiV1Error> {
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        HeaderValue::from_str(format!("Bearer {api_key}").as_str()).map_err(|error| {
            OpenAiV1Error::Internal {
                message: format!("Invalid upstream authorization header: {error}"),
            }
        })?,
    );
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        reqwest::header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );

    for forwarded in ["AH-Trace-Id", "AH-Thread-Id", "X-Request-Id"] {
        if let Some(value) = original_headers.get(forwarded) {
            let name = HeaderName::from_bytes(forwarded.as_bytes()).map_err(|error| {
                OpenAiV1Error::Internal {
                    message: format!("Invalid forwarded header name: {error}"),
                }
            })?;
            let value = HeaderValue::from_str(value).map_err(|error| OpenAiV1Error::Internal {
                message: format!("Invalid forwarded header value: {error}"),
            })?;
            headers.insert(name, value);
        }
    }

    let propagator = TraceContextPropagator::new();
    let current_context = tracing::Span::current().context();
    propagator.inject_context(&current_context, &mut HeaderInjector(&mut headers));

    Ok(headers)
}

struct HeaderInjector<'a>(&'a mut HeaderMap);

impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(value.as_str()),
        ) {
            self.0.insert(name, value);
        }
    }
}

pub(crate) fn apply_upstream_request_body(
    request: reqwest::RequestBuilder,
    body: &OpenAiRequestBody,
) -> Result<reqwest::RequestBuilder, OpenAiV1Error> {
    match body {
        OpenAiRequestBody::Json(value) => Ok(request.json(value)),
        OpenAiRequestBody::Multipart(multipart) => {
            let mut form = reqwest::multipart::Form::new();
            for field in &multipart.fields {
                let base_part = reqwest::multipart::Part::bytes(field.data.clone());
                let with_name = match &field.file_name {
                    Some(file_name) => base_part.file_name(file_name.clone()),
                    None => base_part,
                };
                let part = if let Some(content_type) = &field.content_type {
                    with_name.mime_str(content_type).map_err(|error| OpenAiV1Error::InvalidRequest {
                        message: format!("Invalid image payload: {error}"),
                    })?
                } else {
                    with_name
                };
                form = form.part(field.name.clone(), part);
            }
            Ok(request.multipart(form))
        }
    }
}

#[allow(dead_code)]
pub(crate) fn apply_blocking_upstream_request_body(
    request: reqwest::blocking::RequestBuilder,
    body: &OpenAiRequestBody,
) -> Result<reqwest::blocking::RequestBuilder, OpenAiV1Error> {
    match body {
        OpenAiRequestBody::Json(value) => Ok(request.json(value)),
        OpenAiRequestBody::Multipart(multipart) => {
            let mut form = reqwest::blocking::multipart::Form::new();
            for field in &multipart.fields {
                let base_part = reqwest::blocking::multipart::Part::bytes(field.data.clone());
                let with_name = match &field.file_name {
                    Some(file_name) => base_part.file_name(file_name.clone()),
                    None => base_part,
                };
                let part = if let Some(content_type) = &field.content_type {
                    with_name.mime_str(content_type).map_err(|error| OpenAiV1Error::InvalidRequest {
                        message: format!("Invalid image payload: {error}"),
                    })?
                } else {
                    with_name
                };
                form = form.part(field.name.clone(), part);
            }
            Ok(request.multipart(form))
        }
    }
}

pub(crate) fn request_model_id(body: &OpenAiRequestBody) -> Result<String, OpenAiV1Error> {
    match body {
        OpenAiRequestBody::Json(value) => value
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                message: "model is required".to_owned(),
            }),
        OpenAiRequestBody::Multipart(multipart) => multipart
            .fields
            .iter()
            .find(|field| field.name == "model")
            .and_then(|field| String::from_utf8(field.data.clone()).ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                message: "model is required".to_owned(),
            }),
    }
}

pub(crate) fn json_request_body(body: &OpenAiRequestBody) -> Result<&Value, OpenAiV1Error> {
    body.as_json().ok_or_else(|| OpenAiV1Error::InvalidRequest {
        message: "Invalid request format".to_owned(),
    })
}

fn validate_openai_image_multipart_request(
    route: OpenAiV1Route,
    body: &OpenAiRequestBody,
) -> Result<(), OpenAiV1Error> {
    let multipart = match body {
        OpenAiRequestBody::Multipart(multipart) => multipart,
        OpenAiRequestBody::Json(_) => {
            return Err(OpenAiV1Error::InvalidRequest {
                message: "Invalid request format".to_owned(),
            })
        }
    };

    let image_fields = multipart
        .fields
        .iter()
        .filter(|field| field.name == "image" || field.name == "image[]")
        .collect::<Vec<_>>();
    if image_fields.is_empty() {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "image is required".to_owned(),
        });
    }

    if matches!(route, OpenAiV1Route::ImagesVariations) && image_fields.len() != 1 {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "image variations require exactly one image".to_owned(),
        });
    }

    if matches!(route, OpenAiV1Route::ImagesEdits)
        && !multipart.fields.iter().any(|field| field.name == "prompt" && !field.data.is_empty())
    {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "prompt is required".to_owned(),
        });
    }

    if matches!(route, OpenAiV1Route::ImagesVariations)
        && multipart.fields.iter().any(|field| field.name == "prompt" && !field.data.is_empty())
    {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "prompt is not supported for image variations".to_owned(),
        });
    }

    Ok(())
}

pub(crate) fn json_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| value.get(*key))
}

pub(crate) fn json_i64_field(value: &Value, keys: &[&str]) -> Option<i64> {
    json_field(value, keys).and_then(Value::as_i64)
}

pub(crate) fn json_f64_field(value: &Value, keys: &[&str]) -> Option<f64> {
    json_field(value, keys).and_then(Value::as_f64)
}

pub(crate) fn json_bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    json_field(value, keys).and_then(Value::as_bool)
}

pub(crate) fn json_string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    json_field(value, keys).and_then(Value::as_str)
}

pub(crate) fn extract_usage(route: OpenAiV1Route, response_body: &Value) -> Option<ExtractedUsage> {
    let usage = response_body.get("usage")?;
    match route {
        OpenAiV1Route::Responses | OpenAiV1Route::ResponsesCompact | OpenAiV1Route::Realtime => {
            let empty = Value::Null;
            let prompt_details = json_field(usage, &["input_tokens_details"]).unwrap_or(&empty);
            let completion_details =
                json_field(usage, &["output_tokens_details"]).unwrap_or(&empty);
            let prompt_write_cached_tokens_5m = json_i64_field(
                prompt_details,
                &["write_cached_5min_tokens", "write_cached_5m_tokens"],
            )
            .unwrap_or(0);
            let prompt_write_cached_tokens_1h = json_i64_field(
                prompt_details,
                &["write_cached_1hour_tokens", "write_cached_1h_tokens"],
            )
            .unwrap_or(0);

            Some(ExtractedUsage {
                prompt_tokens: json_i64_field(usage, &["input_tokens"]).unwrap_or(0),
                completion_tokens: json_i64_field(usage, &["output_tokens"]).unwrap_or(0),
                total_tokens: json_i64_field(usage, &["total_tokens"]).unwrap_or(0),
                prompt_audio_tokens: json_i64_field(prompt_details, &["audio_tokens"]).unwrap_or(0),
                prompt_cached_tokens: json_i64_field(prompt_details, &["cached_tokens"])
                    .unwrap_or(0),
                prompt_write_cached_tokens: json_i64_field(
                    prompt_details,
                    &["write_cached_tokens"],
                )
                .unwrap_or(prompt_write_cached_tokens_5m + prompt_write_cached_tokens_1h),
                prompt_write_cached_tokens_5m,
                prompt_write_cached_tokens_1h,
                completion_audio_tokens: json_i64_field(completion_details, &["audio_tokens"])
                    .unwrap_or(0),
                completion_reasoning_tokens: json_i64_field(
                    completion_details,
                    &["reasoning_tokens"],
                )
                .unwrap_or(0),
                completion_accepted_prediction_tokens: json_i64_field(
                    completion_details,
                    &["accepted_prediction_tokens"],
                )
                .unwrap_or(0),
                completion_rejected_prediction_tokens: json_i64_field(
                    completion_details,
                    &["rejected_prediction_tokens"],
                )
                .unwrap_or(0),
            })
        }
        OpenAiV1Route::ChatCompletions
        | OpenAiV1Route::Embeddings
        | OpenAiV1Route::ImagesGenerations
        | OpenAiV1Route::ImagesEdits
        | OpenAiV1Route::ImagesVariations => {
            let empty = Value::Null;
            let prompt_details = json_field(usage, &["prompt_tokens_details"]).unwrap_or(&empty);
            let completion_details =
                json_field(usage, &["completion_tokens_details"]).unwrap_or(&empty);
            let prompt_write_cached_tokens_5m = json_i64_field(
                prompt_details,
                &["write_cached_5min_tokens", "write_cached_5m_tokens"],
            )
            .unwrap_or(0);
            let prompt_write_cached_tokens_1h = json_i64_field(
                prompt_details,
                &["write_cached_1hour_tokens", "write_cached_1h_tokens"],
            )
            .unwrap_or(0);

            Some(ExtractedUsage {
                prompt_tokens: json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0),
                completion_tokens: json_i64_field(usage, &["completion_tokens"]).unwrap_or(0),
                total_tokens: json_i64_field(usage, &["total_tokens"]).unwrap_or(0),
                prompt_audio_tokens: json_i64_field(prompt_details, &["audio_tokens"]).unwrap_or(0),
                prompt_cached_tokens: json_i64_field(prompt_details, &["cached_tokens"])
                    .unwrap_or(0),
                prompt_write_cached_tokens: json_i64_field(
                    prompt_details,
                    &["write_cached_tokens"],
                )
                .unwrap_or(prompt_write_cached_tokens_5m + prompt_write_cached_tokens_1h),
                prompt_write_cached_tokens_5m,
                prompt_write_cached_tokens_1h,
                completion_audio_tokens: json_i64_field(completion_details, &["audio_tokens"])
                    .unwrap_or(0),
                completion_reasoning_tokens: json_i64_field(
                    completion_details,
                    &["reasoning_tokens"],
                )
                .unwrap_or(0),
                completion_accepted_prediction_tokens: json_i64_field(
                    completion_details,
                    &["accepted_prediction_tokens"],
                )
                .unwrap_or(0),
                completion_rejected_prediction_tokens: json_i64_field(
                    completion_details,
                    &["rejected_prediction_tokens"],
                )
                .unwrap_or(0),
            })
        }
    }
}

pub(crate) fn extract_jina_usage(response_body: &Value) -> Option<ExtractedUsage> {
    let usage = response_body.get("usage")?;
    Some(ExtractedUsage {
        prompt_tokens: json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0),
        total_tokens: json_i64_field(usage, &["total_tokens"]).unwrap_or(0),
        ..ExtractedUsage::default()
    })
}

pub(crate) fn compute_usage_cost(
    model: &StoredModelRecord,
    usage: &ExtractedUsage,
) -> ComputedUsageCost {
    let card = parse_model_card(model.model_card_json.as_str());
    let Some(pricing) = card.pricing else {
        return ComputedUsageCost::default();
    };

    let mut cost_items = Vec::new();
    let mut total_cost = 0.0;
    let prompt_tokens =
        (usage.prompt_tokens - usage.prompt_cached_tokens - usage.prompt_write_cached_tokens)
            .max(0);

    for (item_code, quantity, price, variant_code) in [
        ("prompt_tokens", prompt_tokens, pricing.input, None),
        (
            "completion_tokens",
            usage.completion_tokens,
            pricing.output,
            None,
        ),
        (
            "prompt_cached_tokens",
            usage.prompt_cached_tokens,
            pricing.cache_read,
            None,
        ),
    ] {
        if quantity <= 0 || price == 0.0 {
            continue;
        }

        let subtotal = (quantity as f64 / 1_000_000.0) * price;
        total_cost += subtotal;
        cost_items.push(StoredCostItem {
            item_code: item_code.to_owned(),
            prompt_write_cache_variant_code: variant_code.map(str::to_owned),
            quantity,
            tier_breakdown: Vec::new(),
            subtotal,
        });
    }

    if usage.prompt_write_cached_tokens_5m > 0 || usage.prompt_write_cached_tokens_1h > 0 {
        for (quantity, price, variant_code) in [
            (
                usage.prompt_write_cached_tokens_5m,
                pricing.cache_write_5m.unwrap_or(pricing.cache_write),
                Some("five_min"),
            ),
            (
                usage.prompt_write_cached_tokens_1h,
                pricing.cache_write_1h.unwrap_or(pricing.cache_write),
                Some("one_hour"),
            ),
        ] {
            if quantity <= 0 || price == 0.0 {
                continue;
            }

            let subtotal = (quantity as f64 / 1_000_000.0) * price;
            total_cost += subtotal;
            cost_items.push(StoredCostItem {
                item_code: "prompt_write_cached_tokens".to_owned(),
                prompt_write_cache_variant_code: variant_code.map(str::to_owned),
                quantity,
                tier_breakdown: Vec::new(),
                subtotal,
            });
        }
    } else if usage.prompt_write_cached_tokens > 0 && pricing.cache_write != 0.0 {
        let subtotal =
            (usage.prompt_write_cached_tokens as f64 / 1_000_000.0) * pricing.cache_write;
        total_cost += subtotal;
        cost_items.push(StoredCostItem {
            item_code: "prompt_write_cached_tokens".to_owned(),
            prompt_write_cache_variant_code: None,
            quantity: usage.prompt_write_cached_tokens,
            tier_breakdown: Vec::new(),
            subtotal,
        });
    }

    let total_cost = Some(total_cost);
    ComputedUsageCost {
        total_cost,
        cost_items,
        price_reference_id: Some(
            pricing
                .price_reference_id
                .unwrap_or_else(|| format!("sqlite:model:{}:{}", model.developer, model.model_id)),
        ),
    }
}

pub(crate) fn extract_error_message(body: &Value) -> String {
    body.get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            body.get("errors")
                .and_then(Value::as_array)
                .and_then(|errors| errors.first())
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "Upstream request failed".to_owned())
}

pub(crate) fn openai_error_message(error: &OpenAiV1Error) -> String {
    match error {
        OpenAiV1Error::InvalidRequest { message } | OpenAiV1Error::Internal { message } => {
            message.clone()
        }
        OpenAiV1Error::Upstream { body, .. } => extract_error_message(body),
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RouteSelector {
    Compatibility(CompatibilityRoute),
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedCompatibilityRequest {
    pub(crate) request_model_id: String,
    pub(crate) channel_type: &'static str,
    pub(crate) model_type: &'static str,
    pub(crate) upstream_body: Value,
    pub(crate) task_id: Option<String>,
}

pub(crate) fn route_model_type(route: RouteSelector) -> &'static str {
    match route {
        RouteSelector::Compatibility(CompatibilityRoute::JinaEmbeddings) => "embedding",
        RouteSelector::Compatibility(CompatibilityRoute::JinaRerank) => "rerank",
        RouteSelector::Compatibility(CompatibilityRoute::AnthropicMessages)
        | RouteSelector::Compatibility(CompatibilityRoute::GeminiGenerateContent)
        | RouteSelector::Compatibility(CompatibilityRoute::GeminiStreamGenerateContent) => "chat",
        RouteSelector::Compatibility(CompatibilityRoute::DoubaoCreateTask)
        | RouteSelector::Compatibility(CompatibilityRoute::DoubaoGetTask)
        | RouteSelector::Compatibility(CompatibilityRoute::DoubaoDeleteTask) => "video",
    }
}

pub(crate) fn prepare_compatibility_request(
    route: CompatibilityRoute,
    request: &OpenAiV1ExecutionRequest,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    match route {
        CompatibilityRoute::AnthropicMessages => prepare_anthropic_request(json_request_body(&request.body)?),
        CompatibilityRoute::JinaRerank => prepare_jina_rerank_request(json_request_body(&request.body)?),
        CompatibilityRoute::JinaEmbeddings => prepare_jina_embedding_request(json_request_body(&request.body)?),
        CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent => prepare_gemini_request(route, request),
        CompatibilityRoute::DoubaoCreateTask => prepare_doubao_create_request(json_request_body(&request.body)?),
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => {
            prepare_doubao_task_lookup_request(route, request)
        }
    }
}

pub(crate) fn prepare_anthropic_request(
    body: &Value,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let object = body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "Invalid request format".to_owned(),
        })?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;
    let max_tokens = object
        .get("max_tokens")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "max_tokens is required and must be positive".to_owned(),
        })?;
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "messages are required".to_owned(),
        })?;

    let mut openai_messages = Vec::new();
    if let Some(system) = object.get("system") {
        if let Some(system_message) = convert_anthropic_system_to_openai(system)? {
            openai_messages.push(system_message);
        }
    }
    for message in messages {
        openai_messages.push(convert_anthropic_message_to_openai(message)?);
    }

    let mut upstream = serde_json::Map::new();
    upstream.insert("model".to_owned(), Value::String(model.to_owned()));
    upstream.insert("messages".to_owned(), Value::Array(openai_messages));
    upstream.insert(
        "max_tokens".to_owned(),
        Value::Number(serde_json::Number::from(max_tokens)),
    );
    for field in ["temperature", "top_p", "stream", "metadata"] {
        if let Some(value) = object.get(field) {
            upstream.insert(field.to_owned(), value.clone());
        }
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model.to_owned(),
        channel_type: "openai",
        model_type: route_model_type(RouteSelector::Compatibility(
            CompatibilityRoute::AnthropicMessages,
        )),
        upstream_body: Value::Object(upstream),
        task_id: None,
    })
}

pub(crate) fn convert_anthropic_system_to_openai(
    system: &Value,
) -> Result<Option<Value>, OpenAiV1Error> {
    let content = if let Some(text) = system.as_str() {
        Some(Value::String(text.to_owned()))
    } else if let Some(parts) = system.as_array() {
        let content = convert_anthropic_content_parts(parts)?;
        if content.is_null() {
            None
        } else {
            Some(content)
        }
    } else if system.is_null() {
        None
    } else {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "system must be a string or array".to_owned(),
        });
    };

    Ok(content.map(|content| {
        serde_json::json!({
            "role": "system",
            "content": content,
        })
    }))
}

pub(crate) fn convert_anthropic_message_to_openai(message: &Value) -> Result<Value, OpenAiV1Error> {
    let object = message
        .as_object()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "message must be an object".to_owned(),
        })?;
    let role = object
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "message role is required".to_owned(),
        })?;
    let content_value = object
        .get("content")
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "message content is required".to_owned(),
        })?;
    let content = if let Some(text) = content_value.as_str() {
        Value::String(text.to_owned())
    } else if let Some(parts) = content_value.as_array() {
        convert_anthropic_content_parts(parts)?
    } else {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "message content must be a string or array".to_owned(),
        });
    };

    Ok(serde_json::json!({"role": role, "content": content}))
}

pub(crate) fn convert_anthropic_content_parts(parts: &[Value]) -> Result<Value, OpenAiV1Error> {
    let mut converted = Vec::new();
    for part in parts {
        let object = part
            .as_object()
            .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                message: "message content block must be an object".to_owned(),
            })?;
        let part_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match part_type {
            "text" => {
                let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                    OpenAiV1Error::InvalidRequest {
                        message: "text content block requires text".to_owned(),
                    }
                })?;
                converted.push(serde_json::json!({"type": "text", "text": text}));
            }
            "image" => {
                let source = object
                    .get("source")
                    .and_then(Value::as_object)
                    .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                        message: "image content block requires source".to_owned(),
                    })?;
                let image_url = match source.get("type").and_then(Value::as_str) {
                    Some("url") => source
                        .get("url")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    Some("base64") => {
                        let media_type = source
                            .get("media_type")
                            .and_then(Value::as_str)
                            .unwrap_or("application/octet-stream");
                        source
                            .get("data")
                            .and_then(Value::as_str)
                            .map(|data| format!("data:{media_type};base64,{data}"))
                    }
                    _ => None,
                }
                .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                    message: "unsupported image source".to_owned(),
                })?;
                converted.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": {"url": image_url},
                }));
            }
            unsupported => {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: format!("unsupported anthropic content block type: {unsupported}"),
                })
            }
        }
    }

    if converted.len() == 1 && converted[0].get("type") == Some(&Value::String("text".to_owned())) {
        Ok(converted[0]
            .get("text")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())))
    } else {
        Ok(Value::Array(converted))
    }
}

pub(crate) fn prepare_jina_rerank_request(
    body: &Value,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let object = body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "Invalid request format".to_owned(),
        })?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;
    if !object
        .get("query")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "query is required".to_owned(),
        });
    }
    if !object
        .get("documents")
        .and_then(Value::as_array)
        .is_some_and(|value| !value.is_empty())
    {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "documents are required".to_owned(),
        });
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model.to_owned(),
        channel_type: "jina",
        model_type: route_model_type(RouteSelector::Compatibility(CompatibilityRoute::JinaRerank)),
        upstream_body: body.clone(),
        task_id: None,
    })
}

pub(crate) fn prepare_jina_embedding_request(
    body: &Value,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let mut object = body
        .as_object()
        .cloned()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "Invalid request format".to_owned(),
        })?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;
    let input = object
        .get("input")
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "input is required".to_owned(),
        })?;
    validate_embedding_input(input)?;
    if !object.contains_key("task") {
        object.insert("task".to_owned(), Value::String("text-matching".to_owned()));
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model,
        channel_type: "jina",
        model_type: route_model_type(RouteSelector::Compatibility(
            CompatibilityRoute::JinaEmbeddings,
        )),
        upstream_body: Value::Object(object),
        task_id: None,
    })
}

pub(crate) fn validate_embedding_input(input: &Value) -> Result<(), OpenAiV1Error> {
    match input {
        Value::String(text) if text.trim().is_empty() => Err(OpenAiV1Error::InvalidRequest {
            message: "input cannot be empty string".to_owned(),
        }),
        Value::String(_) => Ok(()),
        Value::Array(values) if values.is_empty() => Err(OpenAiV1Error::InvalidRequest {
            message: "input cannot be empty array".to_owned(),
        }),
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                match value {
                    Value::String(text) if text.trim().is_empty() => {
                        return Err(OpenAiV1Error::InvalidRequest {
                            message: format!("input[{index}] cannot be empty string"),
                        })
                    }
                    Value::Array(inner) if inner.is_empty() => {
                        return Err(OpenAiV1Error::InvalidRequest {
                            message: format!("input[{index}] cannot be empty array"),
                        })
                    }
                    Value::String(_) | Value::Number(_) | Value::Array(_) => {}
                    _ => {
                        return Err(OpenAiV1Error::InvalidRequest {
                            message: "input must be a string, token array, or array of inputs"
                                .to_owned(),
                        })
                    }
                }
            }
            Ok(())
        }
        _ => Err(OpenAiV1Error::InvalidRequest {
            message: "input must be a string, token array, or array of inputs".to_owned(),
        }),
    }
}

pub(crate) fn compatibility_upstream_url(
    target: &SelectedOpenAiTarget,
    route: CompatibilityRoute,
    task_id: Option<&str>,
) -> String {
    let trimmed = target.base_url.trim_end_matches('/');
    match route {
        CompatibilityRoute::AnthropicMessages => format!("{trimmed}/chat/completions"),
        CompatibilityRoute::JinaRerank => format!("{trimmed}/rerank"),
        CompatibilityRoute::JinaEmbeddings => format!("{trimmed}/embeddings"),
        CompatibilityRoute::GeminiGenerateContent => format!("{trimmed}/chat/completions"),
        CompatibilityRoute::GeminiStreamGenerateContent => format!("{trimmed}/chat/completions"),
        CompatibilityRoute::DoubaoCreateTask => format!("{trimmed}/videos"),
        CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask => {
            format!("{trimmed}/videos/{}", task_id.unwrap_or_default())
        }
    }
}

pub(crate) fn compatibility_upstream_method(route: CompatibilityRoute) -> reqwest::Method {
    match route {
        CompatibilityRoute::AnthropicMessages
        | CompatibilityRoute::JinaRerank
        | CompatibilityRoute::JinaEmbeddings
        | CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent
        | CompatibilityRoute::DoubaoCreateTask => reqwest::Method::POST,
        CompatibilityRoute::DoubaoGetTask => reqwest::Method::GET,
        CompatibilityRoute::DoubaoDeleteTask => reqwest::Method::DELETE,
    }
}

pub(crate) fn map_compatibility_response(
    route: CompatibilityRoute,
    response_body: Value,
) -> Result<Value, OpenAiV1Error> {
    match route {
        CompatibilityRoute::AnthropicMessages => map_anthropic_response(response_body),
        CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => Ok(response_body),
        CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent => map_gemini_response(response_body),
        CompatibilityRoute::DoubaoCreateTask => map_doubao_create_response(response_body),
        CompatibilityRoute::DoubaoGetTask => map_doubao_get_response(response_body),
        CompatibilityRoute::DoubaoDeleteTask => Ok(Value::Null),
    }
}

pub(crate) fn compatibility_usage(
    route: CompatibilityRoute,
    response_body: &Value,
) -> Option<ExtractedUsage> {
    match route {
        CompatibilityRoute::AnthropicMessages => {
            extract_usage(OpenAiV1Route::ChatCompletions, response_body)
        }
        CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => {
            extract_jina_usage(response_body)
        }
        CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent => {
            extract_usage(OpenAiV1Route::ChatCompletions, response_body)
        }
        CompatibilityRoute::DoubaoCreateTask
        | CompatibilityRoute::DoubaoGetTask
        | CompatibilityRoute::DoubaoDeleteTask => None,
    }
}

pub(crate) fn prepare_gemini_request(
    route: CompatibilityRoute,
    request: &OpenAiV1ExecutionRequest,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let body = json_request_body(&request.body)?;
    let object = body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "Invalid request format".to_owned(),
        })?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| extract_gemini_model_from_path(request.path.as_str()))
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;
    let contents = object
        .get("contents")
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "contents are required".to_owned(),
        })?;

    let mut messages = Vec::new();
    if let Some(system_instruction) = object.get("systemInstruction") {
        if let Some(system_text) = flatten_gemini_parts(system_instruction) {
            messages.push(serde_json::json!({"role":"system","content":system_text}));
        }
    }
    for content in contents {
        let role = content
            .get("role")
            .and_then(Value::as_str)
            .map(|role| if role == "model" { "assistant" } else { "user" })
            .unwrap_or("user");
        let text = flatten_gemini_parts(content).ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "only text Gemini contents are supported".to_owned(),
        })?;
        messages.push(serde_json::json!({"role":role,"content":text}));
    }

    let mut upstream = serde_json::Map::new();
    upstream.insert("model".to_owned(), Value::String(model.to_owned()));
    upstream.insert("messages".to_owned(), Value::Array(messages));
    if route == CompatibilityRoute::GeminiStreamGenerateContent {
        upstream.insert("stream".to_owned(), Value::Bool(true));
    }

    if let Some(generation_config) = object.get("generationConfig").and_then(Value::as_object) {
        copy_json_field(generation_config, &mut upstream, "temperature");
        copy_json_field(generation_config, &mut upstream, "topP");
        copy_json_field_as(
            generation_config,
            &mut upstream,
            "maxOutputTokens",
            "max_tokens",
        );
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model.to_owned(),
        channel_type: "openai",
        model_type: route_model_type(RouteSelector::Compatibility(route)),
        upstream_body: Value::Object(upstream),
        task_id: None,
    })
}

pub(crate) fn prepare_doubao_create_request(
    body: &Value,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let object = body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "Invalid request format".to_owned(),
        })?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;
    let content = object
        .get("content")
        .and_then(Value::as_array)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "content is required".to_owned(),
        })?;

    let prompt = content
        .iter()
        .find_map(|item| {
            item.as_object()
                .filter(|object| object.get("type").and_then(Value::as_str) == Some("text"))
                .and_then(|object| object.get("text").and_then(Value::as_str))
        })
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "content must include a text prompt".to_owned(),
        })?;

    let mut upstream = serde_json::Map::new();
    upstream.insert("model".to_owned(), Value::String(model.to_owned()));
    upstream.insert("prompt".to_owned(), Value::String(prompt.to_owned()));
    if let Some(duration) = object.get("duration") {
        upstream.insert("duration".to_owned(), duration.clone());
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model.to_owned(),
        channel_type: "openai",
        model_type: route_model_type(RouteSelector::Compatibility(
            CompatibilityRoute::DoubaoCreateTask,
        )),
        upstream_body: Value::Object(upstream),
        task_id: None,
    })
}

pub(crate) fn prepare_doubao_task_lookup_request(
    route: CompatibilityRoute,
    request: &OpenAiV1ExecutionRequest,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let task_id = if let Some(task_id) = request.path_params.get("id") {
        task_id.clone()
    } else if let Some(task_id) = extract_task_id_from_path(request.path.as_str()) {
        task_id
    } else {
        return Err(OpenAiV1Error::InvalidRequest {
            message: "task id is required".to_owned(),
        });
    };

    Ok(PreparedCompatibilityRequest {
        request_model_id: "seedance-1.0".to_owned(),
        channel_type: "openai",
        model_type: route_model_type(RouteSelector::Compatibility(route)),
        upstream_body: Value::Null,
        task_id: Some(task_id),
    })
}

pub(crate) fn extract_gemini_model_from_path(path: &str) -> Option<&str> {
    let marker = "/models/";
    let after = path.split(marker).nth(1)?;
    let model = after.split(':').next()?.trim();
    (!model.is_empty()).then_some(model)
}

pub(crate) fn extract_task_id_from_path(path: &str) -> Option<String> {
    path.rsplit('/')
        .next()
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && *segment != "tasks")
        .map(ToOwned::to_owned)
}

pub(crate) fn flatten_gemini_parts(content: &Value) -> Option<String> {
    let parts = content.get("parts")?.as_array()?;
    let texts = parts
        .iter()
        .map(|part| part.get("text").and_then(Value::as_str).map(str::trim))
        .collect::<Option<Vec<_>>>()?;
    let joined = texts
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!joined.is_empty()).then_some(joined)
}

pub(crate) fn copy_json_field(
    source: &serde_json::Map<String, Value>,
    target: &mut serde_json::Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

pub(crate) fn copy_json_field_as(
    source: &serde_json::Map<String, Value>,
    target: &mut serde_json::Map<String, Value>,
    source_key: &str,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key) {
        target.insert(target_key.to_owned(), value.clone());
    }
}

pub(crate) fn map_gemini_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let object = response_body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::Internal {
            message: "Gemini wrapper expected object response".to_owned(),
        })?;
    let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let content = object
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let finish_reason = object
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .unwrap_or("STOP");

    Ok(serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": content}],
            },
            "finishReason": map_openai_finish_reason_to_gemini(finish_reason),
            "index": 0,
        }],
        "usageMetadata": map_gemini_usage_from_openai(object.get("usage")),
        "modelVersion": model,
        "responseId": id,
    }))
}

pub(crate) fn map_doubao_create_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let id = response_body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(serde_json::json!({"id": id}))
}

pub(crate) fn map_doubao_get_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let object = response_body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::Internal {
            message: "Doubao wrapper expected object response".to_owned(),
        })?;
    Ok(serde_json::json!({
        "id": object.get("id").cloned().unwrap_or(Value::String(String::new())),
        "model": object.get("model").cloned().unwrap_or(Value::String(String::new())),
        "status": object.get("status").cloned().unwrap_or(Value::String("queued".to_owned())),
        "content": object.get("content").cloned().unwrap_or(Value::Null),
        "usage": object.get("usage").cloned().unwrap_or(Value::Null),
        "created_at": object.get("created_at").cloned().unwrap_or(Value::from(0)),
        "updated_at": object.get("completed_at").cloned().or_else(|| object.get("updated_at").cloned()).unwrap_or(Value::from(0)),
        "seed": object.get("seed").cloned().unwrap_or(Value::Null),
        "resolution": object.get("resolution").cloned().unwrap_or(Value::String(String::new())),
        "ratio": object.get("ratio").cloned().unwrap_or(Value::String(String::new())),
        "duration": object.get("duration").cloned().unwrap_or(Value::Null),
        "framespersecond": object.get("fps").cloned().unwrap_or(Value::Null),
        "service_tier": object.get("service_tier").cloned().unwrap_or(Value::String(String::new())),
    }))
}

pub(crate) fn map_openai_finish_reason_to_gemini(reason: &str) -> &'static str {
    match reason {
        "stop" => "STOP",
        "length" => "MAX_TOKENS",
        "tool_calls" => "STOP",
        _ => "STOP",
    }
}

pub(crate) fn map_gemini_usage_from_openai(usage: Option<&Value>) -> Value {
    let Some(usage) = usage else {
        return Value::Null;
    };

    let prompt_tokens = json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0);
    let completion_tokens = json_i64_field(usage, &["completion_tokens"]).unwrap_or(0);
    let total_tokens =
        json_i64_field(usage, &["total_tokens"]).unwrap_or(prompt_tokens + completion_tokens);
    let prompt_details = usage
        .get("prompt_tokens_details")
        .cloned()
        .unwrap_or(Value::Null);
    let cached_tokens = json_i64_field(&prompt_details, &["cached_tokens"]).unwrap_or(0);
    let reasoning_tokens = usage
        .get("completion_tokens_details")
        .and_then(|details| json_i64_field(details, &["reasoning_tokens"]))
        .unwrap_or(0);

    serde_json::json!({
        "promptTokenCount": prompt_tokens,
        "candidatesTokenCount": completion_tokens,
        "totalTokenCount": total_tokens,
        "cachedContentTokenCount": cached_tokens,
        "thoughtsTokenCount": reasoning_tokens,
    })
}

pub(crate) fn map_anthropic_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let object = response_body
        .as_object()
        .ok_or_else(|| OpenAiV1Error::Internal {
            message: "Anthropic wrapper expected object response".to_owned(),
        })?
        .clone();
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let choices = object
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| OpenAiV1Error::Internal {
            message: "Anthropic wrapper expected OpenAI choices array".to_owned(),
        })?;
    let message = choices
        .first()
        .and_then(|choice| choice.get("message").or_else(|| choice.get("delta")))
        .ok_or_else(|| OpenAiV1Error::Internal {
            message: "Anthropic wrapper expected assistant message".to_owned(),
        })?;
    let content = map_anthropic_response_content(message.get("content"))?;
    let stop_reason = choices
        .first()
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .map(map_openai_finish_reason_to_anthropic);
    let usage = object
        .get("usage")
        .map(map_anthropic_usage_from_openai)
        .transpose()?;

    let mut anthropic = serde_json::Map::new();
    anthropic.insert("id".to_owned(), Value::String(id));
    anthropic.insert("type".to_owned(), Value::String("message".to_owned()));
    anthropic.insert("role".to_owned(), Value::String("assistant".to_owned()));
    anthropic.insert("content".to_owned(), Value::Array(content));
    anthropic.insert("model".to_owned(), Value::String(model));
    if let Some(stop_reason) = stop_reason {
        anthropic.insert("stop_reason".to_owned(), Value::String(stop_reason));
    }
    if let Some(usage) = usage {
        anthropic.insert("usage".to_owned(), usage);
    }

    Ok(Value::Object(anthropic))
}

pub(crate) fn map_anthropic_response_content(
    content: Option<&Value>,
) -> Result<Vec<Value>, OpenAiV1Error> {
    let Some(content) = content else {
        return Ok(Vec::new());
    };
    if let Some(text) = content.as_str() {
        return Ok(vec![serde_json::json!({"type": "text", "text": text})]);
    }
    if let Some(parts) = content.as_array() {
        let mut blocks = Vec::new();
        for part in parts {
            let Some(object) = part.as_object() else {
                continue;
            };
            match object.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = object.get("text").and_then(Value::as_str) {
                        blocks.push(serde_json::json!({"type": "text", "text": text}));
                    }
                }
                Some("image_url") => {
                    if let Some(url) = object
                        .get("image_url")
                        .and_then(|value| value.get("url"))
                        .and_then(Value::as_str)
                    {
                        let source = if let Some((media_type, data)) = parse_data_url(url) {
                            serde_json::json!({"type": "base64", "media_type": media_type, "data": data})
                        } else {
                            serde_json::json!({"type": "url", "url": url})
                        };
                        blocks.push(serde_json::json!({"type": "image", "source": source}));
                    }
                }
                _ => {}
            }
        }
        return Ok(blocks);
    }

    Err(OpenAiV1Error::Internal {
        message: "Anthropic wrapper expected string or array content".to_owned(),
    })
}

pub(crate) fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (metadata, data) = rest.split_once(',')?;
    let media_type = metadata.strip_suffix(";base64")?;
    Some((media_type.to_owned(), data.to_owned()))
}

pub(crate) fn map_openai_finish_reason_to_anthropic(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_owned(),
        "length" => "max_tokens".to_owned(),
        "tool_calls" => "tool_use".to_owned(),
        other => other.to_owned(),
    }
}

pub(crate) fn map_anthropic_usage_from_openai(usage: &Value) -> Result<Value, OpenAiV1Error> {
    let prompt_tokens = json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0);
    let completion_tokens = json_i64_field(usage, &["completion_tokens"]).unwrap_or(0);
    let prompt_details = usage
        .get("prompt_tokens_details")
        .cloned()
        .unwrap_or(Value::Null);
    let cached_tokens = json_i64_field(&prompt_details, &["cached_tokens"]).unwrap_or(0);
    let write_cached_tokens =
        json_i64_field(&prompt_details, &["write_cached_tokens"]).unwrap_or(0);
    let write_cached_5m = json_i64_field(
        &prompt_details,
        &["write_cached_5min_tokens", "write_cached_5m_tokens"],
    )
    .unwrap_or(0);
    let write_cached_1h = json_i64_field(
        &prompt_details,
        &["write_cached_1hour_tokens", "write_cached_1h_tokens"],
    )
    .unwrap_or(0);

    Ok(serde_json::json!({
        "input_tokens": (prompt_tokens - cached_tokens - write_cached_tokens).max(0),
        "output_tokens": completion_tokens,
        "cache_creation_input_tokens": write_cached_tokens,
        "cache_read_input_tokens": cached_tokens,
        "cache_creation": {
            "ephemeral_5m_input_tokens": write_cached_5m,
            "ephemeral_1h_input_tokens": write_cached_1h,
        }
    }))
}

pub(crate) fn sqlite_timestamp_to_rfc3339(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "1970-01-01T00:00:00Z".to_owned();
    }
    if trimmed.contains('T') {
        if trimmed.ends_with('Z') || trimmed.contains('+') {
            trimmed.to_owned()
        } else {
            format!("{trimmed}Z")
        }
    } else {
        format!("{}Z", trimmed.replace(' ', "T"))
    }
}

pub(crate) fn parse_model_card(raw: &str) -> ParsedModelCard {
    let value = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    let empty = Value::Null;
    let limit = json_field(&value, &["limit"]).unwrap_or(&empty);
    let reasoning = json_field(&value, &["reasoning"]).unwrap_or(&empty);
    let cost = json_field(&value, &["cost", "pricing"]).unwrap_or(&empty);

    ParsedModelCard {
        context_length: json_i64_field(limit, &["context", "contextLength"]),
        max_output_tokens: json_i64_field(limit, &["output", "maxOutputTokens"]),
        capabilities: value.get("vision").map(|_| ModelCapabilities {
            vision: json_bool_field(&value, &["vision"]).unwrap_or(false),
            tool_call: json_bool_field(&value, &["tool_call", "toolCall"]).unwrap_or(false),
            reasoning: json_bool_field(reasoning, &["supported"]).unwrap_or(false),
        }),
        pricing: json_field(&value, &["cost", "pricing"]).map(|_| ParsedModelPricing {
            input: json_f64_field(cost, &["input"]).unwrap_or(0.0),
            output: json_f64_field(cost, &["output"]).unwrap_or(0.0),
            cache_read: json_f64_field(cost, &["cache_read", "cacheRead"]).unwrap_or(0.0),
            cache_write: json_f64_field(cost, &["cache_write", "cacheWrite"]).unwrap_or(0.0),
            cache_write_5m: json_f64_field(
                cost,
                &[
                    "cache_write_5m",
                    "cacheWrite5m",
                    "cache_write_five_min",
                    "cacheWriteFiveMin",
                ],
            ),
            cache_write_1h: json_f64_field(
                cost,
                &[
                    "cache_write_1h",
                    "cacheWrite1h",
                    "cache_write_one_hour",
                    "cacheWriteOneHour",
                ],
            ),
            price_reference_id: json_string_field(
                cost,
                &[
                    "price_reference_id",
                    "priceReferenceId",
                    "reference_id",
                    "referenceId",
                ],
            )
            .or_else(|| {
                json_string_field(
                    &value,
                    &[
                        "cost_price_reference_id",
                        "costPriceReferenceId",
                        "price_reference_id",
                        "priceReferenceId",
                        "reference_id",
                        "referenceId",
                    ],
                )
            })
            .map(ToOwned::to_owned),
        }),
    }
}

pub(crate) fn parse_created_at_to_unix(raw: &str) -> i64 {
    let _ = raw;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[allow(dead_code)]
pub(crate) fn model_supported_by_channel(supported_models_json: &str, model_id: &str) -> bool {
    serde_json::from_str::<Vec<String>>(supported_models_json)
        .unwrap_or_default()
        .iter()
        .any(|current| current == model_id)
}

pub(crate) fn derive_channel_model_entries(
    supported_models_json: &str,
    settings_json: &str,
) -> BTreeMap<String, DerivedChannelModelEntry> {
    let supported_models = serde_json::from_str::<Vec<String>>(supported_models_json).unwrap_or_default();
    let settings = serde_json::from_str::<ParsedChannelSettings>(settings_json).unwrap_or_default();
    let mut entries = BTreeMap::new();

    for model_id in &supported_models {
        entries
            .entry(model_id.clone())
            .or_insert_with(|| DerivedChannelModelEntry {
                actual_model_id: model_id.clone(),
                source: DerivedChannelModelSource::Direct,
            });
    }

    let extra_model_prefix = settings.extra_model_prefix.trim();
    if !extra_model_prefix.is_empty() {
        for model_id in &supported_models {
            let request_model_id = format!("{extra_model_prefix}/{model_id}");
            entries
                .entry(request_model_id)
                .or_insert_with(|| DerivedChannelModelEntry {
                    actual_model_id: model_id.clone(),
                    source: DerivedChannelModelSource::Prefix,
                });
        }
    }

    for prefix in settings
        .auto_trimed_model_prefixes
        .iter()
        .map(|prefix| prefix.trim())
        .filter(|prefix| !prefix.is_empty())
    {
        let needle = format!("{prefix}/");
        for model_id in &supported_models {
            if let Some(trimmed_model_id) = model_id.strip_prefix(needle.as_str()) {
                entries
                    .entry(trimmed_model_id.to_owned())
                    .or_insert_with(|| DerivedChannelModelEntry {
                        actual_model_id: model_id.clone(),
                        source: DerivedChannelModelSource::AutoTrim,
                    });
            }
        }
    }

    for mapping in &settings.model_mappings {
        let request_model_id = mapping.from.trim();
        let actual_model_id = mapping.to.trim();
        if request_model_id.is_empty() || actual_model_id.is_empty() {
            continue;
        }
        if !supported_models
            .iter()
            .any(|supported_model_id| supported_model_id == actual_model_id)
        {
            continue;
        }

        if !entries.contains_key(request_model_id) {
            entries.insert(
                request_model_id.to_owned(),
                DerivedChannelModelEntry {
                    actual_model_id: actual_model_id.to_owned(),
                    source: DerivedChannelModelSource::Mapping,
                },
            );
            if settings.hide_mapped_models {
                entries.remove(actual_model_id);
            }
        }
    }

    if settings.hide_original_models {
        entries.retain(|_, entry| entry.source != DerivedChannelModelSource::Direct);
    }

    entries
}

pub(crate) fn resolve_channel_model_entry(
    supported_models_json: &str,
    settings_json: &str,
    request_model_id: &str,
) -> Option<DerivedChannelModelEntry> {
    derive_channel_model_entries(supported_models_json, settings_json)
        .get(request_model_id)
        .cloned()
}

pub(crate) fn calculate_top_k(candidate_count: usize, max_channel_retries: usize) -> usize {
    candidate_count.min(1 + max_channel_retries)
}

pub(crate) fn compare_openai_target_priority(
    left: &SelectedOpenAiTarget,
    right: &SelectedOpenAiTarget,
) -> std::cmp::Ordering {
    left.base_routing_priority_key()
        .cmp(&right.base_routing_priority_key())
        .then_with(|| {
            left.routing_stats
                .processing_count
                .cmp(&right.routing_stats.processing_count)
        })
        .then_with(|| compare_selection_pressure(left, right))
        .then_with(|| right.ordering_weight.cmp(&left.ordering_weight))
        .then_with(|| left.channel_id.cmp(&right.channel_id))
}

pub(crate) fn compare_selection_pressure(
    left: &SelectedOpenAiTarget,
    right: &SelectedOpenAiTarget,
) -> std::cmp::Ordering {
    let left_weight = std::cmp::max(left.ordering_weight, 1) as i128;
    let right_weight = std::cmp::max(right.ordering_weight, 1) as i128;
    let left_selection = left.routing_stats.selection_count as i128;
    let right_selection = right.routing_stats.selection_count as i128;

    (left_selection * right_weight)
        .cmp(&(right_selection * left_weight))
        .then_with(|| {
            left.routing_stats
                .selection_count
                .cmp(&right.routing_stats.selection_count)
        })
}

pub(crate) fn extract_channel_api_key(credentials_json: &str) -> String {
    let value = serde_json::from_str::<Value>(credentials_json).unwrap_or(Value::Null);
    value
        .get("apiKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("apiKeys")
                .and_then(Value::as_array)
                .and_then(|keys| keys.first())
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use std::sync::Arc;

    use axonhub_http::{
        AnthropicModelListResponse, AuthApiKeyContext, CompatibilityRoute,
        GeminiModelListResponse, ModelListResponse, OpenAiModel, OpenAiV1Error,
        OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Port, OpenAiV1Route,
        RealtimeSessionCreateRequest, RealtimeSessionPatchRequest, RealtimeSessionRecord,
    };

    use super::{
        super::{
            circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker},
            ports::OpenAiV1Repository,
            system::sqlite_test_support::SqliteFoundation,
        },
        SeaOrmOpenAiV1Service,
    };

    pub struct SqliteOpenAiV1Service {
        inner: SeaOrmOpenAiV1Service,
    }

    impl SqliteOpenAiV1Service {
        pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
            Self {
                inner: SeaOrmOpenAiV1Service::new(foundation.seaorm()),
            }
        }

        pub(crate) fn new_with_circuit_breaker(
            foundation: Arc<SqliteFoundation>,
            circuit_breaker: SharedCircuitBreaker,
        ) -> Self {
            Self {
                inner: SeaOrmOpenAiV1Service::new_with_circuit_breaker(
                    foundation.seaorm(),
                    circuit_breaker,
                ),
            }
        }

        pub(crate) fn new_with_circuit_breaker_policy(
            foundation: Arc<SqliteFoundation>,
            policy: CircuitBreakerPolicy,
        ) -> Self {
            Self {
                inner: SeaOrmOpenAiV1Service::new_with_circuit_breaker_policy(
                    foundation.seaorm(),
                    policy,
                ),
            }
        }
    }

    impl OpenAiV1Port for SqliteOpenAiV1Service {
        fn list_models(
            &self,
            include: Option<&str>,
            api_key: &AuthApiKeyContext,
        ) -> Result<ModelListResponse, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::list_models(&self.inner, include, api_key)
        }

        fn retrieve_model(
            &self,
            model_id: &str,
            include: Option<&str>,
            api_key: &AuthApiKeyContext,
        ) -> Result<OpenAiModel, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::retrieve_model(
                &self.inner,
                model_id,
                include,
                api_key,
            )
        }

        fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::list_anthropic_models(&self.inner)
        }

        fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::list_gemini_models(&self.inner)
        }

        fn execute(
            &self,
            route: OpenAiV1Route,
            request: OpenAiV1ExecutionRequest,
        ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::execute(&self.inner, route, request)
        }

        fn execute_compatibility(
            &self,
            route: CompatibilityRoute,
            request: OpenAiV1ExecutionRequest,
        ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::execute_compatibility(
                &self.inner,
                route,
                request,
            )
        }

        fn create_realtime_session(
            &self,
            request: RealtimeSessionCreateRequest,
        ) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::create_realtime_session(
                &self.inner,
                request,
            )
        }

        fn get_realtime_session(
            &self,
            session_id: &str,
        ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::get_realtime_session(&self.inner, session_id)
        }

        fn update_realtime_session(
            &self,
            session_id: &str,
            patch: RealtimeSessionPatchRequest,
        ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::update_realtime_session(
                &self.inner,
                session_id,
                patch,
            )
        }

        fn delete_realtime_session(
            &self,
            session_id: &str,
        ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
            <SeaOrmOpenAiV1Service as OpenAiV1Port>::delete_realtime_session(
                &self.inner,
                session_id,
            )
        }
    }

    impl OpenAiV1Repository for SqliteOpenAiV1Service {
        fn list_models(
            &self,
            include: Option<&str>,
            api_key: &AuthApiKeyContext,
        ) -> Result<ModelListResponse, OpenAiV1Error> {
            <Self as OpenAiV1Port>::list_models(self, include, api_key)
        }

        fn retrieve_model(
            &self,
            model_id: &str,
            include: Option<&str>,
            api_key: &AuthApiKeyContext,
        ) -> Result<OpenAiModel, OpenAiV1Error> {
            <Self as OpenAiV1Port>::retrieve_model(self, model_id, include, api_key)
        }

        fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
            <Self as OpenAiV1Port>::list_anthropic_models(self)
        }

        fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
            <Self as OpenAiV1Port>::list_gemini_models(self)
        }

        fn execute(
            &self,
            route: OpenAiV1Route,
            request: OpenAiV1ExecutionRequest,
        ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
            <Self as OpenAiV1Port>::execute(self, route, request)
        }

        fn execute_compatibility(
            &self,
            route: CompatibilityRoute,
            request: OpenAiV1ExecutionRequest,
        ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
            <Self as OpenAiV1Port>::execute_compatibility(self, route, request)
        }

        fn create_realtime_session(
            &self,
            request: RealtimeSessionCreateRequest,
        ) -> Result<RealtimeSessionRecord, OpenAiV1Error> {
            <Self as OpenAiV1Port>::create_realtime_session(self, request)
        }

        fn get_realtime_session(
            &self,
            session_id: &str,
        ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
            <Self as OpenAiV1Port>::get_realtime_session(self, session_id)
        }

        fn update_realtime_session(
            &self,
            session_id: &str,
            patch: RealtimeSessionPatchRequest,
        ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
            <Self as OpenAiV1Port>::update_realtime_session(self, session_id, patch)
        }

        fn delete_realtime_session(
            &self,
            session_id: &str,
        ) -> Result<Option<RealtimeSessionRecord>, OpenAiV1Error> {
            <Self as OpenAiV1Port>::delete_realtime_session(self, session_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
    use serde_json::json;
    use std::fmt;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    #[derive(Clone, Default)]
    struct RecordingSpanExporter {
        spans: Arc<Mutex<Vec<SpanData>>>,
    }

    impl fmt::Debug for RecordingSpanExporter {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("RecordingSpanExporter").finish()
        }
    }

    impl SpanExporter for RecordingSpanExporter {
        async fn export(&self, batch: Vec<SpanData>) -> opentelemetry_sdk::error::OTelSdkResult {
            self.spans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend(batch);
            Ok(())
        }
    }

    fn with_recorded_spans<T>(f: impl FnOnce(Arc<Mutex<Vec<SpanData>>>) -> T) -> T {
        let exporter = RecordingSpanExporter::default();
        let spans = exporter.spans.clone();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();
        let tracer = provider.tracer("openai-v1-tests");
        let subscriber = Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer));

        let result = tracing::subscriber::with_default(subscriber, || f(spans.clone()));
        let _ = provider.force_flush();
        let _ = provider.shutdown();
        result
    }

    fn span_attributes(span: &SpanData) -> BTreeMap<String, String> {
        span.attributes
            .iter()
            .map(|kv| (kv.key.as_str().to_owned(), kv.value.to_string()))
            .collect()
    }

    fn recorded_openai_execution_span(spans: &Arc<Mutex<Vec<SpanData>>>) -> SpanData {
        spans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|span| span.name.as_ref() == "openai.v1.execution")
            .cloned()
            .expect("openai execution span")
    }

    #[test]
    pub(crate) fn build_upstream_headers_injects_w3c_trace_headers() {
        let _guard = OPENAI_TRACE_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("openai-v1-tests");
        let subscriber = Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer));

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::span!(tracing::Level::INFO, "upstream-request");
            let _enter = span.enter();

            let headers = build_upstream_headers(
                &HashMap::from([
                    ("AH-Trace-Id".to_owned(), "trace-1".to_owned()),
                    ("AH-Thread-Id".to_owned(), "thread-1".to_owned()),
                    ("X-Request-Id".to_owned(), "req-1".to_owned()),
                ]),
                "secret-key",
            )
            .expect("headers");

            assert!(headers.contains_key("traceparent"));
            assert!(headers
                .get("traceparent")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("00-")));
            assert_eq!(
                headers.get("AH-Trace-Id").and_then(|value| value.to_str().ok()),
                Some("trace-1")
            );
            assert_eq!(
                headers.get("AH-Thread-Id").and_then(|value| value.to_str().ok()),
                Some("thread-1")
            );
            assert_eq!(
                headers.get("X-Request-Id").and_then(|value| value.to_str().ok()),
                Some("req-1")
            );
        });

        let _ = provider.force_flush();
        let _ = provider.shutdown();
    }

    #[test]
    pub(crate) fn openai_v1_execution_span_avoids_sensitive_fields() {
        let _guard = OPENAI_TRACE_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        with_recorded_spans(|spans| {
            let request = OpenAiV1ExecutionRequest {
                headers: HashMap::from([
                    (
                        "Authorization".to_owned(),
                        "Bearer inbound-secret-token".to_owned(),
                    ),
                    ("X-API-Key".to_owned(), "api-key-secret-header".to_owned()),
                    ("AH-Thread-Id".to_owned(), "thread-secret-42".to_owned()),
                    ("X-Request-Id".to_owned(), "req-secret-7".to_owned()),
                ]),
                body: OpenAiRequestBody::Json(json!({
                    "model": "gpt-4o",
                    "stream": true,
                    "messages": [{"role": "user", "content": "password=hunter2"}]
                })),
                path: "/v1/chat/completions".to_owned(),
                path_params: HashMap::new(),
                query: HashMap::new(),
                project: axonhub_http::ProjectContext {
                    id: 1,
                    name: "Default Project".to_owned(),
                    status: "active".to_owned(),
                },
                trace: Some(axonhub_http::TraceContext {
                    id: 8,
                    trace_id: "trace-secret-99".to_owned(),
                    project_id: 1,
                    thread_id: None,
                }),
                api_key: axonhub_http::AuthApiKeyContext {
                    id: 11,
                    key: "sk-secret-runtime-key".to_owned(),
                    name: "runtime key".to_owned(),
                    key_type: axonhub_http::ApiKeyType::User,
                    project: axonhub_http::ProjectContext {
                        id: 1,
                        name: "Default Project".to_owned(),
                        status: "active".to_owned(),
                    },
                    scopes: vec![SCOPE_WRITE_REQUESTS.as_str().to_owned()],
                    profiles_json: Some(r#"{"token":"nested-secret"}"#.to_owned()),
                },
                api_key_id: Some(11),
                client_ip: Some("127.0.0.1".to_owned()),
                channel_hint_id: Some(7),
            };
            let result = Err(OpenAiV1Error::Upstream {
                status: 503,
                body: json!({
                    "id": "upstream-secret-id",
                    "error": {"message": "response-secret-body"}
                }),
            });

            let span = openai_execution_span(
                "openai_v1.execute",
                "openai_v1",
                OpenAiV1Route::ChatCompletions.format(),
                &request,
            );
            {
                let _enter = span.enter();
                Span::current().record("target.selected_count", 1_i64);
                Span::current().record("retry.count", 2_i64);
                record_openai_execution_outcome(&span, &result);
            }
            drop(span);

            let span = recorded_openai_execution_span(&spans);
            let attributes = span_attributes(&span);
            assert_eq!(
                attributes.get("operation.name").map(String::as_str),
                Some("openai_v1.execute")
            );
            assert_eq!(attributes.get("route.family").map(String::as_str), Some("openai_v1"));
            assert_eq!(
                attributes.get("route.name").map(String::as_str),
                Some("openai/chat_completions")
            );
            assert_eq!(attributes.get("auth.mode").map(String::as_str), Some("api_key"));
            assert_eq!(
                attributes.get("auth.subject").map(String::as_str),
                Some("user_api_key")
            );
            assert_eq!(attributes.get("request.stream").map(String::as_str), Some("true"));
            assert_eq!(attributes.get("request.bound").map(String::as_str), Some("true"));
            assert_eq!(attributes.get("trace.bound").map(String::as_str), Some("true"));
            assert_eq!(attributes.get("thread.bound").map(String::as_str), Some("true"));
            assert_eq!(attributes.get("channel.hint").map(String::as_str), Some("true"));
            assert_eq!(
                attributes.get("target.selected_count").map(String::as_str),
                Some("1")
            );
            assert_eq!(attributes.get("retry.count").map(String::as_str), Some("2"));
            assert_eq!(
                attributes.get("request.outcome").map(String::as_str),
                Some("upstream_error")
            );
            assert_eq!(attributes.get("http.status_code").map(String::as_str), Some("503"));

            let rendered_attributes = attributes.values().cloned().collect::<Vec<_>>().join(" ");
            for forbidden_value in [
                "sk-secret-runtime-key",
                "Bearer inbound-secret-token",
                "api-key-secret-header",
                "password=hunter2",
                "response-secret-body",
                "upstream-secret-id",
                "thread-secret-42",
                "trace-secret-99",
                "req-secret-7",
                "nested-secret",
            ] {
                assert!(
                    !rendered_attributes.contains(forbidden_value),
                    "forbidden tracing value recorded: {forbidden_value}"
                );
            }

            for forbidden_key_fragment in [
                "authorization",
                "api_key",
                "request.body",
                "response.body",
                "request.headers",
                "credentials",
                "password",
                "password_hash",
            ] {
                assert!(
                    !attributes
                        .keys()
                        .any(|key| key.contains(forbidden_key_fragment)),
                    "forbidden tracing field recorded: {forbidden_key_fragment}"
                );
            }
        });
    }
}

#[cfg(test)]
static OPENAI_TRACE_TEST_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn build_upstream_headers_injects_w3c_trace_headers_inner() {
    tests::build_upstream_headers_injects_w3c_trace_headers();
}

#[cfg(test)]
pub(crate) fn openai_v1_execution_span_avoids_sensitive_fields_inner() {
    tests::openai_v1_execution_span_avoids_sensitive_fields();
}

#[cfg(test)]
#[test]
fn build_upstream_headers_injects_w3c_trace_headers() {
    build_upstream_headers_injects_w3c_trace_headers_inner();
}

#[cfg(test)]
#[test]
fn openai_v1_execution_span_avoids_sensitive_fields() {
    openai_v1_execution_span_avoids_sensitive_fields_inner();
}
