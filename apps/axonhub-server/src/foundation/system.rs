use axonhub_http::{
    InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError, SystemQueryError,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use hex::encode as hex_encode;
use postgres::{types::ToSql, Client as PostgresClient, NoTls};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::sync::Arc;

use super::{
    authz::{
        serialize_scope_slugs, ScopeLevel, ScopeSlug, DEFAULT_SERVICE_API_KEY_SCOPES,
        DEFAULT_USER_API_KEY_SCOPES, NO_AUTH_API_KEY_SCOPES, PROJECT_ADMIN_SCOPES,
        PROJECT_DEVELOPER_SCOPES, PROJECT_VIEWER_SCOPES, ROLE_LEVEL_PROJECT,
    },
    shared::{
        SqliteConnectionFactory, SqliteFoundation, API_KEYS_TABLE_SQL, CHANNELS_TABLE_SQL,
        CHANNEL_PROBES_TABLE_SQL, DATA_STORAGES_TABLE_SQL, DEFAULT_PROJECT_DESCRIPTION,
        DEFAULT_PROJECT_NAME, DEFAULT_SERVICE_API_KEY_NAME, DEFAULT_SERVICE_API_KEY_VALUE,
        DEFAULT_USER_API_KEY_NAME, DEFAULT_USER_API_KEY_VALUE, MODELS_TABLE_SQL,
        NO_AUTH_API_KEY_NAME, NO_AUTH_API_KEY_VALUE, PRIMARY_DATA_STORAGE_DESCRIPTION,
        PRIMARY_DATA_STORAGE_NAME, PRIMARY_DATA_STORAGE_SETTINGS_JSON, PROJECTS_TABLE_SQL,
        PROVIDER_QUOTA_STATUSES_TABLE_SQL, REQUESTS_TABLE_SQL, REQUEST_EXECUTIONS_TABLE_SQL,
        ROLES_TABLE_SQL, SYSTEMS_TABLE_SQL, SYSTEM_KEY_BRAND_NAME, SYSTEM_KEY_DEFAULT_DATA_STORAGE,
        SYSTEM_KEY_INITIALIZED, SYSTEM_KEY_SECRET_KEY, SYSTEM_KEY_VERSION, THREADS_TABLE_SQL,
        TRACES_TABLE_SQL, USAGE_LOGS_TABLE_SQL, USERS_TABLE_SQL, USER_PROJECTS_TABLE_SQL,
        USER_ROLES_TABLE_SQL,
    },
};

#[derive(Debug, Clone)]
pub struct SystemSettingsStore {
    connection_factory: SqliteConnectionFactory,
}

impl SystemSettingsStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
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
pub struct DataStorageStore {
    connection_factory: SqliteConnectionFactory,
}

impl DataStorageStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        connection.execute_batch(DATA_STORAGES_TABLE_SQL)
    }

    #[cfg(test)]
    pub fn find_primary_active_storage(&self) -> rusqlite::Result<Option<StoredDataStorage>> {
        let connection = self.connection_factory.open(true)?;
        self.query_primary_active_storage(&connection)
    }

    pub fn find_storage_by_id(
        &self,
        storage_id: i64,
    ) -> rusqlite::Result<Option<StoredDataStorage>> {
        let connection = self.connection_factory.open(true)?;
        connection
            .query_row(
                "SELECT id, name, description, type, status, settings FROM data_storages WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
                [storage_id],
                |row| {
                    Ok(StoredDataStorage {
                        #[cfg(test)]
                        id: row.get(0)?,
                        #[cfg(test)]
                        name: row.get(1)?,
                        #[cfg(test)]
                        description: row.get(2)?,
                        storage_type: row.get(3)?,
                        #[cfg(test)]
                        status: row.get(4)?,
                        settings_json: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    #[cfg(test)]
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
                        #[cfg(test)]
                        id: row.get(0)?,
                        #[cfg(test)]
                        name: row.get(1)?,
                        #[cfg(test)]
                        description: row.get(2)?,
                        storage_type: row.get(3)?,
                        #[cfg(test)]
                        status: row.get(4)?,
                        settings_json: row.get(5)?,
                    })
                },
            )
            .optional()
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

pub struct PostgresBootstrapService {
    dsn: String,
    version: String,
}

impl PostgresBootstrapService {
    const ALREADY_INITIALIZED_SENTINEL: &'static str = "already_initialized";

    pub fn new(dsn: impl Into<String>, version: String) -> Self {
        Self {
            dsn: dsn.into(),
            version,
        }
    }

    fn run_blocking<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(String) -> Result<T, String> + Send + 'static,
    {
        let dsn = self.dsn.clone();

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(move || operation(dsn))
                .join()
                .map_err(|_| "postgres bootstrap worker thread panicked".to_owned())?
        } else {
            operation(dsn)
        }
    }
}

