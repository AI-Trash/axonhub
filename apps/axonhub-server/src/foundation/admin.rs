use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use async_graphql::Enum;
use axonhub_http::{AdminContentDownload, AdminError, AdminPort, AuthUserContext};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use super::{
    authz::{user_has_project_scope, user_has_system_scope, SCOPE_READ_REQUESTS},
    graphql::{
        AdminGraphqlUpdateAutoBackupSettingsInput, AdminGraphqlUpdateStoragePolicyInput,
        AdminGraphqlUpdateSystemChannelSettingsInput,
    },
    shared::{
        bool_to_sql, current_rfc3339_timestamp, current_unix_timestamp, SqliteConnectionFactory,
        SqliteFoundation, AUTO_BACKUP_PREFIX, AUTO_BACKUP_SUFFIX, BACKUP_VERSION,
        SYSTEM_KEY_AUTO_BACKUP_SETTINGS, SYSTEM_KEY_CHANNEL_SETTINGS, SYSTEM_KEY_STORAGE_POLICY,
    },
    system::{ensure_all_foundation_tables, ensure_operational_tables, SystemSettingsStore},
};

#[derive(Debug, Clone)]
pub struct OperationalStore {
    pub(crate) connection_factory: SqliteConnectionFactory,
}

impl OperationalStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn refresh_file_storage_cache(&self) -> rusqlite::Result<HashMap<i64, CachedFileStorage>> {
        let connection = self.connection_factory.open(true)?;
        ensure_operational_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, settings FROM data_storages
             WHERE deleted_at = 0 AND status = 'active' AND type = 'fs'",
        )?;
        let rows = statement.query_map([], |row| {
            let storage_id: i64 = row.get(0)?;
            let settings_json: String = row.get(1)?;
            Ok((storage_id, settings_json))
        })?;

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
    ) -> rusqlite::Result<Vec<StoredChannelProbeData>> {
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
            let existing = rows.collect::<rusqlite::Result<Vec<_>>>()?;
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

    pub fn list_provider_quota_statuses(&self) -> rusqlite::Result<Vec<StoredProviderQuotaStatus>> {
        let connection = self.connection_factory.open(true)?;
        ensure_operational_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, channel_id, provider_type, status, quota_data, next_reset_at, ready, next_check_at
             FROM provider_quota_statuses
             ORDER BY channel_id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredProviderQuotaStatus {
                id: row.get(0)?,
                channel_id: row.get(1)?,
                provider_type: row.get(2)?,
                status: row.get(3)?,
                quota_data_json: row.get(4)?,
                next_reset_at: row.get(5)?,
                ready: row.get::<_, i64>(6)? != 0,
                next_check_at: row.get(7)?,
            })
        })?;
        rows.collect()
    }
}

pub struct SqliteAdminService {
    pub(crate) foundation: Arc<SqliteFoundation>,
}

