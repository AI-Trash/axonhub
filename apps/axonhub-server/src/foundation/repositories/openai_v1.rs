use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use axonhub_db_entity::{
    api_keys, channels, models, provider_quota_statuses, requests, request_executions, systems,
    usage_logs,
};
use axonhub_http::{OpenAiModel, OpenAiV1Error, OpenAiV1ExecutionRequest, OpenAiV1Route};
use serde::Deserialize;
use serde_json::Value;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Alias, Expr, Func, SimpleExpr};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, ExprTrait, PaginatorTrait, QuerySelect, QueryTrait,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::foundation::{
    admin::provider_quota_type_for_channel,
    admin::default_system_channel_settings,
    circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker},
    openai_v1::{
        calculate_top_k, compare_openai_target_priority, extract_channel_api_key,
        derive_channel_model_entries, request_model_id, resolve_channel_model_entry,
        ChannelRoutingStats, ModelInclude, NewRequestExecutionRecord, NewRequestRecord,
        NewUsageLogRecord, PreparedCompatibilityRequest, SelectedOpenAiTarget,
        StoredModelRecord, UpdateRequestExecutionResultRecord, UpdateRequestResultRecord,
    },
    shared::{SYSTEM_KEY_CHANNEL_SETTINGS, SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_MODEL_SETTINGS},
};

fn int4_to_i64(value: i32) -> i64 {
    i64::from(value)
}

const SYSTEM_KEY_GENERAL_SETTINGS: &str = "system_general_settings";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ParsedApiKeyProfiles {
    #[serde(rename = "activeProfile")]
    active_profile: String,
    profiles: Vec<ParsedApiKeyProfile>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ParsedApiKeyProfile {
    name: String,
    #[serde(rename = "channelIDs")]
    channel_ids: Vec<i64>,
    #[serde(rename = "channelTags")]
    channel_tags: Vec<String>,
    #[serde(rename = "channelTagsMatchMode")]
    channel_tags_match_mode: String,
    #[serde(rename = "modelIDs")]
    model_ids: Vec<String>,
    quota: Option<ParsedApiKeyQuota>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ParsedApiKeyQuota {
    requests: Option<i64>,
    #[serde(rename = "totalTokens")]
    total_tokens: Option<i64>,
    cost: Option<ParsedQuotaCost>,
    period: ParsedApiKeyQuotaPeriod,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ParsedApiKeyQuotaPeriod {
    #[serde(rename = "type")]
    period_type: String,
    #[serde(rename = "pastDuration")]
    past_duration: Option<ParsedApiKeyQuotaPastDuration>,
    #[serde(rename = "calendarDuration")]
    calendar_duration: Option<ParsedApiKeyQuotaCalendarDuration>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ParsedApiKeyQuotaPastDuration {
    value: i64,
    unit: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ParsedApiKeyQuotaCalendarDuration {
    unit: String,
}

#[derive(Debug, Clone, Default)]
struct ParsedQuotaCost {
    value: f64,
    raw: String,
}

#[derive(Debug, Clone)]
struct QuotaWindowBounds {
    start: Option<String>,
    end: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct QuotaUsageAggregate {
    total_tokens: i64,
    total_cost: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ParsedGeneralSettings {
    timezone: String,
}

impl<'de> Deserialize<'de> for ParsedQuotaCost {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(raw) => {
                let numeric = raw.parse::<f64>().map_err(serde::de::Error::custom)?;
                Ok(Self {
                    value: numeric,
                    raw: normalize_decimal_text(raw.as_str()),
                })
            }
            serde_json::Value::Number(number) => {
                let numeric = number
                    .as_f64()
                    .ok_or_else(|| serde::de::Error::custom("invalid quota cost number"))?;
                Ok(Self {
                    value: numeric,
                    raw: normalize_decimal_text(number.to_string().as_str()),
                })
            }
            other => Err(serde::de::Error::custom(format!(
                "unsupported quota cost value: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StoredRequestRouteHint {
    pub(crate) channel_id: i64,
    pub(crate) model_id: String,
}

pub(crate) async fn default_data_storage_id_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
) -> Result<Option<i64>, OpenAiV1Error> {
    systems::Entity::find()
        .filter(systems::Column::Key.eq(SYSTEM_KEY_DEFAULT_DATA_STORAGE))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<systems::KeyValue>()
        .one(db)
        .await
        .map_err(map_openai_db_err)
        .map(|value| value.and_then(|current| current.value.parse::<i64>().ok()))
}

pub(crate) async fn enforce_api_key_quota_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    api_key_id: Option<i64>,
) -> Result<(), OpenAiV1Error> {
    let Some(api_key_id) = api_key_id else {
        return Ok(());
    };

    let Some(quota) = load_active_api_key_quota_seaorm(db, backend, api_key_id).await? else {
        return Ok(());
    };

    let timezone = query_general_settings_timezone_seaorm(db, backend)
        .await?
        .unwrap_or_else(|| "UTC".to_owned());
    let now_epoch_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| OpenAiV1Error::Internal {
            message: format!("Failed to read system time for API key quota check: {error}"),
        })?
        .as_secs() as i64;
    let Some(window) = quota_window_bounds(now_epoch_seconds, timezone.as_str(), &quota.period) else {
        return Ok(());
    };

    if let Some(request_limit) = quota.requests.filter(|limit| *limit > 0) {
        let request_count = query_usage_request_count_seaorm(
            db,
            backend,
            api_key_id,
            window.start.as_deref(),
            window.end.as_deref(),
        )
        .await?;
        if request_count >= request_limit {
            return Err(quota_exceeded_openai_error(format!(
                "requests quota exceeded: {request_count}/{request_limit}"
            )));
        }
    }

    if quota.total_tokens.is_none() && quota.cost.is_none() {
        return Ok(());
    }

    let usage = query_usage_aggregate_seaorm(
        db,
        backend,
        api_key_id,
        window.start.as_deref(),
        window.end.as_deref(),
    )
    .await?;

    if let Some(total_tokens_limit) = quota.total_tokens.filter(|limit| *limit > 0) {
        if usage.total_tokens >= total_tokens_limit {
            return Err(quota_exceeded_openai_error(format!(
                "total_tokens quota exceeded: {}/{total_tokens_limit}",
                usage.total_tokens
            )));
        }
    }

    if let Some(cost_limit) = quota.cost.as_ref() {
        if usage.total_cost >= cost_limit.value {
            return Err(quota_exceeded_openai_error(format!(
                "cost quota exceeded: {}/{}",
                format_decimal_value(usage.total_cost),
                cost_limit.raw
            )));
        }
    }

    Ok(())
}

pub(crate) async fn list_enabled_model_records_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    query_all_channel_models: bool,
    profiles_json: Option<&str>,
) -> Result<Vec<StoredModelRecord>, OpenAiV1Error> {
    let active_profile = active_api_key_profile(profiles_json);
    if query_all_channel_models {
        list_routable_model_records_seaorm(db, backend, active_profile.as_ref()).await
    } else {
        list_explicit_enabled_model_records_seaorm(db, active_profile.as_ref()).await
    }
}

pub(crate) async fn list_enabled_models_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    include: &ModelInclude,
    query_all_channel_models: bool,
    profiles_json: Option<&str>,
) -> Result<Vec<OpenAiModel>, OpenAiV1Error> {
    list_enabled_model_records_seaorm(db, backend, query_all_channel_models, profiles_json)
        .await
        .map(|records| {
            records
                .into_iter()
                .map(|record| record.into_openai_model(include))
                .collect()
        })
}

pub(crate) async fn select_target_channels_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request: &OpenAiV1ExecutionRequest,
    route: OpenAiV1Route,
    circuit_breaker: &SharedCircuitBreaker,
    profiles_json: Option<&str>,
) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
    let request_model = request_model_id(&request.body)?;
    let active_profile = active_api_key_profile(profiles_json);

    if active_profile.as_ref().is_some_and(|profile| {
        !profile.model_ids.is_empty()
            && !profile
                .model_ids
                .iter()
                .any(|candidate| candidate == request_model.as_str())
    }) {
        return Ok(Vec::new());
    }

    let targets = select_openai_route_targets_seaorm(
        db,
        backend,
        request_model.as_str(),
        request.trace.as_ref().map(|trace| trace.id),
        2,
        openai_route_model_type(route),
        request.channel_hint_id,
        circuit_breaker,
        active_profile.as_ref(),
    )
    .await?;

    if targets.is_empty() {
        Err(OpenAiV1Error::InvalidRequest {
            message: "No enabled OpenAI channel is configured for the requested model".to_owned(),
        })
    } else if request
        .channel_hint_id
        .is_some_and(|channel_hint_id| targets[0].channel_id != channel_hint_id)
    {
        Err(OpenAiV1Error::InvalidRequest {
            message: "No enabled OpenAI channel matches the requested channel override".to_owned(),
        })
    } else {
        Ok(targets)
    }
}

async fn select_openai_route_targets_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request_model_id: &str,
    trace_id: Option<i64>,
    max_channel_retries: usize,
    model_type: &str,
    preferred_channel_id: Option<i64>,
    circuit_breaker: &SharedCircuitBreaker,
    active_profile: Option<&ParsedApiKeyProfile>,
) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
    let mut targets = Vec::new();
    for channel_type in ["openai", "codex", "claudecode"] {
        targets.extend(
            select_inference_targets_seaorm(
                db,
                backend,
                request_model_id,
                trace_id,
                max_channel_retries,
                channel_type,
                model_type,
                preferred_channel_id,
                circuit_breaker,
                active_profile,
            )
            .await?,
        );
    }
    targets.sort_by(compare_openai_target_priority);
    if let Some(preferred_channel_id) = preferred_channel_id {
        if let Some(index) = targets.iter().position(|target| target.channel_id == preferred_channel_id) {
            let preferred = targets.remove(index);
            targets.insert(0, preferred);
        }
    }
    let top_k = calculate_top_k(targets.len(), max_channel_retries);
    targets.truncate(top_k);
    Ok(targets)
}

fn openai_route_model_type(route: OpenAiV1Route) -> &'static str {
    match route {
        OpenAiV1Route::ImagesGenerations => "image",
        OpenAiV1Route::ImagesEdits => "image",
        OpenAiV1Route::ImagesVariations => "image",
        OpenAiV1Route::ChatCompletions
        | OpenAiV1Route::Responses
        | OpenAiV1Route::ResponsesCompact
        | OpenAiV1Route::Embeddings
        | OpenAiV1Route::Realtime => "",
    }
}

