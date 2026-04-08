use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use axonhub_db_entity::{
    api_keys, channel_model_price_versions, channel_model_prices, channel_probes, channels,
    data_storages, models, operational_runs, projects, provider_quota_statuses,
    request_executions, requests, systems, threads, traces, usage_logs,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, Set, Statement, TransactionTrait,
};
use serde_json::{json, Value};

use super::{
    admin::{
        default_auto_backup_settings, default_storage_policy, default_system_channel_settings,
        generate_probe_timestamps, provider_quota_type_for_channel, CachedFileStorage,
        StoredAutoBackupSettings, StoredBackupApiKey, StoredBackupChannel, StoredBackupModel,
        StoredBackupPayload, StoredChannelProbeData, StoredChannelProbePoint,
        StoredGcCleanupSummary, StoredProviderQuotaStatus, StoredProxyPreset,
        StoredStoragePolicy, StoredSystemChannelSettings,
    },
    seaorm::SeaOrmConnectionFactory,
    shared::{
        current_rfc3339_timestamp, current_unix_timestamp, format_unix_timestamp,
        AUTO_BACKUP_PREFIX, AUTO_BACKUP_SUFFIX, BACKUP_VERSION,
        SYSTEM_KEY_AUTO_BACKUP_SETTINGS, SYSTEM_KEY_CHANNEL_SETTINGS, SYSTEM_KEY_PROXY_PRESETS,
        SYSTEM_KEY_STORAGE_POLICY, SYSTEM_KEY_USER_AGENT_PASS_THROUGH,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationalRunStatus {
    Running,
    Completed,
    Failed,
}

impl OperationalRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OperationalRunRecord {
    pub(crate) id: i64,
    pub(crate) status: OperationalRunStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct RestoreOptions {
    pub(crate) include_channels: bool,
    pub(crate) include_models: bool,
    pub(crate) include_api_keys: bool,
    pub(crate) include_model_prices: bool,
    pub(crate) overwrite_existing: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmOperationalService {
    db: SeaOrmConnectionFactory,
}

impl SeaOrmOperationalService {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }

    pub(crate) fn storage_policy(&self) -> Result<StoredStoragePolicy, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            load_json_setting(&connection, SYSTEM_KEY_STORAGE_POLICY, default_storage_policy()).await
        })
    }

    pub(crate) fn update_storage_policy(
        &self,
        policy: StoredStoragePolicy,
    ) -> Result<StoredStoragePolicy, String> {
        let db = self.db.clone();
        let policy_to_store = policy.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            store_json_setting(&connection, SYSTEM_KEY_STORAGE_POLICY, &policy_to_store).await?;
            Ok(policy_to_store)
        })
    }

    pub(crate) fn auto_backup_settings(&self) -> Result<StoredAutoBackupSettings, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            load_json_setting(
                &connection,
                SYSTEM_KEY_AUTO_BACKUP_SETTINGS,
                default_auto_backup_settings(),
            )
            .await
        })
    }

    pub(crate) fn update_auto_backup_settings(
        &self,
        settings: StoredAutoBackupSettings,
    ) -> Result<StoredAutoBackupSettings, String> {
        if settings.enabled && settings.data_storage_id <= 0 {
            return Err("dataStorageID is required when auto backup is enabled".to_owned());
        }
        let db = self.db.clone();
        let settings_to_store = settings.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            store_json_setting(
                &connection,
                SYSTEM_KEY_AUTO_BACKUP_SETTINGS,
                &settings_to_store,
            )
            .await?;
            Ok(settings_to_store)
        })
    }

    pub(crate) fn system_channel_settings(&self) -> Result<StoredSystemChannelSettings, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            load_json_setting(
                &connection,
                SYSTEM_KEY_CHANNEL_SETTINGS,
                default_system_channel_settings(),
            )
            .await
        })
    }

    pub(crate) fn update_system_channel_settings(
        &self,
        settings: StoredSystemChannelSettings,
    ) -> Result<StoredSystemChannelSettings, String> {
        let db = self.db.clone();
        let settings_to_store = settings.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            store_json_setting(&connection, SYSTEM_KEY_CHANNEL_SETTINGS, &settings_to_store).await?;
            Ok(settings_to_store)
        })
    }

    pub(crate) fn proxy_presets(&self) -> Result<Vec<StoredProxyPreset>, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            load_json_setting(&connection, SYSTEM_KEY_PROXY_PRESETS, Vec::<StoredProxyPreset>::new()).await
        })
    }

    pub(crate) fn save_proxy_preset(&self, preset: StoredProxyPreset) -> Result<(), String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let mut presets: Vec<StoredProxyPreset> =
                load_json_setting(&connection, SYSTEM_KEY_PROXY_PRESETS, Vec::new()).await?;
            if let Some(existing) = presets.iter_mut().find(|item| item.url == preset.url) {
                *existing = preset;
            } else {
                presets.push(preset);
            }
            store_json_setting(&connection, SYSTEM_KEY_PROXY_PRESETS, &presets).await
        })
    }

    pub(crate) fn delete_proxy_preset(&self, url: &str) -> Result<(), String> {
        let db = self.db.clone();
        let url = url.to_owned();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let presets: Vec<StoredProxyPreset> =
                load_json_setting(&connection, SYSTEM_KEY_PROXY_PRESETS, Vec::new()).await?;
            let filtered = presets.into_iter().filter(|item| item.url != url).collect::<Vec<_>>();
            store_json_setting(&connection, SYSTEM_KEY_PROXY_PRESETS, &filtered).await
        })
    }

    pub(crate) fn user_agent_pass_through(&self) -> Result<bool, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let raw: String =
                load_json_setting(&connection, SYSTEM_KEY_USER_AGENT_PASS_THROUGH, "false".to_owned()).await?;
            Ok(raw.eq_ignore_ascii_case("true"))
        })
    }

    pub(crate) fn set_user_agent_pass_through(&self, enabled: bool) -> Result<(), String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let value = if enabled { "true" } else { "false" };
            store_json_setting(&connection, SYSTEM_KEY_USER_AGENT_PASS_THROUGH, &value).await
        })
    }

    pub(crate) fn channel_probe_data(
        &self,
        channel_ids: &[i64],
    ) -> Result<Vec<StoredChannelProbeData>, String> {
        let db = self.db.clone();
        let ids = channel_ids.to_vec();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let settings: StoredSystemChannelSettings =
                load_json_setting(&connection, SYSTEM_KEY_CHANNEL_SETTINGS, default_system_channel_settings())
                    .await?;
            let timestamps =
                generate_probe_timestamps(settings.probe.interval_minutes(), current_unix_timestamp());
            let Some(start_timestamp) = timestamps.first().copied() else {
                return Ok(Vec::new());
            };
            let Some(end_timestamp) = timestamps.last().copied() else {
                return Ok(Vec::new());
            };

            let mut items = Vec::with_capacity(ids.len());
            for channel_id in ids {
                let points = channel_probes::Entity::find()
                    .filter(channel_probes::Column::ChannelId.eq(channel_id))
                    .filter(channel_probes::Column::Timestamp.gte(start_timestamp))
                    .filter(channel_probes::Column::Timestamp.lte(end_timestamp))
                    .order_by_asc(channel_probes::Column::Timestamp)
                    .all(&connection)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut by_timestamp = std::collections::HashMap::new();
                for point in points {
                    by_timestamp.insert(
                        point.timestamp,
                        StoredChannelProbePoint {
                            timestamp: point.timestamp,
                            total_request_count: point.total_request_count,
                            success_request_count: point.success_request_count,
                            avg_tokens_per_second: point.avg_tokens_per_second,
                            avg_time_to_first_token_ms: point.avg_time_to_first_token_ms,
                        },
                    );
                }

                let mut normalized = Vec::with_capacity(timestamps.len());
                for timestamp in &timestamps {
                    normalized.push(
                        by_timestamp.remove(timestamp).unwrap_or(StoredChannelProbePoint {
                            timestamp: *timestamp,
                            total_request_count: 0,
                            success_request_count: 0,
                            avg_tokens_per_second: None,
                            avg_time_to_first_token_ms: None,
                        }),
                    );
                }

                items.push(StoredChannelProbeData {
                    channel_id,
                    points: normalized,
                });
            }
            Ok(items)
        })
    }

    pub(crate) fn provider_quota_statuses(&self) -> Result<Vec<StoredProviderQuotaStatus>, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let statuses = provider_quota_statuses::Entity::find()
                .order_by_asc(provider_quota_statuses::Column::ChannelId)
                .all(&connection)
                .await
                .map_err(|error| error.to_string())?;
            statuses
                .into_iter()
                .map(stored_provider_quota_status_from_model)
                .collect()
        })
    }

    pub(crate) fn reset_provider_quota_status(
        &self,
        channel_id: i64,
        initiated_by_user_id: Option<i64>,
    ) -> Result<bool, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let run = start_operational_run(
                &connection,
                "quota_reset",
                "manual",
                initiated_by_user_id,
                None,
                Some(channel_id),
                None,
            )
            .await?;

            let result = reset_provider_quota_status_row(&connection, channel_id).await;
            match result {
                Ok(updated) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Completed,
                        Some(json!({"updated": updated, "channelId": channel_id}).to_string()),
                        None,
                    )
                    .await?;
                    Ok(updated)
                }
                Err(error) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Failed,
                        None,
                        Some(error.clone()),
                    )
                    .await?;
                    Err(error)
                }
            }
        })
    }

    pub(crate) fn run_provider_quota_check_tick(
        &self,
        force: bool,
        check_interval: Duration,
        initiated_by_user_id: Option<i64>,
    ) -> Result<usize, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let run = start_operational_run(
                &connection,
                "quota_check",
                if force { "manual" } else { "scheduled" },
                initiated_by_user_id,
                None,
                None,
                None,
            )
            .await?;

            let result = async {
                let now = current_unix_timestamp();
                let next_check_at = now + i64::try_from(check_interval.as_secs()).unwrap_or(0);
                let backend = connection.get_database_backend();
                let channels = query_channel_quota_candidates(&connection, backend).await?;

                let mut updated = 0_usize;
                for (channel_id, channel_type) in channels {
                    let Some(provider_type) = provider_quota_type_for_channel(channel_type.as_str()) else {
                        continue;
                    };
                    if !force {
                        let existing = query_next_quota_check_at(&connection, backend, channel_id).await?;
                        if let Some(existing_next) = existing {
                            if existing_next > now {
                                continue;
                            }
                        }
                    }

                    let details = quota_ready_details(provider_type, channel_id);
                    let payload = json!({
                        "message": details,
                        "source": "manual_recheck",
                        "channelId": channel_id,
                    })
                    .to_string();
                    upsert_provider_quota_status_model(
                        &connection,
                        channel_id,
                        provider_type,
                        "available",
                        true,
                        None,
                        next_check_at,
                        payload,
                    )
                    .await?;
                    updated += 1;
                }
                Ok::<usize, String>(updated)
            }
            .await;

            match result {
                Ok(updated) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Completed,
                        Some(json!({"updated": updated}).to_string()),
                        None,
                    )
                    .await?;
                    Ok(updated)
                }
                Err(error) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Failed,
                        None,
                        Some(error.clone()),
                    )
                    .await?;
                    Err(error)
                }
            }
        })
    }

    pub(crate) fn run_gc_cleanup_now(
        &self,
        vacuum_enabled: bool,
        initiated_by_user_id: Option<i64>,
    ) -> Result<StoredGcCleanupSummary, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let run = start_operational_run(
                &connection,
                "gc_cleanup",
                "manual",
                initiated_by_user_id,
                None,
                None,
                None,
            )
            .await?;

            let result = async {
                let policy: StoredStoragePolicy =
                    load_json_setting(&connection, SYSTEM_KEY_STORAGE_POLICY, default_storage_policy())
                        .await?;
                let mut summary = StoredGcCleanupSummary::default();

                for option in policy.cleanup_options {
                    if !option.enabled {
                        continue;
                    }
                    let cutoff = current_unix_timestamp() - i64::from(option.cleanup_days.max(0)) * 86_400;
                    match option.resource_type.as_str() {
                        "requests" => {
                            summary.request_executions_deleted +=
                                cleanup_request_executions(&connection, cutoff).await?;
                            summary.requests_deleted += cleanup_requests(&connection, cutoff).await?;
                            summary.threads_deleted += cleanup_threads(&connection, cutoff).await?;
                            summary.traces_deleted += cleanup_traces(&connection, cutoff).await?;
                        }
                        "usage_logs" => {
                            summary.usage_logs_deleted += cleanup_usage_logs(&connection, cutoff).await?;
                        }
                        _ => {}
                    }
                }

                let channel_probe_cutoff = current_unix_timestamp() - 3 * 86_400;
                summary.channel_probes_deleted +=
                    cleanup_channel_probes(&connection, channel_probe_cutoff).await?;

                if vacuum_enabled {
                    connection
                        .execute_raw(Statement::from_string(connection.get_database_backend(), "VACUUM".to_owned()))
                        .await
                        .map_err(|error| error.to_string())?;
                    summary.vacuum_ran = true;
                }

                Ok::<StoredGcCleanupSummary, String>(summary)
            }
            .await;

            match result {
                Ok(summary) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Completed,
                        Some(gc_summary_payload(&summary).to_string()),
                        None,
                    )
                    .await?;
                    Ok(summary)
                }
                Err(error) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Failed,
                        None,
                        Some(error.clone()),
                    )
                    .await?;
                    Err(error)
                }
            }
        })
    }

    pub(crate) fn build_backup_payload(
        &self,
        settings: &StoredAutoBackupSettings,
    ) -> Result<StoredBackupPayload, String> {
        let db = self.db.clone();
        let settings = settings.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            build_backup_payload_from_connection(&connection, &settings).await
        })
    }

    pub(crate) fn trigger_backup_now(
        &self,
        initiated_by_user_id: Option<i64>,
    ) -> Result<String, String> {
        let settings = self.auto_backup_settings()?;
        if settings.data_storage_id <= 0 {
            return Err("data storage not configured for backup".to_owned());
        }

        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let run = start_operational_run(
                &connection,
                "auto_backup",
                "manual",
                initiated_by_user_id,
                Some(settings.data_storage_id),
                None,
                None,
            )
            .await?;

            let result = async {
                perform_backup_with_connection(&connection, &settings).await?;
                Ok::<(), String>(())
            }
            .await;

            match result {
                Ok(()) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Completed,
                        Some(json!({"message": "Backup completed successfully"}).to_string()),
                        None,
                    )
                    .await?;
                    Ok("Backup completed successfully".to_owned())
                }
                Err(error) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Failed,
                        None,
                        Some(error.clone()),
                    )
                    .await?;
                    Err(error)
                }
            }
        })
    }

    pub(crate) fn restore_backup(
        &self,
        payload: &[u8],
        options: RestoreOptions,
        initiated_by_user_id: Option<i64>,
    ) -> Result<String, String> {
        let db = self.db.clone();
        let payload = payload.to_vec();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let run = start_operational_run(
                &connection,
                "restore",
                "manual",
                initiated_by_user_id,
                None,
                None,
                None,
            )
            .await?;

            let result = async {
                let backup: StoredBackupPayload =
                    serde_json::from_slice(&payload).map_err(|error| format!("invalid backup payload: {error}"))?;
                if backup.version != BACKUP_VERSION {
                    return Err(format!(
                        "backup version mismatch: expected {BACKUP_VERSION}, got {}",
                        backup.version
                    ));
                }
                let txn = connection.begin().await.map_err(|error| error.to_string())?;
                restore_backup_into_transaction(&txn, &backup, &options).await?;
                txn.commit().await.map_err(|error| error.to_string())?;
                Ok::<(), String>(())
            }
            .await;

            match result {
                Ok(()) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Completed,
                        Some(json!({"message": "Restore completed successfully"}).to_string()),
                        None,
                    )
                    .await?;
                    Ok("Restore completed successfully".to_owned())
                }
                Err(error) => {
                    complete_operational_run(
                        &connection,
                        run.id,
                        OperationalRunStatus::Failed,
                        None,
                        Some(error.clone()),
                    )
                    .await?;
                    Err(error)
                }
            }
        })
    }
}

