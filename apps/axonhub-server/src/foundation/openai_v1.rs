use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axonhub_http::{
    AnthropicModel, AnthropicModelListResponse, CompatibilityRoute, GeminiModel,
    GeminiModelListResponse, ModelCapabilities, ModelListResponse, ModelPricing, OpenAiModel,
    OpenAiV1Error, OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Port,
    OpenAiV1Route,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use super::{
    identity::parse_json_string_vec,
    shared::{bool_to_sql, SqliteConnectionFactory, SqliteFoundation, USAGE_LOGS_TABLE_SQL},
    system::{ensure_channel_model_tables, ensure_request_tables},
};

#[derive(Debug, Clone)]
pub struct ChannelModelStore {
    pub(crate) connection_factory: SqliteConnectionFactory,
}

impl ChannelModelStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)
    }

    #[cfg(test)]
    pub fn upsert_channel(&self, record: &NewChannelRecord<'_>) -> rusqlite::Result<i64> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;
        connection.execute(
            "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0)
             ON CONFLICT(name) DO UPDATE SET
                 type = excluded.type,
                 base_url = excluded.base_url,
                 status = excluded.status,
                 credentials = excluded.credentials,
                 supported_models = excluded.supported_models,
                 auto_sync_supported_models = excluded.auto_sync_supported_models,
                 default_test_model = excluded.default_test_model,
                 settings = excluded.settings,
                 tags = excluded.tags,
                 ordering_weight = excluded.ordering_weight,
                 error_message = excluded.error_message,
                 remark = excluded.remark,
                 deleted_at = 0,
                 updated_at = CURRENT_TIMESTAMP",
            params![
                record.channel_type,
                record.base_url,
                record.name,
                record.status,
                record.credentials_json,
                record.supported_models_json,
                bool_to_sql(record.auto_sync_supported_models),
                record.default_test_model,
                record.settings_json,
                record.tags_json,
                record.ordering_weight,
                record.error_message,
                record.remark,
            ],
        )?;

        query_channel_id(&connection, record.name)
    }

    #[cfg(test)]
    pub fn upsert_model(&self, record: &NewModelRecord<'_>) -> rusqlite::Result<i64> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;
        connection.execute(
            "INSERT INTO models (developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0)
             ON CONFLICT(developer, model_id, type) DO UPDATE SET
                 name = excluded.name,
                 icon = excluded.icon,
                 \"group\" = excluded.\"group\",
                 model_card = excluded.model_card,
                 settings = excluded.settings,
                 status = excluded.status,
                 remark = excluded.remark,
                 deleted_at = 0,
                 updated_at = CURRENT_TIMESTAMP",
            params![
                record.developer,
                record.model_id,
                record.model_type,
                record.name,
                record.icon,
                record.group,
                record.model_card_json,
                record.settings_json,
                record.status,
                record.remark,
            ],
        )?;

        query_model_id(
            &connection,
            record.developer,
            record.model_id,
            record.model_type,
        )
    }

    pub fn list_enabled_models(&self, include: Option<&str>) -> rusqlite::Result<Vec<OpenAiModel>> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;

        let include = ModelInclude::parse(include);
        list_enabled_model_records(&connection)?
            .into_iter()
            .map(|record| Ok(record.into_openai_model(&include)))
            .collect()
    }

    pub fn list_enabled_model_records(&self) -> rusqlite::Result<Vec<StoredModelRecord>> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;
        list_enabled_model_records(&connection)
    }

    pub fn list_channels(&self) -> rusqlite::Result<Vec<StoredChannelSummary>> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, name, type, base_url, status, supported_models, ordering_weight
             FROM channels
             WHERE deleted_at = 0
             ORDER BY ordering_weight DESC, id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredChannelSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                channel_type: row.get(2)?,
                base_url: row.get(3)?,
                status: row.get(4)?,
                supported_models: parse_json_string_vec(row.get::<_, String>(5)?),
                ordering_weight: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn select_inference_targets(
        &self,
        request_model_id: &str,
        trace_id: Option<i64>,
        max_channel_retries: usize,
        channel_type: &str,
        model_type: &str,
    ) -> rusqlite::Result<Vec<SelectedOpenAiTarget>> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;
        ensure_request_tables(&connection)?;

        let mut statement = connection.prepare(
            "SELECT c.id, c.base_url, c.credentials, c.supported_models, c.ordering_weight,
                    m.created_at, m.developer, m.model_id, m.type, m.name, m.icon, m.remark, m.model_card
              FROM channels c
              JOIN models m ON m.model_id = ?1
              WHERE c.deleted_at = 0
                AND c.status = 'enabled'
                AND m.deleted_at = 0
                AND m.status = 'enabled'
                AND c.type = ?3
                AND (?2 = '' OR m.type = ?2)
              ORDER BY c.ordering_weight DESC, c.id ASC",
        )?;
        let mut rows = statement.query(params![request_model_id, model_type, channel_type])?;
        let preferred_trace_channel_id = trace_id
            .map(|trace_id| {
                query_preferred_trace_channel_id(&connection, trace_id, request_model_id)
            })
            .transpose()?
            .flatten();
        let mut candidates = Vec::new();

        while let Some(row) = rows.next()? {
            let supported_models_json: String = row.get(3)?;
            if !model_supported_by_channel(&supported_models_json, request_model_id) {
                continue;
            }

            let credentials_json: String = row.get(2)?;
            let api_key = extract_channel_api_key(&credentials_json);
            if api_key.is_empty() {
                continue;
            }

            let channel_id: i64 = row.get(0)?;
            let ordering_weight: i64 = row.get(4)?;
            let routing_stats = query_channel_routing_stats(&connection, channel_id)?;

            let model = StoredModelRecord {
                id: 0,
                created_at: row.get(5)?,
                developer: row.get(6)?,
                model_id: row.get(7)?,
                model_type: row.get(8)?,
                name: row.get(9)?,
                icon: row.get(10)?,
                remark: row.get(11)?,
                model_card_json: row.get(12)?,
            };

            candidates.push(SelectedOpenAiTarget {
                channel_id,
                base_url: row.get(1)?,
                api_key,
                actual_model_id: request_model_id.to_owned(),
                ordering_weight,
                trace_affinity: preferred_trace_channel_id == Some(channel_id),
                routing_stats,
                model,
            });
        }

        candidates.sort_by(compare_openai_target_priority);

        let top_k = calculate_top_k(candidates.len(), max_channel_retries);
        candidates.truncate(top_k);
        Ok(candidates)
    }
}

