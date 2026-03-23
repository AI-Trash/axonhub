use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_graphql::{
    Context, EmptySubscription, Enum, InputObject, Object, Request as AsyncGraphqlRequest,
    Schema, SimpleObject, Variables,
};
use axonhub_http::{
    AdminAuthError, AdminContentDownload, AdminError, AdminGraphqlPort, AdminPort,
    AnthropicModel, AnthropicModelListResponse, ApiKeyAuthError, ApiKeyType,
    AuthApiKeyContext, AuthContextPort, AuthUserContext, CompatibilityRoute,
    ContextResolveError, GeminiModel, GeminiModelListResponse, GlobalId,
    ExchangeCallbackOAuthRequest, ExchangeOAuthResponse, GraphqlExecutionResult,
    GraphqlRequestPayload, InitializeSystemRequest, ModelCapabilities, ModelListResponse,
    ModelPricing, OpenAiModel, OpenAiV1Error, OpenAiV1ExecutionRequest,
    OpenAiV1ExecutionResponse, OpenAiV1Port, OpenAiV1Route, OpenApiGraphqlPort,
    PollCopilotOAuthRequest, PollCopilotOAuthResponse, ProjectContext,
    ProviderEdgeAdminError, ProviderEdgeAdminPort, RoleInfo, SignInError, SignInRequest,
    SignInSuccess, StartAntigravityOAuthRequest, StartCopilotOAuthRequest,
    StartCopilotOAuthResponse, StartPkceOAuthRequest, StartPkceOAuthResponse,
    SystemBootstrapPort, SystemInitializeError, SystemQueryError, ThreadContext, TraceContext,
    UserProjectInfo,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use hex::encode as hex_encode;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Duration;

pub(crate) const SYSTEM_KEY_INITIALIZED: &str = "system_initialized";
pub(crate) const SYSTEM_KEY_VERSION: &str = "system_version";
pub(crate) const SYSTEM_KEY_SECRET_KEY: &str = "system_jwt_secret_key";
pub(crate) const SYSTEM_KEY_BRAND_NAME: &str = "system_brand_name";
pub(crate) const SYSTEM_KEY_DEFAULT_DATA_STORAGE: &str = "default_data_storage_id";
pub(crate) const SYSTEM_KEY_STORAGE_POLICY: &str = "storage_policy";
pub(crate) const SYSTEM_KEY_CHANNEL_SETTINGS: &str = "system_channel_settings";
pub(crate) const SYSTEM_KEY_AUTO_BACKUP_SETTINGS: &str = "system_auto_backup_settings";
pub(crate) const PRIMARY_DATA_STORAGE_NAME: &str = "Primary";
const PRIMARY_DATA_STORAGE_DESCRIPTION: &str = "Primary database storage";
const PRIMARY_DATA_STORAGE_SETTINGS_JSON: &str = "{}";
const DEFAULT_PROJECT_NAME: &str = "Default Project";
const DEFAULT_PROJECT_DESCRIPTION: &str = "Default project for Rust migration slice";
const DEFAULT_USER_API_KEY_NAME: &str = "Default User Key";
const DEFAULT_USER_API_KEY_VALUE: &str = "api-key-123";
const DEFAULT_SERVICE_API_KEY_NAME: &str = "Default Service Account Key";
const DEFAULT_SERVICE_API_KEY_VALUE: &str = "service-key-123";
const NO_AUTH_API_KEY_NAME: &str = "No Auth System Key";
pub(crate) const NO_AUTH_API_KEY_VALUE: &str = "AXONHUB_API_KEY_NO_AUTH";
const PROVIDER_EDGE_PKCE_SESSION_TTL_SECONDS: i64 = 10 * 60;
const PROVIDER_EDGE_COPILOT_DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const PROVIDER_EDGE_COPILOT_COMPLETE_MESSAGE: &str =
    "Authorization complete. Access token received.";
const PROVIDER_EDGE_COPILOT_PENDING_MESSAGE: &str =
    "Authorization pending. User has not yet authorized the device.";
const PROVIDER_EDGE_COPILOT_SLOW_DOWN_MESSAGE: &str =
    "Polling too fast. Please slow down.";
const SYSTEM_ROLE_PROJECT_ID: i64 = 0;
const ROLE_LEVEL_SYSTEM: &str = "system";
const ROLE_LEVEL_PROJECT: &str = "project";
const SCOPE_READ_SETTINGS: &str = "read_settings";
const SCOPE_READ_CHANNELS: &str = "read_channels";
const SCOPE_READ_REQUESTS: &str = "read_requests";
const SCOPE_WRITE_SETTINGS: &str = "write_settings";
const SCOPE_WRITE_API_KEYS: &str = "write_api_keys";
const BACKUP_VERSION: &str = "1.1";
const AUTO_BACKUP_PREFIX: &str = "axonhub-backup-";
const AUTO_BACKUP_SUFFIX: &str = ".json";

const SYSTEMS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS systems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
";

const DATA_STORAGES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS data_storages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    \"primary\" INTEGER NOT NULL DEFAULT 0,
    type TEXT NOT NULL,
    settings TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);
";

const USERS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    email TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'activated',
    prefer_language TEXT NOT NULL DEFAULT 'en',
    password TEXT NOT NULL,
    first_name TEXT NOT NULL DEFAULT '',
    last_name TEXT NOT NULL DEFAULT '',
    avatar TEXT NOT NULL DEFAULT '',
    is_owner INTEGER NOT NULL DEFAULT 0,
    scopes TEXT NOT NULL DEFAULT '[]'
);
";

const PROJECTS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active'
);
";

const USER_PROJECTS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS user_projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id INTEGER NOT NULL,
    project_id INTEGER NOT NULL,
    is_owner INTEGER NOT NULL DEFAULT 0,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE(user_id, project_id)
);
";

const ROLES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'system',
    project_id INTEGER NOT NULL DEFAULT 0,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE(project_id, name)
);
";

const USER_ROLES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS user_roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id INTEGER NOT NULL,
    role_id INTEGER NOT NULL,
    UNIQUE(user_id, role_id)
);
";

const API_KEYS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    user_id INTEGER NOT NULL,
    project_id INTEGER NOT NULL,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'enabled',
    scopes TEXT NOT NULL DEFAULT '[]',
    profiles TEXT NOT NULL DEFAULT '{}'
);
";

const THREADS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS threads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    thread_id TEXT NOT NULL UNIQUE
);
";

const TRACES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS traces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    trace_id TEXT NOT NULL UNIQUE,
    thread_id INTEGER
);
";

const CHANNELS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS channels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    type TEXT NOT NULL,
    base_url TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'enabled',
    credentials TEXT NOT NULL DEFAULT '{}',
    supported_models TEXT NOT NULL DEFAULT '[]',
    auto_sync_supported_models INTEGER NOT NULL DEFAULT 0,
    default_test_model TEXT NOT NULL DEFAULT '',
    settings TEXT NOT NULL DEFAULT '{}',
    tags TEXT NOT NULL DEFAULT '[]',
    ordering_weight INTEGER NOT NULL DEFAULT 0,
    error_message TEXT NOT NULL DEFAULT '',
    remark TEXT NOT NULL DEFAULT ''
);
";

const MODELS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS models (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    developer TEXT NOT NULL,
    model_id TEXT NOT NULL,
    type TEXT NOT NULL,
    name TEXT NOT NULL,
    icon TEXT NOT NULL DEFAULT '',
    \"group\" TEXT NOT NULL DEFAULT '',
    model_card TEXT NOT NULL DEFAULT '{}',
    settings TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'enabled',
    remark TEXT NOT NULL DEFAULT '',
    UNIQUE(developer, model_id, type)
);
";

const REQUESTS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    api_key_id INTEGER,
    project_id INTEGER NOT NULL,
    trace_id INTEGER,
    data_storage_id INTEGER,
    source TEXT NOT NULL DEFAULT 'api',
    model_id TEXT NOT NULL,
    format TEXT NOT NULL DEFAULT 'openai/chat_completions',
    request_headers TEXT NOT NULL DEFAULT '{}',
    request_body TEXT NOT NULL DEFAULT '{}',
    response_body TEXT,
    response_chunks TEXT,
    channel_id INTEGER,
    external_id TEXT,
    status TEXT NOT NULL,
    stream INTEGER NOT NULL DEFAULT 0,
    client_ip TEXT NOT NULL DEFAULT '',
    metrics_latency_ms INTEGER,
    metrics_first_token_latency_ms INTEGER,
    content_saved INTEGER NOT NULL DEFAULT 0,
    content_storage_id INTEGER,
    content_storage_key TEXT,
    content_saved_at TEXT
);
";

const REQUEST_EXECUTIONS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS request_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    request_id INTEGER NOT NULL,
    channel_id INTEGER,
    data_storage_id INTEGER,
    external_id TEXT,
    model_id TEXT NOT NULL,
    format TEXT NOT NULL DEFAULT 'openai/chat_completions',
    request_body TEXT NOT NULL DEFAULT '{}',
    response_body TEXT,
    response_chunks TEXT,
    error_message TEXT NOT NULL DEFAULT '',
    response_status_code INTEGER,
    status TEXT NOT NULL,
    stream INTEGER NOT NULL DEFAULT 0,
    metrics_latency_ms INTEGER,
    metrics_first_token_latency_ms INTEGER,
    request_headers TEXT NOT NULL DEFAULT '{}'
);
";

const USAGE_LOGS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS usage_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    request_id INTEGER NOT NULL,
    api_key_id INTEGER,
    project_id INTEGER NOT NULL,
    channel_id INTEGER,
    model_id TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_audio_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_cached_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_write_cached_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_write_cached_tokens_5m INTEGER NOT NULL DEFAULT 0,
    prompt_write_cached_tokens_1h INTEGER NOT NULL DEFAULT 0,
    completion_audio_tokens INTEGER NOT NULL DEFAULT 0,
    completion_reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    completion_accepted_prediction_tokens INTEGER NOT NULL DEFAULT 0,
    completion_rejected_prediction_tokens INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'api',
    format TEXT NOT NULL DEFAULT 'openai/chat_completions',
    total_cost REAL,
    cost_items TEXT NOT NULL DEFAULT '[]',
    cost_price_reference_id TEXT NOT NULL DEFAULT ''
);
";

const CHANNEL_PROBES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS channel_probes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    channel_id INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    total_request_count INTEGER NOT NULL DEFAULT 0,
    success_request_count INTEGER NOT NULL DEFAULT 0,
    avg_tokens_per_second REAL,
    avg_time_to_first_token_ms REAL,
    UNIQUE(channel_id, timestamp)
);
";

const PROVIDER_QUOTA_STATUSES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS provider_quota_statuses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    channel_id INTEGER NOT NULL UNIQUE,
    provider_type TEXT NOT NULL,
    status TEXT NOT NULL,
    quota_data TEXT NOT NULL DEFAULT '{}',
    next_reset_at INTEGER,
    ready INTEGER NOT NULL DEFAULT 0,
    next_check_at INTEGER NOT NULL DEFAULT 0
);
";

#[derive(Debug, Clone)]
pub struct SqliteFoundation {
    connection_factory: SqliteConnectionFactory,
}

impl SqliteFoundation {
    pub fn new(dsn: impl Into<String>) -> Self {
        Self {
            connection_factory: SqliteConnectionFactory::new(dsn.into()),
        }
    }

    pub fn open_connection(&self, create_if_missing: bool) -> rusqlite::Result<Connection> {
        self.connection_factory.open(create_if_missing)
    }

    pub fn system_settings(&self) -> SystemSettingsStore {
        SystemSettingsStore::new(self.connection_factory.clone())
    }

    pub fn data_storages(&self) -> DataStorageStore {
        DataStorageStore::new(self.connection_factory.clone())
    }

    pub fn identities(&self) -> IdentityStore {
        IdentityStore::new(self.connection_factory.clone())
    }

    pub fn trace_contexts(&self) -> TraceContextStore {
        TraceContextStore::new(self.connection_factory.clone())
    }

    pub fn channel_models(&self) -> ChannelModelStore {
        ChannelModelStore::new(self.connection_factory.clone())
    }

    pub fn requests(&self) -> RequestStore {
        RequestStore::new(self.connection_factory.clone())
    }

    pub fn usage_costs(&self) -> UsageCostStore {
        UsageCostStore::new(self.connection_factory.clone())
    }

    pub fn operational(&self) -> OperationalStore {
        OperationalStore::new(self.connection_factory.clone())
    }
}

#[derive(Debug, Clone)]
struct SqliteConnectionFactory {
    dsn: Arc<String>,
}

impl SqliteConnectionFactory {
    fn new(dsn: String) -> Self {
        Self { dsn: Arc::new(dsn) }
    }

    fn open(&self, create_if_missing: bool) -> rusqlite::Result<Connection> {
        open_sqlite_connection(self.dsn.as_str(), create_if_missing)
    }
}

#[derive(Debug, Clone)]
pub struct SystemSettingsStore {
    connection_factory: SqliteConnectionFactory,
}

impl SystemSettingsStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_systems_table(&connection)
    }

    pub fn is_initialized(&self) -> rusqlite::Result<bool> {
        let connection = self.connection_factory.open(true)?;
        ensure_systems_table(&connection)?;
        query_is_initialized(&connection)
    }

    pub fn value(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let connection = self.connection_factory.open(true)?;
        ensure_systems_table(&connection)?;
        query_system_value(&connection, key)
    }

    pub fn default_data_storage_id(&self) -> rusqlite::Result<Option<i64>> {
        self.value(SYSTEM_KEY_DEFAULT_DATA_STORAGE)
            .map(|value| value.and_then(|current| current.parse::<i64>().ok()))
    }

    pub fn set_value(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_systems_table(&connection)?;
        upsert_system_value_on_connection(&connection, key, value)
    }
}

#[derive(Debug, Clone)]
pub struct OperationalStore {
    connection_factory: SqliteConnectionFactory,
}

impl OperationalStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_operational_tables(&connection)
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
            let settings = serde_json::from_str::<Value>(settings_json.as_str()).unwrap_or(Value::Null);
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

    pub fn list_channel_probe_data(&self, channel_ids: &[i64]) -> rusqlite::Result<Vec<StoredChannelProbeData>> {
        let connection = self.connection_factory.open(true)?;
        ensure_operational_tables(&connection)?;
        let settings = load_json_setting(
            &SystemSettingsStore::new(self.connection_factory.clone()),
            SYSTEM_KEY_CHANNEL_SETTINGS,
            default_system_channel_settings(),
        )?;
        let timestamps = generate_probe_timestamps(settings.probe.interval_minutes(), current_unix_timestamp());
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
            let rows = statement.query_map(params![channel_id, start_timestamp, end_timestamp], |row| {
                Ok(StoredChannelProbePoint {
                    timestamp: row.get(0)?,
                    total_request_count: row.get(1)?,
                    success_request_count: row.get(2)?,
                    avg_tokens_per_second: row.get(3)?,
                    avg_time_to_first_token_ms: row.get(4)?,
                })
            })?;
            let existing = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut by_timestamp = HashMap::new();
            for point in existing {
                by_timestamp.insert(point.timestamp, point);
            }

            let mut points = Vec::with_capacity(timestamps.len());
            for timestamp in &timestamps {
                points.push(by_timestamp.remove(timestamp).unwrap_or(StoredChannelProbePoint {
                    timestamp: *timestamp,
                    total_request_count: 0,
                    success_request_count: 0,
                    avg_tokens_per_second: None,
                    avg_time_to_first_token_ms: None,
                }));
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

#[derive(Debug, Clone)]
pub struct DataStorageStore {
    connection_factory: SqliteConnectionFactory,
}

impl DataStorageStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        connection.execute_batch(DATA_STORAGES_TABLE_SQL)
    }

    pub fn find_primary_active_storage(&self) -> rusqlite::Result<Option<StoredDataStorage>> {
        let connection = self.connection_factory.open(true)?;
        self.query_primary_active_storage(&connection)
    }

    pub fn find_storage_by_id(&self, storage_id: i64) -> rusqlite::Result<Option<StoredDataStorage>> {
        let connection = self.connection_factory.open(true)?;
        connection
            .query_row(
                "SELECT id, name, description, type, status, settings FROM data_storages WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
                [storage_id],
                |row| {
                    Ok(StoredDataStorage {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        storage_type: row.get(3)?,
                        status: row.get(4)?,
                        settings_json: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    fn query_primary_active_storage(
        &self,
        connection: &Connection,
    ) -> rusqlite::Result<Option<StoredDataStorage>> {
        connection
            .query_row(
                "SELECT id, name, description, type, status, settings FROM data_storages WHERE \"primary\" = 1 AND deleted_at = 0 LIMIT 1",
                [],
                |row| {
                    Ok(StoredDataStorage {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        storage_type: row.get(3)?,
                        status: row.get(4)?,
                        settings_json: row.get(5)?,
                    })
                },
            )
            .optional()
    }
}

#[derive(Debug, Clone)]
pub struct IdentityStore {
    connection_factory: SqliteConnectionFactory,
}

impl IdentityStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_identity_tables(&connection)
    }

    pub fn find_user_by_email(&self, email: &str) -> Result<StoredUser, QueryUserError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| QueryUserError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| QueryUserError::Internal)?;
        query_user_by_email(&connection, email)
    }

    pub fn find_user_by_id(&self, user_id: i64) -> Result<StoredUser, QueryUserError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| QueryUserError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| QueryUserError::Internal)?;
        query_user_by_id(&connection, user_id)
    }

    pub fn find_default_project_for_user(&self, user_id: i64) -> rusqlite::Result<StoredProject> {
        let connection = self.connection_factory.open(true)?;
        ensure_identity_tables(&connection)?;
        query_default_project_for_user(&connection, user_id)
    }

    pub fn find_project_by_id(&self, project_id: i64) -> Result<StoredProject, ApiKeyAuthError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| ApiKeyAuthError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| ApiKeyAuthError::Internal)?;
        query_project(&connection, project_id)
    }

    pub fn find_api_key_by_value(&self, key: &str) -> Result<StoredApiKey, ApiKeyAuthError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| ApiKeyAuthError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| ApiKeyAuthError::Internal)?;
        query_api_key(&connection, key)
    }

    pub fn build_user_context(
        &self,
        user: StoredUser,
    ) -> rusqlite::Result<AuthUserContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_identity_tables(&connection)?;
        build_user_context(&connection, user)
    }
}

#[derive(Debug, Clone)]
pub struct TraceContextStore {
    connection_factory: SqliteConnectionFactory,
}

impl TraceContextStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)
    }

    pub fn get_or_create_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> rusqlite::Result<ThreadContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        get_or_create_thread(&connection, project_id, thread_id)
    }

    pub fn get_or_create_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> rusqlite::Result<TraceContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        get_or_create_trace(&connection, project_id, trace_id, thread_db_id)
    }

    pub fn list_traces_by_project(&self, project_id: i64) -> rusqlite::Result<Vec<TraceContext>> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, trace_id, project_id, thread_id
             FROM traces
             WHERE project_id = ?1
             ORDER BY id DESC",
        )?;
        let rows = statement
            .query_map([project_id], |row| {
                Ok(TraceContext {
                    id: row.get(0)?,
                    trace_id: row.get(1)?,
                    project_id: row.get(2)?,
                    thread_id: row.get(3)?,
                })
            })?;
        rows.collect()
    }
}

#[derive(Debug, Clone)]
pub struct ChannelModelStore {
    connection_factory: SqliteConnectionFactory,
}