impl SystemBootstrapPort for PostgresBootstrapService {
    fn is_initialized(&self) -> Result<bool, SystemQueryError> {
        self.run_blocking(|dsn| {
            let mut client =
                PostgresClient::connect(&dsn, NoTls).map_err(|error| error.to_string())?;
            ensure_all_foundation_tables_postgres(&mut client)
                .map_err(|error| error.to_string())?;
            query_is_initialized_postgres(&mut client).map_err(|error| error.to_string())
        })
        .map_err(|_| SystemQueryError::QueryFailed)
    }

    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
        let version = self.version.clone();
        let owner_email = request.owner_email.clone();
        let owner_password = request.owner_password.clone();
        let owner_first_name = request.owner_first_name.clone();
        let owner_last_name = request.owner_last_name.clone();
        let brand_name = request.brand_name.clone();

        self.run_blocking(move |dsn| {
            let request = InitializeSystemRequest {
                owner_email,
                owner_password,
                owner_first_name,
                owner_last_name,
                brand_name,
            };

            let mut client =
                PostgresClient::connect(&dsn, NoTls).map_err(|error| error.to_string())?;
            ensure_all_foundation_tables_postgres(&mut client)
                .map_err(|error| error.to_string())?;

            let mut transaction = client.transaction().map_err(|error| error.to_string())?;

            if query_is_initialized_postgres_tx(&mut transaction)
                .map_err(|error| error.to_string())?
            {
                return Err(Self::ALREADY_INITIALIZED_SENTINEL.to_owned());
            }

            let primary_data_storage_id = ensure_primary_data_storage_postgres(&mut transaction)
                .map_err(|error| error.to_string())?;
            let owner_user_id = ensure_owner_user_postgres(&mut transaction, &request).map_err(
                |error| match error {
                    SystemInitializeError::AlreadyInitialized => {
                        "System already initialized".to_owned()
                    }
                    SystemInitializeError::InitializeFailed(message) => message,
                },
            )?;
            let default_project_id = ensure_default_project_postgres(&mut transaction)
                .map_err(|error| error.to_string())?;
            ensure_default_project_roles_postgres(&mut transaction, default_project_id).map_err(
                |error| match error {
                    SystemInitializeError::AlreadyInitialized => {
                        "System already initialized".to_owned()
                    }
                    SystemInitializeError::InitializeFailed(message) => message,
                },
            )?;
            ensure_owner_project_membership_postgres(
                &mut transaction,
                owner_user_id,
                default_project_id,
            )
            .map_err(|error| error.to_string())?;
            ensure_default_api_keys_postgres(&mut transaction, owner_user_id, default_project_id)
                .map_err(|error| match error {
                SystemInitializeError::AlreadyInitialized => {
                    "System already initialized".to_owned()
                }
                SystemInitializeError::InitializeFailed(message) => message,
            })?;

            let secret = generate_secret_key_postgres().map_err(|error| error.to_string())?;
            upsert_system_value_postgres(&mut transaction, SYSTEM_KEY_SECRET_KEY, &secret)
                .map_err(|error| match error {
                    SystemInitializeError::AlreadyInitialized => {
                        "System already initialized".to_owned()
                    }
                    SystemInitializeError::InitializeFailed(message) => message,
                })?;
            upsert_system_value_postgres(
                &mut transaction,
                SYSTEM_KEY_BRAND_NAME,
                request.brand_name.trim(),
            )
            .map_err(|error| match error {
                SystemInitializeError::AlreadyInitialized => {
                    "System already initialized".to_owned()
                }
                SystemInitializeError::InitializeFailed(message) => message,
            })?;
            upsert_system_value_postgres(&mut transaction, SYSTEM_KEY_VERSION, &version).map_err(
                |error| match error {
                    SystemInitializeError::AlreadyInitialized => {
                        "System already initialized".to_owned()
                    }
                    SystemInitializeError::InitializeFailed(message) => message,
                },
            )?;
            upsert_system_value_postgres(
                &mut transaction,
                SYSTEM_KEY_DEFAULT_DATA_STORAGE,
                &primary_data_storage_id.to_string(),
            )
            .map_err(|error| match error {
                SystemInitializeError::AlreadyInitialized => {
                    "System already initialized".to_owned()
                }
                SystemInitializeError::InitializeFailed(message) => message,
            })?;
            upsert_system_value_postgres(&mut transaction, SYSTEM_KEY_INITIALIZED, "true")
                .map_err(|error| match error {
                    SystemInitializeError::AlreadyInitialized => {
                        "System already initialized".to_owned()
                    }
                    SystemInitializeError::InitializeFailed(message) => message,
                })?;

            transaction.commit().map_err(|error| error.to_string())
        })
        .map_err(|message| {
            if message == Self::ALREADY_INITIALIZED_SENTINEL {
                SystemInitializeError::AlreadyInitialized
            } else {
                SystemInitializeError::InitializeFailed(message)
            }
        })
    }
}