async fn query_channel_quota_candidates(
    connection: &DatabaseConnection,
    _backend: DatabaseBackend,
) -> Result<Vec<(i64, String)>, String> {
    let channels = channels::Entity::find()
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .filter(channels::Column::Status.eq("enabled"))
        .order_by_asc(channels::Column::Id)
        .all(connection)
        .await
        .map_err(|error| error.to_string())?;
    channels
        .into_iter()
        .map(|channel| Ok::<(i64, String), String>((channel.id, channel.type_field)))
        .collect()
}

async fn query_next_quota_check_at(
    connection: &DatabaseConnection,
    _backend: DatabaseBackend,
    channel_id: i64,
) -> Result<Option<i64>, String> {
    provider_quota_statuses::Entity::find()
        .filter(provider_quota_statuses::Column::ChannelId.eq(channel_id))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?
        .map(|model| parse_timestamp_or_unix(model.next_check_at.as_str()))
        .transpose()
}

async fn reset_provider_quota_status_row(
    connection: &DatabaseConnection,
    channel_id: i64,
) -> Result<bool, String> {
    let backend = connection.get_database_backend();
    let provider_type = query_channel_provider_quota_type(connection, backend, channel_id).await?;
    let Some(provider_type) = provider_type else {
        return Ok(false);
    };
    let next_check_at = current_unix_timestamp();
    let payload = json!({
        "message": quota_reset_details(provider_type.as_str(), channel_id),
        "source": "manual_reset",
        "channelId": channel_id,
    })
    .to_string();
    upsert_provider_quota_status_model(
        connection,
        channel_id,
        provider_type.as_str(),
        "available",
        true,
        None,
        next_check_at,
        payload,
    )
    .await?;
    Ok(true)
}