impl ChannelModelStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_channel_model_tables(&connection)
    }

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
        let rows = statement
            .query_map([], |row| {
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
            .map(|trace_id| query_preferred_trace_channel_id(&connection, trace_id, request_model_id))
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
    connection_factory: SqliteConnectionFactory,
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
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

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
        let rows = statement
            .query_map([project_id], |row| {
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
    connection_factory: SqliteConnectionFactory,
}

impl UsageCostStore {
    fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

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

pub struct SqliteBootstrapService {
    foundation: Arc<SqliteFoundation>,
    version: String,
}

impl SqliteBootstrapService {
    pub fn new(foundation: Arc<SqliteFoundation>, version: String) -> Self {
        Self {
            foundation,
            version,
        }
    }
}

impl SystemBootstrapPort for SqliteBootstrapService {
    fn is_initialized(&self) -> Result<bool, SystemQueryError> {
        self.foundation
            .system_settings()
            .is_initialized()
            .map_err(|_| SystemQueryError::QueryFailed)
    }

    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
        let mut connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_all_foundation_tables(&connection)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

        let transaction = connection
            .transaction()
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

        if query_is_initialized(&transaction)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?
        {
            return Err(SystemInitializeError::AlreadyInitialized);
        }

        let primary_data_storage_id = ensure_primary_data_storage(&transaction)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        let owner_user_id = ensure_owner_user(&transaction, request)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        let default_project_id = ensure_default_project(&transaction)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_default_project_roles(&transaction, default_project_id)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_owner_project_membership(&transaction, owner_user_id, default_project_id)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_default_api_keys(&transaction, owner_user_id, default_project_id)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

        let secret = generate_secret_key(&transaction)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        upsert_system_value(&transaction, SYSTEM_KEY_SECRET_KEY, &secret)?;
        upsert_system_value(
            &transaction,
            SYSTEM_KEY_BRAND_NAME,
            request.brand_name.trim(),
        )?;
        upsert_system_value(&transaction, SYSTEM_KEY_VERSION, &self.version)?;
        upsert_system_value(
            &transaction,
            SYSTEM_KEY_DEFAULT_DATA_STORAGE,
            &primary_data_storage_id.to_string(),
        )?;
        upsert_system_value(&transaction, SYSTEM_KEY_INITIALIZED, "true")?;

        transaction
            .commit()
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
    }
}

pub struct SqliteAuthContextService {
    foundation: Arc<SqliteFoundation>,
    allow_no_auth: bool,
}

pub struct SqliteAdminService {
    foundation: Arc<SqliteFoundation>,
}

pub struct SqliteOpenAiV1Service {
    foundation: Arc<SqliteFoundation>,
}

pub struct SqliteProviderEdgeAdminService {
    config: ProviderEdgeAdminConfig,
    sessions: Arc<Mutex<HashMap<String, ProviderEdgeSession>>>,
    http_client: ProviderEdgeHttpClient,
}

enum ProviderEdgeHttpClient {
    Default,
    Injected(reqwest::blocking::Client),
}

#[derive(Clone)]
pub struct SqliteOperationalService {
    foundation: Arc<SqliteFoundation>,
    file_storage_cache: Arc<RwLock<HashMap<i64, CachedFileStorage>>>,
    last_probe_timestamp: Arc<Mutex<Option<i64>>>,
}

pub struct SqliteAdminGraphqlService {
    schema: Arc<AdminGraphqlSchema>,
}

pub struct SqliteOpenApiGraphqlService {
    schema: Arc<OpenApiGraphqlSchema>,
}

#[derive(Debug, Clone)]
pub struct ProviderEdgeAdminConfig {
    codex_authorize_url: String,
    codex_token_url: String,
    codex_client_id: String,
    codex_redirect_uri: String,
    codex_scopes: String,
    codex_user_agent: String,
    claudecode_authorize_url: String,
    claudecode_token_url: String,
    claudecode_client_id: String,
    claudecode_redirect_uri: String,
    claudecode_scopes: String,
    claudecode_user_agent: String,
    antigravity_authorize_url: String,
    antigravity_token_url: String,
    antigravity_client_id: String,
    antigravity_client_secret: String,
    antigravity_redirect_uri: String,
    antigravity_scopes: String,
    antigravity_load_endpoints: Vec<String>,
    antigravity_user_agent: String,
    antigravity_client_metadata: String,
    copilot_device_code_url: String,
    copilot_access_token_url: String,
    copilot_client_id: String,
    copilot_scope: String,
}

#[derive(Debug, Clone)]
enum ProviderEdgeSession {
    Pkce {
        provider: PkceProvider,
        code_verifier: String,
        project_id: Option<String>,
        created_at: i64,
    },
    CopilotDevice {
        device_code: String,
        expires_in: i64,
        interval: i64,
        user_code: String,
        verification_uri: String,
        created_at: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PkceProvider {
    Codex,
    ClaudeCode,
    Antigravity,
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CopilotDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i64,
    interval: i64,
}

type AdminGraphqlSchema = Schema<AdminGraphqlQueryRoot, AdminGraphqlMutationRoot, EmptySubscription>;
type OpenApiGraphqlSchema = Schema<OpenApiGraphqlQueryRoot, OpenApiGraphqlMutationRoot, EmptySubscription>;

#[derive(Clone)]
struct AdminGraphqlRequestContext {
    project_id: Option<i64>,
    user: AuthUserContext,
}

#[derive(Clone)]
struct OpenApiGraphqlRequestContext {
    owner_api_key: AuthApiKeyContext,
}

#[derive(Clone)]
struct AdminGraphqlQueryRoot {
    foundation: Arc<SqliteFoundation>,
    operational: Arc<SqliteOperationalService>,
}

#[derive(Clone)]
struct AdminGraphqlMutationRoot {
    operational: Arc<SqliteOperationalService>,
}

#[derive(Clone)]
struct OpenApiGraphqlQueryRoot;

#[derive(Clone)]
struct OpenApiGraphqlMutationRoot {
    foundation: Arc<SqliteFoundation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredCleanupOption {
    resource_type: String,
    enabled: bool,
    cleanup_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredStoragePolicy {
    store_chunks: bool,
    store_request_body: bool,
    store_response_body: bool,
    cleanup_options: Vec<StoredCleanupOption>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
enum BackupFrequencySetting {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
enum ProbeFrequencySetting {
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
struct StoredChannelProbeSettings {
    enabled: bool,
    frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSystemChannelSettings {
    probe: StoredChannelProbeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredAutoBackupSettings {
    enabled: bool,
    frequency: BackupFrequencySetting,
    data_storage_id: i64,
    include_channels: bool,
    include_models: bool,
    include_api_keys: bool,
    include_model_prices: bool,
    retention_days: i32,
    last_backup_at: Option<i64>,
    last_backup_error: String,
}

impl BackupFrequencySetting {
    fn minimum_interval_seconds(self) -> i64 {
        match self {
            Self::Daily => 86_400,
            Self::Weekly => 7 * 86_400,
            Self::Monthly => 28 * 86_400,
        }
    }

    fn is_due(self, last_backup_at: Option<i64>, now: i64) -> bool {
        match last_backup_at {
            None => true,
            Some(last_backup_at) => now.saturating_sub(last_backup_at) >= self.minimum_interval_seconds(),
        }
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
    fn interval_minutes(&self) -> i32 {
        self.frequency.interval_minutes()
    }

    fn query_range_minutes(&self) -> i32 {
        self.frequency.query_range_minutes()
    }
}

#[derive(Debug, Clone)]
struct CachedFileStorage {
    root: PathBuf,
}

#[derive(Debug, Clone)]
struct StoredChannelProbePoint {
    timestamp: i64,
    total_request_count: i32,
    success_request_count: i32,
    avg_tokens_per_second: Option<f64>,
    avg_time_to_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone)]
struct StoredChannelProbeData {
    channel_id: i64,
    points: Vec<StoredChannelProbePoint>,
}

#[derive(Debug, Clone)]
struct StoredProviderQuotaStatus {
    id: i64,
    channel_id: i64,
    provider_type: String,
    status: String,
    quota_data_json: String,
    next_reset_at: Option<i64>,
    ready: bool,
    next_check_at: i64,
}

#[derive(Debug, Clone, Default)]
struct StoredGcCleanupSummary {
    requests_deleted: i64,
    request_executions_deleted: i64,
    threads_deleted: i64,
    traces_deleted: i64,
    usage_logs_deleted: i64,
    channel_probes_deleted: i64,
    vacuum_ran: bool,
}

#[derive(Debug, Clone, Serialize)]
struct StoredBackupPayload {
    version: String,
    timestamp: String,
    channels: Vec<StoredBackupChannel>,
    models: Vec<StoredBackupModel>,
    channel_model_prices: Vec<Value>,
    api_keys: Vec<StoredBackupApiKey>,
}

#[derive(Debug, Clone, Serialize)]
struct StoredBackupChannel {
    id: i64,
    name: String,
    channel_type: String,
    base_url: String,
    status: String,
    credentials: Value,
    supported_models: Value,
    default_test_model: String,
    settings: Value,
    tags: Value,
    ordering_weight: i64,
    error_message: String,
    remark: String,
}

#[derive(Debug, Clone, Serialize)]
struct StoredBackupModel {
    id: i64,
    developer: String,
    model_id: String,
    model_type: String,
    name: String,
    icon: String,
    group: String,
    model_card: Value,
    settings: Value,
    status: String,
    remark: String,
}

#[derive(Debug, Clone, Serialize)]
struct StoredBackupApiKey {
    id: i64,
    project_id: i64,
    project_name: String,
    key: String,
    name: String,
    key_type: String,
    status: String,
    scopes: Value,
}

#[derive(Debug, Clone, Default)]
struct ProbeComputation {
    total_request_count: i32,
    success_request_count: i32,
    avg_tokens_per_second: Option<f64>,
    avg_time_to_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemStatus", rename_fields = "camelCase")]
struct AdminGraphqlSystemStatus {
    is_initialized: bool,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "CleanupOption", rename_fields = "camelCase")]
struct AdminGraphqlCleanupOption {
    resource_type: String,
    enabled: bool,
    cleanup_days: i32,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "StoragePolicy", rename_fields = "camelCase")]
struct AdminGraphqlStoragePolicy {
    store_chunks: bool,
    store_request_body: bool,
    store_response_body: bool,
    cleanup_options: Vec<AdminGraphqlCleanupOption>,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "CleanupOptionInput")]
struct AdminGraphqlCleanupOptionInput {
    resource_type: String,
    enabled: bool,
    cleanup_days: i32,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateStoragePolicyInput")]
struct AdminGraphqlUpdateStoragePolicyInput {
    store_chunks: Option<bool>,
    store_request_body: Option<bool>,
    store_response_body: Option<bool>,
    cleanup_options: Option<Vec<AdminGraphqlCleanupOptionInput>>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AutoBackupSettings", rename_fields = "camelCase")]
struct AdminGraphqlAutoBackupSettings {
    enabled: bool,
    frequency: BackupFrequencySetting,
    data_storage_id: i32,
    include_channels: bool,
    include_models: bool,
    include_api_keys: bool,
    include_model_prices: bool,
    retention_days: i32,
    last_backup_at: Option<String>,
    last_backup_error: Option<String>,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateAutoBackupSettingsInput")]
struct AdminGraphqlUpdateAutoBackupSettingsInput {
    enabled: Option<bool>,
    frequency: Option<BackupFrequencySetting>,
    data_storage_id: Option<i32>,
    include_channels: Option<bool>,
    include_models: Option<bool>,
    include_api_keys: Option<bool>,
    include_model_prices: Option<bool>,
    retention_days: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "TriggerBackupPayload", rename_fields = "camelCase")]
struct AdminGraphqlTriggerBackupPayload {
    success: bool,
    message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbeSetting", rename_fields = "camelCase")]
struct AdminGraphqlChannelProbeSetting {
    enabled: bool,
    frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemChannelSettings", rename_fields = "camelCase")]
struct AdminGraphqlSystemChannelSettings {
    probe: AdminGraphqlChannelProbeSetting,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateChannelProbeSettingInput")]
struct AdminGraphqlUpdateChannelProbeSettingInput {
    enabled: bool,
    frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateSystemChannelSettingsInput")]
struct AdminGraphqlUpdateSystemChannelSettingsInput {
    probe: Option<AdminGraphqlUpdateChannelProbeSettingInput>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbePoint", rename_fields = "camelCase")]
struct AdminGraphqlChannelProbePoint {
    timestamp: i64,
    total_request_count: i32,
    success_request_count: i32,
    avg_tokens_per_second: Option<f64>,
    avg_time_to_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbeData", rename_fields = "camelCase")]
struct AdminGraphqlChannelProbeData {
    channel_id: String,
    points: Vec<AdminGraphqlChannelProbePoint>,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "GetChannelProbeDataInput")]
struct AdminGraphqlGetChannelProbeDataInput {
    channel_ids: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProviderQuotaStatus", rename_fields = "camelCase")]
struct AdminGraphqlProviderQuotaStatus {
    id: String,
    channel_id: String,
    provider_type: String,
    status: String,
    ready: bool,
    next_reset_at: Option<String>,
    next_check_at: String,
    message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Channel", rename_fields = "camelCase")]
struct AdminGraphqlChannel {
    id: String,
    name: String,
    channel_type: String,
    base_url: String,
    status: String,
    supported_models: Vec<String>,
    ordering_weight: i32,
    provider_quota_status: Option<AdminGraphqlProviderQuotaStatus>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Model", rename_fields = "camelCase")]
struct AdminGraphqlModel {
    id: String,
    developer: String,
    model_id: String,
    model_type: String,
    name: String,
    icon: String,
    remark: String,
    context_length: Option<i32>,
    max_output_tokens: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Request", rename_fields = "camelCase")]
struct AdminGraphqlRequestSummaryObject {
    id: String,
    project_id: String,
    trace_id: Option<String>,
    channel_id: Option<String>,
    model_id: String,
    format: String,
    status: String,
    source: String,
    external_id: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Trace", rename_fields = "camelCase")]
struct AdminGraphqlTrace {
    id: String,
    trace_id: String,
    project_id: String,
    thread_id: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "APIKey", rename_fields = "camelCase")]
struct OpenApiGraphqlApiKey {
    key: String,
    name: String,
    scopes: Vec<String>,
}

#[derive(Debug, Clone)]
enum CreateLlmApiKeyError {
    InvalidName,
    PermissionDenied,
    Internal(String),
}

impl SqliteAuthContextService {
    pub fn new(foundation: Arc<SqliteFoundation>, allow_no_auth: bool) -> Self {
        Self {
            foundation,
            allow_no_auth,
        }
    }
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
                let _ = self.foundation.usage_costs().record_usage(&NewUsageLogRecord {
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
                let built_headers = build_upstream_headers(upstream_headers, target.api_key.as_str())?;
                let client = reqwest::blocking::Client::new();
                let mut upstream_request = client
                    .request(upstream_method.clone(), upstream_url_for_target(target).as_str())
                    .headers(built_headers);
                if matches!(upstream_method, reqwest::Method::POST) {
                    upstream_request = upstream_request.json(upstream_body);
                }
                let upstream_response = upstream_request.send().map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to execute upstream request: {error}"),
                })?;

                let status = upstream_response.status().as_u16();
                let response_text = upstream_response.text().map_err(|error| OpenAiV1Error::Internal {
                    message: format!("Failed to read upstream response: {error}"),
                })?;
                let raw_response_body: Value = serde_json::from_str(&response_text).map_err(|error| {
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

impl SqliteOperationalService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        Self {
            foundation,
            file_storage_cache: Arc::new(RwLock::new(HashMap::new())),
            last_probe_timestamp: Arc::new(Mutex::new(None)),
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

    pub fn run_auto_backup_tick(&self) -> Result<bool, String> {
        let settings = self.auto_backup_settings()?;
        if !settings.enabled {
            return Ok(false);
        }
        if !settings.frequency.is_due(settings.last_backup_at, current_unix_timestamp()) {
            return Ok(false);
        }
        self.perform_backup(&settings)?;
        Ok(true)
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

    pub fn run_channel_probe_tick(&self) -> Result<usize, String> {
        let settings = self.system_channel_settings()?;
        if !settings.probe.enabled {
            return Ok(0);
        }

        let interval_minutes = settings.probe.interval_minutes();
        let interval_seconds = i64::from(interval_minutes) * 60;
        let now = current_unix_timestamp();
        let aligned_timestamp = now - (now % interval_seconds);
        {
            let mut guard = self
                .last_probe_timestamp
                .lock()
                .map_err(|_| "failed to lock probe scheduler state".to_owned())?;
            if guard.as_ref() == Some(&aligned_timestamp) {
                return Ok(0);
            }
            *guard = Some(aligned_timestamp);
        }

        let start_timestamp = aligned_timestamp - interval_seconds;
        let channels = self
            .foundation
            .channel_models()
            .list_channels()
            .map_err(|error| format!("failed to load channels for probe: {error}"))?;

        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| format!("failed to open probe database: {error}"))?;
        ensure_operational_tables(&connection)
            .map_err(|error| format!("failed to ensure probe schema: {error}"))?;

        let mut stored = 0;
        for channel in channels.into_iter().filter(|channel| channel.status == "enabled") {
            let stats = collect_channel_probe_stats(&connection, channel.id, start_timestamp, aligned_timestamp)
                .map_err(|error| format!("failed to collect channel probe stats: {error}"))?;
            if stats.total_request_count == 0 {
                continue;
            }
            upsert_channel_probe_point(&connection, channel.id, aligned_timestamp, &stats)
                .map_err(|error| format!("failed to store channel probe point: {error}"))?;
            stored += 1;
        }

        Ok(stored)
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

    pub fn run_provider_quota_check_tick(&self, force: bool, check_interval: Duration) -> Result<usize, String> {
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
            .map_err(|error| format!("failed to list channels for provider quota checks: {error}"))?;
        let now = current_unix_timestamp();
        let next_check_at = now + i64::try_from(check_interval.as_secs()).unwrap_or(0);
        let mut updated = 0;

        for channel in channels.into_iter().filter(|channel| channel.status == "enabled") {
            let Some(provider_type) = provider_quota_type_for_channel(channel.channel_type.as_str()) else {
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

    pub fn run_gc_cleanup_now(&self, vacuum_enabled: bool, vacuum_full: bool) -> Result<StoredGcCleanupSummary, String> {
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
                    summary.request_executions_deleted += cleanup_request_executions(&connection, cutoff)
                        .map_err(|error| format!("failed to cleanup request executions: {error}"))?;
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
            .ok_or_else(|| "backup data storage is not an active fs storage in the Rust slice".to_owned())?;
        fs::create_dir_all(storage.root.as_path())
            .map_err(|error| format!("failed to create backup directory: {error}"))?;

        let backup = self.build_backup_payload(settings)?;
        let filename = format!("{AUTO_BACKUP_PREFIX}{}{AUTO_BACKUP_SUFFIX}", current_unix_timestamp());
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

    fn build_backup_payload(&self, settings: &StoredAutoBackupSettings) -> Result<StoredBackupPayload, String> {
        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| format!("failed to open backup database: {error}"))?;
        ensure_all_foundation_tables(&connection)
            .map_err(|error| format!("failed to ensure backup schema: {error}"))?;

        let channels = if settings.include_channels {
            list_backup_channels(&connection).map_err(|error| format!("failed to load backup channels: {error}"))?
        } else {
            Vec::new()
        };
        let models = if settings.include_models {
            list_backup_models(&connection).map_err(|error| format!("failed to load backup models: {error}"))?
        } else {
            Vec::new()
        };
        let api_keys = if settings.include_api_keys {
            list_backup_api_keys(&connection).map_err(|error| format!("failed to load backup api keys: {error}"))?
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
            .checked_sub(Duration::from_secs(u64::try_from(retention_days.max(0)).unwrap_or(0) * 86_400))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        for entry in fs::read_dir(root).map_err(|error| format!("failed to read backup directory: {error}"))? {
            let entry = entry.map_err(|error| format!("failed to inspect backup directory entry: {error}"))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.starts_with(AUTO_BACKUP_PREFIX) || !file_name.ends_with(AUTO_BACKUP_SUFFIX) {
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
        let json = serde_json::to_string(value).map_err(|error| format!("failed to serialize setting: {error}"))?;
        self.foundation
            .system_settings()
            .set_value(key, json.as_str())
            .map_err(|error| format!("failed to persist setting: {error}"))
    }
}

impl SqliteAdminGraphqlService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        let operational = Arc::new(SqliteOperationalService::new(foundation.clone()));
        let schema = Schema::build(
            AdminGraphqlQueryRoot {
                foundation,
                operational: operational.clone(),
            },
            AdminGraphqlMutationRoot { operational },
            EmptySubscription,
        )
        .finish();

        Self {
            schema: Arc::new(schema),
        }
    }
}

impl SqliteOpenApiGraphqlService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        let schema = Schema::build(
            OpenApiGraphqlQueryRoot,
            OpenApiGraphqlMutationRoot { foundation },
            EmptySubscription,
        )
        .finish();

        Self {
            schema: Arc::new(schema),
        }
    }
}

impl ProviderEdgeAdminConfig {
    fn default() -> Self {
        Self {
            codex_authorize_url: "https://auth.openai.com/oauth/authorize".to_owned(),
            codex_token_url: "https://auth.openai.com/oauth/token".to_owned(),
            codex_client_id: "app_EMoamEEZ73f0CkXaXp7hrann".to_owned(),
            codex_redirect_uri: "http://localhost:1455/auth/callback".to_owned(),
            codex_scopes: "openid profile email offline_access".to_owned(),
            codex_user_agent: "codex_cli_rs/0.98.0 (Mac OS 15.6.1; arm64) iTerm.app/3.6.6".to_owned(),
            claudecode_authorize_url: "https://claude.ai/oauth/authorize".to_owned(),
            claudecode_token_url: "https://console.anthropic.com/v1/oauth/token".to_owned(),
            claudecode_client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".to_owned(),
            claudecode_redirect_uri: "http://localhost:54545/callback".to_owned(),
            claudecode_scopes: "org:create_api_key user:profile user:inference".to_owned(),
            claudecode_user_agent: "claude-cli/2.1.78 (external, cli)".to_owned(),
            antigravity_authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            antigravity_token_url: "https://oauth2.googleapis.com/token".to_owned(),
            antigravity_client_id:
                "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com"
                    .to_owned(),
            antigravity_client_secret: "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".to_owned(),
            antigravity_redirect_uri: "http://localhost:51121/oauth-callback".to_owned(),
            antigravity_scopes: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs".to_owned(),
            antigravity_load_endpoints: vec![
                "https://cloudcode-pa.googleapis.com".to_owned(),
                "https://daily-cloudcode-pa.sandbox.googleapis.com".to_owned(),
                "https://autopush-cloudcode-pa.sandbox.googleapis.com".to_owned(),
            ],
            antigravity_user_agent: "antigravity/1.20.4 windows/amd64".to_owned(),
            antigravity_client_metadata:
                r#"{"ideType":"ANTIGRAVITY","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
                    .to_owned(),
            copilot_device_code_url: "https://github.com/login/device/code".to_owned(),
            copilot_access_token_url: "https://github.com/login/oauth/access_token".to_owned(),
            copilot_client_id: "Iv1.b507a08c87ecfe98".to_owned(),
            copilot_scope: "read:user".to_owned(),
        }
    }
}

impl SqliteProviderEdgeAdminService {
    pub fn new() -> Self {
        Self {
            config: ProviderEdgeAdminConfig::default(),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            http_client: ProviderEdgeHttpClient::Default,
        }
    }

    #[cfg(test)]
    fn with_config_and_client(config: ProviderEdgeAdminConfig, http_client: reqwest::blocking::Client) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            http_client: ProviderEdgeHttpClient::Injected(http_client),
        }
    }

    fn http_client(&self) -> &reqwest::blocking::Client {
        match &self.http_client {
            ProviderEdgeHttpClient::Default => provider_edge_default_http_client(),
            ProviderEdgeHttpClient::Injected(client) => client,
        }
    }

    fn run_copilot_http_task<T, Task>(&self, task: Task) -> Result<T, ProviderEdgeAdminError>
    where
        T: Send + 'static,
        Task: FnOnce(reqwest::blocking::Client) -> Result<T, ProviderEdgeAdminError> + Send + 'static,
    {
        match &self.http_client {
            ProviderEdgeHttpClient::Default => std::thread::spawn(move || {
                let client = reqwest::blocking::Client::new();
                task(client)
            })
            .join()
            .map_err(|_| provider_edge_internal_error("copilot upstream task panicked"))?,
            ProviderEdgeHttpClient::Injected(client) => {
                let client = client.clone();
                std::thread::spawn(move || task(client))
                    .join()
                    .map_err(|_| provider_edge_internal_error("copilot upstream task panicked"))?
            }
        }
    }

    fn start_pkce_flow(
        &self,
        provider: PkceProvider,
        project_id: Option<String>,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        let session_id = generate_provider_edge_session_id()?;
        let code_verifier = generate_provider_edge_code_verifier()?;
        let auth_url = self.provider_authorize_url(provider, session_id.as_str(), code_verifier.as_str());

        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .insert(
                session_id.clone(),
                ProviderEdgeSession::Pkce {
                    provider,
                    code_verifier,
                    project_id,
                    created_at: current_unix_timestamp(),
                },
            );

        Ok(StartPkceOAuthResponse { session_id, auth_url })
    }

    fn exchange_pkce_flow(
        &self,
        provider: PkceProvider,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        if request.session_id.trim().is_empty() || request.callback_url.trim().is_empty() {
            return Err(provider_edge_invalid_request(
                "session_id and callback_url are required",
            ));
        }

        let callback = parse_callback(provider, request.callback_url.as_str())?;
        if callback.state != request.session_id {
            return Err(provider_edge_invalid_request("oauth state mismatch"));
        }

        let session = self.take_session(request.session_id.as_str())?;
        let (code_verifier, project_id) = match session {
            ProviderEdgeSession::Pkce {
                provider: stored_provider,
                code_verifier,
                project_id,
                created_at,
            } => {
                if stored_provider != provider {
                    return Err(provider_edge_invalid_request("invalid or expired oauth session"));
                }
                if current_unix_timestamp().saturating_sub(created_at)
                    > PROVIDER_EDGE_PKCE_SESSION_TTL_SECONDS
                {
                    return Err(provider_edge_invalid_request("invalid or expired oauth session"));
                }
                (code_verifier, project_id)
            }
            ProviderEdgeSession::CopilotDevice { .. } => {
                return Err(provider_edge_invalid_request("invalid or expired oauth session"))
            }
        };

        let token = self.exchange_provider_token(provider, callback.code.as_str(), callback.state.as_str(), code_verifier.as_str())?;

        let credentials = match provider {
            PkceProvider::Antigravity => {
                let refresh_token = token.refresh_token.clone().unwrap_or_default();
                let project_id = match project_id {
                    Some(project_id) if !project_id.trim().is_empty() => project_id,
                    _ => self.resolve_antigravity_project_id(token.access_token.as_deref().unwrap_or_default())?,
                };
                format!("{refresh_token}|{project_id}")
            }
            _ => oauth_credentials_json(&token, self.provider_client_id(provider).to_owned()),
        };

        Ok(ExchangeOAuthResponse { credentials })
    }

    fn take_session(&self, session_id: &str) -> Result<ProviderEdgeSession, ProviderEdgeAdminError> {
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .remove(session_id)
            .ok_or_else(|| provider_edge_invalid_request("invalid or expired oauth session"))
    }

    fn load_session(&self, session_id: &str) -> Result<ProviderEdgeSession, ProviderEdgeAdminError> {
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .get(session_id)
            .cloned()
            .ok_or_else(|| provider_edge_invalid_request("invalid or expired session"))
    }

    fn delete_session(&self, session_id: &str) -> Result<(), ProviderEdgeAdminError> {
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .remove(session_id);
        Ok(())
    }

    fn provider_authorize_url(&self, provider: PkceProvider, state: &str, code_verifier: &str) -> String {
        let mut params = vec![
            ("response_type", "code".to_owned()),
            ("client_id", self.provider_client_id(provider).to_owned()),
            ("redirect_uri", self.provider_redirect_uri(provider).to_owned()),
            ("scope", self.provider_scopes(provider).to_owned()),
            (
                "code_challenge",
                provider_edge_code_challenge(code_verifier),
            ),
            ("code_challenge_method", "S256".to_owned()),
            ("state", state.to_owned()),
        ];

        match provider {
            PkceProvider::Codex => {
                params.push(("id_token_add_organizations", "true".to_owned()));
                params.push(("codex_cli_simplified_flow", "true".to_owned()));
            }
            PkceProvider::Antigravity => {
                params.push(("access_type", "offline".to_owned()));
                params.push(("prompt", "consent".to_owned()));
            }
            PkceProvider::ClaudeCode => {}
        }

        format!(
            "{}?{}",
            self.provider_authorize_endpoint(provider),
            form_urlencode(params),
        )
    }

    fn provider_authorize_endpoint(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_authorize_url.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_authorize_url.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_authorize_url.as_str(),
        }
    }

    fn provider_client_id(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_client_id.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_client_id.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_client_id.as_str(),
        }
    }

    fn provider_redirect_uri(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_redirect_uri.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_redirect_uri.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_redirect_uri.as_str(),
        }
    }

    fn provider_scopes(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_scopes.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_scopes.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_scopes.as_str(),
        }
    }

    fn provider_user_agent(&self, provider: PkceProvider) -> Option<&str> {
        match provider {
            PkceProvider::Codex => Some(self.config.codex_user_agent.as_str()),
            PkceProvider::ClaudeCode => Some(self.config.claudecode_user_agent.as_str()),
            PkceProvider::Antigravity => Some(self.config.antigravity_user_agent.as_str()),
        }
    }

    fn provider_token_endpoint(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_token_url.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_token_url.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_token_url.as_str(),
        }
    }

    fn exchange_provider_token(
        &self,
        provider: PkceProvider,
        code: &str,
        state: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenResponse, ProviderEdgeAdminError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Some(user_agent) = self.provider_user_agent(provider) {
            headers.insert(
                USER_AGENT,
                HeaderValue::from_str(user_agent)
                    .map_err(|error| provider_edge_internal_error(format!("invalid user agent header: {error}")))?,
            );
        }

        let response = match provider {
            PkceProvider::ClaudeCode => {
                headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                let mut body = serde_json::Map::new();
                body.insert("grant_type".to_owned(), Value::String("authorization_code".to_owned()));
                body.insert("code".to_owned(), Value::String(code.to_owned()));
                body.insert(
                    "client_id".to_owned(),
                    Value::String(self.provider_client_id(provider).to_owned()),
                );
                body.insert(
                    "redirect_uri".to_owned(),
                    Value::String(self.provider_redirect_uri(provider).to_owned()),
                );
                body.insert(
                    "code_verifier".to_owned(),
                    Value::String(code_verifier.to_owned()),
                );
                body.insert("state".to_owned(), Value::String(state.to_owned()));
                self.http_client()
                    .post(self.provider_token_endpoint(provider))
                    .headers(headers)
                    .json(&Value::Object(body))
                    .send()
                    .map_err(|error| provider_edge_bad_gateway(format!("token exchange failed: {error}")))?
            }
            PkceProvider::Codex | PkceProvider::Antigravity => {
                headers.insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/x-www-form-urlencoded"),
                );
                let mut params = vec![
                    ("grant_type", "authorization_code".to_owned()),
                    ("client_id", self.provider_client_id(provider).to_owned()),
                    ("code", code.to_owned()),
                    (
                        "redirect_uri",
                        self.provider_redirect_uri(provider).to_owned(),
                    ),
                    ("code_verifier", code_verifier.to_owned()),
                ];
                if provider == PkceProvider::Antigravity {
                    params.push((
                        "client_secret",
                        self.config.antigravity_client_secret.clone(),
                    ));
                }
                self.http_client()
                    .post(self.provider_token_endpoint(provider))
                    .headers(headers)
                    .body(form_urlencode(params))
                    .send()
                    .map_err(|error| provider_edge_bad_gateway(format!("token exchange failed: {error}")))?
            }
        };

        let status = response.status();
        let body = response
            .text()
            .map_err(|error| provider_edge_bad_gateway(format!("token exchange failed: {error}")))?;
        if !status.is_success() {
            return Err(provider_edge_bad_gateway(format!(
                "token exchange failed: upstream status {}: {body}",
                status.as_u16()
            )));
        }

        let token: OAuthTokenResponse = serde_json::from_str(body.as_str())
            .map_err(|error| provider_edge_bad_gateway(format!("token exchange failed: {error}")))?;
        if let Some(error) = token.error.as_ref() {
            let description = token.error_description.clone().unwrap_or_default();
            return Err(provider_edge_bad_gateway(format!(
                "token exchange failed: {error} - {description}"
            )));
        }
        if token.access_token.as_deref().unwrap_or_default().is_empty() {
            return Err(provider_edge_bad_gateway(
                "token exchange failed: token response missing access_token",
            ));
        }
        Ok(token)
    }

    fn resolve_antigravity_project_id(&self, access_token: &str) -> Result<String, ProviderEdgeAdminError> {
        if self.config.antigravity_load_endpoints.is_empty() {
            return Err(provider_edge_bad_gateway("failed to resolve project id and none provided: no load endpoints configured"));
        }

        let mut last_error = None;
        let mut default_tier_id = "FREE".to_owned();
        for endpoint in &self.config.antigravity_load_endpoints {
            let url = format!("{endpoint}/v1internal:loadCodeAssist");
            let response = self
                .http_client()
                .post(url)
                .header(AUTHORIZATION, format!("Bearer {access_token}"))
                .header(CONTENT_TYPE, "application/json")
                .header(USER_AGENT, self.config.antigravity_user_agent.as_str())
                .header("X-Goog-Api-Client", "google-cloud-sdk vscode_cloudshelleditor/0.1")
                .header("Client-Metadata", self.config.antigravity_client_metadata.as_str())
                .json(&serde_json::json!({
                    "metadata": {
                        "ideType": "ANTIGRAVITY",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI"
                    }
                }))
                .send();

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            if !response.status().is_success() {
                last_error = Some(format!("status {}", response.status().as_u16()));
                continue;
            }

            let body: Value = response
                .json()
                .map_err(|error| provider_edge_bad_gateway(format!("failed to resolve project id and none provided: {error}")))?;
            if let Some(project_id) = extract_antigravity_project_id(&body) {
                return Ok(project_id);
            }

            if let Some(tier_id) = extract_antigravity_default_tier(&body) {
                default_tier_id = tier_id;
            }
            match self.onboard_antigravity_user(endpoint.as_str(), access_token, default_tier_id.as_str()) {
                Ok(project_id) if !project_id.is_empty() => return Ok(project_id),
                Ok(_) => {}
                Err(error) => {
                    last_error = Some(error.clone());
                }
            }
        }

        Err(provider_edge_bad_gateway(format!(
            "failed to resolve project id and none provided: {}",
            last_error.unwrap_or_else(|| "unknown error".to_owned())
        )))
    }

    fn onboard_antigravity_user(
        &self,
        endpoint: &str,
        access_token: &str,
        tier_id: &str,
    ) -> Result<String, String> {
        let url = format!("{endpoint}/v1internal:onboardUser");
        for _ in 0..3 {
            let response = self
                .http_client()
                .post(url.as_str())
                .header(AUTHORIZATION, format!("Bearer {access_token}"))
                .header(CONTENT_TYPE, "application/json")
                .header(USER_AGENT, self.config.antigravity_user_agent.as_str())
                .header("X-Goog-Api-Client", "google-cloud-sdk vscode_cloudshelleditor/0.1")
                .header("Client-Metadata", self.config.antigravity_client_metadata.as_str())
                .json(&serde_json::json!({
                    "tierId": tier_id,
                    "metadata": {
                        "ideType": "ANTIGRAVITY",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI"
                    }
                }))
                .send();

            let response = match response {
                Ok(response) => response,
                Err(error) => return Err(error.to_string()),
            };
            if !response.status().is_success() {
                continue;
            }
            let body: Value = response.json().map_err(|error| error.to_string())?;
            if body.get("done").and_then(Value::as_bool) == Some(true) {
                if let Some(project_id) = body
                    .get("response")
                    .and_then(|value| value.get("cloudaicompanionProject"))
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                {
                    return Ok(project_id.to_owned());
                }
            }
        }
        Err("failed to onboard user after retries".to_owned())
    }

    fn request_copilot_device_code(&self) -> Result<CopilotDeviceCodeResponse, ProviderEdgeAdminError> {
        let url = self.config.copilot_device_code_url.clone();
        let client_id = self.config.copilot_client_id.clone();
        let scope = self.config.copilot_scope.clone();

        self.run_copilot_http_task(move |client| {
            let response = client
                .post(url.as_str())
                .header(ACCEPT, "application/json")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_urlencode(vec![("client_id", client_id), ("scope", scope)]))
                .send()
                .map_err(|error| provider_edge_bad_gateway(format!("failed to request device code: {error}")))?;
            let status = response.status();
            let body = response
                .text()
                .map_err(|error| provider_edge_bad_gateway(format!("failed to request device code: {error}")))?;
            if !status.is_success() {
                return Err(provider_edge_bad_gateway(format!(
                    "failed to request device code: device code request failed with status {}: {}",
                    status.as_u16(),
                    body
                )));
            }
            let device: CopilotDeviceCodeResponse = serde_json::from_str(body.as_str()).map_err(|error| {
                provider_edge_bad_gateway(format!(
                    "failed to request device code: failed to parse device code response: {error}"
                ))
            })?;
            if device.device_code.trim().is_empty() {
                return Err(provider_edge_bad_gateway(
                    "failed to request device code: device code not received from GitHub",
                ));
            }
            Ok(device)
        })
    }

    fn poll_copilot_token_upstream(
        &self,
        device_code: &str,
    ) -> Result<CopilotPollResponse, ProviderEdgeAdminError> {
        let url = self.config.copilot_access_token_url.clone();
        let client_id = self.config.copilot_client_id.clone();
        let device_code = device_code.to_owned();

        self.run_copilot_http_task(move |client| {
            let response = client
                .post(url.as_str())
                .header(ACCEPT, "application/json")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_urlencode(vec![
                    ("client_id", client_id),
                    ("device_code", device_code),
                    (
                        "grant_type",
                        PROVIDER_EDGE_COPILOT_DEVICE_GRANT_TYPE.to_owned(),
                    ),
                ]))
                .send()
                .map_err(|error| provider_edge_bad_gateway(format!("token poll failed: {error}")))?;
            if !response.status().is_success() {
                return Err(provider_edge_bad_gateway(format!(
                    "token poll failed: access token request failed with status {}",
                    response.status().as_u16()
                )));
            }

            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let body = response
                .text()
                .map_err(|error| provider_edge_bad_gateway(format!("token poll failed: {error}")))?;
            if content_type.contains("application/json") {
                serde_json::from_str(body.as_str()).map_err(|error| {
                    provider_edge_bad_gateway(format!(
                        "token poll failed: failed to parse access token JSON response: {error}"
                    ))
                })
            } else {
                parse_copilot_form_response(body.as_str())
            }
        })
    }
}

impl ProviderEdgeAdminPort for SqliteProviderEdgeAdminService {
    fn start_codex_oauth(
        &self,
        _request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        self.start_pkce_flow(PkceProvider::Codex, None)
    }

    fn exchange_codex_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        self.exchange_pkce_flow(PkceProvider::Codex, request)
    }

    fn start_claudecode_oauth(
        &self,
        _request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        self.start_pkce_flow(PkceProvider::ClaudeCode, None)
    }

    fn exchange_claudecode_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        self.exchange_pkce_flow(PkceProvider::ClaudeCode, request)
    }

    fn start_antigravity_oauth(
        &self,
        request: &StartAntigravityOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        let project_id = request.project_id.trim();
        self.start_pkce_flow(
            PkceProvider::Antigravity,
            if project_id.is_empty() {
                None
            } else {
                Some(project_id.to_owned())
            },
        )
    }

    fn exchange_antigravity_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        self.exchange_pkce_flow(PkceProvider::Antigravity, request)
    }

    fn start_copilot_oauth(
        &self,
        _request: &StartCopilotOAuthRequest,
    ) -> Result<StartCopilotOAuthResponse, ProviderEdgeAdminError> {
        let session_id = generate_provider_edge_session_id()?;
        let device = self.request_copilot_device_code()?;
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .insert(
                session_id.clone(),
                ProviderEdgeSession::CopilotDevice {
                    device_code: device.device_code.clone(),
                    expires_in: device.expires_in,
                    interval: device.interval,
                    user_code: device.user_code.clone(),
                    verification_uri: device.verification_uri.clone(),
                    created_at: current_unix_timestamp(),
                },
            );
        Ok(StartCopilotOAuthResponse {
            session_id,
            user_code: device.user_code,
            verification_uri: device.verification_uri,
            expires_in: device.expires_in,
            interval: device.interval,
        })
    }

    fn poll_copilot_oauth(
        &self,
        request: &PollCopilotOAuthRequest,
    ) -> Result<PollCopilotOAuthResponse, ProviderEdgeAdminError> {
        let session = self.load_session(request.session_id.as_str())?;
        let (device_code, expires_in, created_at) = match session {
            ProviderEdgeSession::CopilotDevice {
                device_code,
                expires_in,
                created_at,
                ..
            } => (device_code, expires_in, created_at),
            ProviderEdgeSession::Pkce { .. } => {
                return Err(provider_edge_invalid_request("invalid or expired session"))
            }
        };
        if current_unix_timestamp() > created_at.saturating_add(expires_in) {
            self.delete_session(request.session_id.as_str())?;
            return Err(provider_edge_invalid_request("device code expired"));
        }

        let response = self.poll_copilot_token_upstream(device_code.as_str())?;
        if let Some(error) = response.error.as_deref() {
            return match error {
                "authorization_pending" => Ok(PollCopilotOAuthResponse {
                    access_token: None,
                    token_type: None,
                    scope: None,
                    status: "pending".to_owned(),
                    message: Some(PROVIDER_EDGE_COPILOT_PENDING_MESSAGE.to_owned()),
                }),
                "slow_down" => Ok(PollCopilotOAuthResponse {
                    access_token: None,
                    token_type: None,
                    scope: None,
                    status: "slow_down".to_owned(),
                    message: Some(PROVIDER_EDGE_COPILOT_SLOW_DOWN_MESSAGE.to_owned()),
                }),
                "expired_token" => {
                    self.delete_session(request.session_id.as_str())?;
                    Err(provider_edge_invalid_request("device code expired"))
                }
                "access_denied" => {
                    self.delete_session(request.session_id.as_str())?;
                    Err(provider_edge_invalid_request("access denied by user"))
                }
                other => Err(provider_edge_bad_gateway(format!(
                    "OAuth error: {other} - {}",
                    response.error_description.unwrap_or_default()
                ))),
            };
        }
        if let Some(access_token) = response.access_token {
            self.delete_session(request.session_id.as_str())?;
            return Ok(PollCopilotOAuthResponse {
                access_token: Some(access_token),
                token_type: response.token_type,
                scope: response.scope,
                status: "complete".to_owned(),
                message: Some(PROVIDER_EDGE_COPILOT_COMPLETE_MESSAGE.to_owned()),
            });
        }
        Err(provider_edge_internal_error(
            "unexpected response from GitHub",
        ))
    }
}

#[derive(Debug, Clone)]
struct ParsedCallback {
    code: String,
    state: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CopilotPollResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn provider_edge_invalid_request(message: impl Into<String>) -> ProviderEdgeAdminError {
    ProviderEdgeAdminError::InvalidRequest {
        message: message.into(),
    }
}

fn provider_edge_bad_gateway(message: impl Into<String>) -> ProviderEdgeAdminError {
    ProviderEdgeAdminError::BadGateway {
        message: message.into(),
    }
}

fn provider_edge_internal_error(message: impl Into<String>) -> ProviderEdgeAdminError {
    ProviderEdgeAdminError::Internal {
        message: message.into(),
    }
}

fn parse_callback(
    provider: PkceProvider,
    callback_url: &str,
) -> Result<ParsedCallback, ProviderEdgeAdminError> {
    let trimmed = callback_url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(provider_edge_invalid_request(
            "callback_url must be a full URL",
        ));
    }
    let url = reqwest::Url::parse(trimmed).map_err(|error| {
        provider_edge_invalid_request(format!("invalid callback_url: {error}"))
    })?;
    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| provider_edge_invalid_request("code parameter not found in callback_url"))?;
    let state = match provider {
        PkceProvider::ClaudeCode => {
            if !url.fragment().unwrap_or_default().trim().is_empty() {
                url.fragment().unwrap_or_default().to_owned()
            } else {
                url.query_pairs()
                    .find(|(key, _)| key == "state")
                    .map(|(_, value)| value.into_owned())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        provider_edge_invalid_request(
                            "state parameter not found in callback_url (should be after # or in query)",
                        )
                    })?
            }
        }
        PkceProvider::Codex | PkceProvider::Antigravity => url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| provider_edge_invalid_request("state parameter not found in callback_url"))?,
    };
    Ok(ParsedCallback { code, state })
}

fn generate_provider_edge_session_id() -> Result<String, ProviderEdgeAdminError> {
    let foundation = SqliteFoundation::new(":memory:");
    let connection = foundation
        .open_connection(true)
        .map_err(|error| provider_edge_internal_error(format!("failed to generate oauth state: {error}")))?;
    connection
        .query_row("SELECT lower(hex(randomblob(32)))", [], |row| row.get(0))
        .map_err(|error| provider_edge_internal_error(format!("failed to generate oauth state: {error}")))
}

fn generate_provider_edge_code_verifier() -> Result<String, ProviderEdgeAdminError> {
    let foundation = SqliteFoundation::new(":memory:");
    let connection = foundation
        .open_connection(true)
        .map_err(|error| provider_edge_internal_error(format!("failed to generate code verifier: {error}")))?;
    let raw: String = connection
        .query_row("SELECT lower(hex(randomblob(64)))", [], |row| row.get(0))
        .map_err(|error| provider_edge_internal_error(format!("failed to generate code verifier: {error}")))?;
    Ok(raw)
}

fn provider_edge_code_challenge(code_verifier: &str) -> String {
    base64_url_no_padding(&sha256_digest(code_verifier.as_bytes()))
}

fn form_urlencode(params: Vec<(&str, String)>) -> String {
    let mut url = reqwest::Url::parse("http://localhost/").unwrap();
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in params {
            query.append_pair(key, value.as_str());
        }
    }
    url.query().unwrap_or_default().to_owned()
}

fn oauth_credentials_json(token: &OAuthTokenResponse, client_id: String) -> String {
    let mut credentials = serde_json::Map::new();
    credentials.insert("client_id".to_owned(), Value::String(client_id));
    credentials.insert(
        "access_token".to_owned(),
        Value::String(token.access_token.clone().unwrap_or_default()),
    );
    if let Some(refresh_token) = token.refresh_token.clone() {
        credentials.insert("refresh_token".to_owned(), Value::String(refresh_token));
    }
    if let Some(id_token) = token.id_token.clone() {
        credentials.insert("id_token".to_owned(), Value::String(id_token));
    }
    if let Some(token_type) = token.token_type.clone() {
        credentials.insert("token_type".to_owned(), Value::String(token_type));
    }
    if let Some(scope) = token.scope.clone() {
        let scopes = scope
            .split_whitespace()
            .map(|value| Value::String(value.to_owned()))
            .collect::<Vec<_>>();
        credentials.insert("scopes".to_owned(), Value::Array(scopes));
    }
    if let Some(expires_in) = token.expires_in {
        credentials.insert(
            "expires_at".to_owned(),
            Value::String(format_unix_timestamp(current_unix_timestamp().saturating_add(expires_in))),
        );
    }
    Value::Object(credentials).to_string()
}

fn extract_antigravity_project_id(body: &Value) -> Option<String> {
    if let Some(project_id) = body
        .get("cloudaicompanionProject")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return Some(project_id.to_owned());
    }
    body.get("cloudaicompanionProject")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_antigravity_default_tier(body: &Value) -> Option<String> {
    let tiers = body.get("allowedTiers")?.as_array()?;
    let first = tiers
        .first()
        .and_then(|tier| tier.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    tiers.iter()
        .find(|tier| tier.get("isDefault").and_then(Value::as_bool) == Some(true))
        .and_then(|tier| tier.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or(first)
}

fn parse_copilot_form_response(body: &str) -> Result<CopilotPollResponse, ProviderEdgeAdminError> {
    let url = reqwest::Url::parse(format!("http://localhost/?{body}").as_str())
        .map_err(|error| provider_edge_bad_gateway(format!("token poll failed: failed to parse access token form response: {error}")))?;
    let values = url.query_pairs().into_owned().collect::<HashMap<String, String>>();
    Ok(CopilotPollResponse {
        access_token: values.get("access_token").cloned().filter(|value| !value.is_empty()),
        token_type: values.get("token_type").cloned().filter(|value| !value.is_empty()),
        scope: values.get("scope").cloned().filter(|value| !value.is_empty()),
        error: values.get("error").cloned().filter(|value| !value.is_empty()),
        error_description: values
            .get("error_description")
            .cloned()
            .filter(|value| !value.is_empty()),
    })
}

fn provider_edge_default_http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        std::thread::spawn(reqwest::blocking::Client::new)
            .join()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

fn sha256_digest(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (input.len() as u64) * 8;
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in data.chunks(64) {
        let mut w = [0_u32; 64];
        for (index, word) in w.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *word = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut digest = [0_u8; 32];
    for (index, word) in h.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn base64_url_no_padding(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::new();
    let mut index = 0;
    while index + 3 <= input.len() {
        let chunk = &input[index..index + 3];
        let value = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        encoded.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 6) & 0x3f) as usize] as char);
        encoded.push(TABLE[(value & 0x3f) as usize] as char);
        index += 3;
    }

    let remainder = input.len().saturating_sub(index);
    if remainder == 1 {
        let value = (input[index] as u32) << 16;
        encoded.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
    } else if remainder == 2 {
        let value = ((input[index] as u32) << 16) | ((input[index + 1] as u32) << 8);
        encoded.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 6) & 0x3f) as usize] as char);
    }

    encoded
}

impl AdminGraphqlPort for SqliteAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let schema = Arc::clone(&self.schema);
        Box::pin(async move {
            execute_graphql_schema(schema, request, AdminGraphqlRequestContext { project_id, user }).await
        })
    }
}

impl OpenApiGraphqlPort for SqliteOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let schema = Arc::clone(&self.schema);
        Box::pin(async move {
            execute_graphql_schema(schema, request, OpenApiGraphqlRequestContext { owner_api_key }).await
        })
    }
}

async fn execute_graphql_schema<Query, Mutation, Data>(
    schema: Arc<Schema<Query, Mutation, EmptySubscription>>,
    payload: GraphqlRequestPayload,
    context_data: Data,
) -> GraphqlExecutionResult
where
    Query: async_graphql::ObjectType + Send + Sync + 'static,
    Mutation: async_graphql::ObjectType + Send + Sync + 'static,
    Data: Send + Sync + Clone + 'static,
{
    let mut request = AsyncGraphqlRequest::new(payload.query)
        .variables(Variables::from_json(payload.variables))
        .data(context_data);
    if let Some(operation_name) = payload
        .operation_name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        request = request.operation_name(operation_name);
    }

    let response = schema.execute(request).await;
    let body = serde_json::to_value(response).unwrap_or_else(|error| {
        serde_json::json!({
            "data": null,
            "errors": [{"message": format!("Failed to serialize GraphQL response: {error}")}]
        })
    });

    GraphqlExecutionResult { status: 200, body }
}

