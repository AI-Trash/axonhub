use std::collections::HashMap;
use std::sync::Arc;

use axonhub_http::{
    AnthropicModel, AnthropicModelListResponse, CompatibilityRoute, GeminiModel,
    GeminiModelListResponse, ModelListResponse, OpenAiModel, OpenAiV1Error,
    OpenAiV1ExecutionRequest, OpenAiV1ExecutionResponse, OpenAiV1Port, OpenAiV1Route,
};
use rusqlite::{params, Connection, OptionalExtension, Result as SqlResult};
use serde::Deserialize;
use serde_json::Value;

use super::{
    admin::{default_system_channel_settings, StoredSystemChannelSettings},
    identity::parse_json_string_vec,
    openai_v1::{
        calculate_top_k, compare_openai_target_priority, compatibility_upstream_method,
        compatibility_upstream_url, compatibility_usage, compute_usage_cost,
        extract_channel_api_key, extract_usage, map_compatibility_response,
        model_supported_by_channel, openai_error_message, prepare_compatibility_request,
        rewrite_model, sanitize_headers_json, sqlite_timestamp_to_rfc3339, validate_openai_request,
        ExtractedUsage, ModelInclude, NewRequestExecutionRecord, NewRequestRecord,
        NewUsageLogRecord, PreparedCompatibilityRequest, SelectedOpenAiTarget,
        StoredChannelSummary, StoredModelRecord, StoredRequestSummary,
        UpdateRequestExecutionResultRecord, UpdateRequestResultRecord,
        DEFAULT_MAX_SAME_CHANNEL_RETRIES,
    },
    ports::OpenAiV1Repository,
    repositories::openai_v1::enforce_api_key_quota_seaorm,
    shared::{bool_to_sql, SqliteConnectionFactory, SqliteFoundation, USAGE_LOGS_TABLE_SQL},
    system::{ensure_channel_model_tables, ensure_request_tables, SystemSettingsStore},
};

#[cfg(test)]
use super::openai_v1::{NewChannelRecord, NewModelRecord};

#[derive(Debug, Clone)]
pub struct ChannelModelStore {
    pub(crate) connection_factory: SqliteConnectionFactory,
}