#[derive(Debug, Clone)]
pub struct RequestStore {
    pub(crate) connection_factory: SqliteConnectionFactory,
}

#[derive(Debug, Clone)]
pub struct StoredRequestRouteHint {
    pub channel_id: i64,
    pub model_id: String,
}

#[derive(Debug, Clone)]
pub struct StoredRequestContentRecord {
    pub id: i64,
    pub project_id: i64,
    pub content_saved: bool,
    pub content_storage_id: Option<i64>,
    pub content_storage_key: Option<String>,
}

impl RequestStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)
    }

    pub fn create_request(&self, record: &NewRequestRecord<'_>) -> rusqlite::Result<i64> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        connection.execute(
            "INSERT INTO requests (
                api_key_id, project_id, trace_id, data_storage_id, source, model_id, format,
                request_headers, request_body, response_body, response_chunks, channel_id,
                external_id, status, stream, client_ip, metrics_latency_ms,
                metrics_first_token_latency_ms, content_saved, content_storage_id,
                content_storage_key, content_saved_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17,
                ?18, ?19, ?20,
                ?21, ?22
            )",
            params![
                record.api_key_id,
                record.project_id,
                record.trace_id,
                record.data_storage_id,
                record.source,
                record.model_id,
                record.format,
                record.request_headers_json,
                record.request_body_json,
                record.response_body_json,
                record.response_chunks_json,
                record.channel_id,
                record.external_id,
                record.status,
                bool_to_sql(record.stream),
                record.client_ip,
                record.metrics_latency_ms,
                record.metrics_first_token_latency_ms,
                bool_to_sql(record.content_saved),
                record.content_storage_id,
                record.content_storage_key,
                record.content_saved_at,
            ],
        )?;

        Ok(connection.last_insert_rowid())
    }

    pub fn create_request_execution(
        &self,
        record: &NewRequestExecutionRecord<'_>,
    ) -> rusqlite::Result<i64> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        connection.execute(
            "INSERT INTO request_executions (
                project_id, request_id, channel_id, data_storage_id, external_id, model_id,
                format, request_body, response_body, response_chunks, error_message,
                response_status_code, status, stream, metrics_latency_ms,
                metrics_first_token_latency_ms, request_headers
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15,
                ?16, ?17
            )",
            params![
                record.project_id,
                record.request_id,
                record.channel_id,
                record.data_storage_id,
                record.external_id,
                record.model_id,
                record.format,
                record.request_body_json,
                record.response_body_json,
                record.response_chunks_json,
                record.error_message,
                record.response_status_code,
                record.status,
                bool_to_sql(record.stream),
                record.metrics_latency_ms,
                record.metrics_first_token_latency_ms,
                record.request_headers_json,
            ],
        )?;

        Ok(connection.last_insert_rowid())
    }

    pub fn update_request_result(
        &self,
        record: &UpdateRequestResultRecord<'_>,
    ) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        connection.execute(
            "UPDATE requests
             SET updated_at = CURRENT_TIMESTAMP,
                 channel_id = COALESCE(?2, channel_id),
                 external_id = COALESCE(?3, external_id),
                 response_body = COALESCE(?4, response_body),
                 status = ?5
             WHERE id = ?1",
            params![
                record.request_id,
                record.channel_id,
                record.external_id,
                record.response_body_json,
                record.status,
            ],
        )?;
        Ok(())
    }

    pub fn update_request_execution_result(
        &self,
        record: &UpdateRequestExecutionResultRecord<'_>,
    ) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        connection.execute(
            "UPDATE request_executions
             SET updated_at = CURRENT_TIMESTAMP,
                 external_id = COALESCE(?2, external_id),
                 response_body = COALESCE(?3, response_body),
                 response_status_code = COALESCE(?4, response_status_code),
                 error_message = COALESCE(?5, error_message),
                 status = ?6
             WHERE id = ?1",
            params![
                record.execution_id,
                record.external_id,
                record.response_body_json,
                record.response_status_code,
                record.error_message,
                record.status,
            ],
        )?;
        Ok(())
    }

    pub fn find_latest_completed_request_by_external_id(
        &self,
        route_format: &str,
        external_id: &str,
    ) -> rusqlite::Result<Option<StoredRequestRouteHint>> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        connection
            .query_row(
                "SELECT channel_id, model_id
                 FROM requests
                 WHERE format = ?1
                   AND external_id = ?2
                   AND status = 'completed'
                   AND channel_id IS NOT NULL
                 ORDER BY id DESC
                 LIMIT 1",
                params![route_format, external_id],
                |row| {
                    Ok(StoredRequestRouteHint {
                        channel_id: row.get(0)?,
                        model_id: row.get(1)?,
                    })
                },
            )
            .optional()
    }

    pub fn find_request_content_record(
        &self,
        request_id: i64,
    ) -> rusqlite::Result<Option<StoredRequestContentRecord>> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        connection
            .query_row(
                "SELECT id, project_id, content_saved, content_storage_id, content_storage_key
                 FROM requests WHERE id = ?1 LIMIT 1",
                [request_id],
                |row| {
                    Ok(StoredRequestContentRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        content_saved: row.get::<_, i64>(2)? != 0,
                        content_storage_id: row.get(3)?,
                        content_storage_key: row.get(4)?,
                    })
                },
            )
            .optional()
    }

    pub fn list_requests_by_project(
        &self,
        project_id: i64,
    ) -> rusqlite::Result<Vec<StoredRequestSummary>> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, trace_id, channel_id, model_id, format, status, source, external_id
             FROM requests
             WHERE project_id = ?1
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(StoredRequestSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                trace_id: row.get(2)?,
                channel_id: row.get(3)?,
                model_id: row.get(4)?,
                format: row.get(5)?,
                status: row.get(6)?,
                source: row.get(7)?,
                external_id: row.get(8)?,
            })
        })?;
        rows.collect()
    }
}