#[derive(Debug)]
pub struct StoredDataStorage {
    #[cfg(test)]
    pub id: i64,
    #[cfg(test)]
    pub name: String,
    #[cfg(test)]
    pub description: String,
    pub storage_type: String,
    #[cfg(test)]
    pub status: String,
    pub settings_json: String,
}

pub(crate) fn ensure_all_foundation_tables(connection: &Connection) -> rusqlite::Result<()> {
    ensure_systems_table(connection)?;
    connection.execute_batch(DATA_STORAGES_TABLE_SQL)?;
    ensure_identity_tables(connection)?;
    ensure_trace_tables(connection)?;
    ensure_channel_model_tables(connection)?;
    ensure_request_tables(connection)?;
    connection.execute_batch(USAGE_LOGS_TABLE_SQL)
}

pub(crate) fn ensure_systems_table(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(SYSTEMS_TABLE_SQL)
}

pub(crate) fn ensure_identity_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(USERS_TABLE_SQL)?;
    connection.execute_batch(PROJECTS_TABLE_SQL)?;
    connection.execute_batch(USER_PROJECTS_TABLE_SQL)?;
    connection.execute_batch(ROLES_TABLE_SQL)?;
    connection.execute_batch(USER_ROLES_TABLE_SQL)?;
    connection.execute_batch(API_KEYS_TABLE_SQL)
}

pub(crate) fn ensure_trace_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(THREADS_TABLE_SQL)?;
    connection.execute_batch(TRACES_TABLE_SQL)
}

pub(crate) fn ensure_channel_model_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(CHANNELS_TABLE_SQL)?;
    connection.execute_batch(MODELS_TABLE_SQL)
}

pub(crate) fn ensure_request_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(REQUESTS_TABLE_SQL)?;
    connection.execute_batch(REQUEST_EXECUTIONS_TABLE_SQL)
}

pub(crate) fn ensure_operational_tables(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(CHANNEL_PROBES_TABLE_SQL)?;
    connection.execute_batch(PROVIDER_QUOTA_STATUSES_TABLE_SQL)
}

