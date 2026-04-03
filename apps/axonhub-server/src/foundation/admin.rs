use async_graphql::Enum;
use axonhub_http::{AdminContentDownload, AdminError, AdminPort, AuthUserContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::{
    authz::{require_user_project_scope, SCOPE_READ_REQUESTS},
    ports::AdminRepository,
    repositories::admin::{AdminStorageRepository, SeaOrmAdminStorageRepository},
    seaorm::SeaOrmConnectionFactory,
};

pub(crate) use super::admin_sqlite_support::*;

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
#[serde(default)]
pub(crate) struct StoredSystemChannelSettings {
    pub(crate) probe: StoredChannelProbeSettings,
    pub(crate) query_all_channel_models: bool,
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

#[derive(Debug, Clone)]
struct DataStorageRecord {
    storage_type: String,
    settings_json: String,
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
        query_all_channel_models: true,
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
