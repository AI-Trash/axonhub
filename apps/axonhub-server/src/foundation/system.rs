use axonhub_http::{InitializeSystemRequest, SystemBootstrapPort, SystemInitializeError, SystemQueryError};

pub(crate) use super::sqlite_support::{
    ensure_all_foundation_tables, ensure_channel_model_tables, ensure_identity_tables,
    ensure_operational_tables, ensure_prompt_tables, ensure_request_tables, hash_password, verify_password,
    SeaOrmDbFactory, SqliteBootstrapService, SystemSettingsStore,
};
use super::sqlite_support;

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
            sqlite_support::seaorm_is_initialized(&db)
                .await
                .map_err(map_db_query_error)
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
            sqlite_support::seaorm_initialize(&db, &version, &request)
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