#[derive(Debug, Clone)]
pub struct UsageCostStore {
    pub(crate) connection_factory: SqliteConnectionFactory,
}

impl UsageCostStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        connection.execute_batch(USAGE_LOGS_TABLE_SQL)
    }

    pub fn record_usage(&self, record: &NewUsageLogRecord<'_>) -> rusqlite::Result<i64> {
        let connection = self.connection_factory.open(true)?;
        connection.execute_batch(USAGE_LOGS_TABLE_SQL)?;
        connection.execute(
            "INSERT INTO usage_logs (
                request_id, api_key_id, project_id, channel_id, model_id,
                prompt_tokens, completion_tokens, total_tokens,
                prompt_audio_tokens, prompt_cached_tokens, prompt_write_cached_tokens,
                prompt_write_cached_tokens_5m, prompt_write_cached_tokens_1h,
                completion_audio_tokens, completion_reasoning_tokens,
                completion_accepted_prediction_tokens, completion_rejected_prediction_tokens,
                source, format, total_cost, cost_items, cost_price_reference_id, deleted_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8,
                ?9, ?10, ?11,
                ?12, ?13,
                ?14, ?15,
                ?16, ?17,
                ?18, ?19, ?20, ?21, ?22, 0
            )",
            params![
                record.request_id,
                record.api_key_id,
                record.project_id,
                record.channel_id,
                record.model_id,
                record.prompt_tokens,
                record.completion_tokens,
                record.total_tokens,
                record.prompt_audio_tokens,
                record.prompt_cached_tokens,
                record.prompt_write_cached_tokens,
                record.prompt_write_cached_tokens_5m,
                record.prompt_write_cached_tokens_1h,
                record.completion_audio_tokens,
                record.completion_reasoning_tokens,
                record.completion_accepted_prediction_tokens,
                record.completion_rejected_prediction_tokens,
                record.source,
                record.format,
                record.total_cost,
                record.cost_items_json,
                record.cost_price_reference_id,
            ],
        )?;

        Ok(connection.last_insert_rowid())
    }
}

