use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use super::sqlite_support::{SqliteConnectionFactory, SqliteFoundation};

pub(crate) const SYSTEM_KEY_INITIALIZED: &str = "system_initialized";
pub(crate) const SYSTEM_KEY_VERSION: &str = "system_version";
pub(crate) const SYSTEM_KEY_SECRET_KEY: &str = "system_jwt_secret_key";
pub(crate) const SYSTEM_KEY_BRAND_NAME: &str = "system_brand_name";
pub(crate) const SYSTEM_KEY_DEFAULT_DATA_STORAGE: &str = "default_data_storage_id";
pub(crate) const SYSTEM_KEY_ONBOARDED: &str = "system_onboarded";
pub(crate) const SYSTEM_KEY_STORAGE_POLICY: &str = "storage_policy";
pub(crate) const SYSTEM_KEY_CHANNEL_SETTINGS: &str = "system_channel_settings";
pub(crate) const SYSTEM_KEY_MODEL_SETTINGS: &str = "system_model_settings";
pub(crate) const SYSTEM_KEY_AUTO_BACKUP_SETTINGS: &str = "system_auto_backup_settings";
pub(crate) const SYSTEM_KEY_PROXY_PRESETS: &str = "system_proxy_presets";
pub(crate) const SYSTEM_KEY_USER_AGENT_PASS_THROUGH: &str = "system_user_agent_pass_through";
pub(crate) const PRIMARY_DATA_STORAGE_NAME: &str = "Primary";
pub(crate) const PRIMARY_DATA_STORAGE_DESCRIPTION: &str = "Primary database storage";
pub(crate) const PRIMARY_DATA_STORAGE_SETTINGS_JSON: &str = "{}";
pub(crate) const DEFAULT_PROJECT_NAME: &str = "Default Project";
pub(crate) const DEFAULT_PROJECT_DESCRIPTION: &str = "Default project";
pub(crate) const DEFAULT_USER_API_KEY_NAME: &str = "Default User Key";
pub(crate) const DEFAULT_USER_API_KEY_VALUE: &str = "api-key-123";
pub(crate) const DEFAULT_SERVICE_API_KEY_NAME: &str = "Default Service Account Key";
pub(crate) const DEFAULT_SERVICE_API_KEY_VALUE: &str = "service-key-123";
pub(crate) const NO_AUTH_API_KEY_NAME: &str = "No Auth System Key";
pub(crate) const NO_AUTH_API_KEY_VALUE: &str = "AXONHUB_API_KEY_NO_AUTH";
#[allow(dead_code)]
pub(crate) const PROVIDER_EDGE_PKCE_SESSION_TTL_SECONDS: i64 = 10 * 60;
#[allow(dead_code)]
pub(crate) const PROVIDER_EDGE_COPILOT_DEVICE_GRANT_TYPE: &str =
    "urn:ietf:params:oauth:grant-type:device_code";
#[allow(dead_code)]
pub(crate) const PROVIDER_EDGE_COPILOT_COMPLETE_MESSAGE: &str =
    "Authorization complete. Access token received.";
#[allow(dead_code)]
pub(crate) const PROVIDER_EDGE_COPILOT_PENDING_MESSAGE: &str =
    "Authorization pending. User has not yet authorized the device.";
#[allow(dead_code)]
pub(crate) const PROVIDER_EDGE_COPILOT_SLOW_DOWN_MESSAGE: &str =
    "Polling too fast. Please slow down.";
pub(crate) const BACKUP_VERSION: &str = "1.1";
pub(crate) const AUTO_BACKUP_PREFIX: &str = "axonhub-backup-";
pub(crate) const AUTO_BACKUP_SUFFIX: &str = ".json";

pub(crate) const SYSTEMS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS systems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
";

pub(crate) const DATA_STORAGES_TABLE_SQL: &str = "
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

pub(crate) const USERS_TABLE_SQL: &str = "
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

pub(crate) const PROJECTS_TABLE_SQL: &str = "
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

pub(crate) const USER_PROJECTS_TABLE_SQL: &str = "
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

pub(crate) const ROLES_TABLE_SQL: &str = "
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

pub(crate) const USER_ROLES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS user_roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id INTEGER NOT NULL,
    role_id INTEGER NOT NULL,
    UNIQUE(user_id, role_id)
);
";

pub(crate) const API_KEYS_TABLE_SQL: &str = "
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

pub(crate) const THREADS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS threads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    thread_id TEXT NOT NULL UNIQUE
);
";

pub(crate) const TRACES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS traces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    trace_id TEXT NOT NULL UNIQUE,
    thread_id INTEGER
);
";

pub(crate) const CHANNELS_TABLE_SQL: &str = "
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

pub(crate) const MODELS_TABLE_SQL: &str = "
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

pub(crate) const REQUESTS_TABLE_SQL: &str = "
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

pub(crate) const REQUEST_EXECUTIONS_TABLE_SQL: &str = "
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

pub(crate) const REALTIME_SESSIONS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS realtime_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL,
    thread_id INTEGER,
    trace_id INTEGER,
    request_id INTEGER,
    api_key_id INTEGER,
    channel_id INTEGER,
    session_id TEXT NOT NULL UNIQUE,
    transport TEXT NOT NULL,
    status TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    opened_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_activity_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    closed_at TEXT,
    expires_at TEXT
);
";

pub(crate) const PROMPTS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS prompts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    project_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'disabled',
    \"order\" INTEGER NOT NULL DEFAULT 0,
    settings TEXT NOT NULL DEFAULT '{}',
    UNIQUE(project_id, name, deleted_at)
);
";

pub(crate) const PROMPT_PROTECTION_RULES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS prompt_protection_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    pattern TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'disabled',
    settings TEXT NOT NULL,
    UNIQUE(name, deleted_at)
);
";

pub(crate) const USAGE_LOGS_TABLE_SQL: &str = "
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

pub(crate) const CHANNEL_PROBES_TABLE_SQL: &str = "
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

pub(crate) const PROVIDER_QUOTA_STATUSES_TABLE_SQL: &str = "
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

pub(crate) fn current_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(crate) fn current_rfc3339_timestamp() -> String {
    let now = current_unix_timestamp();
    format_unix_timestamp(now)
}

pub(crate) fn format_unix_timestamp(timestamp: i64) -> String {
    let system_time = UNIX_EPOCH
        .checked_add(Duration::from_secs(timestamp.max(0) as u64))
        .unwrap_or(UNIX_EPOCH);
    humantime::format_rfc3339_seconds(system_time).to_string()
}

pub(crate) fn bool_to_sql(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub(crate) fn graphql_gid(resource_type: &str, id: i64) -> String {
    format!("gid://axonhub/{resource_type}/{id}")
}

pub(crate) fn i64_to_i32(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}