#[Object]
impl AdminGraphqlQueryRoot {
    async fn system_status(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlSystemStatus> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        let is_initialized = self
            .foundation
            .system_settings()
            .is_initialized()
            .map_err(|error| async_graphql::Error::new(format!("failed to check system status: {error}")))?;

        Ok(AdminGraphqlSystemStatus { is_initialized })
    }

    async fn channels(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlChannel>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        let quota_by_channel = self
            .operational
            .provider_quota_statuses()
            .unwrap_or_default()
            .into_iter()
            .map(|status| (status.channel_id, AdminGraphqlProviderQuotaStatus::from(status)))
            .collect::<HashMap<_, _>>();
        let channels = self
            .foundation
            .channel_models()
            .list_channels()
            .map_err(|error| async_graphql::Error::new(format!("failed to list channels: {error}")))?;

        Ok(channels
            .into_iter()
            .map(|channel| {
                let mut gql = AdminGraphqlChannel::from(channel.clone());
                gql.provider_quota_status = quota_by_channel.get(&channel.id).cloned();
                gql
            })
            .collect())
    }

    async fn storage_policy(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlStoragePolicy> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        self.operational
            .storage_policy()
            .map(AdminGraphqlStoragePolicy::from)
            .map_err(async_graphql::Error::new)
    }

