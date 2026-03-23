use axonhub_http::{
    InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError, SystemQueryError,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use hex::encode as hex_encode;
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
