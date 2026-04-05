use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use axonhub_db_entity::{
    api_keys, channel_model_price_versions, channel_model_prices, channel_probes, channels,
    data_storages, models, operational_runs, projects, provider_quota_statuses,
    request_executions, requests, systems, threads, traces, usage_logs,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set, Statement, TransactionTrait,
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

use super::repositories::common::{
    execute as execute_sql, query_all, query_one as query_one_sql,
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
                        .execute(Statement::from_string(connection.get_database_backend(), "VACUUM".to_owned()))
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
    backend: DatabaseBackend,
) -> Result<Vec<(i64, String)>, String> {
    connection
        .query_all(Statement::from_string(
            backend,
            "SELECT id, type FROM channels WHERE deleted_at = 0 AND status = 'enabled' ORDER BY id ASC"
                .to_owned(),
        ))
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| {
            Ok::<(i64, String), String>(
                (
                    row.try_get_by_index(0).map_err(|error| error.to_string())?,
                    row.try_get_by_index(1).map_err(|error| error.to_string())?,
                ),
            )
        })
        .collect()
}

async fn query_next_quota_check_at(
    connection: &DatabaseConnection,
    backend: DatabaseBackend,
    channel_id: i64,
) -> Result<Option<i64>, String> {
    let row = query_one_sql(
        connection,
        backend,
        "SELECT next_check_at FROM provider_quota_statuses WHERE channel_id = ? LIMIT 1",
        "SELECT next_check_at FROM provider_quota_statuses WHERE channel_id = $1 LIMIT 1",
        "SELECT next_check_at FROM provider_quota_statuses WHERE channel_id = ? LIMIT 1",
        vec![channel_id.into()],
    )
    .await
    .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };

    match backend {
        DatabaseBackend::Sqlite => {
            let value: i64 = row.try_get_by_index(0).map_err(|error| error.to_string())?;
            Ok(Some(value))
        }
        DatabaseBackend::Postgres => {
            let value: String = row.try_get_by_index(0).map_err(|error| error.to_string())?;
            parse_timestamp_or_unix(value.as_str()).map(Some)
        }
        DatabaseBackend::MySql => Err("mysql is unsupported in the Rust slice".to_owned()),
    }
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
    backend: DatabaseBackend,
    channel_id: i64,
) -> Result<Option<String>, String> {
    let row = query_one_sql(
        connection,
        backend,
        "SELECT type FROM channels WHERE id = ? AND deleted_at = 0 LIMIT 1",
        "SELECT type FROM channels WHERE id = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT type FROM channels WHERE id = ? LIMIT 1",
        vec![channel_id.into()],
    )
    .await
    .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let channel_type: String = row.try_get_by_index(0).map_err(|error| error.to_string())?;
    Ok(provider_quota_type_for_channel(channel_type.as_str()).map(str::to_owned))
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
    let backend = connection.get_database_backend();
    let channels_out = if settings.include_channels {
        let rows = connection
            .query_all(Statement::from_string(
                backend,
                "SELECT id, name, type, COALESCE(base_url, ''), status, credentials, supported_models, default_test_model, settings, tags, ordering_weight, COALESCE(error_message, ''), COALESCE(remark, '') FROM channels WHERE deleted_at = 0 ORDER BY id ASC".to_owned(),
            ))
            .await
            .map_err(|error| error.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok(StoredBackupChannel {
                    id: row.try_get_by_index(0).map_err(|error| error.to_string())?,
                    name: row.try_get_by_index(1).map_err(|error| error.to_string())?,
                    channel_type: row.try_get_by_index(2).map_err(|error| error.to_string())?,
                    base_url: row.try_get_by_index(3).map_err(|error| error.to_string())?,
                    status: row.try_get_by_index(4).map_err(|error| error.to_string())?,
                    credentials: parse_json_value(&row.try_get_by_index::<String>(5).map_err(|error| error.to_string())?),
                    supported_models: parse_json_value(&row.try_get_by_index::<String>(6).map_err(|error| error.to_string())?),
                    default_test_model: row.try_get_by_index(7).map_err(|error| error.to_string())?,
                    settings: parse_json_value(&row.try_get_by_index::<String>(8).map_err(|error| error.to_string())?),
                    tags: parse_json_value(&row.try_get_by_index::<String>(9).map_err(|error| error.to_string())?),
                    ordering_weight: row.try_get_by_index::<i64>(10).map_err(|error| error.to_string())?,
                    error_message: row.try_get_by_index(11).map_err(|error| error.to_string())?,
                    remark: row.try_get_by_index(12).map_err(|error| error.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        Vec::new()
    };

    let models_out = if settings.include_models {
        let rows = connection
            .query_all(Statement::from_string(
                backend,
                "SELECT id, developer, model_id, type, name, icon, \"group\", model_card, settings, status, COALESCE(remark, '') FROM models WHERE deleted_at = 0 ORDER BY id ASC".to_owned(),
            ))
            .await
            .map_err(|error| error.to_string())?;
        rows.into_iter()
            .map(|row| {
                Ok(StoredBackupModel {
                    id: row.try_get_by_index(0).map_err(|error| error.to_string())?,
                    developer: row.try_get_by_index(1).map_err(|error| error.to_string())?,
                    model_id: row.try_get_by_index(2).map_err(|error| error.to_string())?,
                    model_type: row.try_get_by_index(3).map_err(|error| error.to_string())?,
                    name: row.try_get_by_index(4).map_err(|error| error.to_string())?,
                    icon: row.try_get_by_index(5).map_err(|error| error.to_string())?,
                    group: row.try_get_by_index(6).map_err(|error| error.to_string())?,
                    model_card: parse_json_value(&row.try_get_by_index::<String>(7).map_err(|error| error.to_string())?),
                    settings: parse_json_value(&row.try_get_by_index::<String>(8).map_err(|error| error.to_string())?),
                    status: row.try_get_by_index(9).map_err(|error| error.to_string())?,
                    remark: row.try_get_by_index(10).map_err(|error| error.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
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
            let backend = txn.get_database_backend();
            let existing = query_one_sql(
                txn,
                backend,
                "SELECT id FROM channels WHERE name = ? LIMIT 1",
                "SELECT id FROM channels WHERE name = $1 LIMIT 1",
                "SELECT id FROM channels WHERE name = ? LIMIT 1",
                vec![channel.name.clone().into()],
            )
            .await
            .map_err(|error| error.to_string())?;

            let credentials = serde_json::to_string(&channel.credentials).map_err(|error| error.to_string())?;
            let supported_models = serde_json::to_string(&channel.supported_models).map_err(|error| error.to_string())?;
            let settings = serde_json::to_string(&channel.settings).map_err(|error| error.to_string())?;
            let tags = serde_json::to_string(&channel.tags).map_err(|error| error.to_string())?;

            let restored_id = if let Some(existing) = existing {
                let existing_id = existing.try_get_by_index::<i64>(0).map_err(|error| error.to_string())?;
                if !options.overwrite_existing {
                    return Err(format!("channel already exists: {}", channel.name));
                }
                execute_sql(
                    txn,
                    backend,
                    "UPDATE channels SET type = ?, base_url = ?, name = ?, status = ?, credentials = ?, supported_models = ?, auto_sync_supported_models = ?, default_test_model = ?, settings = ?, tags = ?, ordering_weight = ?, error_message = ?, remark = ?, deleted_at = 0 WHERE id = ?",
                    "UPDATE channels SET type = $1, base_url = $2, name = $3, status = $4, credentials = $5, supported_models = $6, auto_sync_supported_models = $7, default_test_model = $8, settings = $9, tags = $10, ordering_weight = $11, error_message = $12, remark = $13, deleted_at = 0 WHERE id = $14",
                    "UPDATE channels SET type = ?, base_url = ?, name = ?, status = ?, credentials = ?, supported_models = ?, auto_sync_supported_models = ?, default_test_model = ?, settings = ?, tags = ?, ordering_weight = ?, error_message = ?, remark = ?, deleted_at = 0 WHERE id = ?",
                    vec![
                        channel.channel_type.clone().into(),
                        channel.base_url.clone().into(),
                        channel.name.clone().into(),
                        channel.status.clone().into(),
                        credentials.clone().into(),
                        supported_models.clone().into(),
                        false.into(),
                        channel.default_test_model.clone().into(),
                        settings.clone().into(),
                        tags.clone().into(),
                        i32::try_from(channel.ordering_weight).unwrap_or(i32::MAX).into(),
                        channel.error_message.clone().into(),
                        channel.remark.clone().into(),
                        existing_id.into(),
                    ],
                )
                .await
                .map_err(|error| error.to_string())?;
                existing_id
            } else {
                execute_sql(
                    txn,
                    backend,
                    "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
                    "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 0)",
                    "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
                    vec![
                        channel.channel_type.clone().into(),
                        channel.base_url.clone().into(),
                        channel.name.clone().into(),
                        channel.status.clone().into(),
                        credentials.clone().into(),
                        supported_models.clone().into(),
                        false.into(),
                        channel.default_test_model.clone().into(),
                        settings.clone().into(),
                        tags.clone().into(),
                        i32::try_from(channel.ordering_weight).unwrap_or(i32::MAX).into(),
                        channel.error_message.clone().into(),
                        channel.remark.clone().into(),
                    ],
                )
                .await
                .map_err(|error| error.to_string())?;

                let inserted = query_one_sql(
                    txn,
                    backend,
                    "SELECT id FROM channels WHERE name = ? AND deleted_at = 0 ORDER BY id DESC LIMIT 1",
                    "SELECT id FROM channels WHERE name = $1 AND deleted_at = 0 ORDER BY id DESC LIMIT 1",
                    "SELECT id FROM channels WHERE name = ? AND deleted_at = 0 ORDER BY id DESC LIMIT 1",
                    vec![channel.name.clone().into()],
                )
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    format!("failed to resolve restored channel id for channel {}", channel.name)
                })?;

                inserted
                    .try_get_by_index::<i64>(0)
                    .map_err(|error| error.to_string())?
            };
            channel_name_to_id.insert(channel.name.clone(), restored_id);
        }
    } else {
        let backend = txn.get_database_backend();
        let existing_channels = query_all(
            txn,
            backend,
            "SELECT id, name FROM channels WHERE deleted_at = 0",
            "SELECT id, name FROM channels WHERE deleted_at = 0",
            "SELECT id, name FROM channels WHERE deleted_at = 0",
            Vec::new(),
        )
        .await
        .map_err(|error| error.to_string())?;
        for channel in existing_channels {
            let channel_id = channel.try_get_by_index::<i64>(0).map_err(|error| error.to_string())?;
            let channel_name = channel.try_get_by_index::<String>(1).map_err(|error| error.to_string())?;
            channel_name_to_id.insert(channel_name, channel_id);
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
                    let backend = txn.get_database_backend();
                    execute_sql(
                        txn,
                        backend,
                        "UPDATE channel_model_price_versions SET status = ?, effective_end_at = CURRENT_TIMESTAMP WHERE channel_model_price_id = ? AND status = ?",
                        "UPDATE channel_model_price_versions SET status = $1, effective_end_at = CURRENT_TIMESTAMP WHERE channel_model_price_id = $2 AND status = $3",
                        "UPDATE channel_model_price_versions SET status = ?, effective_end_at = CURRENT_TIMESTAMP WHERE channel_model_price_id = ? AND status = ?",
                        vec!["archived".into(), existing.id.into(), "active".into()],
                    )
                    .await
                    .map_err(|error| error.to_string())?;
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

            let backend = txn.get_database_backend();
            execute_sql(
                txn,
                backend,
                "INSERT INTO channel_model_price_versions (channel_id, model_id, channel_model_price_id, price, status, effective_start_at, effective_end_at, reference_id) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, NULL, ?)",
                "INSERT INTO channel_model_price_versions (channel_id, model_id, channel_model_price_id, price, status, effective_start_at, effective_end_at, reference_id) VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP, NULL, $6)",
                "INSERT INTO channel_model_price_versions (channel_id, model_id, channel_model_price_id, price, status, effective_start_at, effective_end_at, reference_id) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, NULL, ?)",
                vec![
                    channel_id.into(),
                    model_id.to_owned().into(),
                    channel_model_price_id.into(),
                    price_json.into(),
                    "active".into(),
                    reference_id.to_owned().into(),
                ],
            )
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
    let backend = connection.get_database_backend();
    let started_at = current_rfc3339_timestamp();
    execute_sql(
        connection,
        backend,
        "INSERT INTO operational_runs (operation_type, trigger_source, status, result_payload, error_message, initiated_by_user_id, data_storage_id, channel_id, project_id, started_at, finished_at) VALUES (?, ?, ?, NULL, NULL, ?, ?, ?, ?, CURRENT_TIMESTAMP, NULL)",
        "INSERT INTO operational_runs (operation_type, trigger_source, status, result_payload, error_message, initiated_by_user_id, data_storage_id, channel_id, project_id, started_at, finished_at) VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, CURRENT_TIMESTAMP, NULL)",
        "INSERT INTO operational_runs (operation_type, trigger_source, status, result_payload, error_message, initiated_by_user_id, data_storage_id, channel_id, project_id, started_at, finished_at) VALUES (?, ?, ?, NULL, NULL, ?, ?, ?, ?, CURRENT_TIMESTAMP, NULL)",
        vec![
            operation_type.into(),
            trigger_source.into(),
            OperationalRunStatus::Running.as_str().into(),
            initiated_by_user_id.into(),
            data_storage_id.into(),
            channel_id.into(),
            project_id.into(),
        ],
    )
    .await
    .map_err(|error| error.to_string())?;
    let row = query_one_sql(
        connection,
        backend,
        "SELECT id FROM operational_runs ORDER BY id DESC LIMIT 1",
        "SELECT id FROM operational_runs ORDER BY id DESC LIMIT 1",
        "SELECT id FROM operational_runs ORDER BY id DESC LIMIT 1",
        Vec::new(),
    )
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "missing operational run id".to_owned())?;
    let run_id: i64 = row.try_get_by_index(0).map_err(|error| error.to_string())?;
    Ok(OperationalRunRecord {
        id: run_id,
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
    let backend = connection.get_database_backend();
    execute_sql(
        connection,
        backend,
        "UPDATE operational_runs SET status = ?, result_payload = ?, error_message = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?",
        "UPDATE operational_runs SET status = $1, result_payload = $2, error_message = $3, finished_at = CURRENT_TIMESTAMP WHERE id = $4",
        "UPDATE operational_runs SET status = ?, result_payload = ?, error_message = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?",
        vec![
            status.as_str().into(),
            result_payload.into(),
            error_message.into(),
            run_id.into(),
        ],
    )
    .await
    .map_err(|error| error.to_string())?;
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
    let backend = connection.get_database_backend();
    match backend {
        DatabaseBackend::Sqlite => {
            execute_sql(
                connection,
                backend,
                "INSERT INTO provider_quota_statuses (channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(channel_id) DO UPDATE SET provider_type = excluded.provider_type, status = excluded.status, quota_data = excluded.quota_data, next_reset_at = excluded.next_reset_at, ready = excluded.ready, next_check_at = excluded.next_check_at, updated_at = CURRENT_TIMESTAMP",
                "",
                "INSERT INTO provider_quota_statuses (channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(channel_id) DO UPDATE SET provider_type = excluded.provider_type, status = excluded.status, quota_data = excluded.quota_data, next_reset_at = excluded.next_reset_at, ready = excluded.ready, next_check_at = excluded.next_check_at, updated_at = CURRENT_TIMESTAMP",
                vec![
                    channel_id.into(),
                    provider_type.into(),
                    status.into(),
                    quota_data_json.into(),
                    next_reset_at.into(),
                    ready.into(),
                    next_check_at.into(),
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        }
        DatabaseBackend::Postgres => {
            execute_sql(
                connection,
                backend,
                "",
                "INSERT INTO provider_quota_statuses (channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at, deleted_at) VALUES ($1, $2, $3, $4, CASE WHEN $5 IS NULL THEN NULL ELSE TO_TIMESTAMP($5) END, $6, TO_TIMESTAMP($7), 0) ON CONFLICT(channel_id) DO UPDATE SET provider_type = EXCLUDED.provider_type, status = EXCLUDED.status, quota_data = EXCLUDED.quota_data, next_reset_at = EXCLUDED.next_reset_at, ready = EXCLUDED.ready, next_check_at = EXCLUDED.next_check_at, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
                "",
                vec![
                    channel_id.into(),
                    provider_type.into(),
                    status.into(),
                    quota_data_json.into(),
                    next_reset_at.into(),
                    ready.into(),
                    next_check_at.into(),
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        }
        DatabaseBackend::MySql => return Err("mysql is unsupported in the Rust slice".to_owned()),
    }
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