pub struct SqliteOpenAiV1Service {
    pub(crate) foundation: Arc<SqliteFoundation>,
}

impl SqliteOpenAiV1Service {
    const DEFAULT_MAX_CHANNEL_RETRIES: usize = 2;

    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        Self { foundation }
    }

    fn select_target_channels(
        &self,
        request: &OpenAiV1ExecutionRequest,
        _route: OpenAiV1Route,
    ) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
        let request_model = request
            .body
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                message: "model is required".to_owned(),
            })?;

        let targets = self
            .foundation
            .channel_models()
            .select_inference_targets(
                request_model,
                request.trace.as_ref().map(|trace| trace.id),
                Self::DEFAULT_MAX_CHANNEL_RETRIES,
                "openai",
                "",
            )
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to resolve upstream target: {error}"),
            })?;

        if targets.is_empty() {
            Err(OpenAiV1Error::InvalidRequest {
                message: "No enabled OpenAI channel is configured for the requested model"
                    .to_owned(),
            })
        } else {
            Ok(targets)
        }
    }

    fn mark_request_failed(
        &self,
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

        self.foundation
            .requests()
            .update_request_result(&UpdateRequestResultRecord {
                request_id,
                status: "failed",
                external_id,
                response_body_json: response_body_json.as_deref(),
                channel_id,
            })
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to persist failed request state: {error}"),
            })
    }

    fn mark_execution_failed(
        &self,
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

        self.foundation
            .requests()
            .update_request_execution_result(&UpdateRequestExecutionResultRecord {
                execution_id,
                status: "failed",
                external_id,
                response_body_json: response_body_json.as_deref(),
                response_status_code: response_status_code.map(i64::from),
                error_message: Some(error_message),
            })
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to persist failed request execution state: {error}"),
            })
    }

    fn complete_execution(
        &self,
        request: &OpenAiV1ExecutionRequest,
        route_format: &str,
        request_id: i64,
        execution_id: i64,
        target: &SelectedOpenAiTarget,
        status: u16,
        response_body: Value,
        usage: Option<ExtractedUsage>,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        let response_body_json =
            serde_json::to_string(&response_body).map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to serialize upstream response: {error}"),
            })?;
        let external_id = response_body
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        self.foundation
            .requests()
            .update_request_result(&UpdateRequestResultRecord {
                request_id,
                status: "completed",
                external_id: external_id.as_deref(),
                response_body_json: Some(response_body_json.as_str()),
                channel_id: Some(target.channel_id),
            })
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to update request: {error}"),
            })?;
        self.foundation
            .requests()
            .update_request_execution_result(&UpdateRequestExecutionResultRecord {
                execution_id,
                status: "completed",
                external_id: external_id.as_deref(),
                response_body_json: Some(response_body_json.as_str()),
                response_status_code: Some(status as i64),
                error_message: None,
            })
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to update request execution: {error}"),
            })?;

        if let Some(usage) = usage {
            let usage_cost = compute_usage_cost(&target.model, &usage);
            if let Ok(cost_items_json) = serde_json::to_string(&usage_cost.cost_items) {
                let _ = self
                    .foundation
                    .usage_costs()
                    .record_usage(&NewUsageLogRecord {
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
                        completion_accepted_prediction_tokens: usage
                            .completion_accepted_prediction_tokens,
                        completion_rejected_prediction_tokens: usage
                            .completion_rejected_prediction_tokens,
                        source: "api",
                        format: route_format,
                        total_cost: usage_cost.total_cost,
                        cost_items_json: cost_items_json.as_str(),
                        cost_price_reference_id: usage_cost
                            .price_reference_id
                            .as_deref()
                            .unwrap_or(""),
                    });
            }
        }

        Ok(OpenAiV1ExecutionResponse {
            status,
            body: response_body,
        })
    }

    fn should_retry(&self, error: &OpenAiV1Error) -> bool {
        match error {
            OpenAiV1Error::Internal { .. } => true,
            OpenAiV1Error::Upstream { status, .. } => {
                *status == 408 || *status == 409 || *status == 429 || *status >= 500
            }
            OpenAiV1Error::InvalidRequest { .. } => false,
        }
    }

    fn execute_shared_route<UrlBuilder, ResponseMapper, UsageExtractor>(
        &self,
        request: &OpenAiV1ExecutionRequest,
        route_format: &str,
        upstream_method: reqwest::Method,
        targets: Vec<SelectedOpenAiTarget>,
        upstream_body: &Value,
        upstream_headers: &HashMap<String, String>,
        data_storage_id: Option<i64>,
        upstream_url_for_target: UrlBuilder,
        response_mapper: ResponseMapper,
        usage_extractor: UsageExtractor,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error>
    where
        UrlBuilder: Fn(&SelectedOpenAiTarget) -> String,
        ResponseMapper: Fn(Value) -> Result<Value, OpenAiV1Error>,
        UsageExtractor: Fn(&Value) -> Option<ExtractedUsage>,
    {
        let masked_request_headers = sanitize_headers_json(upstream_headers);
        let request_body_json =
            serde_json::to_string(&request.body).map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to serialize request body: {error}"),
            })?;
        let upstream_body_json =
            serde_json::to_string(upstream_body).map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to serialize upstream request body: {error}"),
            })?;
        let stream = request
            .body
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let request_id = self
            .foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: request.api_key_id,
                project_id: request.project.id,
                trace_id: request.trace.as_ref().map(|trace| trace.id),
                data_storage_id,
                source: "api",
                model_id: targets[0].actual_model_id.as_str(),
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
            })
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to persist request: {error}"),
            })?;
        let mut last_error = None;

        for (index, target) in targets.iter().enumerate() {
            self.foundation
                .requests()
                .update_request_result(&UpdateRequestResultRecord {
                    request_id,
                    status: "processing",
                    external_id: None,
                    response_body_json: None,
                    channel_id: Some(target.channel_id),
                })
                .map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to update request attempt channel: {error}"),
                })?;

            let execution_id = match self.foundation.requests().create_request_execution(
                &NewRequestExecutionRecord {
                    project_id: request.project.id,
                    request_id,
                    channel_id: Some(target.channel_id),
                    data_storage_id,
                    external_id: None,
                    model_id: target.actual_model_id.as_str(),
                    format: route_format,
                    request_body_json: upstream_body_json.as_str(),
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
            ) {
                Ok(execution_id) => execution_id,
                Err(error) => {
                    let request_error = OpenAiV1Error::Internal {
                        message: format!("Failed to persist request execution: {error}"),
                    };
                    self.mark_request_failed(request_id, Some(target.channel_id), None, None)?;
                    return Err(request_error);
                }
            };

            let attempt_result = (|| -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
                let built_headers =
                    build_upstream_headers(upstream_headers, target.api_key.as_str())?;
                let client = reqwest::blocking::Client::new();
                let mut upstream_request = client
                    .request(
                        upstream_method.clone(),
                        upstream_url_for_target(target).as_str(),
                    )
                    .headers(built_headers);
                if matches!(upstream_method, reqwest::Method::POST) {
                    upstream_request = upstream_request.json(upstream_body);
                }
                let upstream_response =
                    upstream_request
                        .send()
                        .map_err(|error| OpenAiV1Error::Internal {
                            message: format!("Failed to execute upstream request: {error}"),
                        })?;

                let status = upstream_response.status().as_u16();
                let response_text =
                    upstream_response
                        .text()
                        .map_err(|error| OpenAiV1Error::Internal {
                            message: format!("Failed to read upstream response: {error}"),
                        })?;
                let raw_response_body: Value =
                    serde_json::from_str(&response_text).map_err(|error| {
                        OpenAiV1Error::Internal {
                            message: format!("Failed to decode upstream response: {error}"),
                        }
                    })?;

                if (200..300).contains(&status) {
                    let usage = usage_extractor(&raw_response_body);
                    let response_body = response_mapper(raw_response_body)?;
                    self.complete_execution(
                        request,
                        route_format,
                        request_id,
                        execution_id,
                        target,
                        status,
                        response_body,
                        usage,
                    )
                } else {
                    Err(OpenAiV1Error::Upstream {
                        status,
                        body: raw_response_body,
                    })
                }
            })();

            match attempt_result {
                Ok(response) => return Ok(response),
                Err(error) => {
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

                    self.mark_execution_failed(
                        execution_id,
                        openai_error_message(&error).as_str(),
                        response_body,
                        response_status_code,
                        external_id,
                    )?;

                    let retryable = self.should_retry(&error);
                    let is_last = index + 1 == targets.len();
                    if retryable && !is_last {
                        last_error = Some(error);
                        continue;
                    }

                    self.mark_request_failed(
                        request_id,
                        Some(target.channel_id),
                        response_body,
                        external_id,
                    )?;
                    return Err(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| OpenAiV1Error::Internal {
            message: "No upstream channel attempt was executed".to_owned(),
        }))
    }
}