impl ChannelModelStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)
    }

    #[cfg(test)]
    pub fn upsert_channel(&self, record: &NewChannelRecord<'_>) -> SqlResult<i64> {
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

        connection.query_row(
            "SELECT id FROM channels WHERE name = ?1 AND deleted_at = 0 LIMIT 1",
            [record.name],
            |row| row.get(0),
        )
    }

    #[cfg(test)]
    pub fn upsert_model(&self, record: &NewModelRecord<'_>) -> SqlResult<i64> {
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

        connection.query_row(
            "SELECT id FROM models WHERE developer = ?1 AND model_id = ?2 AND type = ?3 AND deleted_at = 0 LIMIT 1",
            params![record.developer, record.model_id, record.model_type],
            |row| row.get(0),
        )
    }

    pub fn list_enabled_models(&self, include: Option<&str>) -> SqlResult<Vec<OpenAiModel>> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;

        let include = ModelInclude::parse(include);
        list_listed_model_records(&connection, &SystemSettingsStore::new(self.connection_factory.clone()))?
            .into_iter()
            .map(|record| Ok(record.into_openai_model(&include)))
            .collect()
    }

    pub fn list_enabled_model_records(&self) -> SqlResult<Vec<StoredModelRecord>> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)?;
        list_listed_model_records(&connection, &SystemSettingsStore::new(self.connection_factory.clone()))
    }

    pub fn list_channels(&self) -> SqlResult<Vec<StoredChannelSummary>> {
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
        preferred_channel_id: Option<i64>,
    ) -> SqlResult<Vec<SelectedOpenAiTarget>> {
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

        if let Some(preferred_channel_id) = preferred_channel_id {
            if let Some(index) = candidates
                .iter()
                .position(|target| target.channel_id == preferred_channel_id)
            {
                let preferred = candidates.remove(index);
                candidates.insert(0, preferred);
            }
        }

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
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_request_tables(&connection)
    }

    pub fn create_request(&self, record: &NewRequestRecord<'_>) -> SqlResult<i64> {
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
    ) -> SqlResult<i64> {
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

    pub fn update_request_result(&self, record: &UpdateRequestResultRecord<'_>) -> SqlResult<()> {
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
    ) -> SqlResult<()> {
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
    ) -> SqlResult<Option<StoredRequestRouteHint>> {
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
    ) -> SqlResult<Option<StoredRequestContentRecord>> {
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
    ) -> SqlResult<Vec<StoredRequestSummary>> {
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
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let connection = self.connection_factory.open(true)?;
        connection.execute_batch(USAGE_LOGS_TABLE_SQL)
    }

    pub fn record_usage(&self, record: &NewUsageLogRecord<'_>) -> SqlResult<i64> {
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
    pub(crate) const DEFAULT_MAX_CHANNEL_RETRIES: usize = 2;

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
                request.channel_hint_id,
            )
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to resolve upstream target: {error}"),
            })?;

        if targets.is_empty() {
            Err(OpenAiV1Error::InvalidRequest {
                message: "No enabled OpenAI channel is configured for the requested model"
                    .to_owned(),
            })
        } else if request
            .channel_hint_id
            .is_some_and(|channel_hint_id| targets[0].channel_id != channel_hint_id)
        {
            Err(OpenAiV1Error::InvalidRequest {
                message: "No enabled OpenAI channel matches the requested channel override"
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
        should_retry_openai_error(error)
    }

    fn execute_shared_route<UrlBuilder, ResponseMapper, UsageExtractor>(
        &self,
        request: &OpenAiV1ExecutionRequest,
        request_model_id: &str,
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
        self.foundation.seaorm().run_sync({
            let api_key_id = request.api_key_id;
            move |db| async move {
                let connection = db.connect_migrated().await.map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to open quota database connection: {error}"),
                })?;
                enforce_api_key_quota_seaorm(&connection, db.backend(), api_key_id).await
            }
        })?;

        let masked_request_headers = sanitize_headers_json(upstream_headers);
        let request_body_json =
            serde_json::to_string(&request.body).map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to serialize request body: {error}"),
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
            })
            .map_err(|error| OpenAiV1Error::Internal {
                message: format!("Failed to persist request: {error}"),
            })?;
        let mut last_error = None;

        for (index, target) in targets.iter().enumerate() {
            let mut same_channel_attempts = 0;
            loop {
                let attempt_upstream_body = if upstream_body.is_null() {
                    Value::Null
                } else {
                    rewrite_model(upstream_body, target.actual_model_id.as_str())
                };
                let attempt_upstream_body_json = serde_json::to_string(&attempt_upstream_body)
                    .map_err(|error| OpenAiV1Error::Internal {
                        message: format!("Failed to serialize upstream request body: {error}"),
                    })?;

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
                        request_body_json: attempt_upstream_body_json.as_str(),
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
                    let built_headers = super::openai_v1::build_upstream_headers(
                        upstream_headers,
                        target.api_key.as_str(),
                    )?;
                    let client = reqwest::blocking::Client::new();
                    let mut upstream_request = client
                        .request(
                            upstream_method.clone(),
                            upstream_url_for_target(target).as_str(),
                        )
                        .headers(built_headers);
                    if matches!(upstream_method, reqwest::Method::POST) {
                        upstream_request = upstream_request.json(&attempt_upstream_body);
                    }
                    let upstream_response = upstream_request.send().map_err(|error| {
                        OpenAiV1Error::Internal {
                            message: format!("Failed to execute upstream request: {error}"),
                        }
                    })?;

                    let status = upstream_response.status().as_u16();
                    let response_text = upstream_response.text().map_err(|error| {
                        OpenAiV1Error::Internal {
                            message: format!("Failed to read upstream response: {error}"),
                        }
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
                        if retryable && same_channel_attempts < DEFAULT_MAX_SAME_CHANNEL_RETRIES {
                            same_channel_attempts += 1;
                            continue;
                        }

                        let is_last = index + 1 == targets.len();
                        if retryable && !is_last {
                            last_error = Some(error);
                            break;
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
        }

        Err(last_error.unwrap_or_else(|| OpenAiV1Error::Internal {
            message: "No upstream channel attempt was executed".to_owned(),
        }))
    }

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
                None,
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
            request
                .body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default(),
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
                    None,
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
            prepared.request_model_id.as_str(),
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

impl OpenAiV1Repository for SqliteOpenAiV1Service {
    fn list_models(&self, include: Option<&str>) -> Result<ModelListResponse, OpenAiV1Error> {
        <Self as OpenAiV1Port>::list_models(self, include)
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
}

pub(crate) fn list_enabled_model_records(
    connection: &Connection,
) -> SqlResult<Vec<StoredModelRecord>> {
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

fn list_listed_model_records(
    connection: &Connection,
    settings_store: &SystemSettingsStore,
) -> SqlResult<Vec<StoredModelRecord>> {
    if load_system_channel_settings(settings_store)?.query_all_channel_models {
        list_routable_model_records(connection)
    } else {
        list_enabled_model_records(connection)
    }
}

fn load_system_channel_settings(
    settings_store: &SystemSettingsStore,
) -> SqlResult<StoredSystemChannelSettings> {
    let raw_channel_settings = settings_store.value(super::shared::SYSTEM_KEY_CHANNEL_SETTINGS)?;
    let mut settings = raw_channel_settings
        .as_deref()
        .map(parse_system_channel_settings)
        .transpose()?
        .unwrap_or_else(default_system_channel_settings);

    let query_all_channel_models_present = raw_channel_settings
        .as_deref()
        .map(channel_settings_has_query_all_channel_models)
        .transpose()?
        .unwrap_or(false);
    if !query_all_channel_models_present {
        if let Some(query_all_channel_models) = settings_store
            .value(super::shared::SYSTEM_KEY_MODEL_SETTINGS)?
            .as_deref()
            .map(parse_legacy_query_all_channel_models)
            .transpose()?
            .flatten()
        {
            settings.query_all_channel_models = query_all_channel_models;
        }
    }

    Ok(settings)
}

fn parse_system_channel_settings(raw: &str) -> SqlResult<StoredSystemChannelSettings> {
    serde_json::from_str::<StoredSystemChannelSettings>(raw).map_err(json_setting_decode_error)
}

fn channel_settings_has_query_all_channel_models(raw: &str) -> SqlResult<bool> {
    let value = serde_json::from_str::<Value>(raw).map_err(json_setting_decode_error)?;
    Ok(value
        .as_object()
        .is_some_and(|object| object.contains_key("query_all_channel_models")))
}

fn parse_legacy_query_all_channel_models(raw: &str) -> SqlResult<Option<bool>> {
    #[derive(Deserialize)]
    struct LegacySystemModelSettings {
        query_all_channel_models: Option<bool>,
    }

    serde_json::from_str::<LegacySystemModelSettings>(raw)
        .map(|settings| settings.query_all_channel_models)
        .map_err(json_setting_decode_error)
}

fn json_setting_decode_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn list_routable_model_records(connection: &Connection) -> SqlResult<Vec<StoredModelRecord>> {
    let mut statement = connection.prepare(
        "SELECT supported_models, settings
         FROM channels
         WHERE deleted_at = 0
           AND status = 'enabled'
         ORDER BY ordering_weight DESC, id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut routable_model_ids = std::collections::BTreeSet::new();
    for row in rows {
        let (supported_models_json, settings_json) = row?;
        for entry in super::openai_v1::derive_channel_model_entries(
            supported_models_json.as_str(),
            settings_json.as_str(),
        )
        .into_values()
        {
            routable_model_ids.insert(entry.actual_model_id);
        }
    }

    list_enabled_model_records(connection).map(|records| {
        records
            .into_iter()
            .filter(|record| routable_model_ids.contains(&record.model_id))
            .collect()
    })
}

pub(crate) fn should_retry_openai_error(error: &OpenAiV1Error) -> bool {
    match error {
        OpenAiV1Error::Internal { .. } => true,
        OpenAiV1Error::Upstream { status, .. } => {
            *status == 408 || *status == 409 || *status == 429 || *status >= 500
        }
        OpenAiV1Error::InvalidRequest { .. } => false,
    }
}

pub(crate) fn query_preferred_trace_channel_id(
    connection: &Connection,
    trace_id: i64,
    model_id: &str,
) -> SqlResult<Option<i64>> {
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
) -> SqlResult<super::openai_v1::ChannelRoutingStats> {
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
    let statuses = rows.collect::<SqlResult<Vec<_>>>()?;

    let last_status_failed = statuses.first().is_some_and(|status| status == "failed");
    let consecutive_failures = statuses
        .iter()
        .take_while(|status| status.as_str() == "failed")
        .count() as i64;

    Ok(super::openai_v1::ChannelRoutingStats {
        selection_count,
        processing_count,
        consecutive_failures,
        last_status_failed,
    })
}