#[derive(Clone)]
pub struct SqliteOperationalService {
    pub(crate) foundation: Arc<SqliteFoundation>,
    pub(crate) file_storage_cache: Arc<RwLock<HashMap<i64, CachedFileStorage>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredCleanupOption {
    pub(crate) resource_type: String,
    pub(crate) enabled: bool,
    pub(crate) cleanup_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredStoragePolicy {
    pub(crate) store_chunks: bool,
    pub(crate) store_request_body: bool,
    pub(crate) store_response_body: bool,
    pub(crate) cleanup_options: Vec<StoredCleanupOption>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum BackupFrequencySetting {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
pub(crate) enum ProbeFrequencySetting {
    #[graphql(name = "ONE_MINUTE")]
    OneMinute,
    #[graphql(name = "FIVE_MINUTES")]
    FiveMinutes,
    #[graphql(name = "THIRTY_MINUTES")]
    ThirtyMinutes,
    #[graphql(name = "ONE_HOUR")]
    OneHour,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredChannelProbeSettings {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredSystemChannelSettings {
    pub(crate) probe: StoredChannelProbeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredAutoBackupSettings {
    pub(crate) enabled: bool,
    pub(crate) frequency: BackupFrequencySetting,
    pub(crate) data_storage_id: i64,
    pub(crate) include_channels: bool,
    pub(crate) include_models: bool,
    pub(crate) include_api_keys: bool,
    pub(crate) include_model_prices: bool,
    pub(crate) retention_days: i32,
    pub(crate) last_backup_at: Option<i64>,
    pub(crate) last_backup_error: String,
}

impl ProbeFrequencySetting {
    fn interval_minutes(self) -> i32 {
        match self {
            Self::OneMinute => 1,
            Self::FiveMinutes => 5,
            Self::ThirtyMinutes => 30,
            Self::OneHour => 60,
        }
    }

    fn query_range_minutes(self) -> i32 {
        match self {
            Self::OneMinute => 10,
            Self::FiveMinutes => 60,
            Self::ThirtyMinutes => 720,
            Self::OneHour => 1440,
        }
    }
}

impl StoredChannelProbeSettings {
    fn interval_minutes(&self) -> i32 {
        self.frequency.interval_minutes()
    }

    fn query_range_minutes(&self) -> i32 {
        self.frequency.query_range_minutes()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CachedFileStorage {
    pub(crate) root: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredChannelProbePoint {
    pub(crate) timestamp: i64,
    pub(crate) total_request_count: i32,
    pub(crate) success_request_count: i32,
    pub(crate) avg_tokens_per_second: Option<f64>,
    pub(crate) avg_time_to_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredChannelProbeData {
    pub(crate) channel_id: i64,
    pub(crate) points: Vec<StoredChannelProbePoint>,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredProviderQuotaStatus {
    pub(crate) id: i64,
    pub(crate) channel_id: i64,
    pub(crate) provider_type: String,
    pub(crate) status: String,
    pub(crate) quota_data_json: String,
    pub(crate) next_reset_at: Option<i64>,
    pub(crate) ready: bool,
    pub(crate) next_check_at: i64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StoredGcCleanupSummary {
    pub(crate) requests_deleted: i64,
    pub(crate) request_executions_deleted: i64,
    pub(crate) threads_deleted: i64,
    pub(crate) traces_deleted: i64,
    pub(crate) usage_logs_deleted: i64,
    pub(crate) channel_probes_deleted: i64,
    pub(crate) vacuum_ran: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StoredBackupPayload {
    pub(crate) version: String,
    pub(crate) timestamp: String,
    pub(crate) channels: Vec<StoredBackupChannel>,
    pub(crate) models: Vec<StoredBackupModel>,
    pub(crate) channel_model_prices: Vec<Value>,
    pub(crate) api_keys: Vec<StoredBackupApiKey>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StoredBackupChannel {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) channel_type: String,
    pub(crate) base_url: String,
    pub(crate) status: String,
    pub(crate) credentials: Value,
    pub(crate) supported_models: Value,
    pub(crate) default_test_model: String,
    pub(crate) settings: Value,
    pub(crate) tags: Value,
    pub(crate) ordering_weight: i64,
    pub(crate) error_message: String,
    pub(crate) remark: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StoredBackupModel {
    pub(crate) id: i64,
    pub(crate) developer: String,
    pub(crate) model_id: String,
    pub(crate) model_type: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) group: String,
    pub(crate) model_card: Value,
    pub(crate) settings: Value,
    pub(crate) status: String,
    pub(crate) remark: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StoredBackupApiKey {
    pub(crate) id: i64,
    pub(crate) project_id: i64,
    pub(crate) project_name: String,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) key_type: String,
    pub(crate) status: String,
    pub(crate) scopes: Value,
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

    pub fn update_system_channel_settings(
        &self,
        input: AdminGraphqlUpdateSystemChannelSettingsInput,
    ) -> Result<StoredSystemChannelSettings, String> {
        let mut settings = self.system_channel_settings()?;
        if let Some(probe) = input.probe {
            settings.probe = StoredChannelProbeSettings {
                enabled: probe.enabled,
                frequency: probe.frequency,
            };
        }
        self.store_json_setting(SYSTEM_KEY_CHANNEL_SETTINGS, &settings)?;
        Ok(settings)
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

        for channel in channels
            .into_iter()
            .filter(|channel| channel.status == "enabled")
        {
            let Some(provider_type) =
                provider_quota_type_for_channel(channel.channel_type.as_str())
            else {
                continue;
            };

            if !force {
                let due = quota_check_is_due(&connection, channel.id, now)
                    .map_err(|error| format!("failed to load existing quota status: {error}"))?;
                if !due {
                    continue;
                }
            }

            let details = format!(
                "Quota checks for {provider_type} channels remain unsupported in the Rust slice until provider-edge OAuth work lands."
            );
            let quota_data_json = serde_json::json!({"error": details}).to_string();
            upsert_provider_quota_status(
                &connection,
                channel.id,
                provider_type,
                "unknown",
                false,
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

    pub fn run_gc_cleanup_now(
        &self,
        vacuum_enabled: bool,
        vacuum_full: bool,
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

        if vacuum_enabled {
            let sql = if vacuum_full { "VACUUM" } else { "VACUUM" };
            connection
                .execute_batch(sql)
                .map_err(|error| format!("failed to run vacuum: {error}"))?;
            summary.vacuum_ran = true;
        }

        Ok(summary)
    }

    fn perform_backup(&self, settings: &StoredAutoBackupSettings) -> Result<(), String> {
        if settings.data_storage_id <= 0 {
            self.record_backup_status(Some("data storage not configured for backup".to_owned()))?;
            return Err("data storage not configured for backup".to_owned());
        }

        self.refresh_file_systems()?;
        let storage = self
            .cached_file_storage(settings.data_storage_id)
            .ok_or_else(|| {
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
            let entry = entry
                .map_err(|error| format!("failed to inspect backup directory entry: {error}"))?;
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

impl SqliteAdminService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        Self { foundation }
    }
}

impl AdminPort for SqliteAdminService {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError> {
        if !user_has_system_scope(&user, SCOPE_READ_REQUESTS)
            && !user_has_project_scope(&user, project_id, SCOPE_READ_REQUESTS)
        {
            return Err(AdminError::NotFound {
                message: "Request not found".to_owned(),
            });
        }

        let request = self
            .foundation
            .requests()
            .find_request_content_record(request_id)
            .map_err(|error| AdminError::Internal {
                message: format!("Failed to load request: {error}"),
            })?
            .ok_or_else(|| AdminError::NotFound {
                message: "Request not found".to_owned(),
            })?;

        if request.project_id != project_id {
            return Err(AdminError::NotFound {
                message: "Request not found".to_owned(),
            });
        }

        if !request.content_saved {
            return Err(AdminError::NotFound {
                message: "Content not found".to_owned(),
            });
        }

        let content_storage_id =
            request
                .content_storage_id
                .ok_or_else(|| AdminError::NotFound {
                    message: "Content not found".to_owned(),
                })?;
        let key = request
            .content_storage_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AdminError::NotFound {
                message: "Content not found".to_owned(),
            })?;

        let expected_prefix = format!("/{}/requests/{}/", request.project_id, request.id);
        let normalized_key = if key.starts_with('/') {
            key.to_owned()
        } else {
            format!("/{key}")
        };
        if !normalized_key.starts_with(expected_prefix.as_str()) {
            return Err(AdminError::NotFound {
                message: "Content not found".to_owned(),
            });
        }

        let data_storage = self
            .foundation
            .data_storages()
            .find_storage_by_id(content_storage_id)
            .map_err(|error| AdminError::Internal {
                message: format!("Failed to load content storage: {error}"),
            })?
            .ok_or_else(|| AdminError::NotFound {
                message: "Content storage not found".to_owned(),
            })?;

        if data_storage.storage_type == "database" {
            return Err(AdminError::BadRequest {
                message: "Content storage is not file-based".to_owned(),
            });
        }

        if data_storage.storage_type != "fs" {
            return Err(AdminError::NotFound {
                message: "Content not found".to_owned(),
            });
        }

        let settings: Value =
            serde_json::from_str(data_storage.settings_json.as_str()).unwrap_or(Value::Null);
        let base_directory = settings
            .get("directory")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AdminError::NotFound {
                message: "Content not found".to_owned(),
            })?;
        let relative = safe_relative_key_path(normalized_key.as_str()).ok_or_else(|| {
            AdminError::NotFound {
                message: "Content not found".to_owned(),
            }
        })?;

        let full_path = Path::new(base_directory).join(relative.as_path());
        let bytes = fs::read(&full_path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => AdminError::NotFound {
                message: "Content not found".to_owned(),
            },
            _ => AdminError::Internal {
                message: format!("Failed to read content: {error}"),
            },
        })?;

        Ok(AdminContentDownload {
            filename: filename_from_key(normalized_key.as_str(), request.id),
            bytes,
        })
    }
}

pub(crate) fn default_storage_policy() -> StoredStoragePolicy {
    StoredStoragePolicy {
        store_chunks: false,
        store_request_body: true,
        store_response_body: true,
        cleanup_options: vec![
            StoredCleanupOption {
                resource_type: "requests".to_owned(),
                enabled: false,
                cleanup_days: 3,
            },
            StoredCleanupOption {
                resource_type: "usage_logs".to_owned(),
                enabled: false,
                cleanup_days: 30,
            },
        ],
    }
}

pub(crate) fn default_auto_backup_settings() -> StoredAutoBackupSettings {
    StoredAutoBackupSettings {
        enabled: false,
        frequency: BackupFrequencySetting::Daily,
        data_storage_id: 0,
        include_channels: true,
        include_models: true,
        include_api_keys: false,
        include_model_prices: true,
        retention_days: 30,
        last_backup_at: None,
        last_backup_error: String::new(),
    }
}

pub(crate) fn default_system_channel_settings() -> StoredSystemChannelSettings {
    StoredSystemChannelSettings {
        probe: StoredChannelProbeSettings {
            enabled: true,
            frequency: ProbeFrequencySetting::FiveMinutes,
        },
    }
}

pub(crate) fn load_json_setting<T: DeserializeOwned>(
    settings: &SystemSettingsStore,
    key: &str,
    default: T,
) -> rusqlite::Result<T> {
    match settings.value(key)? {
        None => Ok(default),
        Some(value) => serde_json::from_str(value.as_str())
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error))),
    }
}

pub(crate) fn generate_probe_timestamps(interval_minutes: i32, now_timestamp: i64) -> Vec<i64> {
    let settings = StoredChannelProbeSettings {
        enabled: true,
        frequency: match interval_minutes {
            1 => ProbeFrequencySetting::OneMinute,
            5 => ProbeFrequencySetting::FiveMinutes,
            30 => ProbeFrequencySetting::ThirtyMinutes,
            _ => ProbeFrequencySetting::OneHour,
        },
    };
    let interval_seconds = i64::from(interval_minutes.max(1)) * 60;
    let range_seconds = i64::from(settings.query_range_minutes()) * 60;
    let end = now_timestamp - (now_timestamp % interval_seconds);
    let start = end - range_seconds;
    let mut timestamps = Vec::new();
    let mut current = start;
    while current <= end {
        timestamps.push(current);
        current += interval_seconds;
    }
    timestamps
}

pub(crate) fn provider_quota_type_for_channel(channel_type: &str) -> Option<&'static str> {
    match channel_type {
        "claudecode" => Some("claudecode"),
        "codex" => Some("codex"),
        _ => None,
    }
}

pub(crate) fn quota_check_is_due(
    connection: &Connection,
    channel_id: i64,
    now: i64,
) -> rusqlite::Result<bool> {
    let next_check_at: Option<i64> = connection
        .query_row(
            "SELECT next_check_at FROM provider_quota_statuses WHERE channel_id = ?1 LIMIT 1",
            [channel_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(next_check_at.is_none_or(|value| value <= now))
}

pub(crate) fn upsert_provider_quota_status(
    connection: &Connection,
    channel_id: i64,
    provider_type: &str,
    status: &str,
    ready: bool,
    next_reset_at: Option<i64>,
    next_check_at: i64,
    quota_data_json: &str,
) -> rusqlite::Result<()> {
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

pub(crate) fn cleanup_request_executions(
    connection: &Connection,
    cutoff: i64,
) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM request_executions WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

pub(crate) fn cleanup_requests(
    connection: &Connection,
    cutoff: i64,
    operational: &SqliteOperationalService,
) -> rusqlite::Result<i64> {
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

pub(crate) fn cleanup_threads(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM threads WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

pub(crate) fn cleanup_traces(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM traces WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

pub(crate) fn cleanup_usage_logs(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM usage_logs WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

pub(crate) fn cleanup_channel_probes(
    connection: &Connection,
    cutoff: i64,
) -> rusqlite::Result<i64> {
    connection.execute("DELETE FROM channel_probes WHERE timestamp < ?1", [cutoff])?;
    Ok(connection.changes() as i64)
}

pub(crate) fn list_backup_channels(
    connection: &Connection,
) -> rusqlite::Result<Vec<StoredBackupChannel>> {
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

pub(crate) fn list_backup_models(
    connection: &Connection,
) -> rusqlite::Result<Vec<StoredBackupModel>> {
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

pub(crate) fn list_backup_api_keys(
    connection: &Connection,
) -> rusqlite::Result<Vec<StoredBackupApiKey>> {
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

pub(crate) fn parse_graphql_resource_id(value: &str, expected_type: &str) -> Result<i64, String> {
    let trimmed = value.trim();
    let prefix = format!("gid://axonhub/{expected_type}/");
    trimmed
        .strip_prefix(prefix.as_str())
        .ok_or_else(|| format!("invalid {expected_type} id"))?
        .parse::<i64>()
        .map_err(|_| format!("invalid {expected_type} id"))
}

pub(crate) fn safe_relative_key_path(key: &str) -> Option<PathBuf> {
    let trimmed = key.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }

    Some(path.to_path_buf())
}

pub(crate) fn filename_from_key(key: &str, request_id: i64) -> String {
    Path::new(key)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("request-{request_id}-content"))
}