    async fn auto_backup_settings(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlAutoBackupSettings> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        self.operational
            .auto_backup_settings()
            .map(AdminGraphqlAutoBackupSettings::from)
            .map_err(async_graphql::Error::new)
    }

    async fn system_channel_settings(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlSystemChannelSettings> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        self.operational
            .system_channel_settings()
            .map(AdminGraphqlSystemChannelSettings::from)
            .map_err(async_graphql::Error::new)
    }

    async fn channel_probe_data(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlGetChannelProbeDataInput,
    ) -> async_graphql::Result<Vec<AdminGraphqlChannelProbeData>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        self.operational
            .channel_probe_data(&input.channel_ids)
            .map(|items| items.into_iter().map(AdminGraphqlChannelProbeData::from).collect())
            .map_err(async_graphql::Error::new)
    }

    async fn models(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlModel>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        let models = self
            .foundation
            .channel_models()
            .list_enabled_model_records()
            .map_err(|error| async_graphql::Error::new(format!("failed to list models: {error}")))?;

        Ok(models.into_iter().map(AdminGraphqlModel::from).collect())
    }

    async fn requests(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlRequestSummaryObject>> {
        let project_id = require_admin_graphql_project_id(ctx)?;
        require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
        let requests = self
            .foundation
            .requests()
            .list_requests_by_project(project_id)
            .map_err(|error| async_graphql::Error::new(format!("failed to list requests: {error}")))?;

        Ok(requests
            .into_iter()
            .map(AdminGraphqlRequestSummaryObject::from)
            .collect())
    }

    async fn traces(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlTrace>> {
        let project_id = require_admin_graphql_project_id(ctx)?;
        require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
        let traces = self
            .foundation
            .trace_contexts()
            .list_traces_by_project(project_id)
            .map_err(|error| async_graphql::Error::new(format!("failed to list traces: {error}")))?;

        Ok(traces.into_iter().map(AdminGraphqlTrace::from).collect())
    }
}

#[Object]
impl AdminGraphqlMutationRoot {
    async fn update_storage_policy(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateStoragePolicyInput,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .update_storage_policy(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn update_auto_backup_settings(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateAutoBackupSettingsInput,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .update_auto_backup_settings(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn trigger_auto_backup(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlTriggerBackupPayload> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .trigger_backup_now()
            .map(|message| AdminGraphqlTriggerBackupPayload {
                success: true,
                message: Some(message),
            })
            .map_err(async_graphql::Error::new)
    }

    async fn update_system_channel_settings(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateSystemChannelSettingsInput,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .update_system_channel_settings(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn check_provider_quotas(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .run_provider_quota_check_tick(true, Duration::from_secs(20 * 60))
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn trigger_gc_cleanup(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .run_gc_cleanup_now(false, false)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }
}

#[Object]
impl OpenApiGraphqlQueryRoot {
    async fn service_account_project(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<GraphqlProject> {
        let request_context = ctx.data_unchecked::<OpenApiGraphqlRequestContext>();
        Ok(GraphqlProject::from(request_context.owner_api_key.project.clone()))
    }
}

#[Object]
impl OpenApiGraphqlMutationRoot {
    #[graphql(name = "createLLMAPIKey")]
    async fn create_llm_api_key(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> async_graphql::Result<OpenApiGraphqlApiKey> {
        let request_context = ctx.data_unchecked::<OpenApiGraphqlRequestContext>();
        create_llm_api_key(self.foundation.as_ref(), &request_context.owner_api_key, &name)
            .map_err(|error| match error {
                CreateLlmApiKeyError::InvalidName => {
                    async_graphql::Error::new("api key name is required")
                }
                CreateLlmApiKeyError::PermissionDenied => {
                    async_graphql::Error::new("permission denied")
                }
                CreateLlmApiKeyError::Internal(message) => async_graphql::Error::new(message),
            })
    }
}

fn require_admin_graphql_project_id(ctx: &Context<'_>) -> async_graphql::Result<i64> {
    ctx.data_unchecked::<AdminGraphqlRequestContext>()
        .project_id
        .ok_or_else(|| async_graphql::Error::new("project context is required for this query"))
}

fn require_admin_system_scope(ctx: &Context<'_>, scope: &str) -> async_graphql::Result<()> {
    let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
    if admin_user_has_system_scope(&request_context.user, scope) {
        Ok(())
    } else {
        Err(async_graphql::Error::new("permission denied"))
    }
}

fn require_admin_project_scope(
    ctx: &Context<'_>,
    project_id: i64,
    scope: &str,
) -> async_graphql::Result<()> {
    let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
    if admin_user_has_system_scope(&request_context.user, scope)
        || admin_user_has_project_scope(&request_context.user, project_id, scope)
    {
        Ok(())
    } else {
        Err(async_graphql::Error::new("permission denied"))
    }
}

fn admin_user_has_system_scope(user: &AuthUserContext, scope: &str) -> bool {
    user.is_owner
        || user.scopes.iter().any(|current| current == scope)
        || user
            .roles
            .iter()
            .any(|role| role.scopes.iter().any(|current| current == scope))
}

fn admin_user_has_project_scope(user: &AuthUserContext, project_id: i64, scope: &str) -> bool {
    if user.is_owner {
        return true;
    }

    user.projects.iter().any(|project| {
        project.project_id.id == project_id
            && (project.is_owner
                || project.scopes.iter().any(|current| current == scope)
                || project
                    .roles
                    .iter()
                    .any(|role| role.scopes.iter().any(|current| current == scope)))
    })
}

fn create_llm_api_key(
    foundation: &SqliteFoundation,
    owner_api_key: &AuthApiKeyContext,
    name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(CreateLlmApiKeyError::InvalidName);
    }
    if !owner_api_key
        .scopes
        .iter()
        .any(|scope| scope == SCOPE_WRITE_API_KEYS)
    {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let owner_record = foundation
        .identities()
        .find_api_key_by_value(owner_api_key.key.as_str())
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to load owner api key: {error:?}")))?;
    if owner_record.key_type != "service_account" || owner_record.project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let connection = foundation
        .open_connection(true)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to open database: {error}")))?;
    ensure_identity_tables(&connection)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to ensure identity schema: {error}")))?;

    let generated_key: String = connection
        .query_row("SELECT 'ah-' || lower(hex(randomblob(32)))", [], |row| row.get(0))
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to generate api key: {error}")))?;
    let scopes = vec!["read_channels".to_owned(), "write_requests".to_owned()];
    let scopes_json = serde_json::to_string(&scopes)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}")))?;

    connection
        .execute(
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
             VALUES (?1, ?2, ?3, ?4, 'user', 'enabled', ?5, '{}', 0)",
            params![
                owner_record.user_id,
                owner_api_key.project.id,
                generated_key,
                trimmed_name,
                scopes_json,
            ],
        )
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to create api key: {error}")))?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
    })
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Project", rename_fields = "camelCase")]
struct GraphqlProject {
    id: String,
    name: String,
    status: String,
}

impl From<ProjectContext> for GraphqlProject {
    fn from(value: ProjectContext) -> Self {
        Self {
            id: graphql_gid("project", value.id),
            name: value.name,
            status: value.status,
        }
    }
}

impl From<StoredChannelSummary> for AdminGraphqlChannel {
    fn from(value: StoredChannelSummary) -> Self {
        Self {
            id: graphql_gid("channel", value.id),
            name: value.name,
            channel_type: value.channel_type,
            base_url: value.base_url,
            status: value.status,
            supported_models: value.supported_models,
            ordering_weight: i64_to_i32(value.ordering_weight),
            provider_quota_status: None,
        }
    }
}

impl From<StoredModelRecord> for AdminGraphqlModel {
    fn from(value: StoredModelRecord) -> Self {
        let parsed = parse_model_card(value.model_card_json.as_str());
        Self {
            id: graphql_gid("model", value.id),
            developer: value.developer,
            model_id: value.model_id,
            model_type: value.model_type,
            name: value.name,
            icon: value.icon,
            remark: value.remark,
            context_length: parsed.context_length.map(i64_to_i32),
            max_output_tokens: parsed.max_output_tokens.map(i64_to_i32),
        }
    }
}

impl From<StoredRequestSummary> for AdminGraphqlRequestSummaryObject {
    fn from(value: StoredRequestSummary) -> Self {
        Self {
            id: graphql_gid("request", value.id),
            project_id: graphql_gid("project", value.project_id),
            trace_id: value.trace_id.map(|id| graphql_gid("trace", id)),
            channel_id: value.channel_id.map(|id| graphql_gid("channel", id)),
            model_id: value.model_id,
            format: value.format,
            status: value.status,
            source: value.source,
            external_id: value.external_id,
        }
    }
}

impl From<TraceContext> for AdminGraphqlTrace {
    fn from(value: TraceContext) -> Self {
        Self {
            id: graphql_gid("trace", value.id),
            trace_id: value.trace_id,
            project_id: graphql_gid("project", value.project_id),
            thread_id: value.thread_id.map(|id| graphql_gid("thread", id)),
        }
    }
}

impl From<StoredStoragePolicy> for AdminGraphqlStoragePolicy {
    fn from(value: StoredStoragePolicy) -> Self {
        Self {
            store_chunks: value.store_chunks,
            store_request_body: value.store_request_body,
            store_response_body: value.store_response_body,
            cleanup_options: value
                .cleanup_options
                .into_iter()
                .map(|option| AdminGraphqlCleanupOption {
                    resource_type: option.resource_type,
                    enabled: option.enabled,
                    cleanup_days: option.cleanup_days,
                })
                .collect(),
        }
    }
}

impl From<StoredAutoBackupSettings> for AdminGraphqlAutoBackupSettings {
    fn from(value: StoredAutoBackupSettings) -> Self {
        Self {
            enabled: value.enabled,
            frequency: value.frequency,
            data_storage_id: i64_to_i32(value.data_storage_id),
            include_channels: value.include_channels,
            include_models: value.include_models,
            include_api_keys: value.include_api_keys,
            include_model_prices: value.include_model_prices,
            retention_days: value.retention_days,
            last_backup_at: value.last_backup_at.map(format_unix_timestamp),
            last_backup_error: if value.last_backup_error.trim().is_empty() {
                None
            } else {
                Some(value.last_backup_error)
            },
        }
    }
}

impl From<StoredSystemChannelSettings> for AdminGraphqlSystemChannelSettings {
    fn from(value: StoredSystemChannelSettings) -> Self {
        Self {
            probe: AdminGraphqlChannelProbeSetting {
                enabled: value.probe.enabled,
                frequency: value.probe.frequency,
            },
        }
    }
}

impl From<StoredChannelProbeData> for AdminGraphqlChannelProbeData {
    fn from(value: StoredChannelProbeData) -> Self {
        Self {
            channel_id: graphql_gid("channel", value.channel_id),
            points: value
                .points
                .into_iter()
                .map(|point| AdminGraphqlChannelProbePoint {
                    timestamp: point.timestamp,
                    total_request_count: point.total_request_count,
                    success_request_count: point.success_request_count,
                    avg_tokens_per_second: point.avg_tokens_per_second,
                    avg_time_to_first_token_ms: point.avg_time_to_first_token_ms,
                })
                .collect(),
        }
    }
}

impl From<StoredProviderQuotaStatus> for AdminGraphqlProviderQuotaStatus {
    fn from(value: StoredProviderQuotaStatus) -> Self {
        let quota_data = serde_json::from_str::<Value>(value.quota_data_json.as_str()).unwrap_or(Value::Null);
        let message = quota_data
            .get("error")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        Self {
            id: graphql_gid("provider_quota_status", value.id),
            channel_id: graphql_gid("channel", value.channel_id),
            provider_type: value.provider_type,
            status: value.status,
            ready: value.ready,
            next_reset_at: value.next_reset_at.map(format_unix_timestamp),
            next_check_at: format_unix_timestamp(value.next_check_at),
            message,
        }
    }
}

impl SqliteAdminService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        Self { foundation }
    }
}

impl AuthContextPort for SqliteAuthContextService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        let identities = self.foundation.identities();
        let user = identities
            .find_user_by_email(request.email.trim())
            .map_err(|error| match error {
                QueryUserError::NotFound | QueryUserError::InvalidPassword => {
                    SignInError::InvalidCredentials
                }
                QueryUserError::Internal => SignInError::Internal,
            })?;

        if !verify_password(&user.password, &request.password) {
            return Err(SignInError::InvalidCredentials);
        }

        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|_| SignInError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| SignInError::Internal)?;
        ensure_systems_table(&connection).map_err(|_| SignInError::Internal)?;
        let token = generate_jwt_token(&connection, user.id).map_err(|_| SignInError::Internal)?;
        Ok(SignInSuccess {
            user: build_user_context(&connection, user).map_err(|_| SignInError::Internal)?,
            token,
        })
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        let identities = self.foundation.identities();
        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|_| AdminAuthError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| AdminAuthError::Internal)?;
        ensure_systems_table(&connection).map_err(|_| AdminAuthError::Internal)?;
        let secret = query_system_value(&connection, SYSTEM_KEY_SECRET_KEY)
            .map_err(|_| AdminAuthError::Internal)?
            .ok_or(AdminAuthError::InvalidToken)?;

        let decoded = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| AdminAuthError::InvalidToken)?;

        let user =
            identities
                .find_user_by_id(decoded.claims.user_id)
                .map_err(|error| match error {
                    QueryUserError::NotFound | QueryUserError::InvalidPassword => {
                        AdminAuthError::InvalidToken
                    }
                    QueryUserError::Internal => AdminAuthError::Internal,
                })?;
        build_user_context(&connection, user).map_err(|_| AdminAuthError::Internal)
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        let identities = self.foundation.identities();
        let lookup_key = if let Some(value) = key {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed == NO_AUTH_API_KEY_VALUE {
                return Err(ApiKeyAuthError::Invalid);
            }
            trimmed
        } else if allow_no_auth && self.allow_no_auth {
            NO_AUTH_API_KEY_VALUE
        } else {
            return Err(ApiKeyAuthError::Missing);
        };

        let api_key = identities.find_api_key_by_value(lookup_key)?;
        if api_key.status != "enabled" {
            return Err(ApiKeyAuthError::Invalid);
        }
        if api_key.key_type == "noauth" && !(allow_no_auth && self.allow_no_auth) {
            return Err(ApiKeyAuthError::Invalid);
        }

        let project = identities.find_project_by_id(api_key.project_id)?;
        if project.status != "active" {
            return Err(ApiKeyAuthError::Invalid);
        }

        Ok(AuthApiKeyContext {
            id: api_key.id,
            key: api_key.key,
            name: api_key.name,
            key_type: match api_key.key_type.as_str() {
                "service_account" => ApiKeyType::ServiceAccount,
                "noauth" => ApiKeyType::NoAuth,
                _ => ApiKeyType::User,
            },
            project: ProjectContext {
                id: project.id,
                name: project.name,
                status: project.status,
            },
            scopes: api_key.scopes,
        })
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        self.authenticate_api_key(query_key.or(header_key), false)
    }

    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        match self.foundation.identities().find_project_by_id(project_id) {
            Ok(project) if project.status == "active" => Ok(Some(ProjectContext {
                id: project.id,
                name: project.name,
                status: project.status,
            })),
            Ok(_) => Ok(None),
            Err(ApiKeyAuthError::Invalid) => Ok(None),
            Err(ApiKeyAuthError::Internal) | Err(ApiKeyAuthError::Missing) => {
                Err(ContextResolveError::Internal)
            }
        }
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        self.foundation
            .trace_contexts()
            .get_or_create_thread(project_id, thread_id.trim())
            .map(Some)
            .map_err(|_| ContextResolveError::Internal)
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        self.foundation
            .trace_contexts()
            .get_or_create_trace(project_id, trace_id.trim(), thread_db_id)
            .map(Some)
            .map_err(|_| ContextResolveError::Internal)
    }
}