impl OpenAiV1Port for SqliteOpenAiV1Service {
    fn list_models(&self, include: Option<&str>) -> Result<ModelListResponse, OpenAiV1Error> {
        let models = self
            .foundation
            .channel_models()
            .list_enabled_models(include)
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to list models: {error}"),
            })?;

        Ok(ModelListResponse {
            object: "list",
            data: models,
        })
    }

    fn list_anthropic_models(&self) -> Result<AnthropicModelListResponse, OpenAiV1Error> {
        let models = self
            .foundation
            .channel_models()
            .list_enabled_model_records()
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to list models: {error}"),
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
        let first_id = data.first().map(|model| model.id.clone());
        let last_id = data.last().map(|model| model.id.clone());

        Ok(AnthropicModelListResponse {
            object: "list",
            data,
            has_more: false,
            first_id,
            last_id,
        })
    }

    fn list_gemini_models(&self) -> Result<GeminiModelListResponse, OpenAiV1Error> {
        let models = self
            .foundation
            .channel_models()
            .list_enabled_model_records()
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to list models: {error}"),
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
        validate_openai_request(route, &request.body)?;

        let targets = self.select_target_channels(&request, route)?;
        let data_storage_id = self
            .foundation
            .system_settings()
            .default_data_storage_id()
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to load data storage configuration: {error}"),
            })?;

        let upstream_body = rewrite_model(&request.body, targets[0].actual_model_id.as_str());
        self.execute_shared_route(
            &request,
            route.format(),
            reqwest::Method::POST,
            targets,
            &upstream_body,
            &request.headers,
            data_storage_id,
            |target| target.upstream_url(route),
            Ok,
            |response_body| extract_usage(route, response_body),
        )
    }

    fn execute_compatibility(
        &self,
        route: CompatibilityRoute,
        request: OpenAiV1ExecutionRequest,
    ) -> Result<OpenAiV1ExecutionResponse, OpenAiV1Error> {
        let data_storage_id = self
            .foundation
            .system_settings()
            .default_data_storage_id()
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to load data storage configuration: {error}"),
            })?;
        let prepared = prepare_compatibility_request(route, &request)?;
        let targets = if matches!(
            route,
            CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask
        ) {
            self.select_doubao_task_targets(&request, &prepared)?
        } else {
            self.foundation
                .channel_models()
                .select_inference_targets(
                    prepared.request_model_id.as_str(),
                    request.trace.as_ref().map(|trace| trace.id),
                    Self::DEFAULT_MAX_CHANNEL_RETRIES,
                    prepared.channel_type,
                    prepared.model_type,
                )
                .map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to resolve upstream target: {error}"),
                })?
        };

        if targets.is_empty() {
            return Err(OpenAiV1Error::InvalidRequest {
                message: format!(
                    "No enabled {} channel is configured for the requested model",
                    prepared.channel_type
                ),
            });
        }

        let upstream_body = if prepared.upstream_body.is_null() {
            Value::Null
        } else {
            rewrite_model(&prepared.upstream_body, targets[0].actual_model_id.as_str())
        };
        let route_task_id = prepared.task_id.clone();
        self.execute_shared_route(
            &request,
            route.format(),
            compatibility_upstream_method(route),
            targets,
            &upstream_body,
            &request.headers,
            data_storage_id,
            move |target| compatibility_upstream_url(target, route, route_task_id.as_deref()),
            |response_body| map_compatibility_response(route, response_body),
            |response_body| compatibility_usage(route, response_body),
        )
    }
}

