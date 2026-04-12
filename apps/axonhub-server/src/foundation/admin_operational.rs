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
    EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use serde_json::{json, Value};

use super::{
    admin::{
        default_auto_backup_settings, default_retry_policy, default_storage_policy, default_system_channel_settings,
        default_video_storage_settings, generate_probe_timestamps,
        provider_quota_type_for_channel, CachedFileStorage, StoredAutoBackupSettings,
        StoredBackupApiKey, StoredBackupChannel, StoredBackupModel, StoredBackupPayload,
        StoredChannelProbeData, StoredChannelProbePoint, StoredGcCleanupSummary,
        StoredProviderQuotaStatus, StoredProxyPreset, StoredStoragePolicy,
        StoredSystemChannelSettings, StoredVideoStorageSettings, StoredRetryPolicy,
    },
    graphql::AdminGraphqlUpdateVideoStorageSettingsInput,
    seaorm::SeaOrmConnectionFactory,
    shared::{
        current_rfc3339_timestamp, current_unix_timestamp, format_unix_timestamp,
        AUTO_BACKUP_PREFIX, AUTO_BACKUP_SUFFIX, BACKUP_VERSION,
        SYSTEM_KEY_AUTO_BACKUP_SETTINGS, SYSTEM_KEY_CHANNEL_SETTINGS, SYSTEM_KEY_PROXY_PRESETS,
        SYSTEM_KEY_STORAGE_POLICY, SYSTEM_KEY_USER_AGENT_PASS_THROUGH, SYSTEM_KEY_VIDEO_STORAGE_SETTINGS,
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

    #[allow(dead_code)]
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

    pub(crate) fn update_retry_policy(
        &self,
        policy: StoredRetryPolicy,
    ) -> Result<StoredRetryPolicy, String> {
        let db = self.db.clone();
        let policy_to_store = policy.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            store_json_setting(&connection, "retry_policy", &policy_to_store).await?;
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

    #[allow(dead_code)]
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

    pub(crate) fn update_video_storage_settings(
        &self,
        input: AdminGraphqlUpdateVideoStorageSettingsInput,
    ) -> Result<StoredVideoStorageSettings, String> {
        let db = self.db.clone();
        db.run_sync(move |factory| async move {
            let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
            let defaults = default_video_storage_settings();
            let mut settings: StoredVideoStorageSettings = load_json_setting(
                &connection,
                SYSTEM_KEY_VIDEO_STORAGE_SETTINGS,
                default_video_storage_settings(),
            )
            .await?;

            if let Some(enabled) = input.enabled {
                settings.enabled = enabled;
            }
            if let Some(data_storage_id) = input.data_storage_id {
                settings.data_storage_id = data_storage_id;
            }
            if let Some(scan_interval_minutes) = input.scan_interval_minutes {
                settings.scan_interval_minutes = if scan_interval_minutes <= 0 {
                    defaults.scan_interval_minutes
                } else {
                    scan_interval_minutes
                };
            }
            if let Some(scan_limit) = input.scan_limit {
                settings.scan_limit = if scan_limit <= 0 {
                    defaults.scan_limit
                } else {
                    scan_limit
                };
            }

            if settings.enabled {
                if settings.data_storage_id <= 0 {
                    return Err("dataStorageID is required when video storage is enabled".to_owned());
                }

                let storage = data_storages::Entity::find_by_id(settings.data_storage_id)
                    .filter(data_storages::Column::DeletedAt.eq(0_i64))
                    .one(&connection)
                    .await
                    .map_err(|error| format!("failed to load video storage target: {error}"))?;
                let Some(storage) = storage else {
                    return Err("data storage not found".to_owned());
                };

                if storage.primary_flag || storage.type_field == "database" {
                    return Err("video storage must use a non-database data storage".to_owned());
                }
            }

            store_json_setting(&connection, SYSTEM_KEY_VIDEO_STORAGE_SETTINGS, &settings).await?;
            Ok(settings)
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

    #[allow(dead_code)]
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
        _vacuum_enabled: bool,
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

                // Runtime GC cleanup no longer executes VACUUM under the zero-runtime-raw-SQL
                // target, so vacuum_ran remains deterministically false.

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

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, RwLock};
    use std::time::{Duration, SystemTime};

    use axonhub_db_entity::{data_storages, provider_quota_statuses};
    use rusqlite::{
        params, params_from_iter, types::Type as SqlType, Connection, Error as SqlError,
        OptionalExtension, Result as SqlResult,
    };
    use sea_orm::{
        ColumnTrait, DatabaseBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
        QueryTrait,
    };
    use serde::{de::DeserializeOwned, Serialize};
    use serde_json::Value;

    use super::{
        super::{
                admin::{
                    default_auto_backup_settings, default_retry_policy, default_storage_policy,
                    default_system_channel_settings, default_video_storage_settings,
                    generate_probe_timestamps,
                    parse_graphql_resource_id, provider_quota_type_for_channel, CachedFileStorage,
                    StoredAutoBackupSettings, StoredBackupApiKey, StoredBackupChannel,
                    StoredBackupModel, StoredBackupPayload, StoredChannelProbeData,
                    StoredChannelProbePoint, StoredCleanupOption, StoredGcCleanupSummary,
                    StoredAutoDisableChannelStatus, StoredProviderQuotaStatus, StoredProxyPreset, StoredRetryPolicy, StoredStoragePolicy,
                    StoredSystemChannelSettings, StoredVideoStorageSettings,
                },
            graphql::{
                AdminGraphqlUpdateAutoBackupSettingsInput, AdminGraphqlUpdateRetryPolicyInput, AdminGraphqlUpdateStoragePolicyInput,
                AdminGraphqlUpdateSystemChannelSettingsInput,
            },
            shared::{
                bool_to_sql, current_rfc3339_timestamp, current_unix_timestamp,
                format_unix_timestamp, AUTO_BACKUP_PREFIX, AUTO_BACKUP_SUFFIX, BACKUP_VERSION,
                SYSTEM_KEY_AUTO_BACKUP_SETTINGS, SYSTEM_KEY_CHANNEL_SETTINGS,
                SYSTEM_KEY_PROXY_PRESETS, SYSTEM_KEY_STORAGE_POLICY,
                SYSTEM_KEY_USER_AGENT_PASS_THROUGH,
            },
            system::sqlite_test_support::{
                ensure_all_foundation_tables, ensure_operational_tables,
                SqliteConnectionFactory, SqliteFoundation, SystemSettingsStore,
            },
        },
        quota_ready_details, quota_reset_details,
    };

    #[derive(Debug, Clone)]
    pub struct OperationalStore {
        pub(crate) connection_factory: SqliteConnectionFactory,
    }

    impl OperationalStore {
        pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
            Self { connection_factory }
        }

        pub fn refresh_file_storage_cache(&self) -> SqlResult<HashMap<i64, CachedFileStorage>> {
            let connection = self.connection_factory.open(true)?;
            ensure_operational_tables(&connection)?;
            let statement_definition = file_storage_cache_query_statement();
            let mut statement = connection.prepare(statement_definition.sql.as_str())?;
            let rows = statement.query_map(
                params_from_iter(rusqlite_values(&statement_definition)?),
                |row| {
                    let storage_id: i64 = row.get(0)?;
                    let settings_json: String = row.get(1)?;
                    Ok((storage_id, settings_json))
                },
            )?;

            let mut cache = HashMap::new();
            for row in rows {
                let (storage_id, settings_json) = row?;
                let settings =
                    serde_json::from_str::<Value>(settings_json.as_str()).unwrap_or(Value::Null);
                let directory = settings
                    .get("directory")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if let Some(directory) = directory {
                    cache.insert(
                        storage_id,
                        CachedFileStorage {
                            root: PathBuf::from(directory),
                        },
                    );
                }
            }

            Ok(cache)
        }

        pub fn list_channel_probe_data(
            &self,
            channel_ids: &[i64],
        ) -> SqlResult<Vec<StoredChannelProbeData>> {
            let connection = self.connection_factory.open(true)?;
            ensure_operational_tables(&connection)?;
            let settings = load_json_setting(
                &SystemSettingsStore::new(self.connection_factory.clone()),
                SYSTEM_KEY_CHANNEL_SETTINGS,
                default_system_channel_settings(),
            )?;
            let timestamps =
                generate_probe_timestamps(settings.probe.interval_minutes(), current_unix_timestamp());
            let Some(start_timestamp) = timestamps.first().copied() else {
                return Ok(Vec::new());
            };
            let Some(end_timestamp) = timestamps.last().copied() else {
                return Ok(Vec::new());
            };

            let mut data = Vec::with_capacity(channel_ids.len());
            for channel_id in channel_ids {
                let mut statement = connection.prepare(
                    "SELECT timestamp, total_request_count, success_request_count, avg_tokens_per_second, avg_time_to_first_token_ms
                     FROM channel_probes
                     WHERE channel_id = ?1 AND timestamp >= ?2 AND timestamp <= ?3
                     ORDER BY timestamp ASC",
                )?;
                let rows = statement.query_map(
                    params![channel_id, start_timestamp, end_timestamp],
                    |row| {
                        Ok(StoredChannelProbePoint {
                            timestamp: row.get(0)?,
                            total_request_count: row.get(1)?,
                            success_request_count: row.get(2)?,
                            avg_tokens_per_second: row.get(3)?,
                            avg_time_to_first_token_ms: row.get(4)?,
                        })
                    },
                )?;
                let existing = rows.collect::<SqlResult<Vec<_>>>()?;
                let mut by_timestamp = HashMap::new();
                for point in existing {
                    by_timestamp.insert(point.timestamp, point);
                }

                let mut points = Vec::with_capacity(timestamps.len());
                for timestamp in &timestamps {
                    points.push(
                        by_timestamp
                            .remove(timestamp)
                            .unwrap_or(StoredChannelProbePoint {
                                timestamp: *timestamp,
                                total_request_count: 0,
                                success_request_count: 0,
                                avg_tokens_per_second: None,
                                avg_time_to_first_token_ms: None,
                            }),
                    );
                }

                data.push(StoredChannelProbeData {
                    channel_id: *channel_id,
                    points,
                });
            }

            Ok(data)
        }

        pub fn list_provider_quota_statuses(&self) -> SqlResult<Vec<StoredProviderQuotaStatus>> {
            let connection = self.connection_factory.open(true)?;
            ensure_operational_tables(&connection)?;
            let statement_definition = provider_quota_statuses_query_statement();
            let mut statement = connection.prepare(statement_definition.sql.as_str())?;
            let rows = statement.query_map(
                params_from_iter(rusqlite_values(&statement_definition)?),
                |row| {
                    Ok(StoredProviderQuotaStatus {
                        id: row.get(0)?,
                        channel_id: row.get(1)?,
                        provider_type: row.get(2)?,
                        status: row.get(3)?,
                        quota_data_json: row.get(4)?,
                        next_reset_at: match row.get_ref(5)? {
                            rusqlite::types::ValueRef::Null => None,
                            rusqlite::types::ValueRef::Integer(value) => Some(value),
                            rusqlite::types::ValueRef::Text(value) => {
                                Some(parse_timestamp_or_unix_sql(
                                    std::str::from_utf8(value).map_err(|error| {
                                        SqlError::FromSqlConversionFailure(
                                            5,
                                            SqlType::Text,
                                            Box::new(error),
                                        )
                                    })?,
                                    5,
                                )?)
                            }
                            _ => {
                                return Err(SqlError::InvalidColumnType(
                                    5,
                                    "column 5".to_owned(),
                                    SqlType::Text,
                                ))
                            }
                        },
                        ready: row.get::<_, i64>(6)? != 0,
                        next_check_at: match row.get_ref(7)? {
                            rusqlite::types::ValueRef::Integer(value) => value,
                            rusqlite::types::ValueRef::Text(value) => parse_timestamp_or_unix_sql(
                                std::str::from_utf8(value).map_err(|error| {
                                    SqlError::FromSqlConversionFailure(
                                        7,
                                        SqlType::Text,
                                        Box::new(error),
                                    )
                                })?,
                                7,
                            )?,
                            rusqlite::types::ValueRef::Null => {
                                return Err(SqlError::InvalidColumnType(
                                    7,
                                    "column 7".to_owned(),
                                    SqlType::Null,
                                ))
                            }
                            _ => {
                                return Err(SqlError::InvalidColumnType(
                                    7,
                                    "column 7".to_owned(),
                                    SqlType::Text,
                                ))
                            }
                        },
                    })
                },
            )?;
            rows.collect()
        }
    }

    fn file_storage_cache_query_statement() -> sea_orm::Statement {
        data_storages::Entity::find()
            .filter(data_storages::Column::DeletedAt.eq(0_i64))
            .filter(data_storages::Column::Status.eq("active"))
            .filter(data_storages::Column::TypeField.eq("fs"))
            .select_only()
            .column(data_storages::Column::Id)
            .column(data_storages::Column::Settings)
            .build(DatabaseBackend::Sqlite)
    }

    fn provider_quota_statuses_query_statement() -> sea_orm::Statement {
        provider_quota_statuses::Entity::find()
            .select_only()
            .column(provider_quota_statuses::Column::Id)
            .column(provider_quota_statuses::Column::ChannelId)
            .column(provider_quota_statuses::Column::ProviderType)
            .column(provider_quota_statuses::Column::Status)
            .column(provider_quota_statuses::Column::QuotaData)
            .column(provider_quota_statuses::Column::NextResetAt)
            .column(provider_quota_statuses::Column::Ready)
            .column(provider_quota_statuses::Column::NextCheckAt)
            .order_by_asc(provider_quota_statuses::Column::ChannelId)
            .build(DatabaseBackend::Sqlite)
    }

    fn parse_timestamp_or_unix_sql(value: &str, column_index: usize) -> SqlResult<i64> {
        if let Ok(parsed) = value.parse::<i64>() {
            return Ok(parsed);
        }
        humantime::parse_rfc3339_weak(value)
            .map(|time| {
                time.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
            })
            .map_err(|error| {
                SqlError::FromSqlConversionFailure(column_index, SqlType::Text, Box::new(error))
            })
    }

    fn rusqlite_values(statement: &sea_orm::Statement) -> SqlResult<Vec<rusqlite::types::Value>> {
        statement
            .values
            .as_ref()
            .map(|values| {
                values
                    .0
                    .iter()
                    .map(sea_value_to_rusqlite)
                    .collect::<SqlResult<Vec<_>>>()
            })
            .transpose()
            .map(|values| values.unwrap_or_default())
    }

    fn sea_value_to_rusqlite(value: &sea_orm::Value) -> SqlResult<rusqlite::types::Value> {
        use sea_orm::Value;

        match value {
            Value::Bool(Some(inner)) => Ok((*inner as i64).into()),
            Value::TinyInt(Some(inner)) => Ok(i64::from(*inner).into()),
            Value::SmallInt(Some(inner)) => Ok(i64::from(*inner).into()),
            Value::Int(Some(inner)) => Ok(i64::from(*inner).into()),
            Value::BigInt(Some(inner)) => Ok((*inner).into()),
            Value::TinyUnsigned(Some(inner)) => Ok(i64::from(*inner).into()),
            Value::SmallUnsigned(Some(inner)) => Ok(i64::from(*inner).into()),
            Value::Unsigned(Some(inner)) => Ok(i64::from(*inner).into()),
            Value::BigUnsigned(Some(inner)) => i64::try_from(*inner)
                .map(Into::into)
                .map_err(|error| SqlError::ToSqlConversionFailure(Box::new(error))),
            Value::Float(Some(inner)) => Ok(f64::from(*inner).into()),
            Value::Double(Some(inner)) => Ok((*inner).into()),
            Value::String(Some(inner)) => Ok(inner.to_string().into()),
            Value::Char(Some(inner)) => Ok(inner.to_string().into()),
            Value::Bytes(Some(inner)) => Ok(inner.to_vec().into()),
            Value::Bool(None)
            | Value::TinyInt(None)
            | Value::SmallInt(None)
            | Value::Int(None)
            | Value::BigInt(None)
            | Value::TinyUnsigned(None)
            | Value::SmallUnsigned(None)
            | Value::Unsigned(None)
            | Value::BigUnsigned(None)
            | Value::Float(None)
            | Value::Double(None)
            | Value::String(None)
            | Value::Char(None)
            | Value::Bytes(None) => Ok(rusqlite::types::Value::Null),
            _ => Err(SqlError::ToSqlConversionFailure(Box::new(std::io::Error::other(
                format!("unsupported SeaORM sqlite value: {value:?}"),
            )))),
        }
    }

    #[derive(Clone)]
    pub struct SqliteOperationalService {
        pub(crate) foundation: Arc<SqliteFoundation>,
        pub(crate) file_storage_cache: Arc<RwLock<HashMap<i64, CachedFileStorage>>>,
    }

    impl SqliteOperationalService {
        pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
            Self {
                foundation,
                file_storage_cache: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        pub fn refresh_file_systems(&self) -> Result<usize, String> {
            let cache = self
                .foundation
                .operational()
                .refresh_file_storage_cache()
                .map_err(|error| format!("failed to refresh file storages: {error}"))?;
            let count = cache.len();
            let mut writer = self
                .file_storage_cache
                .write()
                .map_err(|_| "failed to lock file storage cache".to_owned())?;
            *writer = cache;
            Ok(count)
        }

        pub fn storage_policy(&self) -> Result<StoredStoragePolicy, String> {
            load_json_setting(
                &self.foundation.system_settings(),
                SYSTEM_KEY_STORAGE_POLICY,
                default_storage_policy(),
            )
            .map_err(|error| format!("failed to load storage policy: {error}"))
        }

        pub fn retry_policy(&self) -> Result<crate::foundation::admin::StoredRetryPolicy, String> {
            load_json_setting(
                &self.foundation.system_settings(),
                "retry_policy",
                crate::foundation::admin::default_retry_policy(),
            )
            .map_err(|error| format!("failed to load retry policy: {error}"))
        }

        pub fn update_storage_policy(
            &self,
            input: AdminGraphqlUpdateStoragePolicyInput,
        ) -> Result<StoredStoragePolicy, String> {
            let mut policy = self.storage_policy()?;
            if let Some(store_chunks) = input.store_chunks {
                policy.store_chunks = store_chunks;
            }
            if let Some(store_request_body) = input.store_request_body {
                policy.store_request_body = store_request_body;
            }
            if let Some(store_response_body) = input.store_response_body {
                policy.store_response_body = store_response_body;
            }
            if let Some(cleanup_options) = input.cleanup_options {
                policy.cleanup_options = cleanup_options
                    .into_iter()
                    .map(|option| StoredCleanupOption {
                        resource_type: option.resource_type,
                        enabled: option.enabled,
                        cleanup_days: option.cleanup_days,
                    })
                    .collect();
            }

            self.store_json_setting(SYSTEM_KEY_STORAGE_POLICY, &policy)?;
            Ok(policy)
        }

        pub fn update_retry_policy(
            &self,
            input: AdminGraphqlUpdateRetryPolicyInput,
        ) -> Result<StoredRetryPolicy, String> {
            let mut policy = self.retry_policy()?;
            if let Some(enabled) = input.enabled {
                policy.enabled = enabled;
            }
            if let Some(max_channel_retries) = input.max_channel_retries {
                policy.max_channel_retries = max_channel_retries.max(0);
            }
            if let Some(max_single_channel_retries) = input.max_single_channel_retries {
                policy.max_single_channel_retries = max_single_channel_retries.max(0);
            }
            if let Some(retry_delay_ms) = input.retry_delay_ms {
                policy.retry_delay_ms = retry_delay_ms.max(0);
            }
            if let Some(load_balancer_strategy) = input.load_balancer_strategy {
                policy.load_balancer_strategy = if load_balancer_strategy == "weighted" {
                    "failover".to_owned()
                } else {
                    load_balancer_strategy
                };
            }
            if let Some(auto_disable_channel) = input.auto_disable_channel {
                if let Some(enabled) = auto_disable_channel.enabled {
                    policy.auto_disable_channel.enabled = enabled;
                }
                if let Some(statuses) = auto_disable_channel.statuses {
                    policy.auto_disable_channel.statuses = statuses
                        .into_iter()
                        .map(|status| StoredAutoDisableChannelStatus {
                            status: status.status,
                            times: status.times,
                        })
                        .collect();
                }
            }

            self.store_json_setting("retry_policy", &policy)?;
            Ok(policy)
        }

        pub fn auto_backup_settings(&self) -> Result<StoredAutoBackupSettings, String> {
            load_json_setting(
                &self.foundation.system_settings(),
                SYSTEM_KEY_AUTO_BACKUP_SETTINGS,
                default_auto_backup_settings(),
            )
            .map_err(|error| format!("failed to load auto backup settings: {error}"))
        }

        pub fn update_auto_backup_settings(
            &self,
            input: AdminGraphqlUpdateAutoBackupSettingsInput,
        ) -> Result<StoredAutoBackupSettings, String> {
            let mut settings = self.auto_backup_settings()?;
            if let Some(enabled) = input.enabled {
                settings.enabled = enabled;
            }
            if let Some(frequency) = input.frequency {
                settings.frequency = frequency;
            }
            if let Some(data_storage_id) = input.data_storage_id {
                settings.data_storage_id = i64::from(data_storage_id);
            }
            if let Some(include_channels) = input.include_channels {
                settings.include_channels = include_channels;
            }
            if let Some(include_models) = input.include_models {
                settings.include_models = include_models;
            }
            if let Some(include_api_keys) = input.include_api_keys {
                settings.include_api_keys = include_api_keys;
            }
            if let Some(include_model_prices) = input.include_model_prices {
                settings.include_model_prices = include_model_prices;
            }
            if let Some(retention_days) = input.retention_days {
                settings.retention_days = retention_days.max(0);
            }
            if settings.enabled && settings.data_storage_id <= 0 {
                return Err("dataStorageID is required when auto backup is enabled".to_owned());
            }

            self.store_json_setting(SYSTEM_KEY_AUTO_BACKUP_SETTINGS, &settings)?;
            Ok(settings)
        }

        pub fn trigger_backup_now(&self) -> Result<String, String> {
            let settings = self.auto_backup_settings()?;
            self.perform_backup(&settings)?;
            Ok("Backup completed successfully".to_owned())
        }

    pub fn system_channel_settings(&self) -> Result<StoredSystemChannelSettings, String> {
        load_json_setting(
            &self.foundation.system_settings(),
            SYSTEM_KEY_CHANNEL_SETTINGS,
            default_system_channel_settings(),
        )
        .map_err(|error| format!("failed to load channel settings: {error}"))
    }

    pub fn video_storage_settings(&self) -> Result<StoredVideoStorageSettings, String> {
        load_json_setting(
            &self.foundation.system_settings(),
            crate::foundation::shared::SYSTEM_KEY_VIDEO_STORAGE_SETTINGS,
            default_video_storage_settings(),
        )
        .map_err(|error| format!("failed to load video storage settings: {error}"))
    }

        pub fn update_system_channel_settings(
            &self,
            input: AdminGraphqlUpdateSystemChannelSettingsInput,
        ) -> Result<StoredSystemChannelSettings, String> {
            let mut settings = self.system_channel_settings()?;
            if let Some(probe) = input.probe {
                settings.probe = super::super::admin::StoredChannelProbeSettings {
                    enabled: probe.enabled,
                    frequency: probe.frequency,
                };
            }
            if let Some(auto_sync) = input.auto_sync {
                settings.auto_sync = super::super::admin::StoredChannelModelAutoSyncSettings {
                    frequency: auto_sync.frequency,
                };
            }
            if let Some(query_all_channel_models) = input.query_all_channel_models {
                settings.query_all_channel_models = query_all_channel_models;
            }
            self.store_json_setting(SYSTEM_KEY_CHANNEL_SETTINGS, &settings)?;
            Ok(settings)
        }

        pub fn proxy_presets(&self) -> Result<Vec<StoredProxyPreset>, String> {
            load_json_setting(
                &self.foundation.system_settings(),
                SYSTEM_KEY_PROXY_PRESETS,
                Vec::<StoredProxyPreset>::new(),
            )
            .map_err(|error| format!("failed to load proxy presets: {error}"))
        }

        pub fn save_proxy_preset(&self, preset: StoredProxyPreset) -> Result<(), String> {
            let mut presets = self.proxy_presets()?;
            if let Some(existing) = presets.iter_mut().find(|item| item.url == preset.url) {
                *existing = preset;
            } else {
                presets.push(preset);
            }
            self.store_json_setting(SYSTEM_KEY_PROXY_PRESETS, &presets)
        }

        pub fn delete_proxy_preset(&self, url: &str) -> Result<(), String> {
            let presets = self
                .proxy_presets()?
                .into_iter()
                .filter(|item| item.url != url)
                .collect::<Vec<_>>();
            self.store_json_setting(SYSTEM_KEY_PROXY_PRESETS, &presets)
        }

        pub fn user_agent_pass_through(&self) -> Result<bool, String> {
            load_json_setting(
                &self.foundation.system_settings(),
                SYSTEM_KEY_USER_AGENT_PASS_THROUGH,
                "false".to_owned(),
            )
            .map(|raw: String| raw.eq_ignore_ascii_case("true"))
            .map_err(|error| format!("failed to load user-agent pass-through setting: {error}"))
        }

        pub fn set_user_agent_pass_through(&self, enabled: bool) -> Result<(), String> {
            let value = if enabled { "true" } else { "false" };
            self.store_json_setting(SYSTEM_KEY_USER_AGENT_PASS_THROUGH, &value)
        }

        pub fn channel_probe_data(
            &self,
            channel_ids: &[String],
        ) -> Result<Vec<StoredChannelProbeData>, String> {
            let parsed_ids = channel_ids
                .iter()
                .map(|value| parse_graphql_resource_id(value, "channel"))
                .collect::<Result<Vec<_>, _>>()?;
            self.foundation
                .operational()
                .list_channel_probe_data(&parsed_ids)
                .map_err(|error| format!("failed to load channel probe data: {error}"))
        }

        pub fn run_provider_quota_check_tick(
            &self,
            force: bool,
            check_interval: Duration,
        ) -> Result<usize, String> {
            let connection = self
                .foundation
                .open_connection(true)
                .map_err(|error| format!("failed to open quota database: {error}"))?;
            ensure_operational_tables(&connection)
                .map_err(|error| format!("failed to ensure quota schema: {error}"))?;

            let channels = self
                .foundation
                .channel_models()
                .list_channels()
                .map_err(|error| {
                    format!("failed to list channels for provider quota checks: {error}")
                })?;
            let now = current_unix_timestamp();
            let next_check_at = now + i64::try_from(check_interval.as_secs()).unwrap_or(0);
            let mut updated = 0;

            for channel in channels.into_iter().filter(|channel| channel.status == "enabled") {
                let Some(provider_type) =
                    provider_quota_type_for_channel(channel.channel_type.as_str())
                else {
                    continue;
                };

                if !force {
                    let due = quota_check_is_due(&connection, channel.id, now).map_err(|error| {
                        format!("failed to load existing quota status: {error}")
                    })?;
                    if !due {
                        continue;
                    }
                }

                let quota_data_json = serde_json::json!({
                    "message": quota_ready_details(provider_type, channel.id),
                    "source": "manual_recheck",
                    "channelId": channel.id,
                })
                .to_string();
                upsert_provider_quota_status(
                    &connection,
                    channel.id,
                    provider_type,
                    "available",
                    true,
                    None,
                    next_check_at,
                    quota_data_json.as_str(),
                )
                .map_err(|error| format!("failed to store provider quota status: {error}"))?;
                updated += 1;
            }

            Ok(updated)
        }

        pub fn provider_quota_statuses(&self) -> Result<Vec<StoredProviderQuotaStatus>, String> {
            self.foundation
                .operational()
                .list_provider_quota_statuses()
                .map_err(|error| format!("failed to load provider quota statuses: {error}"))
        }

        pub fn reset_provider_quota_status(&self, channel_id: i64) -> Result<bool, String> {
            let connection = self
                .foundation
                .open_connection(true)
                .map_err(|error| format!("failed to open quota database: {error}"))?;
            ensure_operational_tables(&connection)
                .map_err(|error| format!("failed to ensure quota schema: {error}"))?;
            let channels = self
                .foundation
                .channel_models()
                .list_channels()
                .map_err(|error| {
                    format!("failed to list channels for provider quota reset: {error}")
                })?;
            let Some(channel) = channels.into_iter().find(|channel| channel.id == channel_id) else {
                return Ok(false);
            };
            let Some(provider_type) = provider_quota_type_for_channel(channel.channel_type.as_str())
            else {
                return Ok(false);
            };
            let next_check_at = current_unix_timestamp();
            let quota_data_json = serde_json::json!({
                "message": quota_reset_details(provider_type, channel.id),
                "source": "manual_reset",
                "channelId": channel.id,
            })
            .to_string();
            upsert_provider_quota_status(
                &connection,
                channel.id,
                provider_type,
                "available",
                true,
                None,
                next_check_at,
                quota_data_json.as_str(),
            )
            .map_err(|error| format!("failed to store reset provider quota status: {error}"))?;
            Ok(true)
        }

        pub fn run_gc_cleanup_now(
            &self,
            _vacuum_enabled: bool,
            _vacuum_full: bool,
        ) -> Result<StoredGcCleanupSummary, String> {
            let policy = self.storage_policy()?;
            let connection = self
                .foundation
                .open_connection(true)
                .map_err(|error| format!("failed to open gc database: {error}"))?;
            ensure_all_foundation_tables(&connection)
                .map_err(|error| format!("failed to ensure gc schema: {error}"))?;
            ensure_operational_tables(&connection)
                .map_err(|error| format!("failed to ensure operational gc schema: {error}"))?;

            let mut summary = StoredGcCleanupSummary::default();
            for option in policy.cleanup_options {
                if !option.enabled {
                    continue;
                }
                let cutoff = current_unix_timestamp() - i64::from(option.cleanup_days.max(0)) * 86_400;
                match option.resource_type.as_str() {
                    "requests" => {
                        summary.request_executions_deleted +=
                            cleanup_request_executions(&connection, cutoff).map_err(|error| {
                                format!("failed to cleanup request executions: {error}")
                            })?;
                        summary.requests_deleted += cleanup_requests(&connection, cutoff, self)
                            .map_err(|error| format!("failed to cleanup requests: {error}"))?;
                        summary.threads_deleted += cleanup_threads(&connection, cutoff)
                            .map_err(|error| format!("failed to cleanup threads: {error}"))?;
                        summary.traces_deleted += cleanup_traces(&connection, cutoff)
                            .map_err(|error| format!("failed to cleanup traces: {error}"))?;
                    }
                    "usage_logs" => {
                        summary.usage_logs_deleted += cleanup_usage_logs(&connection, cutoff)
                            .map_err(|error| format!("failed to cleanup usage logs: {error}"))?;
                    }
                    _ => {}
                }
            }

            let channel_probe_cutoff = current_unix_timestamp() - 3 * 86_400;
            summary.channel_probes_deleted += cleanup_channel_probes(&connection, channel_probe_cutoff)
                .map_err(|error| format!("failed to cleanup channel probes: {error}"))?;

            // Keep the summary field for contract stability, but do not run VACUUM here.

            Ok(summary)
        }

        fn perform_backup(&self, settings: &StoredAutoBackupSettings) -> Result<(), String> {
            if settings.data_storage_id <= 0 {
                self.record_backup_status(Some("data storage not configured for backup".to_owned()))?;
                return Err("data storage not configured for backup".to_owned());
            }

            self.refresh_file_systems()?;
            let storage = self.cached_file_storage(settings.data_storage_id).ok_or_else(|| {
                "backup data storage is not an active fs storage in the Rust slice".to_owned()
            })?;
            fs::create_dir_all(storage.root.as_path())
                .map_err(|error| format!("failed to create backup directory: {error}"))?;

            let backup = self.build_backup_payload(settings)?;
            let filename = format!(
                "{AUTO_BACKUP_PREFIX}{}{AUTO_BACKUP_SUFFIX}",
                current_unix_timestamp()
            );
            let path = storage.root.join(filename);
            let contents = serde_json::to_vec_pretty(&backup)
                .map_err(|error| format!("failed to serialize backup: {error}"))?;
            let write_result = fs::write(path.as_path(), contents)
                .map_err(|error| format!("failed to write backup file: {error}"));

            match write_result {
                Ok(()) => {
                    if settings.retention_days > 0 {
                        self.cleanup_old_backups(storage.root.as_path(), settings.retention_days)?;
                    }
                    self.record_backup_status(None)?;
                    Ok(())
                }
                Err(error) => {
                    self.record_backup_status(Some(error.clone()))?;
                    Err(error)
                }
            }
        }

        fn build_backup_payload(
            &self,
            settings: &StoredAutoBackupSettings,
        ) -> Result<StoredBackupPayload, String> {
            let connection = self
                .foundation
                .open_connection(true)
                .map_err(|error| format!("failed to open backup database: {error}"))?;
            ensure_all_foundation_tables(&connection)
                .map_err(|error| format!("failed to ensure backup schema: {error}"))?;

            let channels = if settings.include_channels {
                list_backup_channels(&connection)
                    .map_err(|error| format!("failed to load backup channels: {error}"))?
            } else {
                Vec::new()
            };
            let models = if settings.include_models {
                list_backup_models(&connection)
                    .map_err(|error| format!("failed to load backup models: {error}"))?
            } else {
                Vec::new()
            };
            let api_keys = if settings.include_api_keys {
                list_backup_api_keys(&connection)
                    .map_err(|error| format!("failed to load backup api keys: {error}"))?
            } else {
                Vec::new()
            };

            Ok(StoredBackupPayload {
                version: BACKUP_VERSION.to_owned(),
                timestamp: current_rfc3339_timestamp(),
                channels,
                models,
                channel_model_prices: Vec::new(),
                api_keys,
            })
        }

        fn cleanup_old_backups(&self, root: &Path, retention_days: i32) -> Result<(), String> {
            let cutoff = SystemTime::now()
                .checked_sub(Duration::from_secs(
                    u64::try_from(retention_days.max(0)).unwrap_or(0) * 86_400,
                ))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            for entry in fs::read_dir(root)
                .map_err(|error| format!("failed to read backup directory: {error}"))?
            {
                let entry = entry.map_err(|error| {
                    format!("failed to inspect backup directory entry: {error}")
                })?;
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if !file_name.starts_with(AUTO_BACKUP_PREFIX)
                    || !file_name.ends_with(AUTO_BACKUP_SUFFIX)
                {
                    continue;
                }
                let metadata = entry
                    .metadata()
                    .map_err(|error| format!("failed to read backup metadata: {error}"))?;
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if modified < cutoff {
                    let _ = fs::remove_file(entry.path());
                }
            }
            Ok(())
        }

        fn record_backup_status(&self, error_message: Option<String>) -> Result<(), String> {
            let mut settings = self.auto_backup_settings()?;
            settings.last_backup_at = Some(current_unix_timestamp());
            settings.last_backup_error = error_message.unwrap_or_default();
            self.store_json_setting(SYSTEM_KEY_AUTO_BACKUP_SETTINGS, &settings)
        }

        fn cached_file_storage(&self, storage_id: i64) -> Option<CachedFileStorage> {
            self.file_storage_cache
                .read()
                .ok()
                .and_then(|cache| cache.get(&storage_id).cloned())
        }

        fn store_json_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<(), String> {
            let json = serde_json::to_string(value)
                .map_err(|error| format!("failed to serialize setting: {error}"))?;
            self.foundation
                .system_settings()
                .set_value(key, json.as_str())
                .map_err(|error| format!("failed to persist setting: {error}"))
        }
    }

    fn load_json_setting<T: DeserializeOwned>(
        settings: &SystemSettingsStore,
        key: &str,
        default: T,
    ) -> SqlResult<T> {
        match settings.value(key)? {
            None => Ok(default),
            Some(value) => serde_json::from_str(value.as_str()).map_err(json_setting_decode_error),
        }
    }

    fn json_setting_decode_error(error: serde_json::Error) -> SqlError {
        SqlError::FromSqlConversionFailure(0, SqlType::Text, Box::new(error))
    }

    fn quota_check_is_due(connection: &Connection, channel_id: i64, now: i64) -> SqlResult<bool> {
        let next_check_at: Option<String> = connection
            .query_row(
                "SELECT next_check_at FROM provider_quota_statuses WHERE channel_id = ?1 LIMIT 1",
                [channel_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(next_check_at
            .as_deref()
            .map(|value| parse_timestamp_or_unix_sql(value, 0))
            .transpose()?
            .is_none_or(|value| value <= now))
    }

    fn upsert_provider_quota_status(
        connection: &Connection,
        channel_id: i64,
        provider_type: &str,
        status: &str,
        ready: bool,
        next_reset_at: Option<i64>,
        next_check_at: i64,
        quota_data_json: &str,
    ) -> SqlResult<()> {
        let next_reset_at = next_reset_at.map(format_unix_timestamp);
        let next_check_at = format_unix_timestamp(next_check_at);
        connection.execute(
            "INSERT INTO provider_quota_statuses (channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(channel_id) DO UPDATE SET
                 provider_type = excluded.provider_type,
                 status = excluded.status,
                 quota_data = excluded.quota_data,
                 next_reset_at = excluded.next_reset_at,
                 ready = excluded.ready,
                 next_check_at = excluded.next_check_at,
                 updated_at = CURRENT_TIMESTAMP",
            params![
                channel_id,
                provider_type,
                status,
                quota_data_json,
                next_reset_at,
                bool_to_sql(ready),
                next_check_at,
            ],
        )?;
        Ok(())
    }

    fn cleanup_request_executions(connection: &Connection, cutoff: i64) -> SqlResult<i64> {
        connection.execute(
            "DELETE FROM request_executions WHERE created_at < datetime(?1, 'unixepoch')",
            [cutoff],
        )?;
        Ok(connection.changes() as i64)
    }

    fn cleanup_requests(
        connection: &Connection,
        cutoff: i64,
        operational: &SqliteOperationalService,
    ) -> SqlResult<i64> {
        let mut statement = connection.prepare(
            "SELECT content_storage_id, content_storage_key FROM requests WHERE created_at < datetime(?1, 'unixepoch') AND content_storage_id IS NOT NULL AND content_storage_key IS NOT NULL",
        )?;
        let rows = statement.query_map([cutoff], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (storage_id, key) = row?;
            if let Some(storage) = operational.cached_file_storage(storage_id) {
                let relative = key.trim_start_matches('/');
                let _ = fs::remove_file(storage.root.join(relative));
            }
        }
        connection.execute(
            "DELETE FROM requests WHERE created_at < datetime(?1, 'unixepoch')",
            [cutoff],
        )?;
        Ok(connection.changes() as i64)
    }

    fn cleanup_threads(connection: &Connection, cutoff: i64) -> SqlResult<i64> {
        connection.execute(
            "DELETE FROM threads WHERE created_at < datetime(?1, 'unixepoch')",
            [cutoff],
        )?;
        Ok(connection.changes() as i64)
    }

    fn cleanup_traces(connection: &Connection, cutoff: i64) -> SqlResult<i64> {
        connection.execute(
            "DELETE FROM traces WHERE created_at < datetime(?1, 'unixepoch')",
            [cutoff],
        )?;
        Ok(connection.changes() as i64)
    }

    fn cleanup_usage_logs(connection: &Connection, cutoff: i64) -> SqlResult<i64> {
        connection.execute(
            "DELETE FROM usage_logs WHERE created_at < datetime(?1, 'unixepoch')",
            [cutoff],
        )?;
        Ok(connection.changes() as i64)
    }

    fn cleanup_channel_probes(connection: &Connection, cutoff: i64) -> SqlResult<i64> {
        connection.execute("DELETE FROM channel_probes WHERE timestamp < ?1", [cutoff])?;
        Ok(connection.changes() as i64)
    }

    fn list_backup_channels(connection: &Connection) -> SqlResult<Vec<StoredBackupChannel>> {
        let mut statement = connection.prepare(
            "SELECT id, name, type, base_url, status, credentials, supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark
             FROM channels WHERE deleted_at = 0 ORDER BY id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredBackupChannel {
                id: row.get(0)?,
                name: row.get(1)?,
                channel_type: row.get(2)?,
                base_url: row.get(3)?,
                status: row.get(4)?,
                credentials: serde_json::from_str::<Value>(row.get::<_, String>(5)?.as_str())
                    .unwrap_or(Value::Null),
                supported_models: serde_json::from_str::<Value>(row.get::<_, String>(6)?.as_str())
                    .unwrap_or(Value::Null),
                default_test_model: row.get(7)?,
                settings: serde_json::from_str::<Value>(row.get::<_, String>(8)?.as_str())
                    .unwrap_or(Value::Null),
                tags: serde_json::from_str::<Value>(row.get::<_, String>(9)?.as_str())
                    .unwrap_or(Value::Null),
                ordering_weight: row.get(10)?,
                error_message: row.get(11)?,
                remark: row.get(12)?,
            })
        })?;
        rows.collect()
    }

    fn list_backup_models(connection: &Connection) -> SqlResult<Vec<StoredBackupModel>> {
        let mut statement = connection.prepare(
            "SELECT id, developer, model_id, type, name, icon, \"group\", model_card, settings, status, remark
             FROM models WHERE deleted_at = 0 ORDER BY id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredBackupModel {
                id: row.get(0)?,
                developer: row.get(1)?,
                model_id: row.get(2)?,
                model_type: row.get(3)?,
                name: row.get(4)?,
                icon: row.get(5)?,
                group: row.get(6)?,
                model_card: serde_json::from_str::<Value>(row.get::<_, String>(7)?.as_str())
                    .unwrap_or(Value::Null),
                settings: serde_json::from_str::<Value>(row.get::<_, String>(8)?.as_str())
                    .unwrap_or(Value::Null),
                status: row.get(9)?,
                remark: row.get(10)?,
            })
        })?;
        rows.collect()
    }

    fn list_backup_api_keys(connection: &Connection) -> SqlResult<Vec<StoredBackupApiKey>> {
        let mut statement = connection.prepare(
            "SELECT ak.id, ak.project_id, COALESCE(p.name, ''), ak.key, ak.name, ak.type, ak.status, ak.scopes
             FROM api_keys ak
             LEFT JOIN projects p ON p.id = ak.project_id
             WHERE ak.deleted_at = 0
             ORDER BY ak.id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredBackupApiKey {
                id: row.get(0)?,
                project_id: row.get(1)?,
                project_name: row.get(2)?,
                key: row.get(3)?,
                name: row.get(4)?,
                key_type: row.get(5)?,
                status: row.get(6)?,
                scopes: serde_json::from_str::<Value>(row.get::<_, String>(7)?.as_str())
                    .unwrap_or(Value::Null),
            })
        })?;
        rows.collect()
    }
}
