use axonhub_http::{
    InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError, SystemQueryError,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use getrandom::getrandom;
use hex::encode as hex_encode;
use rusqlite::{
    params, Connection as SqlConnection, Error as SqlError, OpenFlags, OptionalExtension,
    Result as SqlResult, Transaction,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, ExecResult, TransactionTrait};
use std::sync::Arc;

use super::{
    authz::{
        serialize_scope_slugs, ScopeLevel, ScopeSlug, DEFAULT_SERVICE_API_KEY_SCOPES,
        DEFAULT_USER_API_KEY_SCOPES, NO_AUTH_API_KEY_SCOPES, PROJECT_ADMIN_SCOPES,
        PROJECT_DEVELOPER_SCOPES, PROJECT_VIEWER_SCOPES, ROLE_LEVEL_PROJECT,
    },
    ports::SystemBootstrapRepository,
    seaorm::SeaOrmConnectionFactory,
    shared::{
        API_KEYS_TABLE_SQL, CHANNELS_TABLE_SQL, CHANNEL_PROBES_TABLE_SQL,
        DATA_STORAGES_TABLE_SQL, DEFAULT_PROJECT_DESCRIPTION, DEFAULT_PROJECT_NAME,
        DEFAULT_SERVICE_API_KEY_NAME, DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_NAME,
        DEFAULT_USER_API_KEY_VALUE, MODELS_TABLE_SQL, NO_AUTH_API_KEY_NAME,
        NO_AUTH_API_KEY_VALUE, PRIMARY_DATA_STORAGE_DESCRIPTION, PRIMARY_DATA_STORAGE_NAME,
        PRIMARY_DATA_STORAGE_SETTINGS_JSON, PROJECTS_TABLE_SQL,
        PROVIDER_QUOTA_STATUSES_TABLE_SQL, REQUESTS_TABLE_SQL, REQUEST_EXECUTIONS_TABLE_SQL,
        ROLES_TABLE_SQL, SYSTEMS_TABLE_SQL, SYSTEM_KEY_BRAND_NAME,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_INITIALIZED, SYSTEM_KEY_SECRET_KEY,
        SYSTEM_KEY_VERSION, THREADS_TABLE_SQL, TRACES_TABLE_SQL, USAGE_LOGS_TABLE_SQL,
        USERS_TABLE_SQL, USER_PROJECTS_TABLE_SQL, USER_ROLES_TABLE_SQL,
    },
};
use super::repositories::common::{execute as execute_sql, query_one as query_one_sql};

pub(crate) type SeaOrmDbFactory = SeaOrmConnectionFactory;

fn sql_conversion_error(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> SqlError {
    SqlError::ToSqlConversionFailure(error.into())
}

#[derive(Debug, Clone)]
pub struct SqliteFoundation {
    seaorm_factory: SeaOrmConnectionFactory,
    connection_factory: SqliteConnectionFactory,
}

impl SqliteFoundation {
    pub fn new(dsn: impl Into<String>) -> Self {
        let dsn = dsn.into();
        Self {
            seaorm_factory: SeaOrmConnectionFactory::sqlite(dsn.clone()),
            connection_factory: SqliteConnectionFactory::new(dsn),
        }
    }

    pub fn seaorm(&self) -> SeaOrmConnectionFactory {
        self.seaorm_factory.clone()
    }

    pub fn open_connection(&self, create_if_missing: bool) -> SqlResult<SqlConnection> {
        self.connection_factory.open(create_if_missing)
    }

    pub fn system_settings(&self) -> SystemSettingsStore {
        SystemSettingsStore::new(self.connection_factory.clone())
    }

    pub fn data_storages(&self) -> DataStorageStore {
        DataStorageStore::new(self.connection_factory.clone())
    }

    pub fn identities(&self) -> super::identity::IdentityStore {
        super::identity::IdentityStore::new(self.connection_factory.clone())
    }

    pub fn identity_auth(
        &self,
        allow_no_auth: bool,
    ) -> super::identity_service::IdentityAuthService {
        super::identity_service::IdentityAuthService::new(
            self.identities(),
            self.system_settings(),
            allow_no_auth,
        )
    }

    #[cfg(test)]
    pub fn trace_contexts(&self) -> super::request_context_sqlite_support::TraceContextStore {
        super::request_context_sqlite_support::TraceContextStore::new(self.connection_factory.clone())
    }

    #[cfg(test)]
    pub fn request_context_service(
        &self,
        allow_no_auth: bool,
    ) -> super::request_context_sqlite_support::RequestContextService {
        super::request_context_sqlite_support::RequestContextService::new(
            self.identity_auth(allow_no_auth),
            self.trace_contexts(),
        )
    }

    pub fn channel_models(&self) -> super::openai_v1::ChannelModelStore {
        super::openai_v1::ChannelModelStore::new(self.connection_factory.clone())
    }

    pub fn requests(&self) -> super::openai_v1::RequestStore {
        super::openai_v1::RequestStore::new(self.connection_factory.clone())
    }

    pub fn usage_costs(&self) -> super::openai_v1::UsageCostStore {
        super::openai_v1::UsageCostStore::new(self.connection_factory.clone())
    }

    pub fn operational(&self) -> super::admin::OperationalStore {
        super::admin::OperationalStore::new(self.connection_factory.clone())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SqliteConnectionFactory {
    dsn: Arc<String>,
}

impl SqliteConnectionFactory {
    pub(crate) fn new(dsn: String) -> Self {
        Self { dsn: Arc::new(dsn) }
    }

    pub(crate) fn open(&self, create_if_missing: bool) -> SqlResult<SqlConnection> {
        let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
        if create_if_missing {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        if self.dsn.starts_with("file:") {
            flags |= OpenFlags::SQLITE_OPEN_URI;
        }

        SqlConnection::open_with_flags(self.dsn.as_str(), flags)
    }
}

#[derive(Debug, Clone)]
pub struct SystemSettingsStore {
    connection_factory: SqliteConnectionFactory,
}

impl SystemSettingsStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let db = self.connection_factory.open(true)?;
        ensure_systems_table(&db)
    }

    pub fn is_initialized(&self) -> SqlResult<bool> {
        let db = self.connection_factory.open(true)?;
        ensure_systems_table(&db)?;
        query_is_initialized(&db)
    }

    pub fn value(&self, key: &str) -> SqlResult<Option<String>> {
        let db = self.connection_factory.open(true)?;
        ensure_systems_table(&db)?;
        query_system_value(&db, key)
    }

    pub fn default_data_storage_id(&self) -> SqlResult<Option<i64>> {
        self.value(SYSTEM_KEY_DEFAULT_DATA_STORAGE)
            .map(|value| value.and_then(|current| current.parse::<i64>().ok()))
    }

    pub fn set_value(&self, key: &str, value: &str) -> SqlResult<()> {
        let db = self.connection_factory.open(true)?;
        ensure_systems_table(&db)?;
        upsert_system_value_on_connection(&db, key, value)
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
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let db = self.connection_factory.open(true)?;
        db.execute_batch(DATA_STORAGES_TABLE_SQL)
    }

    #[cfg(test)]
    pub fn find_primary_active_storage(&self) -> SqlResult<Option<StoredDataStorage>> {
        let db = self.connection_factory.open(true)?;
        db.query_row(
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

    pub fn find_storage_by_id(&self, storage_id: i64) -> SqlResult<Option<StoredDataStorage>> {
        let db = self.connection_factory.open(true)?;
        db.query_row(
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
}

pub struct SqliteBootstrapService {
    foundation: Arc<SqliteFoundation>,
    version: String,
}

impl SqliteBootstrapService {
    pub fn new(foundation: Arc<SqliteFoundation>, version: String) -> Self {
        Self { foundation, version }
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
        let mut db = self
            .foundation
            .open_connection(true)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_all_foundation_tables(&db)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

        let tx = db
            .transaction()
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

        if query_is_initialized(&tx)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?
        {
            return Err(SystemInitializeError::AlreadyInitialized);
        }

        let primary_data_storage_id = ensure_primary_data_storage(&tx)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        let owner_user_id = ensure_owner_user(&tx, request)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        let default_project_id = ensure_default_project(&tx)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_default_project_roles(&tx, default_project_id)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_owner_project_membership(&tx, owner_user_id, default_project_id)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        ensure_default_api_keys(&tx, owner_user_id, default_project_id)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;

        let secret = generate_secret_key(&tx)
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
        upsert_system_value(&tx, SYSTEM_KEY_SECRET_KEY, &secret)?;
        upsert_system_value(&tx, SYSTEM_KEY_BRAND_NAME, request.brand_name.trim())?;
        upsert_system_value(&tx, SYSTEM_KEY_VERSION, &self.version)?;
        upsert_system_value(
            &tx,
            SYSTEM_KEY_DEFAULT_DATA_STORAGE,
            &primary_data_storage_id.to_string(),
        )?;
        upsert_system_value(&tx, SYSTEM_KEY_INITIALIZED, "true")?;

        tx.commit()
            .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
    }
}

impl SystemBootstrapRepository for SqliteBootstrapService {
    fn is_initialized(&self) -> Result<bool, SystemQueryError> {
        <Self as SystemBootstrapPort>::is_initialized(self)
    }

    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
        <Self as SystemBootstrapPort>::initialize(self, request)
    }
}

pub(crate) async fn seaorm_is_initialized(
    dbf: &SeaOrmDbFactory,
) -> Result<bool, sea_orm::DbErr> {
    let db = dbf.connect_migrated().await?;
    query_is_initialized_seaorm(&db, dbf.backend()).await
}

pub(crate) async fn seaorm_initialize(
    dbf: &SeaOrmDbFactory,
    version: &str,
    request: &InitializeSystemRequest,
) -> Result<(), sea_orm::DbErr> {
    let db = dbf.connect_migrated().await?;
    let engine = dbf.backend();
    let tx = db.begin().await?;

    if query_is_initialized_seaorm(&tx, engine).await? {
        return Err(sea_orm::DbErr::Custom("system already initialized".to_owned()));
    }

    let primary_data_storage_id = ensure_primary_data_storage_seaorm(&tx, engine).await?;
    let owner_user_id = ensure_owner_user_seaorm(&tx, engine, request).await?;
    let default_project_id = ensure_default_project_seaorm(&tx, engine).await?;
    ensure_default_project_roles_seaorm(&tx, engine, default_project_id).await?;
    ensure_owner_project_membership_seaorm(&tx, engine, owner_user_id, default_project_id).await?;
    ensure_default_api_keys_seaorm(&tx, engine, owner_user_id, default_project_id).await?;

    let secret = generate_secret_key_seaorm()?;
    upsert_system_value_seaorm(&tx, engine, SYSTEM_KEY_SECRET_KEY, &secret).await?;
    upsert_system_value_seaorm(&tx, engine, SYSTEM_KEY_BRAND_NAME, request.brand_name.trim())
        .await?;
    upsert_system_value_seaorm(&tx, engine, SYSTEM_KEY_VERSION, version).await?;
    upsert_system_value_seaorm(
        &tx,
        engine,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE,
        &primary_data_storage_id.to_string(),
    )
    .await?;
    upsert_system_value_seaorm(&tx, engine, SYSTEM_KEY_INITIALIZED, "true").await?;

    tx.commit().await
}

async fn execute_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    execute_sql(db, backend, sqlite_sql, postgres_sql, mysql_sql, values)
    .await
    .map(|_| ())
}

async fn query_optional_i64_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<i64>, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let row = query_one_sql(db, backend, sqlite_sql, postgres_sql, mysql_sql, values).await?;
    row.map(|row| row.try_get_by_index(0)).transpose()
}

async fn query_optional_string_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<String>, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let row = query_one_sql(db, backend, sqlite_sql, postgres_sql, mysql_sql, values).await?;
    row.map(|row| row.try_get_by_index(0)).transpose()
}

fn inserted_id_from_result(result: &ExecResult, backend: DatabaseBackend) -> Result<i64, sea_orm::DbErr> {
    let id = result.last_insert_id();
    if id == 0 {
        Err(sea_orm::DbErr::Custom(format!(
            "missing inserted id for bootstrap {backend:?} operation"
        )))
    } else {
        Ok(id as i64)
    }
}

async fn insert_returning_i64_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    match backend {
        DatabaseBackend::Sqlite => {
            let result = execute_sql(db, backend, sqlite_sql, "", "", values).await?;
            inserted_id_from_result(&result, backend)
        }
        DatabaseBackend::Postgres => {
            let row = query_one_sql(db, backend, "", postgres_sql, "", values)
                .await?
                .ok_or_else(|| sea_orm::DbErr::RecordNotFound(postgres_sql.to_owned()))?;
            row.try_get_by_index(0)
        }
        DatabaseBackend::MySql => {
            let result = execute_sql(db, backend, "", "", mysql_sql, values).await?;
            inserted_id_from_result(&result, backend)
        }
    }
}

pub(crate) async fn query_is_initialized_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
) -> Result<bool, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let value = query_optional_string_seaorm(
        db,
        backend,
        "SELECT value FROM systems WHERE key = ? AND deleted_at = 0 LIMIT 1",
        "SELECT value FROM systems WHERE key = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT value FROM systems WHERE `key` = ? AND deleted_at = 0 LIMIT 1",
        vec![SYSTEM_KEY_INITIALIZED.into()],
    )
    .await?;

    Ok(value
        .map(|current| current.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

pub(crate) async fn upsert_system_value_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    key: &str,
    value: &str,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    execute_seaorm(
        db,
        backend,
        "INSERT INTO systems (key, value, deleted_at) VALUES (?, ?, 0) ON CONFLICT(key) DO UPDATE SET value = excluded.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO systems (key, value, deleted_at) VALUES ($1, $2, 0) ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO systems (`key`, value, deleted_at) VALUES (?, ?, 0) ON DUPLICATE KEY UPDATE value = VALUES(value), deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        vec![key.into(), value.into()],
    )
    .await
}

pub(crate) async fn ensure_primary_data_storage_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    if let Some(id) = query_optional_i64_seaorm(
        db,
        backend,
        "SELECT id FROM data_storages WHERE \"primary\" = 1 AND deleted_at = 0 LIMIT 1",
        "SELECT id FROM data_storages WHERE \"primary\" = TRUE AND deleted_at = 0 LIMIT 1",
        "SELECT id FROM data_storages WHERE `primary` = TRUE AND deleted_at = 0 LIMIT 1",
        vec![],
    )
    .await?
    {
        return Ok(id);
    }

    insert_returning_i64_seaorm(
        db,
        backend,
        "INSERT INTO data_storages (name, description, \"primary\", type, settings, status) VALUES (?, ?, 1, 'database', ?, 'active')",
        "INSERT INTO data_storages (name, description, \"primary\", type, settings, status) VALUES ($1, $2, TRUE, 'database', $3, 'active') RETURNING id",
        "INSERT INTO data_storages (name, description, `primary`, type, settings, status) VALUES (?, ?, TRUE, 'database', ?, 'active')",
        vec![
            PRIMARY_DATA_STORAGE_NAME.into(),
            PRIMARY_DATA_STORAGE_DESCRIPTION.into(),
            PRIMARY_DATA_STORAGE_SETTINGS_JSON.into(),
        ],
    )
    .await
}

pub(crate) fn generate_secret_key_seaorm() -> Result<String, sea_orm::DbErr> {
    let mut bytes = [0_u8; 32];
    getrandom(&mut bytes).map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
    Ok(hex_encode(bytes))
}

pub(crate) async fn ensure_owner_user_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    request: &InitializeSystemRequest,
) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    if let Some(id) = query_optional_i64_seaorm(
        db,
        backend,
        "SELECT id FROM users WHERE is_owner = 1 AND deleted_at = 0 LIMIT 1",
        "SELECT id FROM users WHERE is_owner = TRUE AND deleted_at = 0 LIMIT 1",
        "SELECT id FROM users WHERE is_owner = TRUE AND deleted_at = 0 LIMIT 1",
        vec![],
    )
    .await?
    {
        return Ok(id);
    }

    let password_hash =
        hash_password(request.owner_password.trim()).map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
    insert_returning_i64_seaorm(
        db,
        backend,
        "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at) VALUES (?, 'activated', 'en', ?, ?, ?, '', 1, '[]', 0)",
        "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at) VALUES ($1, 'activated', 'en', $2, $3, $4, '', TRUE, '[]', 0) RETURNING id",
        "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at) VALUES (?, 'activated', 'en', ?, ?, ?, '', TRUE, '[]', 0)",
        vec![
            request.owner_email.trim().into(),
            password_hash.into(),
            request.owner_first_name.trim().into(),
            request.owner_last_name.trim().into(),
        ],
    )
    .await
}

