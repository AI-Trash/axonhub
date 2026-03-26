use axonhub_db_entity::{api_keys, models, systems};
use axonhub_http::AuthApiKeyContext;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use crate::foundation::seaorm::SeaOrmConnectionFactory;

use super::common::query_all;

#[derive(Debug, Clone)]
pub(crate) struct GraphqlModelStatusRecord {
    pub(crate) id: i64,
    pub(crate) status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct OwnerApiKeyRecord {
    pub(crate) user_id: i64,
    pub(crate) key_type: String,
    pub(crate) project_id: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlStoragePolicyRecord {
    pub(crate) value: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlAutoBackupSettingsRecord {
    pub(crate) value: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlSystemChannelSettingsRecord {
    pub(crate) value: String,
}

pub(crate) trait AdminGraphqlSubsetRepository: Send + Sync {
    fn query_model_statuses(&self) -> Result<Vec<GraphqlModelStatusRecord>, String>;
    fn query_storage_policy(&self) -> Result<Option<GraphqlStoragePolicyRecord>, String>;
    fn upsert_storage_policy(&self, value: &str) -> Result<(), String>;
    fn query_auto_backup_settings(&self) -> Result<Option<GraphqlAutoBackupSettingsRecord>, String>;
    fn upsert_auto_backup_settings(&self, value: &str) -> Result<(), String>;
    fn query_system_channel_settings(
        &self,
    ) -> Result<Option<GraphqlSystemChannelSettingsRecord>, String>;
    fn upsert_system_channel_settings(&self, value: &str) -> Result<(), String>;
}

pub(crate) trait OpenApiGraphqlMutationRepository: Send + Sync {
    fn query_owner_api_key(&self, owner_key: &str) -> Result<Option<OwnerApiKeyRecord>, String>;
    fn insert_llm_api_key(
        &self,
        owner_user_id: i64,
        owner_api_key: &AuthApiKeyContext,
        generated_key: &str,
        trimmed_name: &str,
        scopes_json: &str,
    ) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmAdminGraphqlSubsetRepository {
    db: SeaOrmConnectionFactory,
}

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmOpenApiGraphqlMutationRepository {
    db: SeaOrmConnectionFactory,
}

impl SeaOrmAdminGraphqlSubsetRepository {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl SeaOrmOpenApiGraphqlMutationRepository {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl AdminGraphqlSubsetRepository for SeaOrmAdminGraphqlSubsetRepository {
    fn query_model_statuses(&self) -> Result<Vec<GraphqlModelStatusRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_model_statuses_seaorm(&connection).await
        })
    }

    fn query_storage_policy(&self) -> Result<Option<GraphqlStoragePolicyRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_system_json_setting_seaorm(&connection, "storage_policy")
                .await
                .map(|value| value.map(|value| GraphqlStoragePolicyRecord { value }))
        })
    }

    fn upsert_storage_policy(&self, value: &str) -> Result<(), String> {
        let db = self.db.clone();
        let value = value.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            upsert_system_json_setting_seaorm(&connection, "storage_policy", &value).await
        })
    }

    fn query_auto_backup_settings(&self) -> Result<Option<GraphqlAutoBackupSettingsRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_system_json_setting_seaorm(&connection, "system_auto_backup_settings")
                .await
                .map(|value| value.map(|value| GraphqlAutoBackupSettingsRecord { value }))
        })
    }

    fn upsert_auto_backup_settings(&self, value: &str) -> Result<(), String> {
        let db = self.db.clone();
        let value = value.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            upsert_system_json_setting_seaorm(&connection, "system_auto_backup_settings", &value)
                .await
        })
    }

    fn query_system_channel_settings(
        &self,
    ) -> Result<Option<GraphqlSystemChannelSettingsRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_system_json_setting_seaorm(&connection, "system_channel_settings")
                .await
                .map(|value| value.map(|value| GraphqlSystemChannelSettingsRecord { value }))
        })
    }

    fn upsert_system_channel_settings(&self, value: &str) -> Result<(), String> {
        let db = self.db.clone();
        let value = value.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            upsert_system_json_setting_seaorm(&connection, "system_channel_settings", &value).await
        })
    }
}

