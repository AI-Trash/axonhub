use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axonhub_db_entity::{api_keys, channels, models, requests, request_executions, systems, usage_logs};
use axonhub_http::{OpenAiModel, OpenAiV1Error, OpenAiV1ExecutionRequest, OpenAiV1Route};
use serde::Deserialize;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Func, SimpleExpr};
use sea_orm::{ConnectionTrait, DatabaseBackend, PaginatorTrait, QueryResult, QuerySelect, QueryTrait};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::foundation::{
    openai_v1::{
        calculate_top_k, compare_openai_target_priority, extract_channel_api_key,
        derive_channel_model_entries, resolve_channel_model_entry, ChannelRoutingStats,
        ModelInclude, NewRequestExecutionRecord, NewRequestRecord, NewUsageLogRecord,
        PreparedCompatibilityRequest, SelectedOpenAiTarget, StoredModelRecord,
        UpdateRequestExecutionResultRecord, UpdateRequestResultRecord,
    },
    shared::SYSTEM_KEY_DEFAULT_DATA_STORAGE,
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
struct ParsedApiKeyProfile {
    name: String,
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
) -> Result<Vec<StoredModelRecord>, OpenAiV1Error> {
    list_routable_model_records_seaorm(db, backend).await
}

pub(crate) async fn list_enabled_models_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    include: &ModelInclude,
) -> Result<Vec<OpenAiModel>, OpenAiV1Error> {
    list_routable_model_records_seaorm(db, backend)
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
) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
    let request_model = request
        .body
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "model is required".to_owned(),
        })?;

    let targets = select_inference_targets_seaorm(
        db,
        backend,
        request_model,
        request.trace.as_ref().map(|trace| trace.id),
        2,
        "openai",
        openai_route_model_type(route),
        request.channel_hint_id,
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

fn openai_route_model_type(route: OpenAiV1Route) -> &'static str {
    match route {
        OpenAiV1Route::ImagesGenerations => "image",
        OpenAiV1Route::ChatCompletions
        | OpenAiV1Route::Responses
        | OpenAiV1Route::ResponsesCompact
        | OpenAiV1Route::Embeddings => "",
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

        actual_model_ids.insert(model_entry.actual_model_id.clone());
        resolved_channels.push((channel, model_entry, api_key));
    }

    if resolved_channels.is_empty() {
        return Ok(Vec::new());
    }

    let enabled_models = models::Entity::find()
        .filter(models::Column::DeletedAt.eq(0_i64))
        .filter(models::Column::Status.eq("enabled"))
        .filter(models::Column::ModelId.is_in(actual_model_ids.into_iter().collect::<Vec<_>>()))
        .apply_if((!model_type.is_empty()).then_some(model_type), |query, model_type| {
            query.filter(models::Column::TypeField.eq(model_type))
        })
        .into_partial_model::<models::EnabledModelRecord>()
        .all(db)
        .await
        .map_err(map_openai_db_err)?;
    let enabled_models_by_id = enabled_models
        .into_iter()
        .map(stored_model_record_from_enabled_model_record)
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
            ordering_weight,
            trace_affinity: preferred_trace_channel_id == Some(channel_id),
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

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue::Set;

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
        )
        .await
        .unwrap();
        assert_eq!(trimmed_targets.len(), 1);
        assert_eq!(trimmed_targets[0].channel_id, trimmed_channel_id);
        assert_eq!(trimmed_targets[0].actual_model_id, "provider/trimmed-actual-model");
        assert_eq!(trimmed_targets[0].model.model_id, "provider/trimmed-actual-model");
    }
}

async fn load_active_api_key_quota_seaorm(
    db: &impl ConnectionTrait,
    _backend: DatabaseBackend,
    api_key_id: i64,
) -> Result<Option<ParsedApiKeyQuota>, OpenAiV1Error> {
    let profiles_json = api_keys::Entity::find_by_id(api_key_id)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
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
        .query_one(
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
        .query_one(
            usage_logs::Entity::find()
                .select_only()
                .expr(Func::coalesce([
                    Func::sum(Expr::col(usage_logs::Column::TotalTokens)).into(),
                    Expr::value(0_i64).into(),
                ]))
                .expr(Func::coalesce([
                    Func::sum(Expr::col(usage_logs::Column::TotalCost)).into(),
                    Expr::value(0_f64).into(),
                ]))
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
    }
}

fn usage_window_bound_expr(backend: DatabaseBackend, bound: &str) -> SimpleExpr {
    match backend {
        DatabaseBackend::Sqlite => Func::cust(Alias::new("datetime"))
            .arg(Expr::value(bound.to_owned()))
            .into(),
        DatabaseBackend::Postgres => Expr::value(bound.to_owned()).cast_as("TIMESTAMPTZ"),
        DatabaseBackend::MySql => Expr::value(bound.to_owned()).cast_as("DATETIME"),
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
        let api_key = extract_channel_api_key(&channel.credentials);
        if api_key.is_empty() {
            continue;
        }

        for entry in derive_channel_model_entries(&channel.supported_models, "{}").into_values() {
            routable_model_ids.insert(entry.actual_model_id);
        }
    }

    Ok(enabled_models
        .into_iter()
        .filter(|model| routable_model_ids.contains(&model.model_id))
        .map(stored_model_record_from_enabled_model_record)
        .collect())
}

pub(crate) async fn select_doubao_task_targets_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    request: &OpenAiV1ExecutionRequest,
    prepared: &PreparedCompatibilityRequest,
) -> Result<Vec<SelectedOpenAiTarget>, OpenAiV1Error> {
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
    requests::Entity::insert(requests::ActiveModel {
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
        content_saved_at: Set(record.content_saved_at.map(ToOwned::to_owned)),
        ..Default::default()
    })
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


fn stored_model_record_from_seaorm_row(row: QueryResult) -> Result<StoredModelRecord, OpenAiV1Error> {
    Ok(StoredModelRecord {
        id: row.try_get_by_index(0).map_err(map_openai_db_err)?,
        created_at: row.try_get_by_index(1).map_err(map_openai_db_err)?,
        developer: row.try_get_by_index(2).map_err(map_openai_db_err)?,
        model_id: row.try_get_by_index(3).map_err(map_openai_db_err)?,
        model_type: row.try_get_by_index(4).map_err(map_openai_db_err)?,
        name: row.try_get_by_index(5).map_err(map_openai_db_err)?,
        icon: row.try_get_by_index(6).map_err(map_openai_db_err)?,
        remark: row.try_get_by_index(7).map_err(map_openai_db_err)?,
        model_card_json: row.try_get_by_index(8).map_err(map_openai_db_err)?,
    })
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