pub(crate) async fn ensure_default_project_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    if let Some(id) = query_optional_i64_seaorm(
        db,
        backend,
        "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
        "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
        "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
        vec![],
    )
    .await?
    {
        return Ok(id);
    }

    insert_returning_i64_seaorm(
        db,
        backend,
        "INSERT INTO projects (name, description, status, deleted_at) VALUES (?, ?, 'active', 0)",
        "INSERT INTO projects (name, description, status, deleted_at) VALUES ($1, $2, 'active', 0) RETURNING id",
        "INSERT INTO projects (name, description, status, deleted_at) VALUES (?, ?, 'active', 0)",
        vec![DEFAULT_PROJECT_NAME.into(), DEFAULT_PROJECT_DESCRIPTION.into()],
    )
    .await
}

pub(crate) async fn ensure_owner_project_membership_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    user_id: i64,
    project_id: i64,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    execute_seaorm(
        db,
        backend,
        "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES (?, ?, 1, '[]') ON CONFLICT(user_id, project_id) DO UPDATE SET is_owner = 1, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES ($1, $2, TRUE, '[]') ON CONFLICT(user_id, project_id) DO UPDATE SET is_owner = TRUE, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES (?, ?, TRUE, '[]') ON DUPLICATE KEY UPDATE is_owner = VALUES(is_owner), updated_at = CURRENT_TIMESTAMP",
        vec![user_id.into(), project_id.into()],
    )
    .await
}

