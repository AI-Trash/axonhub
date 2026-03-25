use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = match manager.get_database_backend() {
            DatabaseBackend::Sqlite => SQLITE_SCHEMA_SQL,
            DatabaseBackend::Postgres => POSTGRES_SCHEMA_SQL,
            DatabaseBackend::MySql => MYSQL_SCHEMA_SQL,
        };

        manager
            .get_connection()
            .execute_unprepared(sql)
            .await
            .map(|_| ())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = match manager.get_database_backend() {
            DatabaseBackend::Sqlite => SQLITE_DOWN_SQL,
            DatabaseBackend::Postgres => POSTGRES_DOWN_SQL,
            DatabaseBackend::MySql => MYSQL_DOWN_SQL,
        };

        manager
            .get_connection()
            .execute_unprepared(sql)
            .await
            .map(|_| ())
    }
}

const SQLITE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS systems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS data_storages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    "primary" INTEGER NOT NULL DEFAULT 0,
    type TEXT NOT NULL,
    settings TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);
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
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active'
);
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
CREATE TABLE IF NOT EXISTS user_roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id INTEGER NOT NULL,
    role_id INTEGER NOT NULL,
    UNIQUE(user_id, role_id)
);
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
CREATE TABLE IF NOT EXISTS threads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    thread_id TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS traces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id INTEGER NOT NULL,
    trace_id TEXT NOT NULL UNIQUE,
    thread_id INTEGER
);
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
    "group" TEXT NOT NULL DEFAULT '',
    model_card TEXT NOT NULL DEFAULT '{}',
    settings TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'enabled',
    remark TEXT NOT NULL DEFAULT '',
    UNIQUE(developer, model_id, type)
);
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
"#;