impl SqliteOpenAiV1Service {
    fn select_doubao_task_targets(
        &self,
        request: &OpenAiV1ExecutionRequest,
        prepared: &PreparedCompatibilityRequest,
    ) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
        let task_id = prepared
            .task_id
            .as_deref()
            .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                message: "task id is required".to_owned(),
            })?;
        let request_hint = self
            .foundation
            .requests()
            .find_latest_completed_request_by_external_id("doubao/video_create", task_id)
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to resolve Doubao task origin: {error}"),
            })?
            .ok_or_else(|| OpenAiV1Error::Upstream {
                status: 404,
                body: serde_json::json!({"error": {"message": "not found"}}),
            })?;

        let mut targets = self
            .foundation
            .channel_models()
            .select_inference_targets(
                request_hint.model_id.as_str(),
                request.trace.as_ref().map(|trace| trace.id),
                Self::DEFAULT_MAX_CHANNEL_RETRIES,
                prepared.channel_type,
                prepared.model_type,
            )
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to resolve upstream target: {error}"),
            })?;

        if let Some(index) = targets
            .iter()
            .position(|target| target.channel_id == request_hint.channel_id)
        {
            let preferred = targets.remove(index);
            targets.insert(0, preferred);
        } else {
            return Err(OpenAiV1Error::Upstream {
                status: 404,
                body: serde_json::json!({"error": {"message": "not found"}}),
            });
        }

        Ok(targets)
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
    pub ordering_weight: i64,
    pub trace_affinity: bool,
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