pub(crate) async fn select_inference_targets_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request_model_id: &str,
    trace_id: Option<i64>,
    max_channel_retries: usize,
    channel_type: &str,
    model_type: &str,
    preferred_channel_id: Option<i64>,
    circuit_breaker: &SharedCircuitBreaker,
    active_profile: Option<&ParsedApiKeyProfile>,
) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
    let preferred_trace_channel_id = match trace_id {
        Some(trace_id) => query_preferred_trace_channel_id_seaorm(db, backend, trace_id, request_model_id).await?,
        None => None,
    };

    let channel_candidates = channels::Entity::find()
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .filter(channels::Column::Status.eq("enabled"))
        .filter(channels::Column::TypeField.eq(channel_type))
        .order_by_desc(channels::Column::OrderingWeight)
        .order_by_asc(channels::Column::Id)
        .into_partial_model::<channels::RoutingCandidate>()
        .all(db)
        .await
        .map_err(map_openai_db_err)?;

    let mut resolved_channels = Vec::new();
    let mut actual_model_ids = BTreeSet::new();
    for channel in channel_candidates {
        if let Some(profile) = active_profile {
            if !profile.channel_ids.is_empty() && !profile.channel_ids.contains(&channel.id) {
                continue;
            }
            if !profile.channel_tags.is_empty()
                && !profile_matches_channel_tags(profile, channel.tags.as_str())
            {
                continue;
            }
        }
        let Some(model_entry) = resolve_channel_model_entry(
            &channel.supported_models,
            &channel.settings,
            request_model_id,
        ) else {
            continue;
        };
        let api_key = extract_channel_api_key(&channel.credentials);
        if api_key.is_empty() {
            continue;
        }

        if provider_channel_is_blocked_seaorm(db, backend, channel.id).await?
            || circuit_breaker.is_blocked(channel.id, model_entry.actual_model_id.as_str())
        {
            continue;
        }

        actual_model_ids.insert(model_entry.actual_model_id.clone());
        resolved_channels.push((channel, model_entry, api_key));
    }

    if resolved_channels.is_empty() {
        return Ok(Vec::new());
    }

    let enabled_model_ids = actual_model_ids.into_iter().collect::<BTreeSet<_>>();
    let enabled_models = models::Entity::find()
        .filter(models::Column::DeletedAt.eq(0_i64))
        .filter(models::Column::Status.eq("enabled"))
        .apply_if(
            (!model_type.is_empty()).then_some(model_type),
            |query, model_type| query.filter(models::Column::TypeField.eq(model_type)),
        )
        .order_by_asc(models::Column::Id)
        .into_partial_model::<models::EnabledModelRecord>()
        .all(db)
        .await
        .map_err(map_openai_db_err)?
        .into_iter()
        .map(stored_model_record_from_enabled_model_record)
        .filter(|model| enabled_model_ids.contains(&model.model_id))
        .collect::<Vec<_>>();
    let enabled_models_by_id = enabled_models
        .into_iter()
        .map(|model| (model.model_id.clone(), model))
        .collect::<HashMap<_, _>>();

    let mut candidates = Vec::new();
    for (channel, model_entry, api_key) in resolved_channels {
        let Some(model) = enabled_models_by_id.get(&model_entry.actual_model_id).cloned() else {
            continue;
        };
        let channel_id = channel.id;
        let ordering_weight = int4_to_i64(channel.ordering_weight);
        let routing_stats = query_channel_routing_stats_seaorm(db, backend, channel_id).await?;
        candidates.push(SelectedOpenAiTarget {
            channel_id,
            base_url: channel.base_url.unwrap_or_default(),
            api_key,
            actual_model_id: model_entry.actual_model_id,
            provider_type: provider_quota_type_for_channel(channel_type).map(str::to_owned),
            ordering_weight,
            trace_affinity: preferred_trace_channel_id == Some(channel_id),
            circuit_breaker: circuit_breaker.current_snapshot(channel_id, model.model_id.as_str()),
            routing_stats,
            model,
        });
    }

    candidates.sort_by(compare_openai_target_priority);
    if let Some(preferred_channel_id) = preferred_channel_id {
        if let Some(index) = candidates.iter().position(|target| target.channel_id == preferred_channel_id) {
            let preferred = candidates.remove(index);
            candidates.insert(0, preferred);
        }
    }
    let top_k = calculate_top_k(candidates.len(), max_channel_retries);
    candidates.truncate(top_k);
    Ok(candidates)
}