async fn query_channel_provider_quota_type(
    connection: &DatabaseConnection,
    _backend: DatabaseBackend,
    channel_id: i64,
) -> Result<Option<String>, String> {
    let channel = channels::Entity::find_by_id(channel_id)
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    Ok(channel
        .as_ref()
        .and_then(|channel| provider_quota_type_for_channel(channel.type_field.as_str()))
        .map(str::to_owned))
}

pub(crate) async fn persist_provider_quota_status_seaorm(
    connection: &impl ConnectionTrait,
    channel_id: i64,
    provider_type: &str,
    status: &str,
    ready: bool,
    next_reset_at: Option<i64>,
    next_check_at: i64,
    quota_data_json: String,
) -> Result<(), String> {
    upsert_provider_quota_status_model(
        connection,
        channel_id,
        provider_type,
        status,
        ready,
        next_reset_at,
        next_check_at,
        quota_data_json,
    )
    .await
}

pub(crate) fn quota_ready_details(provider_type: &str, channel_id: i64) -> String {
    format!(
        "provider quota recheck marked {provider_type} channel {channel_id} ready for routing"
    )
}

pub(crate) fn quota_reset_details(provider_type: &str, channel_id: i64) -> String {
    format!(
        "provider quota reset marked {provider_type} channel {channel_id} ready for routing"
    )
}