#[cfg(test)]
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

#[cfg(test)]
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
    fn parse(include: Option<&str>) -> Self {
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
    fn into_openai_model(self, include: &ModelInclude) -> OpenAiModel {
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
    fn upstream_url(&self, route: OpenAiV1Route) -> String {
        let trimmed = self.base_url.trim_end_matches('/');
        match route {
            OpenAiV1Route::ChatCompletions => format!("{trimmed}/chat/completions"),
            OpenAiV1Route::Responses => format!("{trimmed}/responses"),
            OpenAiV1Route::Embeddings => format!("{trimmed}/embeddings"),
        }
    }

    fn base_routing_priority_key(&self) -> (i64, i64, i64) {
        (
            if self.trace_affinity { 0 } else { 1 },
            if self.routing_stats.last_status_failed {
                1
            } else {
                0
            },
            self.routing_stats.consecutive_failures,
        )
    }
}

pub(crate) fn validate_openai_request(
    route: OpenAiV1Route,
    body: &Value,
) -> Result<(), OpenAiV1Error> {
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
        OpenAiV1Route::Embeddings => {
            if !object.contains_key("input") {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: "input is required".to_owned(),
                });
            }
        }
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
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("application/json"),
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

    Ok(headers)
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
        OpenAiV1Route::Responses => {
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
        OpenAiV1Route::ChatCompletions | OpenAiV1Route::Embeddings => {
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
        CompatibilityRoute::AnthropicMessages => prepare_anthropic_request(&request.body),
        CompatibilityRoute::JinaRerank => prepare_jina_rerank_request(&request.body),
        CompatibilityRoute::JinaEmbeddings => prepare_jina_embedding_request(&request.body),
        CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent => prepare_gemini_request(route, request),
        CompatibilityRoute::DoubaoCreateTask => prepare_doubao_create_request(&request.body),
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
    let body = &request.body;
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
            message: "only text Gemini contents are supported in the Rust migration slice"
                .to_owned(),
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

pub(crate) fn list_enabled_model_records(
    connection: &Connection,
) -> rusqlite::Result<Vec<StoredModelRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, created_at, developer, model_id, type, name, icon, remark, model_card
         FROM models WHERE deleted_at = 0 AND status = 'enabled' ORDER BY id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredModelRecord {
            id: row.get(0)?,
            created_at: row.get(1)?,
            developer: row.get(2)?,
            model_id: row.get(3)?,
            model_type: row.get(4)?,
            name: row.get(5)?,
            icon: row.get(6)?,
            remark: row.get(7)?,
            model_card_json: row.get(8)?,
        })
    })?;

    rows.collect()
}