const POSTGRES_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS systems (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS data_storages (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    "primary" BOOLEAN NOT NULL DEFAULT FALSE,
    type TEXT NOT NULL,
    settings TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);
CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    email TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'activated',
    prefer_language TEXT NOT NULL DEFAULT 'en',
    password TEXT NOT NULL,
    first_name TEXT NOT NULL DEFAULT '',
    last_name TEXT NOT NULL DEFAULT '',
    avatar TEXT NOT NULL DEFAULT '',
    is_owner BOOLEAN NOT NULL DEFAULT FALSE,
    scopes TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS projects (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active'
);
CREATE TABLE IF NOT EXISTS user_projects (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id BIGINT NOT NULL,
    project_id BIGINT NOT NULL,
    is_owner BOOLEAN NOT NULL DEFAULT FALSE,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE(user_id, project_id)
);
CREATE TABLE IF NOT EXISTS roles (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'system',
    project_id BIGINT NOT NULL DEFAULT 0,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE(project_id, name)
);
CREATE TABLE IF NOT EXISTS user_roles (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    UNIQUE(user_id, role_id)
);
CREATE TABLE IF NOT EXISTS api_keys (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    user_id BIGINT NOT NULL,
    project_id BIGINT NOT NULL,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'enabled',
    scopes TEXT NOT NULL DEFAULT '[]',
    profiles TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS threads (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    thread_id TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS traces (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    trace_id TEXT NOT NULL UNIQUE,
    thread_id BIGINT
);
CREATE TABLE IF NOT EXISTS channels (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    type TEXT NOT NULL,
    base_url TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'enabled',
    credentials TEXT NOT NULL DEFAULT '{}',
    supported_models TEXT NOT NULL DEFAULT '[]',
    auto_sync_supported_models BOOLEAN NOT NULL DEFAULT FALSE,
    default_test_model TEXT NOT NULL DEFAULT '',
    settings TEXT NOT NULL DEFAULT '{}',
    tags TEXT NOT NULL DEFAULT '[]',
    ordering_weight BIGINT NOT NULL DEFAULT 0,
    error_message TEXT NOT NULL DEFAULT '',
    remark TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS models (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    developer TEXT NOT NULL,
    model_id TEXT NOT NULL,
    type TEXT NOT NULL,
    name TEXT NOT NULL,
    icon TEXT NOT NULL DEFAULT '',
    "group" TEXT NOT NULL DEFAULT '',
    model_card TEXT NOT NULL DEFAULT '{}',
    settings TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'enabled',
    remark TEXT NOT NULL DEFAULT '',
    UNIQUE(developer, model_id, type)
);
CREATE TABLE IF NOT EXISTS requests (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    api_key_id BIGINT,
    project_id BIGINT NOT NULL,
    trace_id BIGINT,
    data_storage_id BIGINT,
    source TEXT NOT NULL DEFAULT 'api',
    model_id TEXT NOT NULL,
    format TEXT NOT NULL DEFAULT 'openai/chat_completions',
    request_headers TEXT NOT NULL DEFAULT '{}',
    request_body TEXT NOT NULL DEFAULT '{}',
    response_body TEXT,
    response_chunks TEXT,
    channel_id BIGINT,
    external_id TEXT,
    status TEXT NOT NULL,
    stream BOOLEAN NOT NULL DEFAULT FALSE,
    client_ip TEXT NOT NULL DEFAULT '',
    metrics_latency_ms BIGINT,
    metrics_first_token_latency_ms BIGINT,
    content_saved BOOLEAN NOT NULL DEFAULT FALSE,
    content_storage_id BIGINT,
    content_storage_key TEXT,
    content_saved_at TEXT
);
CREATE TABLE IF NOT EXISTS request_executions (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    request_id BIGINT NOT NULL,
    channel_id BIGINT,
    data_storage_id BIGINT,
    external_id TEXT,
    model_id TEXT NOT NULL,
    format TEXT NOT NULL DEFAULT 'openai/chat_completions',
    request_body TEXT NOT NULL DEFAULT '{}',
    response_body TEXT,
    response_chunks TEXT,
    error_message TEXT NOT NULL DEFAULT '',
    response_status_code BIGINT,
    status TEXT NOT NULL,
    stream BOOLEAN NOT NULL DEFAULT FALSE,
    metrics_latency_ms BIGINT,
    metrics_first_token_latency_ms BIGINT,
    request_headers TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS usage_logs (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    request_id BIGINT NOT NULL,
    api_key_id BIGINT,
    project_id BIGINT NOT NULL,
    channel_id BIGINT,
    model_id TEXT NOT NULL,
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_audio_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_cached_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_write_cached_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_write_cached_tokens_5m BIGINT NOT NULL DEFAULT 0,
    prompt_write_cached_tokens_1h BIGINT NOT NULL DEFAULT 0,
    completion_audio_tokens BIGINT NOT NULL DEFAULT 0,
    completion_reasoning_tokens BIGINT NOT NULL DEFAULT 0,
    completion_accepted_prediction_tokens BIGINT NOT NULL DEFAULT 0,
    completion_rejected_prediction_tokens BIGINT NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'api',
    format TEXT NOT NULL DEFAULT 'openai/chat_completions',
    total_cost DOUBLE PRECISION,
    cost_items TEXT NOT NULL DEFAULT '[]',
    cost_price_reference_id TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS channel_probes (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    channel_id BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    total_request_count INTEGER NOT NULL DEFAULT 0,
    success_request_count INTEGER NOT NULL DEFAULT 0,
    avg_tokens_per_second DOUBLE PRECISION,
    avg_time_to_first_token_ms DOUBLE PRECISION,
    UNIQUE(channel_id, timestamp)
);
CREATE TABLE IF NOT EXISTS provider_quota_statuses (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    channel_id BIGINT NOT NULL UNIQUE,
    provider_type TEXT NOT NULL,
    status TEXT NOT NULL,
    quota_data TEXT NOT NULL DEFAULT '{}',
    next_reset_at BIGINT,
    ready BOOLEAN NOT NULL DEFAULT FALSE,
    next_check_at BIGINT NOT NULL DEFAULT 0
);
"#;

const MYSQL_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS systems (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    `key` TEXT NOT NULL,
    value TEXT NOT NULL,
    UNIQUE KEY uk_systems_key (`key`(255))
);
CREATE TABLE IF NOT EXISTS data_storages (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    `primary` BOOLEAN NOT NULL DEFAULT FALSE,
    type TEXT NOT NULL,
    settings TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    UNIQUE KEY uk_data_storages_name (name(255))
);
CREATE TABLE IF NOT EXISTS users (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    email TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'activated',
    prefer_language TEXT NOT NULL DEFAULT 'en',
    password TEXT NOT NULL,
    first_name TEXT NOT NULL DEFAULT '',
    last_name TEXT NOT NULL DEFAULT '',
    avatar TEXT NOT NULL DEFAULT '',
    is_owner BOOLEAN NOT NULL DEFAULT FALSE,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE KEY uk_users_email (email(255))
);
CREATE TABLE IF NOT EXISTS projects (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active',
    UNIQUE KEY uk_projects_name (name(255))
);
CREATE TABLE IF NOT EXISTS user_projects (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    user_id BIGINT NOT NULL,
    project_id BIGINT NOT NULL,
    is_owner BOOLEAN NOT NULL DEFAULT FALSE,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE KEY uk_user_projects_user_project (user_id, project_id)
);
CREATE TABLE IF NOT EXISTS roles (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'system',
    project_id BIGINT NOT NULL DEFAULT 0,
    scopes TEXT NOT NULL DEFAULT '[]',
    UNIQUE KEY uk_roles_project_name (project_id, name(255))
);
CREATE TABLE IF NOT EXISTS user_roles (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    UNIQUE KEY uk_user_roles_user_role (user_id, role_id)
);
CREATE TABLE IF NOT EXISTS api_keys (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    user_id BIGINT NOT NULL,
    project_id BIGINT NOT NULL,
    `key` TEXT NOT NULL,
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'enabled',
    scopes TEXT NOT NULL DEFAULT '[]',
    profiles TEXT NOT NULL DEFAULT '{}',
    UNIQUE KEY uk_api_keys_key (`key`(255))
);
CREATE TABLE IF NOT EXISTS threads (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    thread_id TEXT NOT NULL,
    UNIQUE KEY uk_threads_thread_id (thread_id(255))
);
CREATE TABLE IF NOT EXISTS traces (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    trace_id TEXT NOT NULL,
    thread_id BIGINT,
    UNIQUE KEY uk_traces_trace_id (trace_id(255))
);
CREATE TABLE IF NOT EXISTS channels (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    type TEXT NOT NULL,
    base_url TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'enabled',
    credentials TEXT NOT NULL,
    supported_models TEXT NOT NULL,
    auto_sync_supported_models BOOLEAN NOT NULL DEFAULT FALSE,
    default_test_model TEXT NOT NULL,
    settings TEXT NOT NULL,
    tags TEXT NOT NULL,
    ordering_weight BIGINT NOT NULL DEFAULT 0,
    error_message TEXT NOT NULL,
    remark TEXT NOT NULL,
    UNIQUE KEY uk_channels_name (name(255))
);
CREATE TABLE IF NOT EXISTS models (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    developer TEXT NOT NULL,
    model_id TEXT NOT NULL,
    type TEXT NOT NULL,
    name TEXT NOT NULL,
    icon TEXT NOT NULL DEFAULT '',
    `group` TEXT NOT NULL DEFAULT '',
    model_card TEXT NOT NULL DEFAULT '{}',
    settings TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'enabled',
    remark TEXT NOT NULL DEFAULT '',
    UNIQUE KEY uk_models_developer_model_type (developer(255), model_id(255), type(255))
);
CREATE TABLE IF NOT EXISTS requests (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    api_key_id BIGINT,
    project_id BIGINT NOT NULL,
    trace_id BIGINT,
    data_storage_id BIGINT,
    source TEXT NOT NULL,
    model_id TEXT NOT NULL,
    format TEXT NOT NULL,
    request_headers LONGTEXT NOT NULL,
    request_body LONGTEXT NOT NULL,
    response_body LONGTEXT,
    response_chunks LONGTEXT,
    channel_id BIGINT,
    external_id TEXT,
    status TEXT NOT NULL,
    stream BOOLEAN NOT NULL DEFAULT FALSE,
    client_ip TEXT NOT NULL,
    metrics_latency_ms BIGINT,
    metrics_first_token_latency_ms BIGINT,
    content_saved BOOLEAN NOT NULL DEFAULT FALSE,
    content_storage_id BIGINT,
    content_storage_key TEXT,
    content_saved_at TEXT
);
CREATE TABLE IF NOT EXISTS request_executions (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    request_id BIGINT NOT NULL,
    channel_id BIGINT,
    data_storage_id BIGINT,
    external_id TEXT,
    model_id TEXT NOT NULL,
    format TEXT NOT NULL,
    request_body LONGTEXT NOT NULL,
    response_body LONGTEXT,
    response_chunks LONGTEXT,
    error_message TEXT NOT NULL,
    response_status_code BIGINT,
    status TEXT NOT NULL,
    stream BOOLEAN NOT NULL DEFAULT FALSE,
    metrics_latency_ms BIGINT,
    metrics_first_token_latency_ms BIGINT,
    request_headers LONGTEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS usage_logs (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    request_id BIGINT NOT NULL,
    api_key_id BIGINT,
    project_id BIGINT NOT NULL,
    channel_id BIGINT,
    model_id TEXT NOT NULL,
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_audio_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_cached_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_write_cached_tokens BIGINT NOT NULL DEFAULT 0,
    prompt_write_cached_tokens_5m BIGINT NOT NULL DEFAULT 0,
    prompt_write_cached_tokens_1h BIGINT NOT NULL DEFAULT 0,
    completion_audio_tokens BIGINT NOT NULL DEFAULT 0,
    completion_reasoning_tokens BIGINT NOT NULL DEFAULT 0,
    completion_accepted_prediction_tokens BIGINT NOT NULL DEFAULT 0,
    completion_rejected_prediction_tokens BIGINT NOT NULL DEFAULT 0,
    source TEXT NOT NULL,
    format TEXT NOT NULL,
    total_cost DOUBLE,
    cost_items LONGTEXT NOT NULL,
    cost_price_reference_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS channel_probes (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    channel_id BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    total_request_count INTEGER NOT NULL DEFAULT 0,
    success_request_count INTEGER NOT NULL DEFAULT 0,
    avg_tokens_per_second DOUBLE,
    avg_time_to_first_token_ms DOUBLE,
    UNIQUE KEY uk_channel_probes_channel_timestamp (channel_id, timestamp)
);
CREATE TABLE IF NOT EXISTS provider_quota_statuses (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    channel_id BIGINT NOT NULL,
    provider_type TEXT NOT NULL,
    status TEXT NOT NULL,
    quota_data LONGTEXT NOT NULL,
    next_reset_at BIGINT,
    ready BOOLEAN NOT NULL DEFAULT FALSE,
    next_check_at BIGINT NOT NULL DEFAULT 0,
    UNIQUE KEY uk_provider_quota_statuses_channel_id (channel_id)
);
"#;

const SQLITE_DOWN_SQL: &str = r#"
DROP TABLE IF EXISTS provider_quota_statuses;
DROP TABLE IF EXISTS channel_probes;
DROP TABLE IF EXISTS usage_logs;
DROP TABLE IF EXISTS request_executions;
DROP TABLE IF EXISTS requests;
DROP TABLE IF EXISTS models;
DROP TABLE IF EXISTS channels;
DROP TABLE IF EXISTS traces;
DROP TABLE IF EXISTS threads;
DROP TABLE IF EXISTS api_keys;
DROP TABLE IF EXISTS user_roles;
DROP TABLE IF EXISTS roles;
DROP TABLE IF EXISTS user_projects;
DROP TABLE IF EXISTS projects;
DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS data_storages;
DROP TABLE IF EXISTS systems;
"#;

const POSTGRES_DOWN_SQL: &str = SQLITE_DOWN_SQL;

const MYSQL_DOWN_SQL: &str = SQLITE_DOWN_SQL;