pub(crate) async fn ensure_default_project_roles_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    project_id: i64,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    ensure_role_with_scopes_seaorm(db, backend, "Admin", ROLE_LEVEL_PROJECT, project_id, PROJECT_ADMIN_SCOPES).await?;
    ensure_role_with_scopes_seaorm(db, backend, "Developer", ROLE_LEVEL_PROJECT, project_id, PROJECT_DEVELOPER_SCOPES).await?;
    ensure_role_with_scopes_seaorm(db, backend, "Viewer", ROLE_LEVEL_PROJECT, project_id, PROJECT_VIEWER_SCOPES).await?;
    Ok(())
}

async fn ensure_role_with_scopes_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    name: &str,
    level: ScopeLevel,
    project_id: i64,
    scopes: &[ScopeSlug],
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let scopes_json = serialize_scope_slugs(scopes)
        .map_err(|error| sea_orm::DbErr::Custom(format!("failed to serialize scopes: {error}")))?;
    execute_seaorm(
        db,
        backend,
        "INSERT INTO roles (name, level, project_id, scopes, deleted_at) VALUES (?, ?, ?, ?, 0) ON CONFLICT(project_id, name) DO UPDATE SET level = excluded.level, scopes = excluded.scopes, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO roles (name, level, project_id, scopes, deleted_at) VALUES ($1, $2, $3, $4, 0) ON CONFLICT(project_id, name) DO UPDATE SET level = EXCLUDED.level, scopes = EXCLUDED.scopes, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO roles (name, level, project_id, scopes, deleted_at) VALUES (?, ?, ?, ?, 0) ON DUPLICATE KEY UPDATE level = VALUES(level), scopes = VALUES(scopes), deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        vec![name.into(), level.as_str().into(), project_id.into(), scopes_json.into()],
    )
    .await
}