impl OpenApiGraphqlMutationRepository for SeaOrmOpenApiGraphqlMutationRepository {
    fn query_owner_api_key(&self, owner_key: &str) -> Result<Option<OwnerApiKeyRecord>, String> {
        let db = self.db.clone();
        let owner_key = owner_key.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_owner_api_key_seaorm(&connection, &owner_key).await
        })
    }

    fn insert_llm_api_key(
        &self,
        owner_user_id: i64,
        owner_api_key: &AuthApiKeyContext,
        generated_key: &str,
        trimmed_name: &str,
        scopes_json: &str,
    ) -> Result<(), String> {
        let db = self.db.clone();
        let owner_api_key = owner_api_key.clone();
        let generated_key = generated_key.to_owned();
        let trimmed_name = trimmed_name.to_owned();
        let scopes_json = scopes_json.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            insert_llm_api_key_seaorm(
                &connection,
                owner_user_id,
                owner_api_key.project.id,
                &generated_key,
                &trimmed_name,
                &scopes_json,
            )
            .await
        })
    }
}

pub(crate) async fn query_all_graphql(
    db: &impl sea_orm::ConnectionTrait,
    backend: sea_orm::DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>, String> {
    query_all(db, backend, sqlite_sql, postgres_sql, mysql_sql, values)
        .await
        .map_err(|error| error.to_string())
}

async fn query_model_statuses_seaorm(
    db: &impl sea_orm::ConnectionTrait,
) -> Result<Vec<GraphqlModelStatusRecord>, String> {
    models::Entity::find()
        .filter(models::Column::DeletedAt.eq(0_i64))
        .order_by_asc(models::Column::Id)
        .into_partial_model::<models::GraphqlStatus>()
        .all(db)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| GraphqlModelStatusRecord {
                    id: row.id,
                    status: row.status,
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

async fn query_system_json_setting_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    key: &str,
) -> Result<Option<String>, String> {
    systems::Entity::find()
        .filter(systems::Column::Key.eq(key))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<systems::KeyValue>()
        .one(db)
        .await
        .map(|row| row.map(|row| row.value))
        .map_err(|error| error.to_string())
}

async fn upsert_system_json_setting_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let existing = systems::Entity::find()
        .filter(systems::Column::Key.eq(key))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;

    if let Some(existing) = existing {
        let mut active_model: systems::ActiveModel = existing.into();
        active_model.value = Set(value.to_owned());
        active_model.deleted_at = Set(0_i64);
        active_model.update(db).await.map_err(|error| error.to_string())?;
        return Ok(());
    }

    systems::Entity::insert(systems::ActiveModel {
        key: Set(key.to_owned()),
        value: Set(value.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

async fn query_owner_api_key_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    owner_key: &str,
) -> Result<Option<OwnerApiKeyRecord>, String> {
    api_keys::Entity::find()
        .filter(api_keys::Column::Key.eq(owner_key))
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<api_keys::OwnerLookup>()
        .one(db)
        .await
        .map(|row| {
            row.map(|row| OwnerApiKeyRecord {
                user_id: row.user_id,
                key_type: row.key_type,
                project_id: row.project_id,
            })
        })
        .map_err(|error| error.to_string())
}

async fn insert_llm_api_key_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    owner_user_id: i64,
    project_id: i64,
    generated_key: &str,
    trimmed_name: &str,
    scopes_json: &str,
) -> Result<(), String> {
    api_keys::Entity::insert(api_keys::ActiveModel {
        user_id: Set(owner_user_id),
        project_id: Set(project_id),
        key: Set(generated_key.to_owned()),
        name: Set(trimmed_name.to_owned()),
        type_field: Set("user".to_owned()),
        status: Set("enabled".to_owned()),
        scopes: Set(scopes_json.to_owned()),
        profiles: Set("{}".to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}
