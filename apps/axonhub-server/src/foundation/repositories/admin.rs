use axonhub_http::AdminError;

use crate::foundation::seaorm::SeaOrmConnectionFactory;

use super::common::query_one;

#[derive(Debug, Clone)]
pub(crate) struct StoredRequestContentRecord {
    pub(crate) id: i64,
    pub(crate) project_id: i64,
    pub(crate) content_saved: bool,
    pub(crate) content_storage_id: Option<i64>,
    pub(crate) content_storage_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DataStorageRecord {
    pub(crate) storage_type: String,
    pub(crate) settings_json: String,
}

pub(crate) trait AdminStorageRepository: Send + Sync {
    fn query_request_content_record(
        &self,
        request_id: i64,
    ) -> Result<Option<StoredRequestContentRecord>, AdminError>;
    fn query_data_storage(&self, storage_id: i64) -> Result<Option<DataStorageRecord>, AdminError>;
}

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmAdminStorageRepository {
    db: SeaOrmConnectionFactory,
}

impl SeaOrmAdminStorageRepository {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl AdminStorageRepository for SeaOrmAdminStorageRepository {
    fn query_request_content_record(
        &self,
        request_id: i64,
    ) -> Result<Option<StoredRequestContentRecord>, AdminError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| AdminError::Internal {
                message: format!("Failed to connect through SeaORM: {error}"),
            })?;
            query_request_content_record_seaorm(&connection, db.backend(), request_id).await
        })
    }

    fn query_data_storage(&self, storage_id: i64) -> Result<Option<DataStorageRecord>, AdminError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| AdminError::Internal {
                message: format!("Failed to connect through SeaORM: {error}"),
            })?;
            query_data_storage_seaorm(&connection, db.backend(), storage_id).await
        })
    }
}

async fn query_request_content_record_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    backend: sea_orm::DatabaseBackend,
    request_id: i64,
) -> Result<Option<StoredRequestContentRecord>, AdminError> {
    query_one_admin(
        db,
        backend,
        "SELECT id, project_id, content_saved, content_storage_id, content_storage_key FROM requests WHERE id = ? LIMIT 1",
        "SELECT id, project_id, content_saved, content_storage_id, content_storage_key FROM requests WHERE id = $1 LIMIT 1",
        "SELECT id, project_id, content_saved, content_storage_id, content_storage_key FROM requests WHERE id = ? LIMIT 1",
        vec![request_id.into()],
    )
    .await
    .map(|row| {
        row.map(|row| StoredRequestContentRecord {
            id: row.try_get_by_index(0).unwrap_or_default(),
            project_id: row.try_get_by_index(1).unwrap_or_default(),
            content_saved: row.try_get_by_index(2).unwrap_or(false),
            content_storage_id: row.try_get_by_index(3).ok(),
            content_storage_key: row.try_get_by_index(4).ok(),
        })
    })
}

async fn query_data_storage_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    backend: sea_orm::DatabaseBackend,
    storage_id: i64,
) -> Result<Option<DataStorageRecord>, AdminError> {
    query_one_admin(
        db,
        backend,
        "SELECT id, name, description, type, status, settings FROM data_storages WHERE id = ? AND deleted_at = 0 LIMIT 1",
        "SELECT id, name, description, type, status, settings FROM data_storages WHERE id = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT id, name, description, type, status, settings FROM data_storages WHERE id = ? AND deleted_at = 0 LIMIT 1",
        vec![storage_id.into()],
    )
    .await
    .map(|row| {
        row.map(|row| DataStorageRecord {
            storage_type: row.try_get_by_index(3).unwrap_or_default(),
            settings_json: row.try_get_by_index(5).unwrap_or_default(),
        })
    })
}

async fn query_one_admin(
    db: &impl sea_orm::ConnectionTrait,
    backend: sea_orm::DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<sea_orm::QueryResult>, AdminError> {
    query_one(db, backend, sqlite_sql, postgres_sql, mysql_sql, values)
        .await
        .map_err(|error| AdminError::Internal {
            message: format!("SeaORM admin query failed: {error}"),
        })
}