async fn provider_channel_is_blocked_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    channel_id: i64,
) -> Result<bool, OpenAiV1Error> {
    let Some(row) = provider_quota_statuses::Entity::find()
        .filter(provider_quota_statuses::Column::ChannelId.eq(channel_id))
        .one(db)
        .await
        .map_err(map_openai_db_err)?
    else {
        return Ok(false);
    };
    Ok(!row.ready || row.status.eq_ignore_ascii_case("exhausted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue::Set;
    use sea_orm::sea_query::OnConflict;

    use crate::foundation::circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker};
    use crate::foundation::seaorm::SeaOrmConnectionFactory;

    async fn insert_channel(
        db: &impl ConnectionTrait,
        name: &str,
        supported_models: &str,
        settings: &str,
        ordering_weight: i32,
    ) -> i64 {
        channels::Entity::insert(channels::ActiveModel {
            type_field: Set("openai".to_owned()),
            base_url: Set(Some(format!("https://{name}.example/v1"))),
            name: Set(name.to_owned()),
            status: Set("enabled".to_owned()),
            credentials: Set(r#"{"apiKey":"test-upstream-key"}"#.to_owned()),
            disabled_api_keys: Set("[]".to_owned()),
            supported_models: Set(supported_models.to_owned()),
            manual_models: Set("[]".to_owned()),
            auto_sync_supported_models: Set(false),
            auto_sync_model_pattern: Set(String::new()),
            tags: Set("[]".to_owned()),
            default_test_model: Set(String::new()),
            policies: Set("{}".to_owned()),
            settings: Set(settings.to_owned()),
            ordering_weight: Set(ordering_weight),
            error_message: Set(Some(String::new())),
            remark: Set(Some("repository test".to_owned())),
            deleted_at: Set(0),
            ..Default::default()
        })
        .exec(db)
        .await
        .unwrap()
        .last_insert_id
    }

    async fn insert_model(db: &impl ConnectionTrait, model_id: &str) {
        models::Entity::insert(models::ActiveModel {
            developer: Set("openai".to_owned()),
            model_id: Set(model_id.to_owned()),
            type_field: Set("chat".to_owned()),
            name: Set(model_id.to_owned()),
            icon: Set("OpenAI".to_owned()),
            group_name: Set("openai".to_owned()),
            model_card: Set("{}".to_owned()),
            settings: Set("{}".to_owned()),
            status: Set("enabled".to_owned()),
            remark: Set(Some("repository test".to_owned())),
            deleted_at: Set(0),
            ..Default::default()
        })
        .exec(db)
        .await
        .unwrap();
    }

    async fn upsert_system_channel_settings(db: &impl ConnectionTrait, value: &str) {
        systems::Entity::insert(systems::ActiveModel {
            key: Set(SYSTEM_KEY_CHANNEL_SETTINGS.to_owned()),
            value: Set(value.to_owned()),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(systems::Column::Key)
                .update_column(systems::Column::Value)
                .to_owned(),
        )
        .exec(db)
        .await
        .unwrap();
    }

    async fn upsert_system_model_settings(db: &impl ConnectionTrait, value: &str) {
        systems::Entity::insert(systems::ActiveModel {
            key: Set(SYSTEM_KEY_MODEL_SETTINGS.to_owned()),
            value: Set(value.to_owned()),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(systems::Column::Key)
                .update_column(systems::Column::Value)
                .to_owned(),
        )
        .exec(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn select_inference_targets_seaorm_resolves_model_mapping_prefix_and_auto_trim_actual_models() {
        let factory = SeaOrmConnectionFactory::sqlite(":memory:".to_owned());
        let db = factory.connect_migrated().await.unwrap();

        let mapped_channel_id = insert_channel(
            &db,
            "mapped-channel",
            r#"["actual-mapped-model"]"#,
            r#"{"modelMappings":[{"from":"mapped-model","to":"actual-mapped-model"}]}"#,
            100,
        )
        .await;
        let prefixed_channel_id = insert_channel(
            &db,
            "prefixed-channel",
            r#"["prefixed-actual-model"]"#,
            r#"{"extraModelPrefix":"vendor"}"#,
            90,
        )
        .await;
        let trimmed_channel_id = insert_channel(
            &db,
            "trimmed-channel",
            r#"["provider/trimmed-actual-model"]"#,
            r#"{"autoTrimedModelPrefixes":["provider"]}"#,
            80,
        )
        .await;

        insert_model(&db, "actual-mapped-model").await;
        insert_model(&db, "prefixed-actual-model").await;
        insert_model(&db, "provider/trimmed-actual-model").await;

        let mapped_targets = select_inference_targets_seaorm(
            &db,
            DatabaseBackend::Sqlite,
            "mapped-model",
            None,
            2,
            "openai",
            "chat",
            None,
            &SharedCircuitBreaker::new(CircuitBreakerPolicy::default()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(mapped_targets.len(), 1);
        assert_eq!(mapped_targets[0].channel_id, mapped_channel_id);
        assert_eq!(mapped_targets[0].actual_model_id, "actual-mapped-model");
        assert_eq!(mapped_targets[0].model.model_id, "actual-mapped-model");

        let prefixed_targets = select_inference_targets_seaorm(
            &db,
            DatabaseBackend::Sqlite,
            "vendor/prefixed-actual-model",
            None,
            2,
            "openai",
            "chat",
            None,
            &SharedCircuitBreaker::new(CircuitBreakerPolicy::default()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(prefixed_targets.len(), 1);
        assert_eq!(prefixed_targets[0].channel_id, prefixed_channel_id);
        assert_eq!(prefixed_targets[0].actual_model_id, "prefixed-actual-model");
        assert_eq!(prefixed_targets[0].model.model_id, "prefixed-actual-model");

        let trimmed_targets = select_inference_targets_seaorm(
            &db,
            DatabaseBackend::Sqlite,
            "trimmed-actual-model",
            None,
            2,
            "openai",
            "chat",
            None,
            &SharedCircuitBreaker::new(CircuitBreakerPolicy::default()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(trimmed_targets.len(), 1);
        assert_eq!(trimmed_targets[0].channel_id, trimmed_channel_id);
        assert_eq!(trimmed_targets[0].actual_model_id, "provider/trimmed-actual-model");
        assert_eq!(trimmed_targets[0].model.model_id, "provider/trimmed-actual-model");
    }

    #[tokio::test]
    async fn list_enabled_model_records_seaorm_defaults_to_routable_models() {
        let factory = SeaOrmConnectionFactory::sqlite(":memory:".to_owned());
        let db = factory.connect_migrated().await.unwrap();

        insert_channel(
            &db,
            "mapped-channel",
            r#"["actual-model"]"#,
            r#"{"modelMappings":[{"from":"alias-model","to":"actual-model"}]}"#,
            100,
        )
        .await;
        insert_model(&db, "actual-model").await;
        insert_model(&db, "alias-model").await;

        let settings = query_system_channel_settings_seaorm(&db).await.unwrap();
        assert!(settings.query_all_channel_models);

        let models = list_enabled_model_records_seaorm(
            &db,
            DatabaseBackend::Sqlite,
            settings.query_all_channel_models,
            None,
        )
        .await
        .unwrap();
        let model_ids = models.into_iter().map(|model| model.model_id).collect::<Vec<_>>();

        assert_eq!(model_ids, vec!["actual-model"]);
    }

    #[tokio::test]
    async fn list_enabled_model_records_seaorm_returns_explicit_models_when_query_all_channel_models_disabled() {
        let factory = SeaOrmConnectionFactory::sqlite(":memory:".to_owned());
        let db = factory.connect_migrated().await.unwrap();

        insert_channel(
            &db,
            "mapped-channel",
            r#"["actual-model"]"#,
            r#"{"modelMappings":[{"from":"alias-model","to":"actual-model"}]}"#,
            100,
        )
        .await;
        insert_model(&db, "actual-model").await;
        insert_model(&db, "alias-model").await;
        upsert_system_channel_settings(
            &db,
            r#"{"probe":{"enabled":true,"frequency":"FiveMinutes"},"query_all_channel_models":false}"#,
        )
        .await;

        let settings = query_system_channel_settings_seaorm(&db).await.unwrap();
        assert!(!settings.query_all_channel_models);

        let models = list_enabled_model_records_seaorm(
            &db,
            DatabaseBackend::Sqlite,
            settings.query_all_channel_models,
            None,
        )
        .await
        .unwrap();
        let model_ids = models.into_iter().map(|model| model.model_id).collect::<Vec<_>>();

        assert_eq!(model_ids, vec!["actual-model", "alias-model"]);
    }

    #[tokio::test]
    async fn query_system_channel_settings_seaorm_falls_back_to_legacy_model_settings() {
        let factory = SeaOrmConnectionFactory::sqlite(":memory:".to_owned());
        let db = factory.connect_migrated().await.unwrap();

        upsert_system_model_settings(&db, r#"{"query_all_channel_models":false}"#).await;

        let settings = query_system_channel_settings_seaorm(&db).await.unwrap();

        assert!(!settings.query_all_channel_models);
        assert!(settings.probe.enabled);
    }

    #[tokio::test]
    async fn list_enabled_model_records_seaorm_uses_channel_settings_when_deriving_routable_models() {
        let factory = SeaOrmConnectionFactory::sqlite(":memory:".to_owned());
        let db = factory.connect_migrated().await.unwrap();

        insert_channel(
            &db,
            "hidden-direct-channel",
            r#"["actual-model"]"#,
            r#"{"hideOriginalModels":true}"#,
            100,
        )
        .await;
        insert_model(&db, "actual-model").await;

        let settings = query_system_channel_settings_seaorm(&db).await.unwrap();
        assert!(settings.query_all_channel_models);

        let models = list_enabled_model_records_seaorm(
            &db,
            DatabaseBackend::Sqlite,
            settings.query_all_channel_models,
            None,
        )
        .await
        .unwrap();

        assert!(models.is_empty());
    }
}

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use super::*;
    use rusqlite::{params, Connection as SqlConnection, OptionalExtension, Result as SqlResult};

    use crate::foundation::{
        admin::{default_system_channel_settings, StoredSystemChannelSettings},
        openai_v1::{
            extract_channel_api_key,
            ChannelRoutingStats, ModelInclude, NewChannelRecord, NewModelRecord,
            NewRequestExecutionRecord, NewRequestRecord, NewUsageLogRecord, SelectedOpenAiTarget,
            StoredModelRecord, StoredRequestSummary, UpdateRequestExecutionResultRecord,
            UpdateRequestResultRecord,
        },
        shared::{bool_to_sql, USAGE_LOGS_TABLE_SQL},
        system::sqlite_test_support::{
            ensure_channel_model_tables, ensure_operational_tables, ensure_request_tables,
            SqliteConnectionFactory, SystemSettingsStore,
        },
    };

    #[derive(Debug, Clone)]
    pub struct ChannelModelStore {
        pub(crate) connection_factory: SqliteConnectionFactory,
    }

    impl ChannelModelStore {
        pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
            Self { connection_factory }
        }

        pub fn ensure_schema(&self) -> SqlResult<()> {
            let connection = self.connection_factory.open(true)?;
            ensure_channel_model_tables(&connection)
        }

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

        pub fn list_channels(&self) -> SqlResult<Vec<crate::foundation::openai_v1::StoredChannelSummary>> {
            let connection = self.connection_factory.open(true)?;
            ensure_channel_model_tables(&connection)?;
            let mut statement = connection.prepare(
                "SELECT id, name, type, base_url, status, supported_models, ordering_weight
                 FROM channels
                 WHERE deleted_at = 0
                 ORDER BY ordering_weight DESC, id ASC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(crate::foundation::openai_v1::StoredChannelSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    channel_type: row.get(2)?,
                    base_url: row.get(3)?,
                    status: row.get(4)?,
                    supported_models: crate::foundation::identity::parse_json_string_vec(row.get::<_, String>(5)?),
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
            circuit_breaker: &SharedCircuitBreaker,
        ) -> SqlResult<Vec<SelectedOpenAiTarget>> {
            let connection = self.connection_factory.open(true)?;
            ensure_channel_model_tables(&connection)?;
            ensure_request_tables(&connection)?;
            ensure_operational_tables(&connection)?;

            let mut statement = connection.prepare(
                "SELECT c.id, c.base_url, c.credentials, c.supported_models, c.ordering_weight,
                        c.settings, m.created_at, m.developer, m.model_id, m.type, m.name, m.icon, m.remark, m.model_card, c.type
                  FROM channels c
                  JOIN models m ON m.deleted_at = 0
                  WHERE c.deleted_at = 0
                    AND c.status = 'enabled'
                    AND m.status = 'enabled'
                    AND c.type = ?1
                    AND (?2 = '' OR m.type = ?2)
                  ORDER BY c.ordering_weight DESC, c.id ASC",
            )?;
            let mut rows = statement.query(params![channel_type, model_type])?;
            let preferred_trace_channel_id = trace_id
                .map(|trace_id| query_preferred_trace_channel_id(&connection, trace_id, request_model_id))
                .transpose()?
                .flatten();
            let mut candidates = Vec::new();

            while let Some(row) = rows.next()? {
                let supported_models_json: String = row.get(3)?;
                let settings_json: String = row.get(5)?;
                let Some(entry) = resolve_channel_model_entry(
                    supported_models_json.as_str(),
                    settings_json.as_str(),
                    request_model_id,
                ) else {
                    continue;
                };

                let credentials_json: String = row.get(2)?;
                let api_key = extract_channel_api_key(&credentials_json);
                if api_key.is_empty() {
                    continue;
                }

                let channel_id: i64 = row.get(0)?;
                let ordering_weight: i64 = row.get(4)?;
                let actual_model_id = entry.actual_model_id;
                let routing_stats = query_channel_routing_stats(&connection, channel_id)?;
                if provider_channel_is_blocked(&connection, channel_id)?
                    || circuit_breaker.is_blocked(channel_id, actual_model_id.as_str())
                {
                    continue;
                }

                let model = StoredModelRecord {
                    id: 0,
                    created_at: row.get(6)?,
                    developer: row.get(7)?,
                    model_id: actual_model_id.clone(),
                    model_type: row.get(9)?,
                    name: row.get(10)?,
                    icon: row.get(11)?,
                    remark: row.get(12)?,
                    model_card_json: row.get(13)?,
                };

                candidates.push(SelectedOpenAiTarget {
                    channel_id,
                    base_url: row.get(1)?,
                    api_key,
                    actual_model_id: actual_model_id.clone(),
                    provider_type: provider_quota_type_for_channel(&row.get::<_, String>(14)?).map(str::to_owned),
                    ordering_weight,
                    trace_affinity: preferred_trace_channel_id == Some(channel_id),
                    circuit_breaker: circuit_breaker.current_snapshot(channel_id, actual_model_id.as_str()),
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

        pub fn create_request_execution(&self, record: &NewRequestExecutionRecord<'_>) -> SqlResult<i64> {
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

        pub fn list_requests_by_project(&self, project_id: i64) -> SqlResult<Vec<StoredRequestSummary>> {
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

    fn list_enabled_model_records(connection: &SqlConnection) -> SqlResult<Vec<StoredModelRecord>> {
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
        connection: &SqlConnection,
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
        let raw_channel_settings = settings_store.value(crate::foundation::shared::SYSTEM_KEY_CHANNEL_SETTINGS)?;
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
                .value(crate::foundation::shared::SYSTEM_KEY_MODEL_SETTINGS)?
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

    fn list_routable_model_records(connection: &SqlConnection) -> SqlResult<Vec<StoredModelRecord>> {
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

        let mut routable_model_ids = BTreeSet::new();
        for row in rows {
            let (supported_models_json, settings_json) = row?;
            for entry in derive_channel_model_entries(
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

    fn provider_channel_is_blocked(connection: &SqlConnection, channel_id: i64) -> SqlResult<bool> {
        let row: Option<(String, i64)> = connection
            .query_row(
                "SELECT status, ready FROM provider_quota_statuses WHERE channel_id = ?1 LIMIT 1",
                [channel_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(row.is_some_and(|(status, ready)| ready == 0 || status.eq_ignore_ascii_case("exhausted")))
    }

    fn query_preferred_trace_channel_id(
        connection: &SqlConnection,
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

    fn query_channel_routing_stats(
        connection: &SqlConnection,
        channel_id: i64,
    ) -> SqlResult<ChannelRoutingStats> {
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

        Ok(ChannelRoutingStats {
            selection_count,
            processing_count,
            consecutive_failures,
            last_status_failed,
        })
    }
}

async fn load_active_api_key_quota_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    api_key_id: i64,
) -> Result<Option<ParsedApiKeyQuota>, OpenAiV1Error> {
    let profiles_json = api_keys::Entity::find_by_id(api_key_id)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<api_keys::ProfilesOnly>()
        .one(db)
        .await
        .map_err(map_openai_db_err)?
        .map(|row| row.profiles);

    Ok(profiles_json.and_then(|raw| active_api_key_quota(raw.as_str())))
}

async fn query_general_settings_timezone_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
) -> Result<Option<String>, OpenAiV1Error> {
    let value = systems::Entity::find()
        .filter(systems::Column::Key.eq(SYSTEM_KEY_GENERAL_SETTINGS))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<systems::KeyValue>()
        .one(db)
        .await
        .map_err(map_openai_db_err)?
        .map(|row| row.value);

    Ok(value.and_then(|raw| {
        serde_json::from_str::<ParsedGeneralSettings>(&raw)
            .ok()
            .map(|settings| settings.timezone.trim().to_owned())
            .filter(|timezone| !timezone.is_empty())
    }))
}

async fn query_usage_request_count_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    api_key_id: i64,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<i64, OpenAiV1Error> {
    let row = db
        .query_one_raw(
            usage_logs::Entity::find()
                .select_only()
                .expr(Expr::cust("COUNT(*)"))
                .filter(usage_logs::Column::ApiKeyId.eq(api_key_id))
                .apply_if(start, |query, start| {
                    query.filter(Expr::expr(usage_created_at_expr(backend)).gte(usage_window_bound_expr(backend, start)))
                })
                .apply_if(end, |query, end| {
                    query.filter(Expr::expr(usage_created_at_expr(backend)).lt(usage_window_bound_expr(backend, end)))
                })
                .build(backend),
        )
        .await
        .map_err(map_openai_db_err)?;

    Ok(row
        .and_then(|row| row.try_get_by_index::<i64>(0).ok())
        .unwrap_or_default())
}

async fn query_usage_aggregate_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    api_key_id: i64,
    start: Option<&str>,
    end: Option<&str>,
) -> Result<QuotaUsageAggregate, OpenAiV1Error> {
    let row = db
        .query_one_raw(
            usage_logs::Entity::find()
                .select_only()
                .expr(Expr::cust("COALESCE(SUM(total_tokens), 0)"))
                .expr(Expr::cust("COALESCE(SUM(total_cost), 0)"))
                .filter(usage_logs::Column::ApiKeyId.eq(api_key_id))
                .apply_if(start, |query, start| {
                    query.filter(Expr::expr(usage_created_at_expr(backend)).gte(usage_window_bound_expr(backend, start)))
                })
                .apply_if(end, |query, end| {
                    query.filter(Expr::expr(usage_created_at_expr(backend)).lt(usage_window_bound_expr(backend, end)))
                })
                .build(backend),
        )
        .await
        .map_err(map_openai_db_err)?;

    Ok(row
        .map(|row| QuotaUsageAggregate {
            total_tokens: row.try_get_by_index(0).unwrap_or_default(),
            total_cost: row.try_get_by_index(1).unwrap_or_default(),
        })
        .unwrap_or_default())
}

fn usage_created_at_expr(backend: DatabaseBackend) -> SimpleExpr {
    match backend {
        DatabaseBackend::Sqlite => Func::cust(Alias::new("datetime"))
            .arg(Expr::col(usage_logs::Column::CreatedAt))
            .into(),
        DatabaseBackend::Postgres | DatabaseBackend::MySql => Expr::col(usage_logs::Column::CreatedAt).into(),
        _ => unreachable!("unsupported database backend: {:?}", backend),
    }
}

fn usage_window_bound_expr(backend: DatabaseBackend, bound: &str) -> SimpleExpr {
    match backend {
        DatabaseBackend::Sqlite => Func::cust(Alias::new("datetime"))
            .arg(Expr::value(bound.to_owned()))
            .into(),
        DatabaseBackend::Postgres => Expr::value(bound.to_owned()).cast_as("TIMESTAMPTZ"),
        DatabaseBackend::MySql => Expr::value(bound.to_owned()).cast_as("DATETIME"),
        _ => unreachable!("unsupported database backend: {:?}", backend),
    }
}

fn active_api_key_quota(raw: &str) -> Option<ParsedApiKeyQuota> {
    let parsed = serde_json::from_str::<ParsedApiKeyProfiles>(raw).ok()?;
    if parsed.active_profile.is_empty() {
        return None;
    }

    parsed
        .profiles
        .into_iter()
        .find(|profile| profile.name == parsed.active_profile)
        .and_then(|profile| profile.quota)
}

fn active_api_key_profile(raw: Option<&str>) -> Option<ParsedApiKeyProfile> {
    let parsed = serde_json::from_str::<ParsedApiKeyProfiles>(raw?).ok()?;
    if parsed.active_profile.is_empty() {
        return None;
    }

    parsed
        .profiles
        .into_iter()
        .find(|profile| profile.name == parsed.active_profile)
}

fn profile_matches_channel_tags(profile: &ParsedApiKeyProfile, raw_tags: &str) -> bool {
    let tags = serde_json::from_str::<Vec<String>>(raw_tags).unwrap_or_default();
    if profile.channel_tags.is_empty() {
        return true;
    }

    if profile.channel_tags_match_mode.eq_ignore_ascii_case("all") {
        profile.channel_tags.iter().all(|tag| tags.contains(tag))
    } else {
        tags.iter().any(|tag| profile.channel_tags.contains(tag))
    }
}

fn quota_window_bounds(
    now_epoch_seconds: i64,
    timezone: &str,
    period: &ParsedApiKeyQuotaPeriod,
) -> Option<QuotaWindowBounds> {
    let timezone = timezone.trim();
    let use_utc_calendar = timezone.is_empty() || timezone.eq_ignore_ascii_case("UTC");

    match period.period_type.as_str() {
        "all_time" => Some(QuotaWindowBounds {
            start: None,
            end: Some(format_sql_utc_timestamp(now_epoch_seconds)),
        }),
        "past_duration" => {
            let duration = period.past_duration.as_ref()?;
            let duration_seconds = match duration.unit.as_str() {
                "minute" if duration.value > 0 => duration.value.checked_mul(60)?,
                "hour" if duration.value > 0 => duration.value.checked_mul(60 * 60)?,
                "day" if duration.value > 0 => duration.value.checked_mul(24 * 60 * 60)?,
                _ => return None,
            };
            let start_epoch_seconds = now_epoch_seconds.checked_sub(duration_seconds)?;
            Some(QuotaWindowBounds {
                start: Some(format_sql_utc_timestamp(start_epoch_seconds)),
                end: Some(format_sql_utc_timestamp(now_epoch_seconds)),
            })
        }
        "calendar_duration" if use_utc_calendar => {
            let calendar = period.calendar_duration.as_ref()?;
            match calendar.unit.as_str() {
                "day" => {
                    let start_epoch_seconds = start_of_day_epoch_seconds(now_epoch_seconds);
                    Some(QuotaWindowBounds {
                        start: Some(format_sql_utc_timestamp(start_epoch_seconds)),
                        end: Some(format_sql_utc_timestamp(start_epoch_seconds + 24 * 60 * 60)),
                    })
                }
                "month" => {
                    let (year, month, _) = civil_from_epoch_seconds(now_epoch_seconds);
                    let start_epoch_seconds = epoch_seconds_from_civil(year, month, 1);
                    let (next_year, next_month) = if month == 12 {
                        (year + 1, 1)
                    } else {
                        (year, month + 1)
                    };
                    let end_epoch_seconds = epoch_seconds_from_civil(next_year, next_month, 1);
                    Some(QuotaWindowBounds {
                        start: Some(format_sql_utc_timestamp(start_epoch_seconds)),
                        end: Some(format_sql_utc_timestamp(end_epoch_seconds)),
                    })
                }
                _ => None,
            }
        }
        "calendar_duration" => Some(QuotaWindowBounds {
            start: None,
            end: Some(format_sql_utc_timestamp(now_epoch_seconds)),
        }),
        _ => None,
    }
}

fn quota_exceeded_openai_error(message: String) -> OpenAiV1Error {
    OpenAiV1Error::Upstream {
        status: 403,
        body: serde_json::json!({
            "error": {
                "message": message,
                "type": "quota_exceeded_error",
                "code": "quota_exceeded"
            }
        }),
    }
}

fn normalize_decimal_text(value: &str) -> String {
    let trimmed = value.trim();
    if let Some((whole, fraction)) = trimmed.split_once('.') {
        let normalized_fraction = fraction.trim_end_matches('0');
        if normalized_fraction.is_empty() {
            whole.to_owned()
        } else {
            format!("{whole}.{normalized_fraction}")
        }
    } else {
        trimmed.to_owned()
    }
}

fn format_decimal_value(value: f64) -> String {
    normalize_decimal_text(format!("{value:.12}").as_str())
}

fn start_of_day_epoch_seconds(epoch_seconds: i64) -> i64 {
    epoch_seconds - epoch_seconds.rem_euclid(24 * 60 * 60)
}

fn format_sql_utc_timestamp(epoch_seconds: i64) -> String {
    let (year, month, day) = civil_from_epoch_seconds(epoch_seconds);
    let seconds_of_day = epoch_seconds.rem_euclid(24 * 60 * 60);
    let hour = seconds_of_day / (60 * 60);
    let minute = (seconds_of_day % (60 * 60)) / 60;
    let second = seconds_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
    )
}

fn civil_from_epoch_seconds(epoch_seconds: i64) -> (i32, u32, u32) {
    let epoch_days = epoch_seconds.div_euclid(24 * 60 * 60);
    civil_from_days(epoch_days)
}

fn epoch_seconds_from_civil(year: i32, month: u32, day: u32) -> i64 {
    days_from_civil(year, month, day) * 24 * 60 * 60
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let shifted = days_since_epoch + 719_468;
    let era = if shifted >= 0 {
        shifted / 146_097
    } else {
        (shifted - 146_096) / 146_097
    };
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096)
        / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = i64::from(year) - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year / 400
    } else {
        (adjusted_year - 399) / 400
    };
    let year_of_era = adjusted_year - era * 400;
    let month = i64::from(month);
    let day = i64::from(day);
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

async fn list_routable_model_records_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    active_profile: Option<&ParsedApiKeyProfile>,
) -> Result<Vec<StoredModelRecord>, OpenAiV1Error> {
    let enabled_channels = channels::Entity::find()
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .filter(channels::Column::Status.eq("enabled"))
        .order_by_desc(channels::Column::OrderingWeight)
        .order_by_asc(channels::Column::Id)
        .into_partial_model::<channels::RoutingCandidate>()
        .all(db)
        .await
        .map_err(map_openai_db_err)?;

    let enabled_models = models::Entity::find()
        .filter(models::Column::DeletedAt.eq(0_i64))
        .filter(models::Column::Status.eq("enabled"))
        .order_by_asc(models::Column::Id)
        .into_partial_model::<models::EnabledModelRecord>()
        .all(db)
        .await
        .map_err(map_openai_db_err)?;

    let mut routable_model_ids = std::collections::BTreeSet::new();
    for channel in enabled_channels {
        if let Some(profile) = active_profile {
            if !profile.channel_ids.is_empty() && !profile.channel_ids.contains(&channel.id) {
                continue;
            }
            if !profile.channel_tags.is_empty()
                && !profile_matches_channel_tags(profile, channel.tags.as_str())
            {
                continue;
            }
        }
        let api_key = extract_channel_api_key(&channel.credentials);
        if api_key.is_empty() {
            continue;
        }

        for entry in derive_channel_model_entries(&channel.supported_models, &channel.settings).into_values() {
            routable_model_ids.insert(entry.actual_model_id);
        }
    }

    Ok(enabled_models
        .into_iter()
        .filter(|model| {
            active_profile
                .map(|profile| {
                    profile.model_ids.is_empty() || profile.model_ids.contains(&model.model_id)
                })
                .unwrap_or(true)
        })
        .filter(|model| routable_model_ids.contains(&model.model_id))
        .map(stored_model_record_from_enabled_model_record)
        .collect())
}

async fn list_explicit_enabled_model_records_seaorm(
    db: &impl ConnectionTrait,
    active_profile: Option<&ParsedApiKeyProfile>,
) -> Result<Vec<StoredModelRecord>, OpenAiV1Error> {
    models::Entity::find()
        .filter(models::Column::DeletedAt.eq(0_i64))
        .filter(models::Column::Status.eq("enabled"))
        .order_by_asc(models::Column::Id)
        .into_partial_model::<models::EnabledModelRecord>()
        .all(db)
        .await
        .map_err(map_openai_db_err)
        .map(|rows| {
            rows.into_iter()
                .filter(|model| {
                    active_profile
                        .map(|profile| {
                            profile.model_ids.is_empty()
                                || profile.model_ids.contains(&model.model_id)
                        })
                        .unwrap_or(true)
                })
                .map(stored_model_record_from_enabled_model_record)
                .collect()
        })
}

pub(crate) async fn query_system_channel_settings_seaorm(
    db: &impl ConnectionTrait,
) -> Result<crate::foundation::admin::StoredSystemChannelSettings, OpenAiV1Error> {
    let raw_channel_settings = systems::Entity::find()
        .filter(systems::Column::Key.eq(SYSTEM_KEY_CHANNEL_SETTINGS))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<systems::KeyValue>()
        .one(db)
        .await
        .map_err(map_openai_db_err)?
        .map(|row| row.value);

    let mut settings = raw_channel_settings
        .as_deref()
        .map(parse_system_channel_settings_seaorm)
        .transpose()?
        .unwrap_or_else(default_system_channel_settings);

    let query_all_channel_models_present = raw_channel_settings
        .as_deref()
        .map(channel_settings_has_query_all_channel_models_seaorm)
        .transpose()?
        .unwrap_or(false);
    if !query_all_channel_models_present {
        if let Some(query_all_channel_models) = systems::Entity::find()
            .filter(systems::Column::Key.eq(SYSTEM_KEY_MODEL_SETTINGS))
            .filter(systems::Column::DeletedAt.eq(0_i64))
            .into_partial_model::<systems::KeyValue>()
            .one(db)
            .await
            .map_err(map_openai_db_err)?
            .map(|row| row.value)
            .as_deref()
            .map(parse_legacy_query_all_channel_models_seaorm)
            .transpose()?
            .flatten()
        {
            settings.query_all_channel_models = query_all_channel_models;
        }
    }

    Ok(settings)
}

fn parse_system_channel_settings_seaorm(
    raw: &str,
) -> Result<crate::foundation::admin::StoredSystemChannelSettings, OpenAiV1Error> {
    serde_json::from_str::<crate::foundation::admin::StoredSystemChannelSettings>(raw).map_err(
        |error| OpenAiV1Error::Internal {
            message: format!("Failed to decode system channel settings: {error}"),
        },
    )
}

fn channel_settings_has_query_all_channel_models_seaorm(
    raw: &str,
) -> Result<bool, OpenAiV1Error> {
    let value = serde_json::from_str::<Value>(raw).map_err(|error| OpenAiV1Error::Internal {
        message: format!("Failed to decode system channel settings: {error}"),
    })?;
    Ok(value
        .as_object()
        .is_some_and(|object| object.contains_key("query_all_channel_models")))
}

fn parse_legacy_query_all_channel_models_seaorm(raw: &str) -> Result<Option<bool>, OpenAiV1Error> {
    #[derive(Debug, Clone, Default, Deserialize)]
    #[serde(default)]
    struct LegacySystemModelSettings {
        query_all_channel_models: Option<bool>,
    }

    serde_json::from_str::<LegacySystemModelSettings>(raw)
        .map(|settings| settings.query_all_channel_models)
        .map_err(|error| OpenAiV1Error::Internal {
            message: format!("Failed to decode legacy system model settings: {error}"),
        })
}

pub(crate) async fn select_doubao_task_targets_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request: &OpenAiV1ExecutionRequest,
    prepared: &PreparedCompatibilityRequest,
) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
    let active_profile = active_api_key_profile(request.api_key.profiles_json.as_deref());
    let task_id = prepared
        .task_id
        .as_deref()
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "task id is required".to_owned(),
        })?;
    let request_hint = find_latest_completed_request_by_external_id_seaorm(
        db,
        backend,
        "doubao/video_create",
        task_id,
    )
    .await?
    .ok_or_else(|| OpenAiV1Error::Upstream {
        status: 404,
        body: serde_json::json!({"error": {"message": "not found"}}),
    })?;
    let mut targets = select_inference_targets_seaorm(
        db,
        backend,
        request_hint.model_id.as_str(),
        request.trace.as_ref().map(|trace| trace.id),
        2,
        prepared.channel_type,
        prepared.model_type,
        None,
        &SharedCircuitBreaker::new(CircuitBreakerPolicy::default()),
        active_profile.as_ref(),
    )
    .await?;
    if let Some(index) = targets.iter().position(|target| target.channel_id == request_hint.channel_id) {
        let preferred = targets.remove(index);
        targets.insert(0, preferred);
        Ok(targets)
    } else {
        Err(OpenAiV1Error::Upstream {
            status: 404,
            body: serde_json::json!({"error": {"message": "not found"}}),
        })
    }
}

pub(crate) async fn find_latest_completed_request_by_external_id_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    route_format: &str,
    external_id: &str,
) -> Result<Option<StoredRequestRouteHint>, OpenAiV1Error> {
    requests::Entity::find()
        .filter(requests::Column::Format.eq(route_format))
        .filter(requests::Column::ExternalId.eq(external_id))
        .filter(requests::Column::Status.eq("completed"))
        .filter(requests::Column::ChannelId.is_not_null())
        .order_by_desc(requests::Column::Id)
        .into_partial_model::<requests::RouteHint>()
        .one(db)
        .await
        .map_err(map_openai_db_err)
        .map(|row| {
            row.map(|row| StoredRequestRouteHint {
                channel_id: row.channel_id.unwrap_or_default(),
                model_id: row.model_id,
            })
        })
}

pub(crate) async fn create_request_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    record: &NewRequestRecord<'_>,
) -> Result<i64, OpenAiV1Error> {
    let mut model = requests::ActiveModel {
        api_key_id: Set(record.api_key_id),
        project_id: Set(record.project_id),
        trace_id: Set(record.trace_id),
        data_storage_id: Set(record.data_storage_id),
        source: Set(record.source.to_owned()),
        model_id: Set(record.model_id.to_owned()),
        format: Set(record.format.to_owned()),
        request_headers: Set(Some(record.request_headers_json.to_owned())),
        request_body: Set(record.request_body_json.to_owned()),
        response_body: Set(record.response_body_json.map(ToOwned::to_owned)),
        response_chunks: Set(record.response_chunks_json.map(ToOwned::to_owned)),
        channel_id: Set(record.channel_id),
        external_id: Set(record.external_id.map(ToOwned::to_owned)),
        status: Set(record.status.to_owned()),
        stream: Set(record.stream),
        client_ip: Set(record.client_ip.to_owned()),
        metrics_latency_ms: Set(record.metrics_latency_ms),
        metrics_first_token_latency_ms: Set(record.metrics_first_token_latency_ms),
        content_saved: Set(record.content_saved),
        content_storage_id: Set(record.content_storage_id),
        content_storage_key: Set(record.content_storage_key.map(ToOwned::to_owned)),
        ..Default::default()
    };
    if let Some(content_saved_at) = record.content_saved_at {
        model.content_saved_at = Set(Some(content_saved_at.to_owned()));
    }

    requests::Entity::insert(model)
    .exec(db)
    .await
    .map(|inserted| inserted.last_insert_id)
    .map_err(map_openai_db_err)
}

pub(crate) async fn create_request_execution_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    record: &NewRequestExecutionRecord<'_>,
) -> Result<i64, OpenAiV1Error> {
    request_executions::Entity::insert(request_executions::ActiveModel {
        project_id: Set(record.project_id),
        request_id: Set(record.request_id),
        channel_id: Set(record.channel_id),
        data_storage_id: Set(record.data_storage_id),
        external_id: Set(record.external_id.map(ToOwned::to_owned)),
        model_id: Set(record.model_id.to_owned()),
        format: Set(record.format.to_owned()),
        request_body: Set(record.request_body_json.to_owned()),
        response_body: Set(record.response_body_json.map(ToOwned::to_owned)),
        response_chunks: Set(record.response_chunks_json.map(ToOwned::to_owned)),
        error_message: Set(Some(record.error_message.to_owned())),
        response_status_code: Set(record.response_status_code),
        status: Set(record.status.to_owned()),
        stream: Set(record.stream),
        metrics_latency_ms: Set(record.metrics_latency_ms),
        metrics_first_token_latency_ms: Set(record.metrics_first_token_latency_ms),
        request_headers: Set(Some(record.request_headers_json.to_owned())),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|inserted| inserted.last_insert_id)
    .map_err(map_openai_db_err)
}

pub(crate) async fn update_request_result_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    record: &UpdateRequestResultRecord<'_>,
) -> Result<(), OpenAiV1Error> {
    let mut update = requests::Entity::update_many()
        .col_expr(requests::Column::UpdatedAt, Expr::current_timestamp().into())
        .col_expr(requests::Column::Status, Expr::value(record.status.to_owned()))
        .filter(requests::Column::Id.eq(record.request_id));

    if let Some(channel_id) = record.channel_id {
        update = update.col_expr(requests::Column::ChannelId, Expr::value(channel_id));
    }

    if let Some(external_id) = record.external_id {
        update = update.col_expr(
            requests::Column::ExternalId,
            Expr::value(external_id.to_owned()),
        );
    }

    if let Some(response_body_json) = record.response_body_json {
        update = update.col_expr(
            requests::Column::ResponseBody,
            Expr::value(response_body_json.to_owned()),
        );
    }

    update.exec(db).await.map(|_| ()).map_err(map_openai_db_err)
}

pub(crate) async fn update_request_execution_result_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    record: &UpdateRequestExecutionResultRecord<'_>,
) -> Result<(), OpenAiV1Error> {
    let mut update = request_executions::Entity::update_many()
        .col_expr(
            request_executions::Column::UpdatedAt,
            Expr::current_timestamp().into(),
        )
        .col_expr(
            request_executions::Column::Status,
            Expr::value(record.status.to_owned()),
        )
        .filter(request_executions::Column::Id.eq(record.execution_id));

    if let Some(external_id) = record.external_id {
        update = update.col_expr(
            request_executions::Column::ExternalId,
            Expr::value(external_id.to_owned()),
        );
    }

    if let Some(response_body_json) = record.response_body_json {
        update = update.col_expr(
            request_executions::Column::ResponseBody,
            Expr::value(response_body_json.to_owned()),
        );
    }

    if let Some(response_status_code) = record.response_status_code {
        update = update.col_expr(
            request_executions::Column::ResponseStatusCode,
            Expr::value(response_status_code),
        );
    }

    if let Some(error_message) = record.error_message {
        update = update.col_expr(
            request_executions::Column::ErrorMessage,
            Expr::value(error_message.to_owned()),
        );
    }

    update.exec(db).await.map(|_| ()).map_err(map_openai_db_err)
}

pub(crate) async fn record_usage_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    record: &NewUsageLogRecord<'_>,
) -> Result<i64, OpenAiV1Error> {
    usage_logs::Entity::insert(usage_logs::ActiveModel {
        request_id: Set(record.request_id),
        api_key_id: Set(record.api_key_id),
        project_id: Set(record.project_id),
        channel_id: Set(record.channel_id),
        model_id: Set(record.model_id.to_owned()),
        prompt_tokens: Set(record.prompt_tokens),
        completion_tokens: Set(record.completion_tokens),
        total_tokens: Set(record.total_tokens),
        prompt_audio_tokens: Set(record.prompt_audio_tokens),
        prompt_cached_tokens: Set(record.prompt_cached_tokens),
        prompt_write_cached_tokens: Set(record.prompt_write_cached_tokens),
        prompt_write_cached_tokens_5m: Set(record.prompt_write_cached_tokens_5m),
        prompt_write_cached_tokens_1h: Set(record.prompt_write_cached_tokens_1h),
        completion_audio_tokens: Set(record.completion_audio_tokens),
        completion_reasoning_tokens: Set(record.completion_reasoning_tokens),
        completion_accepted_prediction_tokens: Set(record.completion_accepted_prediction_tokens),
        completion_rejected_prediction_tokens: Set(record.completion_rejected_prediction_tokens),
        source: Set(record.source.to_owned()),
        format: Set(record.format.to_owned()),
        total_cost: Set(record.total_cost),
        cost_items: Set(record.cost_items_json.to_owned()),
        cost_price_reference_id: Set(Some(record.cost_price_reference_id.to_owned())),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|inserted| inserted.last_insert_id)
    .map_err(map_openai_db_err)
}

fn map_openai_db_err(error: sea_orm::DbErr) -> OpenAiV1Error {
    OpenAiV1Error::Internal {
        message: error.to_string(),
    }
}


fn stored_model_record_from_enabled_model_record(record: models::EnabledModelRecord) -> StoredModelRecord {
    StoredModelRecord {
        id: record.id,
        created_at: record.created_at,
        developer: record.developer,
        model_id: record.model_id,
        model_type: record.model_type,
        name: record.name,
        icon: record.icon,
        remark: record.remark.unwrap_or_default(),
        model_card_json: record.model_card,
    }
}

async fn query_preferred_trace_channel_id_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    trace_id: i64,
    model_id: &str,
) -> Result<Option<i64>, OpenAiV1Error> {
    let _ = backend;
    requests::Entity::find()
        .filter(requests::Column::TraceId.eq(trace_id))
        .filter(requests::Column::ModelId.eq(model_id))
        .filter(requests::Column::Status.eq("completed"))
        .filter(requests::Column::ChannelId.is_not_null())
        .order_by_desc(requests::Column::Id)
        .into_partial_model::<requests::TraceChannelAffinity>()
        .one(db)
        .await
        .map_err(map_openai_db_err)
        .map(|row| row.and_then(|row| row.channel_id))
}

async fn query_channel_routing_stats_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    channel_id: i64,
) -> Result<ChannelRoutingStats, OpenAiV1Error> {
    let _ = backend;
    let selection_count = i64::try_from(
        requests::Entity::find()
        .filter(requests::Column::ChannelId.eq(channel_id))
        .count(db)
        .await
        .map_err(map_openai_db_err)?,
    )
    .unwrap_or(i64::MAX);
    let processing_count = i64::try_from(
        requests::Entity::find()
        .filter(requests::Column::ChannelId.eq(channel_id))
        .filter(requests::Column::Status.eq("processing"))
        .count(db)
        .await
        .map_err(map_openai_db_err)?,
    )
    .unwrap_or(i64::MAX);
    let statuses = request_executions::Entity::find()
        .filter(request_executions::Column::ChannelId.eq(channel_id))
        .order_by_desc(request_executions::Column::Id)
        .limit(10)
        .into_partial_model::<request_executions::StatusOnly>()
        .all(db)
        .await
        .map_err(map_openai_db_err)?
        .into_iter()
        .map(|row| row.status)
        .collect::<Vec<_>>();
    let last_status_failed = statuses.first().is_some_and(|status| status == "failed");
    let consecutive_failures = statuses.iter().take_while(|status| status.as_str() == "failed").count() as i64;
    Ok(ChannelRoutingStats {
        selection_count,
        processing_count,
        consecutive_failures,
        last_status_failed,
    })
}
