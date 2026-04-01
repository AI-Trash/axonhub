use axonhub_http::AdminError;
use axonhub_db_entity::{data_storages, requests};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

use crate::foundation::seaorm::SeaOrmConnectionFactory;

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
    _backend: sea_orm::DatabaseBackend,
    request_id: i64,
) -> Result<Option<StoredRequestContentRecord>, AdminError> {
    requests::Entity::find_by_id(request_id)
        .into_partial_model::<requests::ContentStorageLookup>()
        .one(db)
        .await
        .map_err(|error| AdminError::Internal {
            message: format!("SeaORM admin query failed: {error}"),
        })
        .map(|row| {
            row.map(|row| StoredRequestContentRecord {
                id: row.id,
                project_id: row.project_id,
                content_saved: row.content_saved,
                content_storage_id: row.content_storage_id,
                content_storage_key: row.content_storage_key,
            })
        })
}

async fn query_data_storage_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    _backend: sea_orm::DatabaseBackend,
    storage_id: i64,
) -> Result<Option<DataStorageRecord>, AdminError> {
    data_storages::Entity::find_by_id(storage_id)
        .filter(data_storages::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<data_storages::StorageConfig>()
        .one(db)
        .await
        .map_err(|error| AdminError::Internal {
            message: format!("SeaORM admin query failed: {error}"),
        })
        .map(|row| {
            row.map(|row| DataStorageRecord {
                storage_type: row.storage_type,
                settings_json: row.settings,
            })
        })
}