impl AdminPort for SqliteAdminService {
    fn download_request_content(
        &self,
        project_id: i64,
        request_id: i64,
        user: AuthUserContext,
    ) -> Result<AdminContentDownload, AdminError> {
        if !admin_user_has_system_scope(&user, SCOPE_READ_REQUESTS)
            && !admin_user_has_project_scope(&user, project_id, SCOPE_READ_REQUESTS)
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

        let content_storage_id = request.content_storage_id.ok_or_else(|| AdminError::NotFound {
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

        let settings: Value = serde_json::from_str(data_storage.settings_json.as_str()).unwrap_or(Value::Null);
        let base_directory = settings
            .get("directory")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AdminError::NotFound {
                message: "Content not found".to_owned(),
            })?;
        let relative = safe_relative_key_path(normalized_key.as_str()).ok_or_else(|| AdminError::NotFound {
            message: "Content not found".to_owned(),
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
                    supported_generation_methods: vec![
                        "generateContent",
                        "streamGenerateContent",
                    ],
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
        let targets = if matches!(route, CompatibilityRoute::DoubaoGetTask | CompatibilityRoute::DoubaoDeleteTask) {
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
        let task_id = prepared.task_id.as_deref().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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

#[derive(Debug)]
pub struct StoredDataStorage {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub storage_type: String,
    pub status: String,
    pub settings_json: String,
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

#[derive(Debug)]
pub struct StoredUser {
    pub id: i64,
    pub email: String,
    pub status: String,
    pub prefer_language: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub is_owner: bool,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub struct StoredProject {
    pub id: i64,
    pub name: String,
    pub status: String,
}

#[derive(Debug)]
pub struct StoredRole {
    pub id: i64,
    pub name: String,
    pub level: String,
    pub project_id: i64,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub struct StoredApiKey {
    pub id: i64,
    pub user_id: i64,
    pub key: String,
    pub name: String,
    pub key_type: String,
    pub status: String,
    pub project_id: i64,
    pub scopes: Vec<String>,
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

#[derive(Debug)]
pub enum QueryUserError {
    NotFound,
    InvalidPassword,
    Internal,
}

#[derive(Debug, Clone, Default)]
struct ModelInclude {
    all: bool,
    fields: Vec<String>,
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
struct ParsedModelCard {
    context_length: Option<i64>,
    max_output_tokens: Option<i64>,
    capabilities: Option<ModelCapabilities>,
    pricing: Option<ParsedModelPricing>,
}

#[derive(Debug, Clone, Default)]
struct ParsedModelPricing {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    cache_write_5m: Option<f64>,
    cache_write_1h: Option<f64>,
    price_reference_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ExtractedUsage {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    prompt_audio_tokens: i64,
    prompt_cached_tokens: i64,
    prompt_write_cached_tokens: i64,
    prompt_write_cached_tokens_5m: i64,
    prompt_write_cached_tokens_1h: i64,
    completion_audio_tokens: i64,
    completion_reasoning_tokens: i64,
    completion_accepted_prediction_tokens: i64,
    completion_rejected_prediction_tokens: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct StoredCostTier {
    #[serde(rename = "upTo", skip_serializing_if = "Option::is_none")]
    up_to: Option<i64>,
    units: i64,
    subtotal: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct StoredCostItem {
    #[serde(rename = "itemCode")]
    item_code: String,
    #[serde(rename = "promptWriteCacheVariantCode", skip_serializing_if = "Option::is_none")]
    prompt_write_cache_variant_code: Option<String>,
    quantity: i64,
    #[serde(rename = "tierBreakdown", skip_serializing_if = "Vec::is_empty")]
    tier_breakdown: Vec<StoredCostTier>,
    subtotal: f64,
}

#[derive(Debug, Clone, Default)]
struct ComputedUsageCost {
    total_cost: Option<f64>,
    cost_items: Vec<StoredCostItem>,
    price_reference_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    user_id: i64,
    exp: usize,
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
            if self.routing_stats.last_status_failed { 1 } else { 0 },
            self.routing_stats.consecutive_failures,
        )
    }
}

pub(crate) fn open_sqlite_connection(
    dsn: &str,
    create_if_missing: bool,
) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(dsn, sqlite_open_flags(dsn, create_if_missing))
}

fn sqlite_open_flags(dsn: &str, create_if_missing: bool) -> OpenFlags {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
    if create_if_missing {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    if dsn.starts_with("file:") {
        flags |= OpenFlags::SQLITE_OPEN_URI;
    }

    flags
}

fn validate_openai_request(route: OpenAiV1Route, body: &Value) -> Result<(), OpenAiV1Error> {
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

fn rewrite_model(body: &Value, actual_model_id: &str) -> Value {
    let mut rewritten = body.clone();
    if let Some(object) = rewritten.as_object_mut() {
        object.insert(
            "model".to_owned(),
            Value::String(actual_model_id.to_owned()),
        );
    }
    rewritten
}

fn sanitize_headers_json(headers: &HashMap<String, String>) -> String {
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

fn build_upstream_headers(
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
    headers.insert(reqwest::header::ACCEPT, HeaderValue::from_static("application/json"));

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

fn json_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| value.get(*key))
}

fn json_i64_field(value: &Value, keys: &[&str]) -> Option<i64> {
    json_field(value, keys).and_then(Value::as_i64)
}

fn json_f64_field(value: &Value, keys: &[&str]) -> Option<f64> {
    json_field(value, keys).and_then(Value::as_f64)
}

fn json_bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    json_field(value, keys).and_then(Value::as_bool)
}

fn json_string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    json_field(value, keys).and_then(Value::as_str)
}

fn extract_usage(route: OpenAiV1Route, response_body: &Value) -> Option<ExtractedUsage> {
    let usage = response_body.get("usage")?;
    match route {
        OpenAiV1Route::Responses => {
            let empty = Value::Null;
            let prompt_details = json_field(usage, &["input_tokens_details"]).unwrap_or(&empty);
            let completion_details =
                json_field(usage, &["output_tokens_details"]).unwrap_or(&empty);
            let prompt_write_cached_tokens_5m =
                json_i64_field(prompt_details, &["write_cached_5min_tokens", "write_cached_5m_tokens"])
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
                prompt_cached_tokens: json_i64_field(prompt_details, &["cached_tokens"]).unwrap_or(0),
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
            let prompt_write_cached_tokens_5m =
                json_i64_field(prompt_details, &["write_cached_5min_tokens", "write_cached_5m_tokens"])
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
                prompt_cached_tokens: json_i64_field(prompt_details, &["cached_tokens"]).unwrap_or(0),
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

fn extract_jina_usage(response_body: &Value) -> Option<ExtractedUsage> {
    let usage = response_body.get("usage")?;
    Some(ExtractedUsage {
        prompt_tokens: json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0),
        total_tokens: json_i64_field(usage, &["total_tokens"]).unwrap_or(0),
        ..ExtractedUsage::default()
    })
}

fn compute_usage_cost(model: &StoredModelRecord, usage: &ExtractedUsage) -> ComputedUsageCost {
    let card = parse_model_card(model.model_card_json.as_str());
    let Some(pricing) = card.pricing else {
        return ComputedUsageCost::default();
    };

    let mut cost_items = Vec::new();
    let mut total_cost = 0.0;
    let prompt_tokens = (usage.prompt_tokens
        - usage.prompt_cached_tokens
        - usage.prompt_write_cached_tokens)
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
        let subtotal = (usage.prompt_write_cached_tokens as f64 / 1_000_000.0) * pricing.cache_write;
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
            pricing.price_reference_id.unwrap_or_else(|| {
                format!("sqlite:model:{}:{}", model.developer, model.model_id)
            })
        ),
    }
}

fn extract_error_message(body: &Value) -> String {
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

fn openai_error_message(error: &OpenAiV1Error) -> String {
    match error {
        OpenAiV1Error::InvalidRequest { message } | OpenAiV1Error::Internal { message } => {
            message.clone()
        }
        OpenAiV1Error::Upstream { body, .. } => extract_error_message(body),
    }
}

#[derive(Debug, Clone, Copy)]
enum RouteSelector {
    Compatibility(CompatibilityRoute),
}

#[derive(Debug, Clone)]
struct PreparedCompatibilityRequest {
    request_model_id: String,
    channel_type: &'static str,
    model_type: &'static str,
    upstream_body: Value,
    task_id: Option<String>,
}

fn route_model_type(route: RouteSelector) -> &'static str {
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

fn prepare_compatibility_request(
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

fn prepare_anthropic_request(body: &Value) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let object = body.as_object().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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
        model_type: route_model_type(RouteSelector::Compatibility(CompatibilityRoute::AnthropicMessages)),
        upstream_body: Value::Object(upstream),
        task_id: None,
    })
}

fn convert_anthropic_system_to_openai(system: &Value) -> Result<Option<Value>, OpenAiV1Error> {
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

fn convert_anthropic_message_to_openai(message: &Value) -> Result<Value, OpenAiV1Error> {
    let object = message.as_object().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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

fn convert_anthropic_content_parts(parts: &[Value]) -> Result<Value, OpenAiV1Error> {
    let mut converted = Vec::new();
    for part in parts {
        let object = part.as_object().ok_or_else(|| OpenAiV1Error::InvalidRequest {
            message: "message content block must be an object".to_owned(),
        })?;
        let part_type = object.get("type").and_then(Value::as_str).unwrap_or_default();
        match part_type {
            "text" => {
                let text = object
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| OpenAiV1Error::InvalidRequest {
                        message: "text content block requires text".to_owned(),
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
                    Some("url") => source.get("url").and_then(Value::as_str).map(ToOwned::to_owned),
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

fn prepare_jina_rerank_request(body: &Value) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let object = body.as_object().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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

fn prepare_jina_embedding_request(body: &Value) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let mut object = body.as_object().cloned().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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
    let input = object.get("input").ok_or_else(|| OpenAiV1Error::InvalidRequest {
        message: "input is required".to_owned(),
    })?;
    validate_embedding_input(input)?;
    if !object.contains_key("task") {
        object.insert("task".to_owned(), Value::String("text-matching".to_owned()));
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model,
        channel_type: "jina",
        model_type: route_model_type(RouteSelector::Compatibility(CompatibilityRoute::JinaEmbeddings)),
        upstream_body: Value::Object(object),
        task_id: None,
    })
}

fn validate_embedding_input(input: &Value) -> Result<(), OpenAiV1Error> {
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
                            message: "input must be a string, token array, or array of inputs".to_owned(),
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

fn compatibility_upstream_url(
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

fn compatibility_upstream_method(route: CompatibilityRoute) -> reqwest::Method {
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

fn map_compatibility_response(
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

fn compatibility_usage(route: CompatibilityRoute, response_body: &Value) -> Option<ExtractedUsage> {
    match route {
        CompatibilityRoute::AnthropicMessages => extract_usage(OpenAiV1Route::ChatCompletions, response_body),
        CompatibilityRoute::JinaRerank | CompatibilityRoute::JinaEmbeddings => {
            extract_jina_usage(response_body)
        }
        CompatibilityRoute::GeminiGenerateContent
        | CompatibilityRoute::GeminiStreamGenerateContent => extract_usage(OpenAiV1Route::ChatCompletions, response_body),
        CompatibilityRoute::DoubaoCreateTask
        | CompatibilityRoute::DoubaoGetTask
        | CompatibilityRoute::DoubaoDeleteTask => None,
    }
}

fn prepare_gemini_request(
    route: CompatibilityRoute,
    request: &OpenAiV1ExecutionRequest,
) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let body = &request.body;
    let object = body.as_object().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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
        copy_json_field_as(generation_config, &mut upstream, "maxOutputTokens", "max_tokens");
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model.to_owned(),
        channel_type: "openai",
        model_type: route_model_type(RouteSelector::Compatibility(route)),
        upstream_body: Value::Object(upstream),
        task_id: None,
    })
}

fn prepare_doubao_create_request(body: &Value) -> Result<PreparedCompatibilityRequest, OpenAiV1Error> {
    let object = body.as_object().ok_or_else(|| OpenAiV1Error::InvalidRequest {
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
    upstream.insert(
        "prompt".to_owned(),
        Value::String(prompt.to_owned()),
    );
    if let Some(duration) = object.get("duration") {
        upstream.insert("duration".to_owned(), duration.clone());
    }

    Ok(PreparedCompatibilityRequest {
        request_model_id: model.to_owned(),
        channel_type: "openai",
        model_type: route_model_type(RouteSelector::Compatibility(CompatibilityRoute::DoubaoCreateTask)),
        upstream_body: Value::Object(upstream),
        task_id: None,
    })
}

fn prepare_doubao_task_lookup_request(
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

fn extract_gemini_model_from_path(path: &str) -> Option<&str> {
    let marker = "/models/";
    let after = path.split(marker).nth(1)?;
    let model = after.split(':').next()?.trim();
    (!model.is_empty()).then_some(model)
}

fn extract_task_id_from_path(path: &str) -> Option<String> {
    path.rsplit('/').next()
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && *segment != "tasks")
        .map(ToOwned::to_owned)
}

fn flatten_gemini_parts(content: &Value) -> Option<String> {
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

fn copy_json_field(
    source: &serde_json::Map<String, Value>,
    target: &mut serde_json::Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

fn copy_json_field_as(
    source: &serde_json::Map<String, Value>,
    target: &mut serde_json::Map<String, Value>,
    source_key: &str,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key) {
        target.insert(target_key.to_owned(), value.clone());
    }
}

fn map_gemini_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let object = response_body.as_object().ok_or_else(|| OpenAiV1Error::Internal {
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

fn map_doubao_create_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let id = response_body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(serde_json::json!({"id": id}))
}

fn map_doubao_get_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
    let object = response_body.as_object().ok_or_else(|| OpenAiV1Error::Internal {
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

fn map_openai_finish_reason_to_gemini(reason: &str) -> &'static str {
    match reason {
        "stop" => "STOP",
        "length" => "MAX_TOKENS",
        "tool_calls" => "STOP",
        _ => "STOP",
    }
}

fn map_gemini_usage_from_openai(usage: Option<&Value>) -> Value {
    let Some(usage) = usage else {
        return Value::Null;
    };

    let prompt_tokens = json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0);
    let completion_tokens = json_i64_field(usage, &["completion_tokens"]).unwrap_or(0);
    let total_tokens = json_i64_field(usage, &["total_tokens"]).unwrap_or(prompt_tokens + completion_tokens);
    let prompt_details = usage.get("prompt_tokens_details").cloned().unwrap_or(Value::Null);
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

fn map_anthropic_response(response_body: Value) -> Result<Value, OpenAiV1Error> {
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

fn map_anthropic_response_content(content: Option<&Value>) -> Result<Vec<Value>, OpenAiV1Error> {
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

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (metadata, data) = rest.split_once(',')?;
    let media_type = metadata.strip_suffix(";base64")?;
    Some((media_type.to_owned(), data.to_owned()))
}

fn map_openai_finish_reason_to_anthropic(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_owned(),
        "length" => "max_tokens".to_owned(),
        "tool_calls" => "tool_use".to_owned(),
        other => other.to_owned(),
    }
}

fn map_anthropic_usage_from_openai(usage: &Value) -> Result<Value, OpenAiV1Error> {
    let prompt_tokens = json_i64_field(usage, &["prompt_tokens"]).unwrap_or(0);
    let completion_tokens = json_i64_field(usage, &["completion_tokens"]).unwrap_or(0);
    let prompt_details = usage.get("prompt_tokens_details").cloned().unwrap_or(Value::Null);
    let cached_tokens = json_i64_field(&prompt_details, &["cached_tokens"]).unwrap_or(0);
    let write_cached_tokens = json_i64_field(&prompt_details, &["write_cached_tokens"]).unwrap_or(0);
    let write_cached_5m =
        json_i64_field(&prompt_details, &["write_cached_5min_tokens", "write_cached_5m_tokens"])
            .unwrap_or(0);
    let write_cached_1h =
        json_i64_field(&prompt_details, &["write_cached_1hour_tokens", "write_cached_1h_tokens"])
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

fn sqlite_timestamp_to_rfc3339(raw: &str) -> String {
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

fn parse_model_card(raw: &str) -> ParsedModelCard {
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
                &["price_reference_id", "priceReferenceId", "reference_id", "referenceId"],
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

fn parse_created_at_to_unix(raw: &str) -> i64 {
    let _ = raw;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn list_enabled_model_records(connection: &Connection) -> rusqlite::Result<Vec<StoredModelRecord>> {
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

fn model_supported_by_channel(supported_models_json: &str, model_id: &str) -> bool {
    serde_json::from_str::<Vec<String>>(supported_models_json)
        .unwrap_or_default()
        .iter()
        .any(|current| current == model_id)
}

fn calculate_top_k(candidate_count: usize, max_channel_retries: usize) -> usize {
    candidate_count.min(1 + max_channel_retries)
}

fn compare_openai_target_priority(
    left: &SelectedOpenAiTarget,
    right: &SelectedOpenAiTarget,
) -> std::cmp::Ordering {
    left.base_routing_priority_key()
        .cmp(&right.base_routing_priority_key())
        .then_with(|| left.routing_stats.processing_count.cmp(&right.routing_stats.processing_count))
        .then_with(|| compare_selection_pressure(left, right))
        .then_with(|| right.ordering_weight.cmp(&left.ordering_weight))
        .then_with(|| left.channel_id.cmp(&right.channel_id))
}

fn compare_selection_pressure(
    left: &SelectedOpenAiTarget,
    right: &SelectedOpenAiTarget,
) -> std::cmp::Ordering {
    let left_weight = std::cmp::max(left.ordering_weight, 1) as i128;
    let right_weight = std::cmp::max(right.ordering_weight, 1) as i128;
    let left_selection = left.routing_stats.selection_count as i128;
    let right_selection = right.routing_stats.selection_count as i128;

    (left_selection * right_weight)
        .cmp(&(right_selection * left_weight))
        .then_with(|| left.routing_stats.selection_count.cmp(&right.routing_stats.selection_count))
}

fn query_preferred_trace_channel_id(
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

fn query_channel_routing_stats(
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

fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn current_rfc3339_timestamp() -> String {
    let now = current_unix_timestamp();
    format_unix_timestamp(now)
}

fn format_unix_timestamp(timestamp: i64) -> String {
    let system_time = UNIX_EPOCH
        .checked_add(Duration::from_secs(timestamp.max(0) as u64))
        .unwrap_or(UNIX_EPOCH);
    humantime::format_rfc3339_seconds(system_time).to_string()
}

fn default_storage_policy() -> StoredStoragePolicy {
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

fn default_auto_backup_settings() -> StoredAutoBackupSettings {
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

fn default_system_channel_settings() -> StoredSystemChannelSettings {
    StoredSystemChannelSettings {
        probe: StoredChannelProbeSettings {
            enabled: true,
            frequency: ProbeFrequencySetting::FiveMinutes,
        },
    }
}

fn load_json_setting<T: DeserializeOwned>(
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

fn generate_probe_timestamps(interval_minutes: i32, now_timestamp: i64) -> Vec<i64> {
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

fn collect_channel_probe_stats(
    connection: &Connection,
    channel_id: i64,
    start_timestamp: i64,
    end_timestamp: i64,
) -> rusqlite::Result<ProbeComputation> {
    let mut statement = connection.prepare(
        "SELECT COUNT(*),
                SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END),
                SUM(COALESCE((SELECT total_tokens FROM usage_logs WHERE request_id = requests.id ORDER BY id DESC LIMIT 1), 0)),
                SUM(COALESCE(metrics_latency_ms, 0)),
                SUM(COALESCE(metrics_first_token_latency_ms, 0)),
                SUM(CASE WHEN stream != 0 THEN 1 ELSE 0 END)
         FROM requests
         WHERE channel_id = ?1 AND created_at >= datetime(?2, 'unixepoch') AND created_at < datetime(?3, 'unixepoch')",
    )?;
    statement.query_row(params![channel_id, start_timestamp, end_timestamp], |row| {
        let total: i64 = row.get(0)?;
        let success: i64 = row.get(1)?;
        let total_tokens: i64 = row.get(2)?;
        let latency_ms: i64 = row.get(3)?;
        let first_token_latency_ms: i64 = row.get(4)?;
        let streaming: i64 = row.get(5)?;
        let avg_tokens_per_second = if total_tokens > 0 && latency_ms > 0 {
            Some(total_tokens as f64 / (latency_ms as f64 / 1000.0))
        } else {
            None
        };
        let avg_time_to_first_token_ms = if streaming > 0 && first_token_latency_ms > 0 {
            Some(first_token_latency_ms as f64 / streaming as f64)
        } else {
            None
        };
        Ok(ProbeComputation {
            total_request_count: total as i32,
            success_request_count: success as i32,
            avg_tokens_per_second,
            avg_time_to_first_token_ms,
        })
    })
}

fn upsert_channel_probe_point(
    connection: &Connection,
    channel_id: i64,
    timestamp: i64,
    stats: &ProbeComputation,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO channel_probes (channel_id, timestamp, total_request_count, success_request_count, avg_tokens_per_second, avg_time_to_first_token_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(channel_id, timestamp) DO UPDATE SET
             total_request_count = excluded.total_request_count,
             success_request_count = excluded.success_request_count,
             avg_tokens_per_second = excluded.avg_tokens_per_second,
             avg_time_to_first_token_ms = excluded.avg_time_to_first_token_ms,
             updated_at = CURRENT_TIMESTAMP",
        params![
            channel_id,
            timestamp,
            stats.total_request_count,
            stats.success_request_count,
            stats.avg_tokens_per_second,
            stats.avg_time_to_first_token_ms,
        ],
    )?;
    Ok(())
}

fn provider_quota_type_for_channel(channel_type: &str) -> Option<&'static str> {
    match channel_type {
        "claudecode" => Some("claudecode"),
        "codex" => Some("codex"),
        _ => None,
    }
}

fn quota_check_is_due(connection: &Connection, channel_id: i64, now: i64) -> rusqlite::Result<bool> {
    let next_check_at: Option<i64> = connection
        .query_row(
            "SELECT next_check_at FROM provider_quota_statuses WHERE channel_id = ?1 LIMIT 1",
            [channel_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(next_check_at.is_none_or(|value| value <= now))
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

fn cleanup_request_executions(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
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
) -> rusqlite::Result<i64> {
    let mut statement = connection.prepare(
        "SELECT content_storage_id, content_storage_key FROM requests WHERE created_at < datetime(?1, 'unixepoch') AND content_storage_id IS NOT NULL AND content_storage_key IS NOT NULL",
    )?;
    let rows = statement.query_map([cutoff], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
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

fn cleanup_threads(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM threads WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

fn cleanup_traces(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM traces WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

fn cleanup_usage_logs(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute(
        "DELETE FROM usage_logs WHERE created_at < datetime(?1, 'unixepoch')",
        [cutoff],
    )?;
    Ok(connection.changes() as i64)
}

fn cleanup_channel_probes(connection: &Connection, cutoff: i64) -> rusqlite::Result<i64> {
    connection.execute("DELETE FROM channel_probes WHERE timestamp < ?1", [cutoff])?;
    Ok(connection.changes() as i64)
}

fn list_backup_channels(connection: &Connection) -> rusqlite::Result<Vec<StoredBackupChannel>> {
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
            credentials: serde_json::from_str::<Value>(row.get::<_, String>(5)?.as_str()).unwrap_or(Value::Null),
            supported_models: serde_json::from_str::<Value>(row.get::<_, String>(6)?.as_str()).unwrap_or(Value::Null),
            default_test_model: row.get(7)?,
            settings: serde_json::from_str::<Value>(row.get::<_, String>(8)?.as_str()).unwrap_or(Value::Null),
            tags: serde_json::from_str::<Value>(row.get::<_, String>(9)?.as_str()).unwrap_or(Value::Null),
            ordering_weight: row.get(10)?,
            error_message: row.get(11)?,
            remark: row.get(12)?,
        })
    })?;
    rows.collect()
}

fn list_backup_models(connection: &Connection) -> rusqlite::Result<Vec<StoredBackupModel>> {
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
            model_card: serde_json::from_str::<Value>(row.get::<_, String>(7)?.as_str()).unwrap_or(Value::Null),
            settings: serde_json::from_str::<Value>(row.get::<_, String>(8)?.as_str()).unwrap_or(Value::Null),
            status: row.get(9)?,
            remark: row.get(10)?,
        })
    })?;
    rows.collect()
}

fn list_backup_api_keys(connection: &Connection) -> rusqlite::Result<Vec<StoredBackupApiKey>> {
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
            scopes: serde_json::from_str::<Value>(row.get::<_, String>(7)?.as_str()).unwrap_or(Value::Null),
        })
    })?;
    rows.collect()
}

fn parse_graphql_resource_id(value: &str, expected_type: &str) -> Result<i64, String> {
    let trimmed = value.trim();
    let prefix = format!("gid://axonhub/{expected_type}/");
    trimmed
        .strip_prefix(prefix.as_str())
        .ok_or_else(|| format!("invalid {expected_type} id"))?
        .parse::<i64>()
        .map_err(|_| format!("invalid {expected_type} id"))
}

fn extract_channel_api_key(credentials_json: &str) -> String {
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

fn ensure_all_foundation_tables(connection: &Connection) -> rusqlite::Result<()> {
    ensure_systems_table(connection)?;
    connection.execute_batch(DATA_STORAGES_TABLE_SQL)?;
    ensure_identity_tables(connection)?;
    ensure_trace_tables(connection)?;
    ensure_channel_model_tables(connection)?;
    ensure_request_tables(connection)?;
    connection.execute_batch(USAGE_LOGS_TABLE_SQL)
}

fn ensure_systems_table(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(SYSTEMS_TABLE_SQL)
}

fn ensure_identity_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(USERS_TABLE_SQL)?;
    connection.execute_batch(PROJECTS_TABLE_SQL)?;
    connection.execute_batch(USER_PROJECTS_TABLE_SQL)?;
    connection.execute_batch(ROLES_TABLE_SQL)?;
    connection.execute_batch(USER_ROLES_TABLE_SQL)?;
    connection.execute_batch(API_KEYS_TABLE_SQL)
}

fn ensure_trace_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(THREADS_TABLE_SQL)?;
    connection.execute_batch(TRACES_TABLE_SQL)
}

fn ensure_channel_model_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(CHANNELS_TABLE_SQL)?;
    connection.execute_batch(MODELS_TABLE_SQL)
}

fn ensure_request_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(REQUESTS_TABLE_SQL)?;
    connection.execute_batch(REQUEST_EXECUTIONS_TABLE_SQL)
}

fn ensure_operational_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(CHANNEL_PROBES_TABLE_SQL)?;
    connection.execute_batch(PROVIDER_QUOTA_STATUSES_TABLE_SQL)
}

fn upsert_system_value_on_connection(
    connection: &Connection,
    key: &str,
    value: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO systems (key, value, deleted_at) VALUES (?1, ?2, 0)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )?;
    Ok(())
}

fn query_is_initialized(connection: &Connection) -> rusqlite::Result<bool> {
    let value: Option<String> = connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
            [SYSTEM_KEY_INITIALIZED],
            |row| row.get(0),
        )
        .optional()?;

    Ok(value
        .map(|current| current.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

fn ensure_primary_data_storage(transaction: &Transaction<'_>) -> rusqlite::Result<i64> {
    let existing: Option<i64> = transaction
        .query_row(
            "SELECT id FROM data_storages WHERE \"primary\" = 1 AND deleted_at = 0 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    transaction.execute(
        "INSERT INTO data_storages (name, description, \"primary\", type, settings, status) VALUES (?1, ?2, 1, 'database', ?3, 'active')",
        params![
            PRIMARY_DATA_STORAGE_NAME,
            PRIMARY_DATA_STORAGE_DESCRIPTION,
            PRIMARY_DATA_STORAGE_SETTINGS_JSON,
        ],
    )?;

    Ok(transaction.last_insert_rowid())
}

fn generate_secret_key(transaction: &Transaction<'_>) -> rusqlite::Result<String> {
    transaction.query_row("SELECT lower(hex(randomblob(32)))", [], |row| row.get(0))
}

fn hash_password(password: &str) -> rusqlite::Result<String> {
    hash(password, DEFAULT_COST)
        .map(|hashed| hex_encode(hashed.as_bytes()))
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn verify_password(stored_hex: &str, password: &str) -> bool {
    hex::decode(stored_hex)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|hash| verify(password, &hash).ok())
        .unwrap_or(false)
}

fn ensure_owner_user(
    transaction: &Transaction<'_>,
    request: &InitializeSystemRequest,
) -> rusqlite::Result<i64> {
    let existing: Option<i64> = transaction
        .query_row(
            "SELECT id FROM users WHERE is_owner = 1 AND deleted_at = 0 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let password_hash = hash_password(request.owner_password.trim())?;
    transaction.execute(
        "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at)
         VALUES (?1, 'activated', 'en', ?2, ?3, ?4, '', 1, '[]', 0)",
        params![
            request.owner_email.trim(),
            password_hash,
            request.owner_first_name.trim(),
            request.owner_last_name.trim(),
        ],
    )?;

    Ok(transaction.last_insert_rowid())
}

fn ensure_default_project(transaction: &Transaction<'_>) -> rusqlite::Result<i64> {
    let existing: Option<i64> = transaction
        .query_row(
            "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    transaction.execute(
        "INSERT INTO projects (name, description, status, deleted_at) VALUES (?1, ?2, 'active', 0)",
        params![DEFAULT_PROJECT_NAME, DEFAULT_PROJECT_DESCRIPTION],
    )?;

    Ok(transaction.last_insert_rowid())
}

fn ensure_owner_project_membership(
    transaction: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO user_projects (user_id, project_id, is_owner, scopes)
         VALUES (?1, ?2, 1, '[]')
         ON CONFLICT(user_id, project_id) DO UPDATE SET is_owner = 1, updated_at = CURRENT_TIMESTAMP",
        params![user_id, project_id],
    )?;

    Ok(())
}

fn ensure_default_project_roles(
    transaction: &Transaction<'_>,
    project_id: i64,
) -> rusqlite::Result<()> {
    ensure_role_with_scopes(
        transaction,
        "Admin",
        ROLE_LEVEL_PROJECT,
        project_id,
        &[
            "read_users",
            "write_users",
            "read_roles",
            "write_roles",
            "read_api_keys",
            "write_api_keys",
            "read_requests",
            "write_requests",
        ],
    )?;
    ensure_role_with_scopes(
        transaction,
        "Developer",
        ROLE_LEVEL_PROJECT,
        project_id,
        &["read_users", "read_api_keys", "write_api_keys", "read_requests"],
    )?;
    ensure_role_with_scopes(
        transaction,
        "Viewer",
        ROLE_LEVEL_PROJECT,
        project_id,
        &["read_users", "read_requests"],
    )?;

    Ok(())
}

fn ensure_role_with_scopes(
    transaction: &Transaction<'_>,
    name: &str,
    level: &str,
    project_id: i64,
    scopes: &[&str],
) -> rusqlite::Result<()> {
    let scopes_json = serde_json::to_string(scopes)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO roles (name, level, project_id, scopes, deleted_at)
         VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT(project_id, name) DO UPDATE SET
             level = excluded.level,
             scopes = excluded.scopes,
             deleted_at = 0,
             updated_at = CURRENT_TIMESTAMP",
        params![name, level, project_id, scopes_json],
    )?;

    Ok(())
}

fn ensure_default_api_keys(
    transaction: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
) -> rusqlite::Result<()> {
    ensure_api_key_with_scopes(
        transaction,
        user_id,
        project_id,
        DEFAULT_USER_API_KEY_VALUE,
        DEFAULT_USER_API_KEY_NAME,
        "user",
        &["read_channels", "write_requests"],
    )?;
    ensure_api_key_with_scopes(
        transaction,
        user_id,
        project_id,
        DEFAULT_SERVICE_API_KEY_VALUE,
        DEFAULT_SERVICE_API_KEY_NAME,
        "service_account",
        &["read_channels", "write_requests", "write_api_keys"],
    )?;
    ensure_api_key_with_scopes(
        transaction,
        user_id,
        project_id,
        NO_AUTH_API_KEY_VALUE,
        NO_AUTH_API_KEY_NAME,
        "noauth",
        &["read_channels", "write_requests"],
    )?;

    Ok(())
}

fn ensure_api_key(
    transaction: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
) -> rusqlite::Result<()> {
    ensure_api_key_with_scopes(
        transaction,
        user_id,
        project_id,
        key,
        name,
        key_type,
        &["read_channels", "write_requests"],
    )
}

fn ensure_api_key_with_scopes(
    transaction: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    scopes: &[&str],
) -> rusqlite::Result<()> {
    let scopes_json = serde_json::to_string(scopes)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', ?6, '{}', 0)
         ON CONFLICT(key) DO UPDATE SET name = excluded.name, type = excluded.type, scopes = excluded.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        params![user_id, project_id, key, name, key_type, scopes_json],
    )?;

    Ok(())
}

fn query_user_by_email(connection: &Connection, email: &str) -> Result<StoredUser, QueryUserError> {
    connection
        .query_row(
            "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
             FROM users WHERE email = ?1 AND deleted_at = 0 LIMIT 1",
            [email],
            |row| {
                Ok(StoredUser {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    status: row.get(2)?,
                    prefer_language: row.get(3)?,
                    password: row.get(4)?,
                    first_name: row.get(5)?,
                    last_name: row.get(6)?,
                    avatar: row.get(7)?,
                    is_owner: row.get::<_, i64>(8)? != 0,
                    scopes: parse_json_string_vec(row.get::<_, String>(9)?),
                })
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => QueryUserError::NotFound,
            _ => QueryUserError::Internal,
        })
        .and_then(|user| {
            if user.status != "activated" {
                Err(QueryUserError::InvalidPassword)
            } else {
                Ok(user)
            }
        })
}

fn query_user_by_id(connection: &Connection, user_id: i64) -> Result<StoredUser, QueryUserError> {
    connection
        .query_row(
            "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
             FROM users WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
            [user_id],
            |row| {
                Ok(StoredUser {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    status: row.get(2)?,
                    prefer_language: row.get(3)?,
                    password: row.get(4)?,
                    first_name: row.get(5)?,
                    last_name: row.get(6)?,
                    avatar: row.get(7)?,
                    is_owner: row.get::<_, i64>(8)? != 0,
                    scopes: parse_json_string_vec(row.get::<_, String>(9)?),
                })
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => QueryUserError::NotFound,
            _ => QueryUserError::Internal,
        })
        .and_then(|user| {
            if user.status != "activated" {
                Err(QueryUserError::InvalidPassword)
            } else {
                Ok(user)
            }
        })
}

fn query_default_project_for_user(
    connection: &Connection,
    user_id: i64,
) -> rusqlite::Result<StoredProject> {
    connection.query_row(
        "SELECT p.id, p.name, p.status
         FROM projects p
         JOIN user_projects up ON up.project_id = p.id
         WHERE up.user_id = ?1 AND p.deleted_at = 0
         ORDER BY p.id ASC
         LIMIT 1",
        [user_id],
        |row| {
            Ok(StoredProject {
                id: row.get(0)?,
                name: row.get(1)?,
                status: row.get(2)?,
            })
        },
    )
}

fn query_project(
    connection: &Connection,
    project_id: i64,
) -> Result<StoredProject, ApiKeyAuthError> {
    connection
        .query_row(
            "SELECT id, name, status FROM projects WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
            [project_id],
            |row| {
                Ok(StoredProject {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    status: row.get(2)?,
                })
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => ApiKeyAuthError::Invalid,
            _ => ApiKeyAuthError::Internal,
        })
}

fn query_api_key(connection: &Connection, key: &str) -> Result<StoredApiKey, ApiKeyAuthError> {
    connection
        .query_row(
            "SELECT id, user_id, key, name, type, status, project_id, scopes
             FROM api_keys WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
            [key],
            |row| {
                Ok(StoredApiKey {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    key: row.get(2)?,
                    name: row.get(3)?,
                    key_type: row.get(4)?,
                    status: row.get(5)?,
                    project_id: row.get(6)?,
                    scopes: parse_json_string_vec(row.get::<_, String>(7)?),
                })
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => ApiKeyAuthError::Invalid,
            _ => ApiKeyAuthError::Internal,
        })
}

fn query_user_roles(connection: &Connection, user_id: i64) -> rusqlite::Result<Vec<StoredRole>> {
    let mut statement = connection.prepare(
        "SELECT r.id, r.name, r.level, r.project_id, r.scopes
         FROM roles r
         JOIN user_roles ur ON ur.role_id = r.id
         WHERE ur.user_id = ?1 AND r.deleted_at = 0
         ORDER BY r.id ASC",
    )?;
    let rows = statement.query_map([user_id], |row| {
        Ok(StoredRole {
            id: row.get(0)?,
            name: row.get(1)?,
            level: row.get(2)?,
            project_id: row.get(3)?,
            scopes: parse_json_string_vec(row.get::<_, String>(4)?),
        })
    })?;
    rows.collect()
}

fn build_user_context(
    connection: &Connection,
    user: StoredUser,
) -> rusqlite::Result<AuthUserContext> {
    let roles = query_user_roles(connection, user.id)?;

    let system_roles = roles
        .iter()
        .filter(|role| role.project_id == SYSTEM_ROLE_PROJECT_ID || role.level == ROLE_LEVEL_SYSTEM)
        .map(|role| RoleInfo {
            name: role.name.clone(),
            scopes: role.scopes.clone(),
        })
        .collect::<Vec<_>>();

    let mut all_scopes = user.scopes.clone();
    for role in &roles {
        if role.project_id == SYSTEM_ROLE_PROJECT_ID || role.level == ROLE_LEVEL_SYSTEM {
            for scope in &role.scopes {
                if !all_scopes.iter().any(|current| current == scope) {
                    all_scopes.push(scope.clone());
                }
            }
        }
    }

    let mut statement = connection.prepare(
        "SELECT project_id, is_owner, scopes FROM user_projects WHERE user_id = ?1 ORDER BY project_id ASC",
    )?;
    let rows = statement.query_map([user.id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)? != 0,
            parse_json_string_vec(row.get::<_, String>(2)?),
        ))
    })?;
    let memberships = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    let projects = memberships
        .into_iter()
        .map(|(project_id, is_owner, scopes)| {
            let project = query_project(connection, project_id).map_err(|error| match error {
                ApiKeyAuthError::Internal => rusqlite::Error::InvalidQuery,
                _ => rusqlite::Error::QueryReturnedNoRows,
            })?;
            let project_roles = roles
                .iter()
                .filter(|role| role.project_id == project_id && role.level == ROLE_LEVEL_PROJECT)
                .map(|role| RoleInfo {
                    name: role.name.clone(),
                    scopes: role.scopes.clone(),
                })
                .collect::<Vec<_>>();

            Ok(UserProjectInfo {
                project_id: GlobalId {
                    resource_type: "project".to_owned(),
                    id: project.id,
                },
                is_owner,
                scopes,
                roles: project_roles,
            })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(AuthUserContext {
        id: user.id,
        email: user.email,
        first_name: user.first_name,
        last_name: user.last_name,
        is_owner: user.is_owner,
        prefer_language: user.prefer_language,
        avatar: Some(user.avatar),
        scopes: all_scopes,
        roles: system_roles,
        projects,
    })
}

fn generate_jwt_token(connection: &Connection, user_id: i64) -> rusqlite::Result<String> {
    let secret = query_system_value(connection, SYSTEM_KEY_SECRET_KEY)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    let claims = JwtClaims {
        user_id,
        exp: (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 60 * 60 * 24 * 7) as usize,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn safe_relative_key_path(key: &str) -> Option<PathBuf> {
    let trimmed = key.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let path = Path::new(trimmed);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return None;
    }

    Some(path.to_path_buf())
}

fn filename_from_key(key: &str, request_id: i64) -> String {
    Path::new(key)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("request-{request_id}-content"))
}

fn query_system_value(connection: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
            [key],
            |row| row.get(0),
        )
        .optional()
}

fn parse_json_string_vec(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn get_or_create_thread(
    connection: &Connection,
    project_id: i64,
    thread_id: &str,
) -> rusqlite::Result<ThreadContext> {
    let existing = connection
        .query_row(
            "SELECT id, thread_id, project_id FROM threads WHERE thread_id = ?1 LIMIT 1",
            [thread_id],
            |row| {
                Ok(ThreadContext {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    project_id: row.get(2)?,
                })
            },
        )
        .optional()?;

    if let Some(thread) = existing {
        if thread.project_id == project_id {
            return Ok(thread);
        }
        return Err(rusqlite::Error::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO threads (project_id, thread_id) VALUES (?1, ?2)",
        params![project_id, thread_id],
    )?;

    Ok(ThreadContext {
        id: connection.last_insert_rowid(),
        thread_id: thread_id.to_owned(),
        project_id,
    })
}

fn get_or_create_trace(
    connection: &Connection,
    project_id: i64,
    trace_id: &str,
    thread_db_id: Option<i64>,
) -> rusqlite::Result<TraceContext> {
    let existing = connection
        .query_row(
            "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = ?1 LIMIT 1",
            [trace_id],
            |row| {
                Ok(TraceContext {
                    id: row.get(0)?,
                    trace_id: row.get(1)?,
                    project_id: row.get(2)?,
                    thread_id: row.get(3)?,
                })
            },
        )
        .optional()?;

    if let Some(trace) = existing {
        if trace.project_id == project_id
            && (thread_db_id.is_none() || trace.thread_id == thread_db_id)
        {
            return Ok(trace);
        }
        return Err(rusqlite::Error::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO traces (project_id, trace_id, thread_id) VALUES (?1, ?2, ?3)",
        params![project_id, trace_id, thread_db_id],
    )?;

    Ok(TraceContext {
        id: connection.last_insert_rowid(),
        trace_id: trace_id.to_owned(),
        project_id,
        thread_id: thread_db_id,
    })
}

fn upsert_system_value(
    transaction: &Transaction<'_>,
    key: &str,
    value: &str,
) -> Result<(), SystemInitializeError> {
    transaction
        .execute(
            "INSERT INTO systems (key, value, deleted_at) VALUES (?1, ?2, 0)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
            params![key, value],
        )
        .map(|_| ())
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
}

fn query_channel_id(connection: &Connection, name: &str) -> rusqlite::Result<i64> {
    connection.query_row(
        "SELECT id FROM channels WHERE name = ?1 AND deleted_at = 0 LIMIT 1",
        [name],
        |row| row.get(0),
    )
}

fn query_model_id(
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

fn bool_to_sql(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn graphql_gid(resource_type: &str, id: i64) -> String {
    format!("gid://axonhub/{resource_type}/{id}")
}

fn i64_to_i32(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhub_http::{
        router, AdminCapability, AdminGraphqlCapability, AuthContextCapability, AuthUserContext,
        HttpState, OpenAiV1Capability, OpenApiGraphqlCapability, ProviderEdgeAdminCapability,
        SignInRequest, SystemBootstrapCapability, TraceConfig,
    };
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::util::ServiceExt;

    fn temp_sqlite_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axonhub-{name}-{unique}.db"))
    }

    fn test_admin_user() -> AuthUserContext {
        AuthUserContext {
            id: 1,
            email: "owner@example.com".to_owned(),
            first_name: "System".to_owned(),
            last_name: "Owner".to_owned(),
            is_owner: true,
            prefer_language: "en".to_owned(),
            avatar: Some(String::new()),
            scopes: vec![
                SCOPE_READ_SETTINGS.to_owned(),
                SCOPE_READ_CHANNELS.to_owned(),
                SCOPE_READ_REQUESTS.to_owned(),
            ],
            roles: Vec::new(),
            projects: Vec::new(),
        }
    }

    fn insert_test_user(connection: &Connection, email: &str, password: &str, scopes: &[&str]) -> i64 {
        let hashed_password = hash_password(password).unwrap();
        let scopes_json = serde_json::to_string(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at)
                 VALUES (?1, 'activated', 'en', ?2, 'Test', 'User', '', 0, ?3, 0)",
                params![email, hashed_password, scopes_json],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    fn insert_project_membership(
        connection: &Connection,
        user_id: i64,
        project_id: i64,
        is_owner: bool,
        scopes: &[&str],
    ) {
        let scopes_json = serde_json::to_string(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO user_projects (user_id, project_id, is_owner, scopes)
                 VALUES (?1, ?2, ?3, ?4)",
                params![user_id, project_id, if is_owner { 1 } else { 0 }, scopes_json],
            )
            .unwrap();
    }

    fn insert_role(
        connection: &Connection,
        name: &str,
        level: &str,
        project_id: i64,
        scopes: &[&str],
    ) -> i64 {
        let scopes_json = serde_json::to_string(scopes).unwrap();
        connection
            .execute(
                "INSERT INTO roles (name, level, project_id, scopes, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, 0)",
                params![name, level, project_id, scopes_json],
            )
            .unwrap();
        connection.last_insert_rowid()
    }

    fn attach_role(connection: &Connection, user_id: i64, role_id: i64) {
        connection
            .execute(
                "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                params![user_id, role_id],
            )
            .unwrap();
    }

    fn signin_token(foundation: Arc<SqliteFoundation>, email: &str, password: &str) -> String {
        let auth = SqliteAuthContextService::new(foundation, false);
        auth.admin_signin(&SignInRequest {
            email: email.to_owned(),
            password: password.to_owned(),
        })
        .unwrap()
        .token
    }

    fn graphql_test_app(
        foundation: Arc<SqliteFoundation>,
        bootstrap: SqliteBootstrapService,
    ) -> axum::Router {
        router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "test-only unsupported openai".to_owned(),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Available {
                graphql: Arc::new(SqliteAdminGraphqlService::new(foundation.clone())),
            },
            openapi_graphql: OpenApiGraphqlCapability::Available {
                graphql: Arc::new(SqliteOpenApiGraphqlService::new(foundation)),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        })
    }

    #[test]
    fn foundation_request_usage_and_catalog_stores_share_same_sqlite_schema() {
        let db_path = temp_sqlite_path("foundation-request-usage");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        foundation.system_settings().ensure_schema().unwrap();
        foundation.data_storages().ensure_schema().unwrap();
        foundation.identities().ensure_schema().unwrap();
        foundation.trace_contexts().ensure_schema().unwrap();
        foundation.channel_models().ensure_schema().unwrap();
        foundation.requests().ensure_schema().unwrap();
        foundation.usage_costs().ensure_schema().unwrap();

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Primary",
                channel_type: "openai",
                base_url: "https://api.openai.com/v1",
                status: "enabled",
                credentials_json: "{}",
                supported_models_json: "[\"gpt-4o\"]",
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[\"primary\"]",
                ordering_weight: 100,
                error_message: "",
                remark: "Rust migration foundation test",
            })
            .unwrap();

        let _model_row_id = foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: "{}",
                settings_json: "{}",
                status: "enabled",
                remark: "Rust migration foundation test",
            })
            .unwrap();

        let api_key_id = foundation
            .identities()
            .find_api_key_by_value(DEFAULT_USER_API_KEY_VALUE)
            .unwrap()
            .id;
        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let default_project = foundation
            .identities()
            .find_default_project_for_user(1)
            .unwrap();
        let data_storage_id = foundation
            .system_settings()
            .default_data_storage_id()
            .unwrap()
            .unwrap();
        let primary_storage = foundation
            .data_storages()
            .find_primary_active_storage()
            .unwrap()
            .unwrap();
        let trace_id = foundation
            .trace_contexts()
            .get_or_create_trace(project_id, "trace-foundation-1", None)
            .unwrap()
            .id;
        let user_context = foundation
            .identities()
            .build_user_context(foundation.identities().find_user_by_id(1).unwrap())
            .unwrap();

        let request_id = foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(api_key_id),
                project_id,
                trace_id: Some(trace_id),
                data_storage_id: Some(data_storage_id),
                source: "api",
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_headers_json: "{}",
                request_body_json: "{\"messages\":[]}",
                response_body_json: Some("{\"id\":\"resp_1\"}"),
                response_chunks_json: Some("[]"),
                channel_id: Some(channel_id),
                external_id: Some("req-ext-1"),
                status: "completed",
                stream: false,
                client_ip: "127.0.0.1",
                metrics_latency_ms: Some(120),
                metrics_first_token_latency_ms: Some(45),
                content_saved: false,
                content_storage_id: None,
                content_storage_key: None,
                content_saved_at: None,
            })
            .unwrap();

        let execution_id = foundation
            .requests()
            .create_request_execution(&NewRequestExecutionRecord {
                project_id,
                request_id,
                channel_id: Some(channel_id),
                data_storage_id: Some(data_storage_id),
                external_id: Some("exec-ext-1"),
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_body_json: "{\"provider\":true}",
                response_body_json: Some("{\"ok\":true}"),
                response_chunks_json: Some("[]"),
                error_message: "",
                response_status_code: Some(200),
                status: "completed",
                stream: false,
                metrics_latency_ms: Some(120),
                metrics_first_token_latency_ms: Some(45),
                request_headers_json: "{}",
            })
            .unwrap();

        let usage_id = foundation
            .usage_costs()
            .record_usage(&NewUsageLogRecord {
                request_id,
                api_key_id: Some(api_key_id),
                project_id,
                channel_id: Some(channel_id),
                model_id: "gpt-4o",
                prompt_tokens: 11,
                completion_tokens: 13,
                total_tokens: 24,
                prompt_audio_tokens: 0,
                prompt_cached_tokens: 0,
                prompt_write_cached_tokens: 0,
                prompt_write_cached_tokens_5m: 0,
                prompt_write_cached_tokens_1h: 0,
                completion_audio_tokens: 0,
                completion_reasoning_tokens: 0,
                completion_accepted_prediction_tokens: 0,
                completion_rejected_prediction_tokens: 0,
                source: "api",
                format: "openai/chat_completions",
                total_cost: Some(0.42),
                cost_items_json: "[{\"type\":\"input\",\"amount\":0.12}]",
                cost_price_reference_id: "price-v1",
            })
            .unwrap();

        let connection = foundation.open_connection(false).unwrap();
        assert_eq!(primary_storage.id, data_storage_id);
        assert_eq!(primary_storage.name, PRIMARY_DATA_STORAGE_NAME);
        assert_eq!(primary_storage.description, "Primary database storage");
        assert_eq!(primary_storage.storage_type, "database");
        assert_eq!(primary_storage.status, "active");
        assert_eq!(primary_storage.settings_json, "{}");
        assert_eq!(default_project.id, project_id);
        assert_eq!(user_context.id, 1);
        assert_eq!(user_context.projects[0].project_id.id, project_id);
        let primary_storage_name: String = connection
            .query_row(
                "SELECT name FROM data_storages WHERE id = ?1",
                [data_storage_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(primary_storage_name, PRIMARY_DATA_STORAGE_NAME);

        let persisted_request: (i64, String, i64) = connection
            .query_row(
                "SELECT id, model_id, channel_id FROM requests WHERE id = ?1",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(persisted_request.0, request_id);
        assert_eq!(persisted_request.1, "gpt-4o");
        assert_eq!(persisted_request.2, channel_id);

        let persisted_execution_request_id: i64 = connection
            .query_row(
                "SELECT request_id FROM request_executions WHERE id = ?1",
                [execution_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_execution_request_id, request_id);

        let persisted_usage: (i64, i64, f64) = connection
            .query_row(
                "SELECT request_id, total_tokens, total_cost FROM usage_logs WHERE id = ?1",
                [usage_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(persisted_usage.0, request_id);
        assert_eq!(persisted_usage.1, 24);
        assert_eq!(persisted_usage.2, 0.42);

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn foundational_admin_read_primitives_expose_catalog_request_and_trace_rows() {
        let db_path = temp_sqlite_path("task13-admin-primitives");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let trace = foundation
            .trace_contexts()
            .get_or_create_trace(project_id, "trace-task13", None)
            .unwrap();

        let channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task13 Channel",
                channel_type: "openai",
                base_url: "https://example.com/v1",
                status: "enabled",
                credentials_json: r#"{"apiKey":"key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: r#"["primary"]"#,
                ordering_weight: 100,
                error_message: "",
                remark: "Task 13 channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: "{}",
                settings_json: "{}",
                status: "enabled",
                remark: "Task 13 model",
            })
            .unwrap();

        let request_id = foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(1),
                project_id,
                trace_id: Some(trace.id),
                data_storage_id: None,
                source: "playground",
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_headers_json: "{}",
                request_body_json: r#"{"messages":[]}"#,
                response_body_json: Some(r#"{"id":"resp-task13"}"#),
                response_chunks_json: None,
                channel_id: Some(channel_id),
                external_id: Some("resp-task13"),
                status: "completed",
                stream: false,
                client_ip: "127.0.0.1",
                metrics_latency_ms: None,
                metrics_first_token_latency_ms: None,
                content_saved: false,
                content_storage_id: None,
                content_storage_key: None,
                content_saved_at: None,
            })
            .unwrap();

        let channels = foundation.channel_models().list_channels().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, channel_id);
        assert_eq!(channels[0].supported_models, vec!["gpt-4o".to_owned()]);

        let requests = foundation.requests().list_requests_by_project(project_id).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].id, request_id);
        assert_eq!(requests[0].trace_id, Some(trace.id));
        assert_eq!(requests[0].source, "playground");

        let traces = foundation.trace_contexts().list_traces_by_project(project_id).unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].trace_id, "trace-task13");

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn admin_request_content_download_enforces_project_scope_and_path_safety() {
        let db_path = temp_sqlite_path("task13-request-content");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let admin = SqliteAdminService::new(foundation.clone());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;
        let content_dir = std::env::temp_dir().join(format!(
            "axonhub-task13-content-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&content_dir).unwrap();

        let storage_id = foundation
            .data_storages()
            .find_primary_active_storage()
            .unwrap()
            .unwrap()
            .id
            + 100;
        let connection = foundation.open_connection(true).unwrap();
        connection
            .execute(
                "INSERT INTO data_storages (id, name, description, \"primary\", type, settings, status, deleted_at)
                 VALUES (?1, ?2, ?3, 0, 'fs', ?4, 'active', 0)",
                params![
                    storage_id,
                    "Task13 FS",
                    "task13",
                    serde_json::json!({"directory": content_dir.to_string_lossy()}).to_string(),
                ],
            )
            .unwrap();

        let request_id = foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(1),
                project_id,
                trace_id: None,
                data_storage_id: Some(storage_id),
                source: "api",
                model_id: "gpt-4o",
                format: "openai/video",
                request_headers_json: "{}",
                request_body_json: "{}",
                response_body_json: None,
                response_chunks_json: None,
                channel_id: None,
                external_id: None,
                status: "completed",
                stream: false,
                client_ip: "",
                metrics_latency_ms: None,
                metrics_first_token_latency_ms: None,
                content_saved: true,
                content_storage_id: Some(storage_id),
                content_storage_key: Some("/1/requests/1/video/video.mp4"),
                content_saved_at: Some("2026-03-23T00:00:00Z"),
            })
            .unwrap();

        let real_key = format!("/{project_id}/requests/{request_id}/video/video.mp4");
        connection
            .execute(
                "UPDATE requests SET content_storage_key = ?2 WHERE id = ?1",
                params![request_id, real_key],
            )
            .unwrap();
        let full_path = content_dir.join(format!("{project_id}/requests/{request_id}/video/video.mp4"));
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        fs::write(&full_path, b"video-content").unwrap();

        let downloaded = admin
            .download_request_content(project_id, request_id, test_admin_user())
            .unwrap();
        assert_eq!(downloaded.filename, "video.mp4");
        assert_eq!(downloaded.bytes, b"video-content");

        let wrong_project = admin
            .download_request_content(project_id + 1, request_id, test_admin_user())
            .unwrap_err();
        assert!(matches!(wrong_project, AdminError::NotFound { .. }));

        connection
            .execute(
                "UPDATE requests SET content_storage_key = '/../../etc/passwd' WHERE id = ?1",
                [request_id],
            )
            .unwrap();
        let traversal = admin
            .download_request_content(project_id, request_id, test_admin_user())
            .unwrap_err();
        assert!(matches!(traversal, AdminError::NotFound { .. }));

        fs::remove_dir_all(content_dir).ok();
        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_routes_complete_persistence_and_keep_residual_501() {
        let db_path = temp_sqlite_path("task7-openai-runtime");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Mock",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 7 runtime test",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0,"cacheRead":0.5,"cacheWrite":0.25,"cacheWrite5m":0.125},"vision":true,"toolCall":true,"reasoning":{"supported":true},"costPriceReferenceId":"price-ref-task9"}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 7 runtime test",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        });

        let models_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/models?include=all")
                    .method(Method::GET)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(models_response.status(), StatusCode::OK);
        let models_json = read_json_response(models_response).await;
        assert_eq!(models_json["data"][0]["id"], "gpt-4o");

        for (path, body) in [
            (
                "/v1/chat/completions",
                r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
            ),
            (
                "/v1/responses",
                r#"{"model":"gpt-4o","input":"hi"}"#,
            ),
            (
                "/v1/embeddings",
                r#"{"model":"gpt-4o","input":"hi"}"#,
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .method(Method::POST)
                        .header("content-type", "application/json")
                        .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                        .header("X-Project-ID", "gid://axonhub/project/1")
                        .header("AH-Thread-Id", "thread-task7")
                        .header("AH-Trace-Id", "trace-task7")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let unported = app
            .oneshot(
                Request::builder()
                    .uri("/v1/images")
                    .method(Method::POST)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unported.status(), StatusCode::NOT_IMPLEMENTED);

        let connection = foundation.open_connection(false).unwrap();
        let request_statuses: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT status FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_statuses, vec!["completed", "completed", "completed"]);

        let request_trace_channels: Vec<(i64, i64)> = {
            let mut statement = connection
                .prepare("SELECT trace_id, channel_id FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_trace_channels.len(), 3);
        assert!(request_trace_channels.iter().all(|(trace_id, _)| *trace_id > 0));
        let first_trace_id = request_trace_channels[0].0;
        assert!(request_trace_channels
            .iter()
            .all(|(trace_id, channel_id)| *trace_id == first_trace_id && *channel_id > 0));

        let trace_thread_link: (String, i64) = connection
            .query_row(
                "SELECT t.trace_id, t.thread_id FROM traces t ORDER BY id ASC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(trace_thread_link.0, "trace-task7");
        assert!(trace_thread_link.1 > 0);

        let thread_id: String = connection
            .query_row("SELECT thread_id FROM threads ORDER BY id ASC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(thread_id, "thread-task7");

        let execution_statuses: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT status FROM request_executions ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(execution_statuses, vec!["completed", "completed", "completed"]);

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 3);

        let usage_rows: Vec<(String, i64, i64, i64, i64, i64, i64, i64, i64, i64, f64, String, String)> = {
            let mut statement = connection
                .prepare(
                    "SELECT format, prompt_tokens, completion_tokens, total_tokens,
                            prompt_cached_tokens, prompt_write_cached_tokens,
                            prompt_write_cached_tokens_5m, completion_reasoning_tokens,
                            completion_accepted_prediction_tokens,
                            completion_rejected_prediction_tokens,
                            total_cost, cost_price_reference_id, cost_items
                     FROM usage_logs ORDER BY id ASC",
                )
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                    ))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(usage_rows.len(), 3);
        let responses_usage = &usage_rows[1];
        assert_eq!(responses_usage.1, 12);
        assert_eq!(responses_usage.2, 4);
        assert_eq!(responses_usage.3, 16);
        assert_eq!(responses_usage.4, 3);
        assert_eq!(responses_usage.5, 4);
        assert_eq!(responses_usage.6, 4);
        assert_eq!(responses_usage.7, 1);
        assert_eq!(responses_usage.8, 2);
        assert_eq!(responses_usage.9, 3);
        assert!((responses_usage.10 - 0.000015).abs() < 1e-12);
        assert_eq!(responses_usage.11, "price-ref-task9");
        assert!(responses_usage.12.contains("\"itemCode\":\"prompt_tokens\""));
        assert!(responses_usage.12.contains("\"itemCode\":\"prompt_write_cached_tokens\""));
        assert!(responses_usage
            .12
            .contains("\"promptWriteCacheVariantCode\":\"five_min\""));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_failure_persists_terminal_request_and_execution_state() {
        let db_path = temp_sqlite_path("task9-openai-failure-persistence");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let failing_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Task9 Failure",
                channel_type: "openai",
                base_url: format!("{}/primary-fail", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 9 failure channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 9 failure model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Thread-Id", "thread-task9-failure")
                    .header("AH-Trace-Id", "trace-task9-failure")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"fail me"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = read_json_response(response).await;
        assert_eq!(json["error"]["message"], "primary unavailable");

        let connection = foundation.open_connection(false).unwrap();
        let request_row: (String, i64, String) = connection
            .query_row(
                "SELECT status, channel_id, response_body FROM requests ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(request_row.0, "failed");
        assert_eq!(request_row.1, failing_channel_id);
        assert!(request_row.2.contains("primary unavailable"));

        let execution_row: (String, i64, String, i64, String) = connection
            .query_row(
                "SELECT status, channel_id, error_message, response_status_code, response_body
                 FROM request_executions ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(execution_row.0, "failed");
        assert_eq!(execution_row.1, failing_channel_id);
        assert_eq!(execution_row.2, "primary unavailable");
        assert_eq!(execution_row.3, 503);
        assert!(execution_row.4.contains("primary unavailable"));

        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(usage_count, 0);

        let request_trace_id: i64 = connection
            .query_row("SELECT trace_id FROM requests ORDER BY id DESC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let trace_thread: (String, i64) = connection
            .query_row(
                "SELECT trace_id, thread_id FROM traces WHERE id = ?1",
                [request_trace_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(trace_thread.0, "trace-task9-failure");
        assert!(trace_thread.1 > 0);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_fails_over_to_backup_channel_when_primary_fails() {
        let db_path = temp_sqlite_path("task8-openai-failover");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Primary Fail",
                channel_type: "openai",
                base_url: format!("{}/primary-fail", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 200,
                error_message: "",
                remark: "Task 8 failover primary",
            })
            .unwrap();
        let backup_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Backup Healthy",
                channel_type: "openai",
                base_url: format!("{}/backup", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 failover backup",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096},"cost":{"input":1.0,"output":2.0,"cache_read":0.5,"cache_write":0.25}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 8 failover model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task8-failover")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = read_json_response(response).await;
        assert_eq!(json["id"], "chatcmpl_backup");

        let connection = foundation.open_connection(false).unwrap();
        let request_channel_id: i64 = connection
            .query_row("SELECT channel_id FROM requests ORDER BY id DESC LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(request_channel_id, backup_channel_id);

        let execution_statuses: Vec<(i64, String)> = {
            let mut statement = connection
                .prepare("SELECT channel_id, status FROM request_executions ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(execution_statuses.len(), 2);
        assert_eq!(execution_statuses[0].1, "failed");
        assert_eq!(execution_statuses[1], (backup_channel_id, "completed".to_owned()));

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_reuses_same_channel_for_repeated_trace_when_both_healthy() {
        let db_path = temp_sqlite_path("task8-openai-trace-affinity");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let preferred_channel_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Affinity A",
                channel_type: "openai",
                base_url: format!("{}/affinity-a", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 affinity preferred",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Affinity B",
                channel_type: "openai",
                base_url: format!("{}/affinity-b", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 affinity backup",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 8 affinity model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        });

        for expected_id in ["chatcmpl_affinity_a", "chatcmpl_affinity_a"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/chat/completions")
                        .method(Method::POST)
                        .header("content-type", "application/json")
                        .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                        .header("X-Project-ID", "gid://axonhub/project/1")
                        .header("AH-Trace-Id", "trace-task8-affinity")
                        .body(Body::from(
                            r#"{"model":"gpt-4o","messages":[{"role":"user","content":"hello"}]}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let json = read_json_response(response).await;
            assert_eq!(json["id"], expected_id);
        }

        let connection = foundation.open_connection(false).unwrap();
        let request_channel_ids: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT channel_id FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_channel_ids, vec![preferred_channel_id, preferred_channel_id]);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openai_v1_does_not_pin_later_healthy_non_affinity_requests_to_prior_failover_backup() {
        let db_path = temp_sqlite_path("task8-openai-failover-selection-repair");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let _failover_primary_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Primary Fail",
                channel_type: "openai",
                base_url: format!("{}/primary-fail", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 200,
                error_message: "",
                remark: "Task 8 repair failing primary",
            })
            .unwrap();
        let failover_backup_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Backup Healthy",
                channel_type: "openai",
                base_url: format!("{}/backup", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 repair failover backup",
            })
            .unwrap();
        let healthy_affinity_a_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Healthy A",
                channel_type: "openai",
                base_url: format!("{}/affinity-a", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 repair healthy affinity A",
            })
            .unwrap();
        let healthy_affinity_b_id = foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "OpenAI Repair Healthy B",
                channel_type: "openai",
                base_url: format!("{}/affinity-b", mock_openai_server_url()).as_str(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 8 repair healthy affinity B",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: r#"{"limit":{"context":128000,"output":4096}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 8 repair model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        });

        let failover_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task8-repair-failover")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"failover first"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(failover_response.status(), StatusCode::OK);
        let failover_json = read_json_response(failover_response).await;
        assert_eq!(failover_json["id"], "chatcmpl_backup");

        let healthy_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/chat/completions")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("AH-Trace-Id", "trace-task8-repair-healthy")
                    .body(Body::from(
                        r#"{"model":"gpt-4o","messages":[{"role":"user","content":"healthy later"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(healthy_response.status(), StatusCode::OK);
        let healthy_json = read_json_response(healthy_response).await;
        assert_eq!(healthy_json["id"], "chatcmpl_affinity_a");

        let connection = foundation.open_connection(false).unwrap();
        let request_channel_ids: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT channel_id FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(request_channel_ids, vec![failover_backup_id, healthy_affinity_a_id]);
        assert_ne!(request_channel_ids[1], failover_backup_id);
        assert_ne!(request_channel_ids[1], healthy_affinity_b_id);

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn gemini_and_doubao_wrappers_use_shared_core_and_keep_neighboring_truthful_501() {
        let db_path = temp_sqlite_path("task12-gemini-doubao");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task12 Shared Core",
                channel_type: "openai",
                base_url: mock_openai_server_url(),
                status: "enabled",
                credentials_json: r#"{"apiKey":"test-upstream-key"}"#,
                supported_models_json: r#"["gemini-2.5-flash","seedance-1.0"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gemini-2.5-flash",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "Task 12 shared core channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task12 Wrong Channel",
                channel_type: "openai",
                base_url: "http://127.0.0.1:1/v1",
                status: "enabled",
                credentials_json: r#"{"apiKey":"wrong-key"}"#,
                supported_models_json: r#"["seedance-1.0"]"#,
                auto_sync_supported_models: false,
                default_test_model: "seedance-1.0",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 10,
                error_message: "",
                remark: "Task 12 wrong Doubao channel",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "google",
                model_id: "gemini-2.5-flash",
                model_type: "chat",
                name: "Gemini 2.5 Flash",
                icon: "Gemini",
                group: "gemini",
                model_card_json: r#"{"limit":{"context":1048576,"output":8192}}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 12 Gemini model",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "bytedance",
                model_id: "seedance-1.0",
                model_type: "video",
                name: "Seedance 1.0",
                icon: "Doubao",
                group: "doubao",
                model_card_json: r#"{}"#,
                settings_json: "{}",
                status: "enabled",
                remark: "Task 12 Doubao model",
            })
            .unwrap();

        let app = router(HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Available {
                system: Arc::new(bootstrap),
            },
            auth_context: AuthContextCapability::Available {
                auth: Arc::new(SqliteAuthContextService::new(foundation.clone(), false)),
            },
            openai_v1: OpenAiV1Capability::Available {
                openai: Arc::new(SqliteOpenAiV1Service::new(foundation.clone())),
            },
            admin: AdminCapability::Available {
                admin: Arc::new(SqliteAdminService::new(foundation.clone())),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "test-only unsupported admin graphql".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "test-only unsupported openapi graphql".to_owned(),
            },
            provider_edge_admin: ProviderEdgeAdminCapability::Unsupported {
                message: "test-only unsupported provider-edge admin".to_owned(),
            },
            allow_no_auth: false,
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        });

        let gemini_models = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gemini_models.status(), StatusCode::OK);
        let gemini_models_json = read_json_response(gemini_models).await;
        assert_eq!(gemini_models_json["models"][0]["name"], "models/gemini-2.5-flash");

        let gemini_generate = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/models/gemini-2.5-flash:generateContent?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gemini_generate.status(), StatusCode::OK);
        let gemini_generate_json = read_json_response(gemini_generate).await;
        assert_eq!(gemini_generate_json["candidates"][0]["content"]["parts"][0]["text"], "hi");

        let gemini_stream = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1beta/models/gemini-2.5-flash:streamGenerateContent?key=api-key-123")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"contents":[{"role":"user","parts":[{"text":"stream me"}]}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gemini_stream.status(), StatusCode::OK);

        let doubao_create = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks")
                    .method(Method::POST)
                    .header("content-type", "application/json")
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::from(r#"{"model":"seedance-1.0","content":[{"type":"text","text":"make a trailer"}]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doubao_create.status(), StatusCode::OK);
        let doubao_create_json = read_json_response(doubao_create).await;
        assert_eq!(doubao_create_json["id"], "video_mock_task");

        let doubao_get = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks/video_mock_task")
                    .method(Method::GET)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doubao_get.status(), StatusCode::OK);
        let doubao_get_json = read_json_response(doubao_get).await;
        assert_eq!(doubao_get_json["id"], "video_mock_task");
        assert_eq!(doubao_get_json["content"]["video_url"], "https://example.com/generated.mp4");

        let doubao_delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/doubao/v3/contents/generations/tasks/video_mock_task")
                    .method(Method::DELETE)
                    .header("X-API-Key", DEFAULT_USER_API_KEY_VALUE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(doubao_delete.status(), StatusCode::OK);

        let unsupported = app
            .oneshot(
                Request::builder()
                    .uri("/gemini/v1/files?key=api-key-123")
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unsupported.status(), StatusCode::NOT_IMPLEMENTED);

        let connection = foundation.open_connection(false).unwrap();
        let request_formats: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT format FROM requests ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert!(request_formats.contains(&"gemini/generate_content".to_owned()));
        assert!(request_formats.contains(&"gemini/stream_generate_content".to_owned()));
        assert!(request_formats.contains(&"doubao/video_create".to_owned()));
        assert!(request_formats.contains(&"doubao/video_get".to_owned()));
        assert!(request_formats.contains(&"doubao/video_delete".to_owned()));

        let failed_request_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE format IN ('doubao/video_get', 'doubao/video_delete') AND status = 'failed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_request_count, 0);

        let failed_execution_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM request_executions WHERE format IN ('doubao/video_get', 'doubao/video_delete') AND status = 'failed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_execution_count, 0);

        let doubao_channels: Vec<i64> = {
            let mut statement = connection
                .prepare("SELECT channel_id FROM requests WHERE format IN ('doubao/video_create', 'doubao/video_get', 'doubao/video_delete') ORDER BY id ASC")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(doubao_channels.len(), 3);
        assert!(doubao_channels.iter().all(|channel_id| *channel_id == doubao_channels[0]));

        std::fs::remove_file(db_path).ok();
    }

    async fn read_json_response(response: axum::response::Response) -> Value {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn mock_openai_server_url() -> &'static str {
        static SERVER_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        SERVER_URL
            .get_or_init(|| {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                thread::spawn(move || {
                    for stream in listener.incoming() {
                        let mut stream = match stream {
                            Ok(stream) => stream,
                            Err(_) => continue,
                        };
                        let mut buffer = [0_u8; 8192];
                        let size = match stream.read(&mut buffer) {
                            Ok(size) => size,
                            Err(_) => continue,
                        };
                        let request = String::from_utf8_lossy(&buffer[..size]);
                        let request_line = request.lines().next().unwrap_or_default().to_owned();
                        let method = request_line
                            .split_whitespace()
                            .next()
                            .unwrap_or("GET");
                        let path = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let body = if path.contains("/primary-fail/") && path.ends_with("/chat/completions") {
                            r#"{"error":{"message":"primary unavailable"}}"#
                        } else if method == "GET" && path.ends_with("/videos/video_mock_task") {
                            r#"{"id":"video_mock_task","model":"seedance-1.0","status":"succeeded","content":{"video_url":"https://example.com/generated.mp4"},"created_at":1,"completed_at":2}"#
                        } else if method == "DELETE" && path.ends_with("/videos/video_mock_task") {
                            r#"{"id":"video_mock_task"}"#
                        } else if method == "POST" && path.ends_with("/videos") {
                            r#"{"id":"video_mock_task"}"#
                        } else if path.contains("/backup/") && path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_backup","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"backup"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                        } else if path.contains("/affinity-a/") && path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_affinity_a","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"affinity-a"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                        } else if path.contains("/affinity-b/") && path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_affinity_b","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"affinity-b"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#
                        } else if path.ends_with("/chat/completions") {
                            r#"{"id":"chatcmpl_mock","object":"chat.completion","created":1,"model":"gpt-4o","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":2},"completion_tokens_details":{"reasoning_tokens":1}}}"#
                        } else if path.ends_with("/responses") {
                            r#"{"id":"resp_mock","object":"response","created_at":1,"model":"gpt-4o","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi","annotations":[]}],"status":"completed"}],"usage":{"input_tokens":12,"input_tokens_details":{"cached_tokens":3,"write_cached_tokens":4,"write_cached_5min_tokens":4},"output_tokens":4,"output_tokens_details":{"reasoning_tokens":1,"accepted_prediction_tokens":2,"rejected_prediction_tokens":3},"total_tokens":16}}"#
                        } else {
                            r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1,0.2],"index":0}],"model":"gpt-4o","usage":{"prompt_tokens":8,"total_tokens":8}}"#
                        };
                        let status_line = if path.contains("/primary-fail/") && path.ends_with("/chat/completions") {
                            "HTTP/1.1 503 Service Unavailable"
                        } else if (method == "POST" && path.ends_with("/videos/video_mock_task"))
                            || ((method == "GET" || method == "DELETE") && path.ends_with("/videos"))
                        {
                            "HTTP/1.1 404 Not Found"
                        } else {
                            "HTTP/1.1 200 OK"
                        };
                        let response = format!(
                            "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            status_line,
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                });
                format!("http://{}/v1", address)
            })
            .as_str()
    }

    #[tokio::test]
    async fn admin_graphql_enforces_system_and_project_scopes() {
        let db_path = temp_sqlite_path("task15-admin-graphql-rbac");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let connection = foundation.open_connection(true).unwrap();
        ensure_identity_tables(&connection).unwrap();
        let project_id = foundation.identities().find_project_by_id(1).unwrap().id;

        foundation
            .channel_models()
            .upsert_channel(&NewChannelRecord {
                name: "Task15 Channel",
                channel_type: "openai",
                base_url: "https://example.com/v1",
                status: "enabled",
                credentials_json: r#"{"apiKey":"key"}"#,
                supported_models_json: r#"["gpt-4o"]"#,
                auto_sync_supported_models: false,
                default_test_model: "gpt-4o",
                settings_json: "{}",
                tags_json: "[]",
                ordering_weight: 100,
                error_message: "",
                remark: "task15",
            })
            .unwrap();
        foundation
            .channel_models()
            .upsert_model(&NewModelRecord {
                developer: "openai",
                model_id: "gpt-4o",
                model_type: "chat",
                name: "GPT-4o",
                icon: "OpenAI",
                group: "openai",
                model_card_json: "{}",
                settings_json: "{}",
                status: "enabled",
                remark: "task15",
            })
            .unwrap();
        let trace = foundation
            .trace_contexts()
            .get_or_create_trace(project_id, "trace-task15", None)
            .unwrap();
        foundation
            .requests()
            .create_request(&NewRequestRecord {
                api_key_id: Some(1),
                project_id,
                trace_id: Some(trace.id),
                data_storage_id: None,
                source: "api",
                model_id: "gpt-4o",
                format: "openai/chat_completions",
                request_headers_json: "{}",
                request_body_json: "{}",
                response_body_json: None,
                response_chunks_json: None,
                channel_id: None,
                external_id: None,
                status: "completed",
                stream: false,
                client_ip: "",
                metrics_latency_ms: None,
                metrics_first_token_latency_ms: None,
                content_saved: false,
                content_storage_id: None,
                content_storage_key: None,
                content_saved_at: None,
            })
            .unwrap();

        let no_scope_user_id = insert_test_user(&connection, "viewer@example.com", "password123", &[]);
        insert_project_membership(&connection, no_scope_user_id, project_id, false, &[]);

        let system_reader_id = insert_test_user(&connection, "system@example.com", "password123", &[SCOPE_READ_SETTINGS, SCOPE_READ_CHANNELS]);
        insert_project_membership(&connection, system_reader_id, project_id, false, &[]);

        let project_reader_id = insert_test_user(&connection, "project@example.com", "password123", &[]);
        insert_project_membership(&connection, project_reader_id, project_id, false, &[]);
        let project_role_id = insert_role(&connection, "Request Reader", ROLE_LEVEL_PROJECT, project_id, &[SCOPE_READ_REQUESTS]);
        attach_role(&connection, project_reader_id, project_role_id);

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let no_scope_token = signin_token(foundation.clone(), "viewer@example.com", "password123");
        let denied_channels = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {no_scope_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ channels { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied_channels.status(), StatusCode::OK);
        let denied_channels_json = read_json_response(denied_channels).await;
        assert_eq!(denied_channels_json["data"]["channels"], Value::Null);
        assert_eq!(denied_channels_json["errors"][0]["message"], "permission denied");

        let system_token = signin_token(foundation.clone(), "system@example.com", "password123");
        let allowed_channels = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {system_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ systemStatus { isInitialized } channels { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed_channels_json = read_json_response(allowed_channels).await;
        assert_eq!(allowed_channels_json["data"]["systemStatus"]["isInitialized"], true);
        assert_eq!(allowed_channels_json["data"]["channels"][0]["id"], "gid://axonhub/channel/1");

        let project_token = signin_token(foundation.clone(), "project@example.com", "password123");
        let allowed_requests = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {project_token}"))
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ requests { id } traces { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed_requests_json = read_json_response(allowed_requests).await;
        assert_eq!(allowed_requests_json["data"]["requests"][0]["id"], "gid://axonhub/request/1");
        assert_eq!(allowed_requests_json["data"]["traces"][0]["id"], "gid://axonhub/trace/1");

        let denied_requests = app
            .oneshot(
                Request::builder()
                    .uri("/admin/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {system_token}"))
                    .header("X-Project-ID", "gid://axonhub/project/1")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"{ requests { id } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_requests_json = read_json_response(denied_requests).await;
        assert_eq!(denied_requests_json["data"]["requests"], Value::Null);
        assert_eq!(denied_requests_json["errors"][0]["message"], "permission denied");

        std::fs::remove_file(db_path).ok();
    }

    #[tokio::test]
    async fn openapi_create_llm_api_key_requires_write_api_keys_scope_and_service_account() {
        let db_path = temp_sqlite_path("task15-openapi-rbac");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let connection = foundation.open_connection(true).unwrap();
        connection
            .execute(
                "UPDATE api_keys SET scopes = ?2 WHERE key = ?1",
                params![DEFAULT_SERVICE_API_KEY_VALUE, serde_json::to_string(&vec![SCOPE_WRITE_API_KEYS]).unwrap()],
            )
            .unwrap();

        let app = graphql_test_app(foundation.clone(), bootstrap);

        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {DEFAULT_SERVICE_API_KEY_VALUE}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { createLLMAPIKey(name: \"SDK Key\") { name scopes } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed_json = read_json_response(allowed).await;
        assert_eq!(allowed_json["data"]["createLLMAPIKey"]["name"], "SDK Key");
        assert_eq!(allowed_json["data"]["createLLMAPIKey"]["scopes"][0], SCOPE_READ_CHANNELS);

        connection
            .execute(
                "UPDATE api_keys SET scopes = ?2 WHERE key = ?1",
                params![DEFAULT_SERVICE_API_KEY_VALUE, serde_json::to_string(&vec![SCOPE_READ_CHANNELS]).unwrap()],
            )
            .unwrap();

        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {DEFAULT_SERVICE_API_KEY_VALUE}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { createLLMAPIKey(name: \"SDK Key\") { name } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let denied_json = read_json_response(denied).await;
        assert_eq!(denied_json["data"]["createLLMAPIKey"], Value::Null);
        assert_eq!(denied_json["errors"][0]["message"], "permission denied");

        let invalid_user_key = app
            .oneshot(
                Request::builder()
                    .uri("/openapi/v1/graphql")
                    .method(Method::POST)
                    .header("Authorization", format!("Bearer {DEFAULT_USER_API_KEY_VALUE}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"mutation { createLLMAPIKey(name: \"SDK Key\") { name } }"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_user_key.status(), StatusCode::UNAUTHORIZED);

        std::fs::remove_file(db_path).ok();
    }
}