pub(crate) fn upsert_system_value_on_connection(
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

pub(crate) fn query_is_initialized(connection: &Connection) -> rusqlite::Result<bool> {
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

pub(crate) fn ensure_primary_data_storage(transaction: &Transaction<'_>) -> rusqlite::Result<i64> {
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

pub(crate) fn generate_secret_key(transaction: &Transaction<'_>) -> rusqlite::Result<String> {
    transaction.query_row("SELECT lower(hex(randomblob(32)))", [], |row| row.get(0))
}

pub(crate) fn hash_password(password: &str) -> rusqlite::Result<String> {
    hash(password, DEFAULT_COST)
        .map(|hashed| hex_encode(hashed.as_bytes()))
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

pub(crate) fn verify_password(stored_hex: &str, password: &str) -> bool {
    hex::decode(stored_hex)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|hash| verify(password, &hash).ok())
        .unwrap_or(false)
}

pub(crate) fn ensure_owner_user(
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

pub(crate) fn ensure_default_project(transaction: &Transaction<'_>) -> rusqlite::Result<i64> {
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

pub(crate) fn ensure_owner_project_membership(
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

pub(crate) fn ensure_default_project_roles(
    transaction: &Transaction<'_>,
    project_id: i64,
) -> rusqlite::Result<()> {
    ensure_role_with_scopes(
        transaction,
        "Admin",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_ADMIN_SCOPES,
    )?;
    ensure_role_with_scopes(
        transaction,
        "Developer",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_DEVELOPER_SCOPES,
    )?;
    ensure_role_with_scopes(
        transaction,
        "Viewer",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_VIEWER_SCOPES,
    )?;

    Ok(())
}

pub(crate) fn ensure_role_with_scopes(
    transaction: &Transaction<'_>,
    name: &str,
    level: ScopeLevel,
    project_id: i64,
    scopes: &[ScopeSlug],
) -> rusqlite::Result<()> {
    let scopes_json = serialize_scope_slugs(scopes)?;
    transaction.execute(
        "INSERT INTO roles (name, level, project_id, scopes, deleted_at)
         VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT(project_id, name) DO UPDATE SET
             level = excluded.level,
             scopes = excluded.scopes,
             deleted_at = 0,
             updated_at = CURRENT_TIMESTAMP",
        params![name, level.as_str(), project_id, scopes_json],
    )?;

    Ok(())
}

pub(crate) fn ensure_default_api_keys(
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
        DEFAULT_USER_API_KEY_SCOPES,
    )?;
    ensure_api_key_with_scopes(
        transaction,
        user_id,
        project_id,
        DEFAULT_SERVICE_API_KEY_VALUE,
        DEFAULT_SERVICE_API_KEY_NAME,
        "service_account",
        DEFAULT_SERVICE_API_KEY_SCOPES,
    )?;
    ensure_api_key_with_scopes(
        transaction,
        user_id,
        project_id,
        NO_AUTH_API_KEY_VALUE,
        NO_AUTH_API_KEY_NAME,
        "noauth",
        NO_AUTH_API_KEY_SCOPES,
    )?;

    Ok(())
}

pub(crate) fn ensure_api_key_with_scopes(
    transaction: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    scopes: &[ScopeSlug],
) -> rusqlite::Result<()> {
    let scopes_json = serialize_scope_slugs(scopes)?;
    transaction.execute(
        "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', ?6, '{}', 0)
         ON CONFLICT(key) DO UPDATE SET name = excluded.name, type = excluded.type, scopes = excluded.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        params![user_id, project_id, key, name, key_type, scopes_json],
    )?;

    Ok(())
}

pub(crate) fn query_system_value(
    connection: &Connection,
    key: &str,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
            [key],
            |row| row.get(0),
        )
        .optional()
}

pub(crate) fn upsert_system_value(
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

const POSTGRES_SYSTEMS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS systems (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL
);
";

const POSTGRES_DATA_STORAGES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS data_storages (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    \"primary\" BOOLEAN NOT NULL DEFAULT FALSE,
    type TEXT NOT NULL,
    settings TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active'
);
";

const POSTGRES_USERS_TABLE_SQL: &str = "
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
";

const POSTGRES_PROJECTS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS projects (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at BIGINT NOT NULL DEFAULT 0,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active'
);
";

const POSTGRES_USER_PROJECTS_TABLE_SQL: &str = "
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
";

const POSTGRES_ROLES_TABLE_SQL: &str = "
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
";

const POSTGRES_USER_ROLES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS user_roles (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    UNIQUE(user_id, role_id)
);
";

const POSTGRES_API_KEYS_TABLE_SQL: &str = "
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
";

const POSTGRES_THREADS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS threads (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    thread_id TEXT NOT NULL UNIQUE
);
";

const POSTGRES_TRACES_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS traces (
    id BIGSERIAL PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    project_id BIGINT NOT NULL,
    trace_id TEXT NOT NULL UNIQUE,
    thread_id BIGINT
);
";

const POSTGRES_CHANNELS_TABLE_SQL: &str = "
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
";

const POSTGRES_MODELS_TABLE_SQL: &str = "
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
    \"group\" TEXT NOT NULL DEFAULT '',
    model_card TEXT NOT NULL DEFAULT '{}',
    settings TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'enabled',
    remark TEXT NOT NULL DEFAULT '',
    UNIQUE(developer, model_id, type)
);
";

const POSTGRES_REQUESTS_TABLE_SQL: &str = "
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
";

const POSTGRES_REQUEST_EXECUTIONS_TABLE_SQL: &str = "
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
";

const POSTGRES_USAGE_LOGS_TABLE_SQL: &str = "
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
";

pub(crate) fn ensure_systems_table_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(POSTGRES_SYSTEMS_TABLE_SQL)
}

pub(crate) fn ensure_identity_tables_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    for statement in [
        POSTGRES_USERS_TABLE_SQL,
        POSTGRES_PROJECTS_TABLE_SQL,
        POSTGRES_USER_PROJECTS_TABLE_SQL,
        POSTGRES_ROLES_TABLE_SQL,
        POSTGRES_USER_ROLES_TABLE_SQL,
        POSTGRES_API_KEYS_TABLE_SQL,
    ] {
        client.batch_execute(statement)?;
    }
    Ok(())
}

pub(crate) fn ensure_trace_tables_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    for statement in [POSTGRES_THREADS_TABLE_SQL, POSTGRES_TRACES_TABLE_SQL] {
        client.batch_execute(statement)?;
    }
    Ok(())
}

pub(crate) fn ensure_data_storages_table_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(POSTGRES_DATA_STORAGES_TABLE_SQL)
}

