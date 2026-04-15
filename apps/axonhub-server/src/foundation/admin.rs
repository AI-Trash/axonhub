use async_graphql::Enum;
use axonhub_http::{AdminContentDownload, AdminError, AdminPort, AuthUserContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) mod oauth;

use super::{
    authz::{require_user_project_scope, SCOPE_READ_REQUESTS},
    ports::AdminRepository,
    repositories::admin::{AdminStorageRepository, SeaOrmAdminStorageRepository},
    seaorm::SeaOrmConnectionFactory,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct StoredAutoDisableChannelStatus {
    pub(crate) status: i32,
    pub(crate) times: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct StoredAutoDisableChannel {
    pub(crate) enabled: bool,
    pub(crate) statuses: Vec<StoredAutoDisableChannelStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct StoredRetryPolicy {
    pub(crate) enabled: bool,
    pub(crate) max_channel_retries: i32,
    pub(crate) max_single_channel_retries: i32,
    pub(crate) retry_delay_ms: i32,
    pub(crate) load_balancer_strategy: String,
    #[serde(default)]
    pub(crate) auto_disable_channel: StoredAutoDisableChannel,
}

impl Default for StoredRetryPolicy {
    fn default() -> Self {
        default_retry_policy()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum BackupFrequencySetting {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Enum)]
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

impl<'de> Deserialize<'de> for ProbeFrequencySetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "1m" | "ONE_MINUTE" | "OneMinute" => Self::OneMinute,
            "5m" | "FIVE_MINUTES" | "FiveMinutes" => Self::FiveMinutes,
            "30m" | "THIRTY_MINUTES" | "ThirtyMinutes" => Self::ThirtyMinutes,
            "1h" | "ONE_HOUR" | "OneHour" => Self::OneHour,
            _ => Self::OneHour,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum AutoSyncFrequencySetting {
    #[graphql(name = "ONE_HOUR")]
    OneHour,
    #[graphql(name = "SIX_HOURS")]
    SixHours,
    #[graphql(name = "ONE_DAY")]
    OneDay,
}

impl<'de> Deserialize<'de> for AutoSyncFrequencySetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "1h" | "ONE_HOUR" | "OneHour" => Self::OneHour,
            "6h" | "SIX_HOURS" | "SixHours" => Self::SixHours,
            "1d" | "ONE_DAY" | "OneDay" => Self::OneDay,
            "1m" | "5m" | "30m" => Self::OneHour,
            _ => Self::OneHour,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AutoSyncFrequencySetting, ProbeFrequencySetting, StoredSystemChannelSettings};

    #[test]
    fn probe_frequency_accepts_legacy_duration_and_variant_spellings() {
        let one_minute: ProbeFrequencySetting = serde_json::from_str("\"1m\"").unwrap();
        assert_eq!(one_minute, ProbeFrequencySetting::OneMinute);

        let five_minutes: ProbeFrequencySetting = serde_json::from_str("\"5m\"").unwrap();
        assert_eq!(five_minutes, ProbeFrequencySetting::FiveMinutes);

        let thirty_minutes: ProbeFrequencySetting = serde_json::from_str("\"30m\"").unwrap();
        assert_eq!(thirty_minutes, ProbeFrequencySetting::ThirtyMinutes);

        let one_hour: ProbeFrequencySetting = serde_json::from_str("\"OneHour\"").unwrap();
        assert_eq!(one_hour, ProbeFrequencySetting::OneHour);
    }

    #[test]
    fn auto_sync_frequency_accepts_legacy_variant_spellings() {
        let one_hour: AutoSyncFrequencySetting = serde_json::from_str("\"OneHour\"").unwrap();
        assert_eq!(one_hour, AutoSyncFrequencySetting::OneHour);

        let six_hours: AutoSyncFrequencySetting = serde_json::from_str("\"SixHours\"").unwrap();
        assert_eq!(six_hours, AutoSyncFrequencySetting::SixHours);

        let one_day: AutoSyncFrequencySetting = serde_json::from_str("\"OneDay\"").unwrap();
        assert_eq!(one_day, AutoSyncFrequencySetting::OneDay);
    }

    #[test]
    fn stored_system_channel_settings_accepts_legacy_auto_sync_variants() {
        let legacy_json = r#"{
            "probe": {"enabled": true, "frequency": "5m"},
            "auto_sync": {"frequency": "SixHours"},
            "query_all_channel_models": true
        }"#;

        let settings: StoredSystemChannelSettings = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(
            settings.auto_sync.frequency,
            AutoSyncFrequencySetting::SixHours
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredChannelProbeSettings {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredChannelModelAutoSyncSettings {
    pub(crate) frequency: AutoSyncFrequencySetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct StoredSystemChannelSettings {
    pub(crate) probe: StoredChannelProbeSettings,
    pub(crate) auto_sync: StoredChannelModelAutoSyncSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct StoredSystemModelSettings {
    pub(crate) fallback_to_channels_on_model_not_found: bool,
    pub(crate) query_all_channel_models: bool,
}

impl Default for StoredSystemModelSettings {
    fn default() -> Self {
        default_system_model_settings()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct StoredProxyPreset {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) username: String,
    pub(crate) password: String,
}

impl Default for StoredSystemChannelSettings {
    fn default() -> Self {
        default_system_channel_settings()
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoredVideoStorageSettings {
    pub(crate) enabled: bool,
    pub(crate) data_storage_id: i64,
    pub(crate) scan_interval_minutes: i32,
    pub(crate) scan_limit: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct StoredSystemGeneralSettings {
    #[serde(alias = "currencyCode")]
    pub(crate) currency_code: String,
    pub(crate) timezone: String,
}

impl Default for StoredSystemGeneralSettings {
    fn default() -> Self {
        default_system_general_settings()
    }
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
    pub(crate) fn interval_minutes(&self) -> i32 {
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

#[derive(Debug, Clone)]
pub(crate) struct StoredCircuitBreakerStatus {
    pub(crate) channel_id: i64,
    pub(crate) model_id: String,
    pub(crate) state: String,
    pub(crate) consecutive_failures: i32,
    pub(crate) next_probe_at_seconds: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredBackupPayload {
    pub(crate) version: String,
    pub(crate) timestamp: String,
    pub(crate) channels: Vec<StoredBackupChannel>,
    pub(crate) models: Vec<StoredBackupModel>,
    pub(crate) channel_model_prices: Vec<Value>,
    pub(crate) api_keys: Vec<StoredBackupApiKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub struct SeaOrmAdminService {
    storage: SeaOrmAdminStorageRepository,
}

impl SeaOrmAdminService {
    pub fn new(db: SeaOrmConnectionFactory) -> Self {
        Self {
            storage: SeaOrmAdminStorageRepository::new(db),
        }
    }
}

impl AdminPort for SeaOrmAdminService {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError> {
        if let Err(error) = require_user_project_scope(&user, project_id, SCOPE_READ_REQUESTS) {
            return Err(AdminError::Forbidden {
                message: error.message().to_owned(),
            });
        }

        let request = self
            .storage
            .query_request_content_record(request_id)?
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
            .storage
            .query_data_storage(content_storage_id)?
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

impl AdminRepository for SeaOrmAdminService {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError> {
        <Self as AdminPort>::download_request_content(self, project_id, request_id, user)
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

pub(crate) fn default_retry_policy() -> StoredRetryPolicy {
    StoredRetryPolicy {
        enabled: true,
        max_channel_retries: 3,
        max_single_channel_retries: 2,
        retry_delay_ms: 1000,
        load_balancer_strategy: "adaptive".to_owned(),
        auto_disable_channel: StoredAutoDisableChannel::default(),
    }
}

pub(crate) fn normalize_retry_policy_load_balancer_strategy(strategy: &str) -> String {
    let trimmed = strategy.trim();
    if trimmed.is_empty() {
        return default_retry_policy().load_balancer_strategy;
    }

    if trimmed.eq_ignore_ascii_case("weighted") {
        return "failover".to_owned();
    }

    trimmed.to_owned()
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

pub(crate) fn default_video_storage_settings() -> StoredVideoStorageSettings {
    StoredVideoStorageSettings {
        enabled: false,
        data_storage_id: 0,
        scan_interval_minutes: 5,
        scan_limit: 100,
    }
}

pub(crate) fn default_system_channel_settings() -> StoredSystemChannelSettings {
    StoredSystemChannelSettings {
        probe: StoredChannelProbeSettings {
            enabled: true,
            frequency: ProbeFrequencySetting::FiveMinutes,
        },
        auto_sync: StoredChannelModelAutoSyncSettings {
            frequency: AutoSyncFrequencySetting::OneHour,
        },
    }
}

pub(crate) fn default_system_model_settings() -> StoredSystemModelSettings {
    StoredSystemModelSettings {
        fallback_to_channels_on_model_not_found: true,
        query_all_channel_models: true,
    }
}

fn default_fallback_to_channels_on_model_not_found() -> bool {
    true
}

pub(crate) fn default_system_general_settings() -> StoredSystemGeneralSettings {
    StoredSystemGeneralSettings {
        currency_code: "USD".to_owned(),
        timezone: "UTC".to_owned(),
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

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use axonhub_http::{AdminContentDownload, AdminError, AdminPort, AuthUserContext};
    use serde_json::Value;

    use super::{
        super::{
            authz::{require_user_project_scope, SCOPE_READ_REQUESTS},
            ports::AdminRepository,
            system::sqlite_test_support::SqliteFoundation,
        },
        filename_from_key, safe_relative_key_path,
    };

    pub struct SqliteAdminService {
        pub(crate) foundation: Arc<SqliteFoundation>,
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
            if let Err(error) = require_user_project_scope(&user, project_id, SCOPE_READ_REQUESTS) {
                return Err(AdminError::Forbidden {
                    message: error.message().to_owned(),
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

    impl AdminRepository for SqliteAdminService {
        fn download_request_content(
            &self,
            project_id: i64,
            request_id: i64,
            user: AuthUserContext,
        ) -> Result<AdminContentDownload, AdminError> {
            <Self as AdminPort>::download_request_content(self, project_id, request_id, user)
        }
    }
}
