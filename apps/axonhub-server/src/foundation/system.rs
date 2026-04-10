use axonhub_http::{InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError, SystemQueryError};

use super::bootstrap_seaorm::{seaorm_initialize, seaorm_is_initialized, SeaOrmDbFactory};
pub(crate) use super::passwords::{hash_password, verify_password};

#[cfg(test)]
pub(crate) use sqlite_test_support::{SqliteBootstrapService, SqliteFoundation};

pub struct SeaOrmBootstrapService {
    db: SeaOrmDbFactory,
    version: String,
}

impl SeaOrmBootstrapService {
    pub fn new(db: SeaOrmDbFactory, version: String) -> Self {
        Self { db, version }
    }
}

impl SystemBootstrapPort for SeaOrmBootstrapService {
    fn is_initialized(&self) -> Result<bool, SystemQueryError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            seaorm_is_initialized(&db).await.map_err(map_db_query_error)
        })
    }

    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
        let db = self.db.clone();
        let version = self.version.clone();
        let request = InitializeSystemRequest {
            owner_email: request.owner_email.clone(),
            owner_password: request.owner_password.clone(),
            owner_first_name: request.owner_first_name.clone(),
            owner_last_name: request.owner_last_name.clone(),
            brand_name: request.brand_name.clone(),
        };

        db.run_sync(move |db| async move {
            seaorm_initialize(&db, &version, &request)
                .await
                .map_err(map_db_init_error)
        })
        .map_err(|error| match error {
            SystemInitializeError::InitializeFailed(message)
                if message == "system already initialized" =>
            {
                SystemInitializeError::AlreadyInitialized
            }
            other => other,
        })
    }
}

impl super::ports::SystemBootstrapRepository for SeaOrmBootstrapService {
    fn is_initialized(&self) -> Result<bool, SystemQueryError> {
        <Self as SystemBootstrapPort>::is_initialized(self)
    }

    fn initialize(&self, request: &InitializeSystemRequest) -> Result<(), SystemInitializeError> {
        <Self as SystemBootstrapPort>::initialize(self, request)
    }
}

fn map_db_query_error(_: sea_orm::DbErr) -> SystemQueryError {
    SystemQueryError::QueryFailed
}