pub(crate) fn ensure_request_tables_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    for statement in [
        POSTGRES_REQUESTS_TABLE_SQL,
        POSTGRES_REQUEST_EXECUTIONS_TABLE_SQL,
    ] {
        client.batch_execute(statement)?;
    }
    Ok(())
}

pub(crate) fn ensure_channel_model_tables_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    for statement in [POSTGRES_CHANNELS_TABLE_SQL, POSTGRES_MODELS_TABLE_SQL] {
        client.batch_execute(statement)?;
    }
    Ok(())
}

pub(crate) fn ensure_usage_logs_table_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    client.batch_execute(POSTGRES_USAGE_LOGS_TABLE_SQL)
}

fn ensure_all_foundation_tables_postgres(
    client: &mut PostgresClient,
) -> Result<(), postgres::Error> {
    for statement in [
        POSTGRES_SYSTEMS_TABLE_SQL,
        POSTGRES_DATA_STORAGES_TABLE_SQL,
        POSTGRES_USERS_TABLE_SQL,
        POSTGRES_PROJECTS_TABLE_SQL,
        POSTGRES_USER_PROJECTS_TABLE_SQL,
        POSTGRES_ROLES_TABLE_SQL,
        POSTGRES_USER_ROLES_TABLE_SQL,
        POSTGRES_API_KEYS_TABLE_SQL,
        POSTGRES_THREADS_TABLE_SQL,
        POSTGRES_TRACES_TABLE_SQL,
        POSTGRES_CHANNELS_TABLE_SQL,
        POSTGRES_MODELS_TABLE_SQL,
        POSTGRES_REQUESTS_TABLE_SQL,
        POSTGRES_REQUEST_EXECUTIONS_TABLE_SQL,
        POSTGRES_USAGE_LOGS_TABLE_SQL,
    ] {
        client.batch_execute(statement)?;
    }
    Ok(())
}