pub(crate) fn model_supported_by_channel(supported_models_json: &str, model_id: &str) -> bool {
    serde_json::from_str::<Vec<String>>(supported_models_json)
        .unwrap_or_default()
        .iter()
        .any(|current| current == model_id)
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

pub(crate) fn query_preferred_trace_channel_id(
    connection: &Connection,
    trace_id: i64,
    model_id: &str,
) -> rusqlite::Result<Option<i64>> {
    connection
        .query_row(
            "SELECT channel_id
             FROM requests
             WHERE trace_id = ?1
               AND model_id = ?2
               AND status = 'completed'
               AND channel_id IS NOT NULL
             ORDER BY id DESC
             LIMIT 1",
            params![trace_id, model_id],
            |row| row.get(0),
        )
        .optional()
}

pub(crate) fn query_channel_routing_stats(
    connection: &Connection,
    channel_id: i64,
) -> rusqlite::Result<ChannelRoutingStats> {
    let selection_count = connection.query_row(
        "SELECT COUNT(*) FROM requests WHERE channel_id = ?1",
        [channel_id],
        |row| row.get(0),
    )?;
    let processing_count = connection.query_row(
        "SELECT COUNT(*) FROM requests WHERE channel_id = ?1 AND status = 'processing'",
        [channel_id],
        |row| row.get(0),
    )?;

    let mut statement = connection.prepare(
        "SELECT status FROM request_executions
         WHERE channel_id = ?1
         ORDER BY id DESC
         LIMIT 10",
    )?;
    let rows = statement.query_map([channel_id], |row| row.get::<_, String>(0))?;
    let statuses = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    let last_status_failed = statuses.first().is_some_and(|status| status == "failed");
    let consecutive_failures = statuses
        .iter()
        .take_while(|status| status.as_str() == "failed")
        .count() as i64;

    Ok(ChannelRoutingStats {
        selection_count,
        processing_count,
        consecutive_failures,
        last_status_failed,
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
pub(crate) fn query_channel_id(connection: &Connection, name: &str) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT id FROM channels WHERE name = ?1 AND deleted_at = 0 LIMIT 1",
        [name],
        |row| row.get(0),
    )
}

#[cfg(test)]
pub(crate) fn query_model_id(
    connection: &Connection,
    developer: &str,
    model_id: &str,
    model_type: &str,
) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT id FROM models WHERE developer = ?1 AND model_id = ?2 AND type = ?3 AND deleted_at = 0 LIMIT 1",
        params![developer, model_id, model_type],
        |row| row.get(0),
    )
}