pub(crate) async fn ensure_default_api_keys_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    user_id: i64,
    project_id: i64,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    ensure_api_key_with_scopes_seaorm(db, backend, user_id, project_id, DEFAULT_USER_API_KEY_VALUE, DEFAULT_USER_API_KEY_NAME, "user", DEFAULT_USER_API_KEY_SCOPES).await?;
    ensure_api_key_with_scopes_seaorm(db, backend, user_id, project_id, DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_SERVICE_API_KEY_NAME, "service_account", DEFAULT_SERVICE_API_KEY_SCOPES).await?;
    ensure_api_key_with_scopes_seaorm(db, backend, user_id, project_id, NO_AUTH_API_KEY_VALUE, NO_AUTH_API_KEY_NAME, "noauth", NO_AUTH_API_KEY_SCOPES).await?;
    Ok(())
}

async fn ensure_api_key_with_scopes_seaorm<C>(
    db: &C,
    backend: DatabaseBackend,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    scopes: &[ScopeSlug],
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let scopes_json = serialize_scope_slugs(scopes)
        .map_err(|error| sea_orm::DbErr::Custom(format!("failed to serialize scopes: {error}")))?;
    execute_seaorm(
        db,
        backend,
        "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at) VALUES (?, ?, ?, ?, ?, 'enabled', ?, '{}', 0) ON CONFLICT(key) DO UPDATE SET name = excluded.name, type = excluded.type, scopes = excluded.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at) VALUES ($1, $2, $3, $4, $5, 'enabled', $6, '{}', 0) ON CONFLICT(key) DO UPDATE SET name = EXCLUDED.name, type = EXCLUDED.type, scopes = EXCLUDED.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        "INSERT INTO api_keys (user_id, project_id, `key`, name, type, status, scopes, profiles, deleted_at) VALUES (?, ?, ?, ?, ?, 'enabled', ?, '{}', 0) ON DUPLICATE KEY UPDATE name = VALUES(name), type = VALUES(type), scopes = VALUES(scopes), status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        vec![
            user_id.into(),
            project_id.into(),
            key.into(),
            name.into(),
            key_type.into(),
            scopes_json.into(),
        ],
    )
    .await
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

pub(crate) fn ensure_all_foundation_tables(db: &SqlConnection) -> SqlResult<()> {
    ensure_systems_table(db)?;
    db.execute_batch(DATA_STORAGES_TABLE_SQL)?;
    ensure_identity_tables(db)?;
    ensure_trace_tables(db)?;
    ensure_channel_model_tables(db)?;
    ensure_request_tables(db)?;
    db.execute_batch(USAGE_LOGS_TABLE_SQL)
}

pub(crate) fn ensure_systems_table(db: &SqlConnection) -> SqlResult<()> {
    db.execute_batch(SYSTEMS_TABLE_SQL)
}

pub(crate) fn ensure_identity_tables(db: &SqlConnection) -> SqlResult<()> {
    db.execute_batch(USERS_TABLE_SQL)?;
    db.execute_batch(PROJECTS_TABLE_SQL)?;
    db.execute_batch(USER_PROJECTS_TABLE_SQL)?;
    db.execute_batch(ROLES_TABLE_SQL)?;
    db.execute_batch(USER_ROLES_TABLE_SQL)?;
    db.execute_batch(API_KEYS_TABLE_SQL)
}

pub(crate) fn ensure_trace_tables(db: &SqlConnection) -> SqlResult<()> {
    db.execute_batch(THREADS_TABLE_SQL)?;
    db.execute_batch(TRACES_TABLE_SQL)
}

pub(crate) fn ensure_channel_model_tables(db: &SqlConnection) -> SqlResult<()> {
    db.execute_batch(CHANNELS_TABLE_SQL)?;
    db.execute_batch(MODELS_TABLE_SQL)
}

pub(crate) fn ensure_request_tables(db: &SqlConnection) -> SqlResult<()> {
    db.execute_batch(REQUESTS_TABLE_SQL)?;
    db.execute_batch(REQUEST_EXECUTIONS_TABLE_SQL)
}

pub(crate) fn ensure_operational_tables(db: &SqlConnection) -> SqlResult<()> {
    db.execute_batch(CHANNEL_PROBES_TABLE_SQL)?;
    db.execute_batch(PROVIDER_QUOTA_STATUSES_TABLE_SQL)
}

pub(crate) fn upsert_system_value_on_connection(
    db: &SqlConnection,
    key: &str,
    value: &str,
) -> SqlResult<()> {
    db.execute(
        "INSERT INTO systems (key, value, deleted_at) VALUES (?1, ?2, 0)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )?;
    Ok(())
}

pub(crate) fn query_is_initialized(db: &SqlConnection) -> SqlResult<bool> {
    let value: Option<String> = db
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

pub(crate) fn ensure_primary_data_storage(tx: &Transaction<'_>) -> SqlResult<i64> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM data_storages WHERE \"primary\" = 1 AND deleted_at = 0 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    tx.execute(
        "INSERT INTO data_storages (name, description, \"primary\", type, settings, status) VALUES (?1, ?2, 1, 'database', ?3, 'active')",
        params![
            PRIMARY_DATA_STORAGE_NAME,
            PRIMARY_DATA_STORAGE_DESCRIPTION,
            PRIMARY_DATA_STORAGE_SETTINGS_JSON,
        ],
    )?;

    Ok(tx.last_insert_rowid())
}

pub(crate) fn generate_secret_key(tx: &Transaction<'_>) -> SqlResult<String> {
    tx.query_row("SELECT lower(hex(randomblob(32)))", [], |row| row.get(0))
}

pub(crate) fn hash_password(password: &str) -> SqlResult<String> {
    hash(password, DEFAULT_COST)
        .map(|hashed| hex_encode(hashed.as_bytes()))
        .map_err(sql_conversion_error)
}

pub(crate) fn verify_password(stored_hex: &str, password: &str) -> bool {
    hex::decode(stored_hex)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|hash| verify(password, &hash).ok())
        .unwrap_or(false)
}

pub(crate) fn ensure_owner_user(
    tx: &Transaction<'_>,
    request: &InitializeSystemRequest,
) -> SqlResult<i64> {
    let existing: Option<i64> = tx
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
    tx.execute(
        "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at)
         VALUES (?1, 'activated', 'en', ?2, ?3, ?4, '', 1, '[]', 0)",
        params![
            request.owner_email.trim(),
            password_hash,
            request.owner_first_name.trim(),
            request.owner_last_name.trim(),
        ],
    )?;

    Ok(tx.last_insert_rowid())
}

pub(crate) fn ensure_default_project(tx: &Transaction<'_>) -> SqlResult<i64> {
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM projects WHERE deleted_at = 0 ORDER BY id ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    tx.execute(
        "INSERT INTO projects (name, description, status, deleted_at) VALUES (?1, ?2, 'active', 0)",
        params![DEFAULT_PROJECT_NAME, DEFAULT_PROJECT_DESCRIPTION],
    )?;

    Ok(tx.last_insert_rowid())
}

pub(crate) fn ensure_owner_project_membership(
    tx: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
) -> SqlResult<()> {
    tx.execute(
        "INSERT INTO user_projects (user_id, project_id, is_owner, scopes)
         VALUES (?1, ?2, 1, '[]')
         ON CONFLICT(user_id, project_id) DO UPDATE SET is_owner = 1, updated_at = CURRENT_TIMESTAMP",
        params![user_id, project_id],
    )?;

    Ok(())
}

pub(crate) fn ensure_default_project_roles(
    tx: &Transaction<'_>,
    project_id: i64,
) -> SqlResult<()> {
    ensure_role_with_scopes(tx, "Admin", ROLE_LEVEL_PROJECT, project_id, PROJECT_ADMIN_SCOPES)?;
    ensure_role_with_scopes(
        tx,
        "Developer",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_DEVELOPER_SCOPES,
    )?;
    ensure_role_with_scopes(tx, "Viewer", ROLE_LEVEL_PROJECT, project_id, PROJECT_VIEWER_SCOPES)?;

    Ok(())
}

pub(crate) fn ensure_role_with_scopes(
    tx: &Transaction<'_>,
    name: &str,
    level: ScopeLevel,
    project_id: i64,
    scopes: &[ScopeSlug],
) -> SqlResult<()> {
    let scopes_json = serialize_scope_slugs(scopes)
        .map_err(sql_conversion_error)?;
    tx.execute(
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
    tx: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
) -> SqlResult<()> {
    ensure_api_key_with_scopes(
        tx,
        user_id,
        project_id,
        DEFAULT_USER_API_KEY_VALUE,
        DEFAULT_USER_API_KEY_NAME,
        "user",
        DEFAULT_USER_API_KEY_SCOPES,
    )?;
    ensure_api_key_with_scopes(
        tx,
        user_id,
        project_id,
        DEFAULT_SERVICE_API_KEY_VALUE,
        DEFAULT_SERVICE_API_KEY_NAME,
        "service_account",
        DEFAULT_SERVICE_API_KEY_SCOPES,
    )?;
    ensure_api_key_with_scopes(
        tx,
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
    tx: &Transaction<'_>,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    scopes: &[ScopeSlug],
) -> SqlResult<()> {
    let scopes_json = serialize_scope_slugs(scopes)
        .map_err(sql_conversion_error)?;
    tx.execute(
        "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', ?6, '{}', 0)
         ON CONFLICT(key) DO UPDATE SET name = excluded.name, type = excluded.type, scopes = excluded.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        params![user_id, project_id, key, name, key_type, scopes_json],
    )?;

    Ok(())
}

pub(crate) fn query_system_value(db: &SqlConnection, key: &str) -> SqlResult<Option<String>> {
    db.query_row(
        "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
        [key],
        |row| row.get(0),
    )
    .optional()
}

pub(crate) fn upsert_system_value(
    tx: &Transaction<'_>,
    key: &str,
    value: &str,
) -> Result<(), SystemInitializeError> {
    tx.execute(
        "INSERT INTO systems (key, value, deleted_at) VALUES (?1, ?2, 0)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
}