pub(crate) fn query_system_value_postgres(
    client: &mut PostgresClient,
    key: &str,
) -> Result<Option<String>, postgres::Error> {
    client
        .query_opt(
            "SELECT value FROM systems WHERE key = $1 AND deleted_at = 0 LIMIT 1",
            &[&key],
        )
        .map(|row| row.map(|current| current.get(0)))
}

fn query_is_initialized_postgres(client: &mut PostgresClient) -> Result<bool, postgres::Error> {
    let value = query_system_value_postgres(client, SYSTEM_KEY_INITIALIZED)?;

    Ok(value
        .map(|current| current.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

fn query_is_initialized_postgres_tx(
    transaction: &mut postgres::Transaction<'_>,
) -> Result<bool, postgres::Error> {
    let value = transaction.query_opt(
        "SELECT value FROM systems WHERE key = $1 AND deleted_at = 0 LIMIT 1",
        &[&SYSTEM_KEY_INITIALIZED],
    )?;

    Ok(value
        .and_then(|row| row.try_get::<_, String>(0).ok())
        .map(|current| current.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

fn ensure_primary_data_storage_postgres(
    transaction: &mut postgres::Transaction<'_>,
) -> Result<i64, postgres::Error> {
    if let Some(row) = transaction.query_opt(
        "SELECT id FROM data_storages WHERE \"primary\" = TRUE AND deleted_at = 0 LIMIT 1",
        &[],
    )? {
        let id: i64 = row.get(0);
        return Ok(id);
    }

    let row = transaction.query_one(
        "INSERT INTO data_storages (name, description, \"primary\", type, settings, status) VALUES ($1, $2, TRUE, 'database', $3, 'active') RETURNING id",
        &[
            &PRIMARY_DATA_STORAGE_NAME,
            &PRIMARY_DATA_STORAGE_DESCRIPTION,
            &PRIMARY_DATA_STORAGE_SETTINGS_JSON,
        ],
    )?;

    Ok(row.get(0))
}

fn generate_secret_key_postgres() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)?;
    Ok(hex_encode(bytes))
}

fn hash_password_postgres(password: &str) -> Result<String, SystemInitializeError> {
    hash(password, DEFAULT_COST)
        .map(|hashed| hex_encode(hashed.as_bytes()))
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
}

fn ensure_owner_user_postgres(
    transaction: &mut postgres::Transaction<'_>,
    request: &InitializeSystemRequest,
) -> Result<i64, SystemInitializeError> {
    if let Some(row) = transaction
        .query_opt(
            "SELECT id FROM users WHERE is_owner = TRUE AND deleted_at = 0 LIMIT 1",
            &[],
        )
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?
    {
        let id: i64 = row.get(0);
        return Ok(id);
    }

    let password_hash = hash_password_postgres(request.owner_password.trim())?;
    let params: [&(dyn ToSql + Sync); 4] = [
        &request.owner_email.trim(),
        &password_hash,
        &request.owner_first_name.trim(),
        &request.owner_last_name.trim(),
    ];
    let row = transaction
        .query_one(
            "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at)
             VALUES ($1, 'activated', 'en', $2, $3, $4, '', TRUE, '[]', 0)
             RETURNING id",
            &params,
        )
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

    Ok(row.get(0))
}

fn ensure_default_project_postgres(
    transaction: &mut postgres::Transaction<'_>,
) -> Result<i64, postgres::Error> {
    if let Some(row) = transaction.query_opt(
        "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
        &[],
    )? {
        let id: i64 = row.get(0);
        return Ok(id);
    }

    let row = transaction.query_one(
        "INSERT INTO projects (name, description, status, deleted_at) VALUES ($1, $2, 'active', 0) RETURNING id",
        &[&DEFAULT_PROJECT_NAME, &DEFAULT_PROJECT_DESCRIPTION],
    )?;

    Ok(row.get(0))
}

fn ensure_owner_project_membership_postgres(
    transaction: &mut postgres::Transaction<'_>,
    user_id: i64,
    project_id: i64,
) -> Result<(), postgres::Error> {
    transaction.execute(
        "INSERT INTO user_projects (user_id, project_id, is_owner, scopes)
         VALUES ($1, $2, TRUE, '[]')
         ON CONFLICT(user_id, project_id) DO UPDATE SET is_owner = TRUE, updated_at = CURRENT_TIMESTAMP",
        &[&user_id, &project_id],
    )?;

    Ok(())
}

fn ensure_default_project_roles_postgres(
    transaction: &mut postgres::Transaction<'_>,
    project_id: i64,
) -> Result<(), SystemInitializeError> {
    ensure_role_with_scopes_postgres(
        transaction,
        "Admin",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_ADMIN_SCOPES,
    )?;
    ensure_role_with_scopes_postgres(
        transaction,
        "Developer",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_DEVELOPER_SCOPES,
    )?;
    ensure_role_with_scopes_postgres(
        transaction,
        "Viewer",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_VIEWER_SCOPES,
    )?;
    Ok(())
}

fn ensure_role_with_scopes_postgres(
    transaction: &mut postgres::Transaction<'_>,
    name: &str,
    level: ScopeLevel,
    project_id: i64,
    scopes: &[ScopeSlug],
) -> Result<(), SystemInitializeError> {
    let scopes_json = serialize_scope_slugs(scopes)
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
    let level_str = level.as_str();
    let params: [&(dyn ToSql + Sync); 4] = [&name, &level_str, &project_id, &scopes_json];
    transaction
        .execute(
            "INSERT INTO roles (name, level, project_id, scopes, deleted_at)
         VALUES ($1, $2, $3, $4, 0)
         ON CONFLICT(project_id, name) DO UPDATE SET
             level = EXCLUDED.level,
             scopes = EXCLUDED.scopes,
             deleted_at = 0,
             updated_at = CURRENT_TIMESTAMP",
            &params,
        )
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

    Ok(())
}

fn ensure_default_api_keys_postgres(
    transaction: &mut postgres::Transaction<'_>,
    user_id: i64,
    project_id: i64,
) -> Result<(), SystemInitializeError> {
    ensure_api_key_with_scopes_postgres(
        transaction,
        user_id,
        project_id,
        DEFAULT_USER_API_KEY_VALUE,
        DEFAULT_USER_API_KEY_NAME,
        "user",
        DEFAULT_USER_API_KEY_SCOPES,
    )?;
    ensure_api_key_with_scopes_postgres(
        transaction,
        user_id,
        project_id,
        DEFAULT_SERVICE_API_KEY_VALUE,
        DEFAULT_SERVICE_API_KEY_NAME,
        "service_account",
        DEFAULT_SERVICE_API_KEY_SCOPES,
    )?;
    ensure_api_key_with_scopes_postgres(
        transaction,
        user_id,
        project_id,
        NO_AUTH_API_KEY_VALUE,
        NO_AUTH_API_KEY_NAME,
        "noauth",
        NO_AUTH_API_KEY_SCOPES,
    )?;
    Ok(())
}

fn ensure_api_key_with_scopes_postgres(
    transaction: &mut postgres::Transaction<'_>,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    scopes: &[ScopeSlug],
) -> Result<(), SystemInitializeError> {
    let scopes_json = serialize_scope_slugs(scopes)
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
    let params: [&(dyn ToSql + Sync); 6] =
        [&user_id, &project_id, &key, &name, &key_type, &scopes_json];
    transaction.execute(
        "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
         VALUES ($1, $2, $3, $4, $5, 'enabled', $6, '{}', 0)
         ON CONFLICT(key) DO UPDATE SET name = EXCLUDED.name, type = EXCLUDED.type, scopes = EXCLUDED.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        &params,
    )
    .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
    Ok(())
}

fn upsert_system_value_postgres(
    transaction: &mut postgres::Transaction<'_>,
    key: &str,
    value: &str,
) -> Result<(), SystemInitializeError> {
    transaction
        .execute(
            "INSERT INTO systems (key, value, deleted_at) VALUES ($1, $2, 0)
             ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
            &[&key, &value],
        )
        .map(|_| ())
        .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
}