fn map_db_init_error(error: sea_orm::DbErr) -> SystemInitializeError {
    SystemInitializeError::InitializeFailed(error.to_string())
}

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use std::sync::Arc;

    use axonhub_db_entity::{data_storages, systems};
    use axonhub_http::{
        InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError, SystemQueryError,
    };
    use rusqlite::{
        params, Connection as SqlConnection, Error as SqlError, OpenFlags, OptionalExtension,
        Result as SqlResult, Transaction,
    };
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    pub(crate) use super::super::bootstrap_seaorm::{
        seaorm_initialize, seaorm_is_initialized, SeaOrmDbFactory,
    };
    use super::super::{
        authz::{
            serialize_scope_slugs, ScopeLevel, ScopeSlug, DEFAULT_SERVICE_API_KEY_SCOPES,
            DEFAULT_USER_API_KEY_SCOPES, NO_AUTH_API_KEY_SCOPES, PROJECT_ADMIN_SCOPES,
            PROJECT_DEVELOPER_SCOPES, PROJECT_VIEWER_SCOPES, ROLE_LEVEL_PROJECT,
        },
        bootstrap_seaorm::upsert_system_value_seaorm,
        seaorm::SeaOrmConnectionFactory,
        shared::{
            API_KEYS_TABLE_SQL, CHANNELS_TABLE_SQL, CHANNEL_PROBES_TABLE_SQL,
            DATA_STORAGES_TABLE_SQL, DEFAULT_PROJECT_DESCRIPTION, DEFAULT_PROJECT_NAME,
            DEFAULT_SERVICE_API_KEY_NAME, DEFAULT_SERVICE_API_KEY_VALUE,
            DEFAULT_USER_API_KEY_NAME, DEFAULT_USER_API_KEY_VALUE, MODELS_TABLE_SQL,
            NO_AUTH_API_KEY_NAME, NO_AUTH_API_KEY_VALUE, PRIMARY_DATA_STORAGE_DESCRIPTION,
            PRIMARY_DATA_STORAGE_NAME, PRIMARY_DATA_STORAGE_SETTINGS_JSON, PROJECTS_TABLE_SQL,
            PROMPTS_TABLE_SQL, PROMPT_PROTECTION_RULES_TABLE_SQL,
            PROVIDER_QUOTA_STATUSES_TABLE_SQL, REALTIME_SESSIONS_TABLE_SQL,
            REQUESTS_TABLE_SQL, REQUEST_EXECUTIONS_TABLE_SQL, ROLES_TABLE_SQL,
            SYSTEMS_TABLE_SQL, SYSTEM_KEY_BRAND_NAME, SYSTEM_KEY_DEFAULT_DATA_STORAGE,
            SYSTEM_KEY_INITIALIZED, SYSTEM_KEY_ONBOARDED, SYSTEM_KEY_SECRET_KEY,
            SYSTEM_KEY_VERSION, THREADS_TABLE_SQL, TRACES_TABLE_SQL, USERS_TABLE_SQL,
            USER_PROJECTS_TABLE_SQL, USER_ROLES_TABLE_SQL, USAGE_LOGS_TABLE_SQL,
        },
    };
    use super::super::request_context::{serialize_onboarding_record, OnboardingRecord};
    pub(crate) use super::{hash_password, verify_password};

    fn sql_conversion_error(
        error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> SqlError {
        SqlError::ToSqlConversionFailure(error.into())
    }

    fn sql_error_from_seaorm(error: sea_orm::DbErr) -> SqlError {
        SqlError::ToSqlConversionFailure(Box::new(std::io::Error::other(error.to_string())))
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

        pub fn identities(&self) -> super::super::identity_service::sqlite_test_support::IdentityStore {
            super::super::identity_service::sqlite_test_support::IdentityStore::new(
                self.connection_factory.clone(),
            )
        }

        pub fn identity_auth(
            &self,
            allow_no_auth: bool,
        ) -> super::super::identity_service::SeaOrmIdentityService {
            super::super::identity_service::SeaOrmIdentityService::new(
                self.seaorm(),
                allow_no_auth,
            )
        }

        pub fn trace_contexts(
            &self,
        ) -> super::super::request_context::sqlite_test_support::TraceContextStore {
            super::super::request_context::sqlite_test_support::TraceContextStore::new(
                self.connection_factory.clone(),
            )
        }

        pub fn request_context_service(
            &self,
            allow_no_auth: bool,
        ) -> super::super::request_context_service::RequestContextService {
            super::super::request_context_service::RequestContextService::new(
                self.identity_auth(allow_no_auth),
                self.trace_contexts(),
            )
        }

        pub fn channel_models(
            &self,
        ) -> super::super::repositories::openai_v1::sqlite_test_support::ChannelModelStore {
            super::super::repositories::openai_v1::sqlite_test_support::ChannelModelStore::new(
                self.connection_factory.clone(),
            )
        }

        pub fn requests(
            &self,
        ) -> super::super::repositories::openai_v1::sqlite_test_support::RequestStore {
            super::super::repositories::openai_v1::sqlite_test_support::RequestStore::new(
                self.connection_factory.clone(),
            )
        }

        pub fn usage_costs(
            &self,
        ) -> super::super::repositories::openai_v1::sqlite_test_support::UsageCostStore {
            super::super::repositories::openai_v1::sqlite_test_support::UsageCostStore::new(
                self.connection_factory.clone(),
            )
        }

        pub fn operational(
            &self,
        ) -> super::super::admin_operational::sqlite_test_support::OperationalStore {
            super::super::admin_operational::sqlite_test_support::OperationalStore::new(
                self.connection_factory.clone(),
            )
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

        pub(crate) fn ensure_schema(&self) -> SqlResult<()> {
            let db = self.connection_factory.open(true)?;
            ensure_systems_table(&db)
        }

        pub(crate) fn is_initialized(&self) -> SqlResult<bool> {
            let factory = SeaOrmConnectionFactory::sqlite(self.connection_factory.dsn.as_ref().clone());
            factory
                .run_sync(move |db| async move { seaorm_is_initialized(&db).await })
                .map_err(sql_error_from_seaorm)
        }

        pub(crate) fn value(&self, key: &str) -> SqlResult<Option<String>> {
            let key = key.to_owned();
            let factory = SeaOrmConnectionFactory::sqlite(self.connection_factory.dsn.as_ref().clone());
            factory
                .run_sync(move |db| async move {
                    let connection = db.connect_migrated().await?;
                    systems::Entity::find()
                        .filter(systems::Column::Key.eq(key))
                        .filter(systems::Column::DeletedAt.eq(0_i64))
                        .into_partial_model::<systems::KeyValue>()
                        .one(&connection)
                        .await
                        .map(|row| row.map(|row| row.value))
                })
                .map_err(sql_error_from_seaorm)
        }

        pub(crate) fn default_data_storage_id(&self) -> SqlResult<Option<i64>> {
            self.value(SYSTEM_KEY_DEFAULT_DATA_STORAGE)
                .map(|value| value.and_then(|current| current.parse::<i64>().ok()))
        }

        pub(crate) fn set_value(&self, key: &str, value: &str) -> SqlResult<()> {
            let key = key.to_owned();
            let value = value.to_owned();
            let factory = SeaOrmConnectionFactory::sqlite(self.connection_factory.dsn.as_ref().clone());
            factory
                .run_sync(move |db| async move {
                    let connection = db.connect_migrated().await?;
                    upsert_system_value_seaorm(&connection, &key, &value).await
                })
                .map_err(sql_error_from_seaorm)
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

    #[derive(Debug, Clone)]
    pub struct DataStorageStore {
        connection_factory: SqliteConnectionFactory,
    }

    impl DataStorageStore {
        pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
            Self { connection_factory }
        }

        pub(crate) fn ensure_schema(&self) -> SqlResult<()> {
            let db = self.connection_factory.open(true)?;
            db.execute_batch(DATA_STORAGES_TABLE_SQL)
        }

        pub(crate) fn find_primary_active_storage(&self) -> SqlResult<Option<StoredDataStorage>> {
            let factory = SeaOrmConnectionFactory::sqlite(self.connection_factory.dsn.as_ref().clone());
            factory
                .run_sync(move |db| async move {
                    let connection = db.connect_migrated().await?;
                    data_storages::Entity::find()
                        .filter(data_storages::Column::PrimaryFlag.eq(true))
                        .filter(data_storages::Column::DeletedAt.eq(0_i64))
                        .one(&connection)
                        .await
                        .map(|row| {
                            row.map(|row| StoredDataStorage {
                                id: row.id,
                                name: row.name,
                                description: row.description,
                                storage_type: row.type_field,
                                status: row.status,
                                settings_json: row.settings,
                            })
                        })
                })
                .map_err(sql_error_from_seaorm)
        }

        pub(crate) fn find_storage_by_id(
            &self,
            storage_id: i64,
        ) -> SqlResult<Option<StoredDataStorage>> {
            let factory = SeaOrmConnectionFactory::sqlite(self.connection_factory.dsn.as_ref().clone());
            factory
                .run_sync(move |db| async move {
                    let connection = db.connect_migrated().await?;
                    data_storages::Entity::find_by_id(storage_id)
                        .filter(data_storages::Column::DeletedAt.eq(0_i64))
                        .one(&connection)
                        .await
                        .map(|row| {
                            row.map(|row| StoredDataStorage {
                                id: row.id,
                                name: row.name,
                                description: row.description,
                                storage_type: row.type_field,
                                status: row.status,
                                settings_json: row.settings,
                            })
                        })
                })
                .map_err(sql_error_from_seaorm)
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

        fn initialize(
            &self,
            request: &InitializeSystemRequest,
        ) -> Result<(), SystemInitializeError> {
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
            let onboarding = default_onboarding_record();
            let onboarding = serialize_onboarding_record(&onboarding)
                .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))?;
            upsert_system_value(&tx, SYSTEM_KEY_ONBOARDED, &onboarding)?;
            upsert_system_value(&tx, SYSTEM_KEY_INITIALIZED, "true")?;

            tx.commit()
                .map_err(|error| SystemInitializeError::InitializeFailed(error.to_string()))
        }
    }

    impl super::super::ports::SystemBootstrapRepository for SqliteBootstrapService {
        fn is_initialized(&self) -> Result<bool, SystemQueryError> {
            <Self as SystemBootstrapPort>::is_initialized(self)
        }

        fn initialize(
            &self,
            request: &InitializeSystemRequest,
        ) -> Result<(), SystemInitializeError> {
            <Self as SystemBootstrapPort>::initialize(self, request)
        }
    }

    pub(crate) fn ensure_all_foundation_tables(db: &SqlConnection) -> SqlResult<()> {
        ensure_systems_table(db)?;
        db.execute_batch(DATA_STORAGES_TABLE_SQL)?;
        ensure_identity_tables(db)?;
        ensure_trace_tables(db)?;
        ensure_channel_model_tables(db)?;
        ensure_prompt_tables(db)?;
        ensure_request_tables(db)?;
        db.execute_batch(USAGE_LOGS_TABLE_SQL)?;
        ensure_operational_tables(db)
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
        db.execute_batch(REQUEST_EXECUTIONS_TABLE_SQL)?;
        db.execute_batch(REALTIME_SESSIONS_TABLE_SQL)
    }

    pub(crate) fn ensure_prompt_tables(db: &SqlConnection) -> SqlResult<()> {
        db.execute_batch(PROMPTS_TABLE_SQL)?;
        db.execute_batch(PROMPT_PROTECTION_RULES_TABLE_SQL)
    }

    pub(crate) fn ensure_operational_tables(db: &SqlConnection) -> SqlResult<()> {
        db.execute_batch(CHANNEL_PROBES_TABLE_SQL)?;
        db.execute_batch(PROVIDER_QUOTA_STATUSES_TABLE_SQL)
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

        let password_hash = hash_password(request.owner_password.trim()).map_err(sql_conversion_error)?;
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
        let scopes_json = serialize_scope_slugs(scopes).map_err(sql_conversion_error)?;
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
        let scopes_json = serialize_scope_slugs(scopes).map_err(sql_conversion_error)?;
        tx.execute(
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', ?6, '{}', 0)
             ON CONFLICT(key) DO UPDATE SET name = excluded.name, type = excluded.type, scopes = excluded.scopes, status = 'enabled', deleted_at = 0, updated_at = CURRENT_TIMESTAMP",
            params![user_id, project_id, key, name, key_type, scopes_json],
        )?;

        Ok(())
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

    fn default_onboarding_record() -> OnboardingRecord {
        OnboardingRecord::default()
    }
}