pub(crate) fn quota_exhausted_details(provider_type: &str, channel_id: i64, message: &str) -> String {
    format!(
        "provider quota exhausted for {provider_type} channel {channel_id}: {message}"
    )
}

async fn load_json_setting<T: serde::de::DeserializeOwned>(
    connection: &DatabaseConnection,
    key: &str,
    default: T,
) -> Result<T, String> {
    let stored = systems::Entity::find()
        .filter(systems::Column::Key.eq(key))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    let Some(stored) = stored else {
        return Ok(default);
    };
    serde_json::from_str(stored.value.as_str())
        .map_err(|error| format!("failed to decode stored admin setting: {error}"))
}

async fn store_json_setting<T: serde::Serialize>(
    connection: &DatabaseConnection,
    key: &str,
    value: &T,
) -> Result<(), String> {
    let value = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let existing = systems::Entity::find()
        .filter(systems::Column::Key.eq(key))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(existing) = existing {
        let mut active: systems::ActiveModel = existing.into();
        active.value = Set(value);
        active.deleted_at = Set(0_i64);
        active.update(connection).await.map_err(|error| error.to_string())?;
        return Ok(());
    }
    systems::Entity::insert(systems::ActiveModel {
        key: Set(key.to_owned()),
        value: Set(value),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(connection)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

async fn build_backup_payload_from_connection(
    connection: &DatabaseConnection,
    settings: &StoredAutoBackupSettings,
) -> Result<StoredBackupPayload, String> {
    let channels_out = if settings.include_channels {
        channels::Entity::find()
            .filter(channels::Column::DeletedAt.eq(0_i64))
            .order_by_asc(channels::Column::Id)
            .all(connection)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|channel| StoredBackupChannel {
                id: channel.id,
                name: channel.name,
                channel_type: channel.type_field,
                base_url: channel.base_url.unwrap_or_default(),
                status: channel.status,
                credentials: parse_json_value(&channel.credentials),
                supported_models: parse_json_value(&channel.supported_models),
                default_test_model: channel.default_test_model,
                settings: parse_json_value(&channel.settings),
                tags: parse_json_value(&channel.tags),
                ordering_weight: i64::from(channel.ordering_weight),
                error_message: channel.error_message.unwrap_or_default(),
                remark: channel.remark.unwrap_or_default(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let models_out = if settings.include_models {
        models::Entity::find()
            .filter(models::Column::DeletedAt.eq(0_i64))
            .order_by_asc(models::Column::Id)
            .all(connection)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|model| StoredBackupModel {
                id: model.id,
                developer: model.developer,
                model_id: model.model_id,
                model_type: model.type_field,
                name: model.name,
                icon: model.icon,
                group: model.group_name,
                model_card: parse_json_value(&model.model_card),
                settings: parse_json_value(&model.settings),
                status: model.status,
                remark: model.remark.unwrap_or_default(),
            })
            .collect()
    } else {
        Vec::new()
    };

    let channel_model_prices_out = if settings.include_model_prices {
        channel_model_prices::Entity::find()
            .filter(channel_model_prices::Column::DeletedAt.eq(0_i64))
            .find_also_related(channels::Entity)
            .order_by_asc(channel_model_prices::Column::Id)
            .all(connection)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter_map(|(price, channel)| {
                channel.map(|channel| {
                    json!({
                        "channelName": channel.name,
                        "modelID": price.model_id,
                        "price": parse_json_value(&price.price),
                        "referenceID": price.reference_id,
                    })
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let api_keys_out = if settings.include_api_keys {
        api_keys::Entity::find()
            .filter(api_keys::Column::DeletedAt.eq(0_i64))
            .find_also_related(projects::Entity)
            .order_by_asc(api_keys::Column::Id)
            .all(connection)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|(api_key, project)| StoredBackupApiKey {
                id: api_key.id,
                project_id: api_key.project_id,
                project_name: project.map(|project| project.name).unwrap_or_default(),
                key: api_key.key,
                name: api_key.name,
                key_type: api_key.type_field,
                status: api_key.status,
                scopes: parse_json_value(&api_key.scopes),
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(StoredBackupPayload {
        version: BACKUP_VERSION.to_owned(),
        timestamp: current_rfc3339_timestamp(),
        channels: channels_out,
        models: models_out,
        channel_model_prices: channel_model_prices_out,
        api_keys: api_keys_out,
    })
}

async fn perform_backup_with_connection(
    connection: &DatabaseConnection,
    settings: &StoredAutoBackupSettings,
) -> Result<(), String> {
    let storage = load_active_fs_storage(connection, settings.data_storage_id).await?;
    let storage = storage.ok_or_else(|| {
        "backup data storage is not an active fs storage in the Rust slice".to_owned()
    })?;

    fs::create_dir_all(storage.root.as_path())
        .map_err(|error| format!("failed to create backup directory: {error}"))?;

    let payload = build_backup_payload_from_connection(connection, settings).await?;
    let filename = format!(
        "{AUTO_BACKUP_PREFIX}{}{AUTO_BACKUP_SUFFIX}",
        current_unix_timestamp()
    );
    let path = storage.root.join(filename);
    let contents = serde_json::to_vec_pretty(&payload)
        .map_err(|error| format!("failed to serialize backup: {error}"))?;
    fs::write(path.as_path(), contents).map_err(|error| format!("failed to write backup file: {error}"))?;

    if settings.retention_days > 0 {
        cleanup_old_backups(storage.root.as_path(), settings.retention_days)?;
    }
    record_backup_status(connection, None).await
}

async fn record_backup_status(
    connection: &DatabaseConnection,
    error_message: Option<String>,
) -> Result<(), String> {
    let mut settings: StoredAutoBackupSettings =
        load_json_setting(connection, SYSTEM_KEY_AUTO_BACKUP_SETTINGS, default_auto_backup_settings())
            .await?;
    settings.last_backup_at = Some(current_unix_timestamp());
    settings.last_backup_error = error_message.unwrap_or_default();
    store_json_setting(connection, SYSTEM_KEY_AUTO_BACKUP_SETTINGS, &settings).await
}

fn cleanup_old_backups(root: &Path, retention_days: i32) -> Result<(), String> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(
            u64::try_from(retention_days.max(0)).unwrap_or(0) * 86_400,
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in fs::read_dir(root).map_err(|error| format!("failed to read backup directory: {error}"))? {
        let entry = entry.map_err(|error| format!("failed to inspect backup directory entry: {error}"))?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(AUTO_BACKUP_PREFIX) || !file_name.ends_with(AUTO_BACKUP_SUFFIX) {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| format!("failed to read backup metadata: {error}"))?;
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < cutoff {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

async fn load_active_fs_storage(
    connection: &DatabaseConnection,
    storage_id: i64,
) -> Result<Option<CachedFileStorage>, String> {
    let storage = data_storages::Entity::find_by_id(storage_id)
        .filter(data_storages::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?;
    let Some(storage) = storage else {
        return Ok(None);
    };
    if storage.type_field != "fs" || !storage.status.eq_ignore_ascii_case("active") {
        return Ok(None);
    }
    let settings = parse_json_value(&storage.settings);
    let Some(directory) = settings.get("directory").and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    Ok(Some(CachedFileStorage {
        root: directory.into(),
    }))
}

async fn restore_backup_into_transaction(
    txn: &sea_orm::DatabaseTransaction,
    backup: &StoredBackupPayload,
    options: &RestoreOptions,
) -> Result<(), String> {
    let mut channel_name_to_id = std::collections::HashMap::new();

    if options.include_channels {
        for channel in &backup.channels {
            let existing = channels::Entity::find()
                .filter(channels::Column::Name.eq(channel.name.clone()))
                .one(txn)
                .await
                .map_err(|error| error.to_string())?;

            let credentials = serde_json::to_string(&channel.credentials).map_err(|error| error.to_string())?;
            let supported_models = serde_json::to_string(&channel.supported_models).map_err(|error| error.to_string())?;
            let settings = serde_json::to_string(&channel.settings).map_err(|error| error.to_string())?;
            let tags = serde_json::to_string(&channel.tags).map_err(|error| error.to_string())?;

            let restored_id = if let Some(existing) = existing {
                let existing_id = existing.id;
                if !options.overwrite_existing {
                    return Err(format!("channel already exists: {}", channel.name));
                }
                let mut active: channels::ActiveModel = existing.into();
                active.type_field = Set(channel.channel_type.clone());
                active.base_url = Set(Some(channel.base_url.clone()));
                active.name = Set(channel.name.clone());
                active.status = Set(channel.status.clone());
                active.credentials = Set(credentials.clone());
                active.supported_models = Set(supported_models.clone());
                active.auto_sync_supported_models = Set(false);
                active.default_test_model = Set(channel.default_test_model.clone());
                active.settings = Set(settings.clone());
                active.tags = Set(tags.clone());
                active.ordering_weight = Set(i32::try_from(channel.ordering_weight).unwrap_or(i32::MAX));
                active.error_message = Set(Some(channel.error_message.clone()));
                active.remark = Set(Some(channel.remark.clone()));
                active.deleted_at = Set(0_i64);
                active.update(txn).await.map_err(|error| error.to_string())?;
                existing_id
            } else {
                channels::Entity::insert(channels::ActiveModel {
                    type_field: Set(channel.channel_type.clone()),
                    base_url: Set(Some(channel.base_url.clone())),
                    name: Set(channel.name.clone()),
                    status: Set(channel.status.clone()),
                    credentials: Set(credentials.clone()),
                    supported_models: Set(supported_models.clone()),
                    auto_sync_supported_models: Set(false),
                    default_test_model: Set(channel.default_test_model.clone()),
                    settings: Set(settings.clone()),
                    tags: Set(tags.clone()),
                    ordering_weight: Set(i32::try_from(channel.ordering_weight).unwrap_or(i32::MAX)),
                    error_message: Set(Some(channel.error_message.clone())),
                    remark: Set(Some(channel.remark.clone())),
                    deleted_at: Set(0_i64),
                    ..Default::default()
                })
                .exec(txn)
                .await
                .map_err(|error| error.to_string())?
                .last_insert_id
            };
            channel_name_to_id.insert(channel.name.clone(), restored_id);
        }
    } else {
        for channel in channels::Entity::find()
            .filter(channels::Column::DeletedAt.eq(0_i64))
            .all(txn)
            .await
            .map_err(|error| error.to_string())?
        {
            channel_name_to_id.insert(channel.name, channel.id);
        }
    }

    if options.include_model_prices {
        for price in &backup.channel_model_prices {
            let Some(channel_name) = price.get("channelName").and_then(Value::as_str) else {
                continue;
            };
            let Some(model_id) = price.get("modelID").and_then(Value::as_str) else {
                continue;
            };
            let Some(reference_id) = price.get("referenceID").and_then(Value::as_str) else {
                return Err(format!(
                    "channel model price reference ID is empty: channel={channel_name} model_id={model_id}"
                ));
            };
            let Some(channel_id) = channel_name_to_id.get(channel_name).copied() else {
                continue;
            };
            let price_value = price.get("price").cloned().unwrap_or(Value::Null);
            let price_json = serde_json::to_string(&price_value)
                .map_err(|error| error.to_string())?;
            let existing = channel_model_prices::Entity::find()
                .filter(channel_model_prices::Column::ChannelId.eq(channel_id))
                .filter(channel_model_prices::Column::ModelId.eq(model_id.to_owned()))
                .filter(channel_model_prices::Column::DeletedAt.eq(0_i64))
                .one(txn)
                .await
                .map_err(|error| error.to_string())?;

            let channel_model_price_id = if let Some(existing) = existing {
                if existing.reference_id == reference_id && existing.price == price_json {
                    existing.id
                } else {
                    if !options.overwrite_existing {
                        return Err(format!(
                            "channel model price already exists: channel={channel_name} model_id={model_id}"
                        ));
                    }
                    let active_versions = channel_model_price_versions::Entity::find()
                        .filter(channel_model_price_versions::Column::ChannelModelPriceId.eq(existing.id))
                        .filter(channel_model_price_versions::Column::Status.eq("active"))
                        .all(txn)
                        .await
                        .map_err(|error| error.to_string())?;
                    for version in active_versions {
                        let mut active: channel_model_price_versions::ActiveModel = version.into();
                        active.status = Set("archived".to_owned());
                        active.effective_end_at = Set(Some(format_unix_timestamp(current_unix_timestamp())));
                        active.update(txn).await.map_err(|error| error.to_string())?;
                    }
                    let mut active: channel_model_prices::ActiveModel = existing.into();
                    active.price = Set(price_json.clone());
                    active.reference_id = Set(reference_id.to_owned());
                    active.update(txn).await.map_err(|error| error.to_string())?.id
                }
            } else {
                channel_model_prices::Entity::insert(channel_model_prices::ActiveModel {
                    channel_id: Set(channel_id),
                    model_id: Set(model_id.to_owned()),
                    price: Set(price_json.clone()),
                    reference_id: Set(reference_id.to_owned()),
                    deleted_at: Set(0_i64),
                    ..Default::default()
                })
                .exec(txn)
                .await
                .map_err(|error| error.to_string())?
                .last_insert_id
            };

            channel_model_price_versions::Entity::insert(channel_model_price_versions::ActiveModel {
                channel_id: Set(channel_id),
                model_id: Set(model_id.to_owned()),
                channel_model_price_id: Set(channel_model_price_id),
                price: Set(price_json),
                status: Set("active".to_owned()),
                effective_end_at: Set(None),
                reference_id: Set(reference_id.to_owned()),
                ..Default::default()
            })
            .exec(txn)
            .await
            .map_err(|error| error.to_string())?;
        }
    }

    if options.include_models {
        for model in &backup.models {
            let existing = models::Entity::find()
                .filter(models::Column::Developer.eq(model.developer.clone()))
                .filter(models::Column::ModelId.eq(model.model_id.clone()))
                .filter(models::Column::TypeField.eq(model.model_type.clone()))
                .filter(models::Column::DeletedAt.eq(0_i64))
                .one(txn)
                .await
                .map_err(|error| error.to_string())?;

            let model_card = serde_json::to_string(&model.model_card).map_err(|error| error.to_string())?;
            let settings = serde_json::to_string(&model.settings).map_err(|error| error.to_string())?;

            if let Some(existing) = existing {
                if !options.overwrite_existing {
                    return Err(format!("model already exists: {}", model.model_id));
                }
                let mut active: models::ActiveModel = existing.into();
                active.name = Set(model.name.clone());
                active.icon = Set(model.icon.clone());
                active.group_name = Set(model.group.clone());
                active.model_card = Set(model_card);
                active.settings = Set(settings);
                active.status = Set(model.status.clone());
                active.remark = Set(Some(model.remark.clone()));
                active.deleted_at = Set(0_i64);
                active.update(txn).await.map_err(|error| error.to_string())?;
            } else {
                models::Entity::insert(models::ActiveModel {
                    developer: Set(model.developer.clone()),
                    model_id: Set(model.model_id.clone()),
                    type_field: Set(model.model_type.clone()),
                    name: Set(model.name.clone()),
                    icon: Set(model.icon.clone()),
                    group_name: Set(model.group.clone()),
                    model_card: Set(model_card),
                    settings: Set(settings),
                    status: Set(model.status.clone()),
                    remark: Set(Some(model.remark.clone())),
                    deleted_at: Set(0_i64),
                    ..Default::default()
                })
                .exec(txn)
                .await
                .map_err(|error| error.to_string())?;
            }
        }
    }

    if options.include_api_keys {
        for api_key in &backup.api_keys {
            let existing = api_keys::Entity::find()
                .filter(api_keys::Column::Key.eq(api_key.key.clone()))
                .filter(api_keys::Column::DeletedAt.eq(0_i64))
                .one(txn)
                .await
                .map_err(|error| error.to_string())?;
            let scopes = serde_json::to_string(&api_key.scopes).map_err(|error| error.to_string())?;
            if let Some(existing) = existing {
                if !options.overwrite_existing {
                    return Err(format!("api key already exists: {}", api_key.name));
                }
                let mut active: api_keys::ActiveModel = existing.into();
                active.project_id = Set(api_key.project_id);
                active.name = Set(api_key.name.clone());
                active.type_field = Set(api_key.key_type.clone());
                active.status = Set(api_key.status.clone());
                active.scopes = Set(scopes);
                active.deleted_at = Set(0_i64);
                active.update(txn).await.map_err(|error| error.to_string())?;
            } else {
                api_keys::Entity::insert(api_keys::ActiveModel {
                    project_id: Set(api_key.project_id),
                    key: Set(api_key.key.clone()),
                    name: Set(api_key.name.clone()),
                    type_field: Set(api_key.key_type.clone()),
                    status: Set(api_key.status.clone()),
                    scopes: Set(scopes),
                    profiles: Set("{}".to_owned()),
                    deleted_at: Set(0_i64),
                    user_id: Set(1_i64),
                    ..Default::default()
                })
                .exec(txn)
                .await
                .map_err(|error| error.to_string())?;
            }
        }
    }

    Ok(())
}

async fn cleanup_request_executions(connection: &DatabaseConnection, cutoff: i64) -> Result<i64, String> {
    request_executions::Entity::delete_many()
        .filter(request_executions::Column::CreatedAt.lt(format_unix_timestamp(cutoff)))
        .exec(connection)
        .await
        .map(|result| result.rows_affected as i64)
        .map_err(|error| error.to_string())
}

async fn cleanup_requests(connection: &DatabaseConnection, cutoff: i64) -> Result<i64, String> {
    requests::Entity::delete_many()
        .filter(requests::Column::CreatedAt.lt(format_unix_timestamp(cutoff)))
        .exec(connection)
        .await
        .map(|result| result.rows_affected as i64)
        .map_err(|error| error.to_string())
}

async fn cleanup_threads(connection: &DatabaseConnection, cutoff: i64) -> Result<i64, String> {
    threads::Entity::delete_many()
        .filter(threads::Column::CreatedAt.lt(format_unix_timestamp(cutoff)))
        .exec(connection)
        .await
        .map(|result| result.rows_affected as i64)
        .map_err(|error| error.to_string())
}

async fn cleanup_traces(connection: &DatabaseConnection, cutoff: i64) -> Result<i64, String> {
    traces::Entity::delete_many()
        .filter(traces::Column::CreatedAt.lt(format_unix_timestamp(cutoff)))
        .exec(connection)
        .await
        .map(|result| result.rows_affected as i64)
        .map_err(|error| error.to_string())
}

async fn cleanup_usage_logs(connection: &DatabaseConnection, cutoff: i64) -> Result<i64, String> {
    usage_logs::Entity::delete_many()
        .filter(usage_logs::Column::CreatedAt.lt(format_unix_timestamp(cutoff)))
        .exec(connection)
        .await
        .map(|result| result.rows_affected as i64)
        .map_err(|error| error.to_string())
}

async fn cleanup_channel_probes(connection: &DatabaseConnection, cutoff: i64) -> Result<i64, String> {
    channel_probes::Entity::delete_many()
        .filter(channel_probes::Column::Timestamp.lt(cutoff))
        .exec(connection)
        .await
        .map(|result| result.rows_affected as i64)
        .map_err(|error| error.to_string())
}

async fn start_operational_run(
    connection: &DatabaseConnection,
    operation_type: &str,
    trigger_source: &str,
    initiated_by_user_id: Option<i64>,
    data_storage_id: Option<i64>,
    channel_id: Option<i64>,
    project_id: Option<i64>,
) -> Result<OperationalRunRecord, String> {
    let created = operational_runs::Entity::insert(operational_runs::ActiveModel {
        operation_type: Set(operation_type.to_owned()),
        trigger_source: Set(trigger_source.to_owned()),
        status: Set(OperationalRunStatus::Running.as_str().to_owned()),
        result_payload: Set(None),
        error_message: Set(None),
        initiated_by_user_id: Set(initiated_by_user_id),
        data_storage_id: Set(data_storage_id),
        channel_id: Set(channel_id),
        project_id: Set(project_id),
        finished_at: Set(None),
        ..Default::default()
    })
    .exec(connection)
    .await
    .map_err(|error| error.to_string())?;
    Ok(OperationalRunRecord {
        id: created.last_insert_id,
        status: OperationalRunStatus::Running,
    })
}

async fn complete_operational_run(
    connection: &DatabaseConnection,
    run_id: i64,
    status: OperationalRunStatus,
    result_payload: Option<String>,
    error_message: Option<String>,
) -> Result<(), String> {
    let run = operational_runs::Entity::find_by_id(run_id)
        .one(connection)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("operational run {run_id} not found"))?;
    let mut active: operational_runs::ActiveModel = run.into();
    active.status = Set(status.as_str().to_owned());
    active.result_payload = Set(result_payload);
    active.error_message = Set(error_message);
    active.finished_at = Set(Some(current_rfc3339_timestamp()));
    active.update(connection).await.map_err(|error| error.to_string())?;
    Ok(())
}

async fn upsert_provider_quota_status_model(
    connection: &impl ConnectionTrait,
    channel_id: i64,
    provider_type: &str,
    status: &str,
    ready: bool,
    next_reset_at: Option<i64>,
    next_check_at: i64,
    quota_data_json: String,
) -> Result<(), String> {
    let next_reset_at = next_reset_at.map(format_unix_timestamp);
    let next_check_at = format_unix_timestamp(next_check_at);

    if let Some(existing) = provider_quota_statuses::Entity::find()
        .filter(provider_quota_statuses::Column::ChannelId.eq(channel_id))
        .one(connection)
        .await
        .map_err(|error| error.to_string())?
    {
        let mut active: provider_quota_statuses::ActiveModel = existing.into();
        active.provider_type = Set(provider_type.to_owned());
        active.status = Set(status.to_owned());
        active.quota_data = Set(quota_data_json);
        active.next_reset_at = Set(next_reset_at);
        active.ready = Set(ready);
        active.next_check_at = Set(next_check_at);
        active.deleted_at = Set(0_i64);
        active
            .update(connection)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    provider_quota_statuses::Entity::insert(provider_quota_statuses::ActiveModel {
        channel_id: Set(channel_id),
        provider_type: Set(provider_type.to_owned()),
        status: Set(status.to_owned()),
        quota_data: Set(quota_data_json),
        next_reset_at: Set(next_reset_at),
        ready: Set(ready),
        next_check_at: Set(next_check_at),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(connection)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn stored_provider_quota_status_from_model(
    model: provider_quota_statuses::Model,
) -> Result<StoredProviderQuotaStatus, String> {
    Ok(StoredProviderQuotaStatus {
        id: model.id,
        channel_id: model.channel_id,
        provider_type: model.provider_type,
        status: model.status,
        quota_data_json: model.quota_data,
        next_reset_at: model
            .next_reset_at
            .as_deref()
            .map(parse_timestamp_or_unix)
            .transpose()?,
        ready: model.ready,
        next_check_at: parse_timestamp_or_unix(model.next_check_at.as_str())?,
    })
}

fn parse_timestamp_or_unix(value: &str) -> Result<i64, String> {
    value
        .trim()
        .parse::<i64>()
        .or_else(|_| {
            humantime::parse_rfc3339_weak(value)
                .map_err(|error| error.to_string())
                .and_then(|time| {
                    time.duration_since(SystemTime::UNIX_EPOCH)
                        .map_err(|error| error.to_string())
                        .map(|duration| duration.as_secs() as i64)
                })
        })
}

fn parse_json_value(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or(Value::Null)
}

fn gc_summary_payload(summary: &StoredGcCleanupSummary) -> Value {
    json!({
        "requestsDeleted": summary.requests_deleted,
        "requestExecutionsDeleted": summary.request_executions_deleted,
        "threadsDeleted": summary.threads_deleted,
        "tracesDeleted": summary.traces_deleted,
        "usageLogsDeleted": summary.usage_logs_deleted,
        "channelProbesDeleted": summary.channel_probes_deleted,
        "vacuumRan": summary.vacuum_ran,
    })
}
