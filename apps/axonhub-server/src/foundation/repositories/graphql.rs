use axonhub_db_entity::{
    api_keys, channels, data_storages, models, projects, provider_quota_statuses, roles, systems,
    user_projects, user_roles, users,
};
use axonhub_http::AuthApiKeyContext;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};

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

#[derive(Debug, Clone)]
pub(crate) struct GraphqlDefaultDataStorageRecord {
    pub(crate) value: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlDataStorageStatusRecord {
    pub(crate) id: i64,
    pub(crate) status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlDataStorageConfigRecord {
    pub(crate) storage_type: String,
    pub(crate) settings: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlRoleSummaryRecord {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) scopes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlUserProjectMembershipRecord {
    pub(crate) project_id: i64,
    pub(crate) is_owner: bool,
    pub(crate) scopes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlUserProfileRecord {
    pub(crate) id: i64,
    pub(crate) email: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) is_owner: bool,
    pub(crate) prefer_language: String,
    pub(crate) avatar: Option<String>,
    pub(crate) scopes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlUserRecord {
    pub(crate) id: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) email: String,
    pub(crate) status: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) is_owner: bool,
    pub(crate) prefer_language: String,
    pub(crate) avatar: Option<String>,
    pub(crate) scopes: String,
}

pub(crate) trait AdminGraphqlSubsetRepository: Send + Sync {
    fn query_model_statuses(&self) -> Result<Vec<GraphqlModelStatusRecord>, String>;
    fn query_default_data_storage(&self) -> Result<Option<GraphqlDefaultDataStorageRecord>, String>;
    fn upsert_default_data_storage(&self, value: &str) -> Result<(), String>;
    fn query_data_storage_status(&self, id: i64) -> Result<Option<GraphqlDataStorageStatusRecord>, String>;
    fn query_data_storage_config(&self, id: i64) -> Result<Option<GraphqlDataStorageConfigRecord>, String>;
    fn query_storage_policy(&self) -> Result<Option<GraphqlStoragePolicyRecord>, String>;
    fn upsert_storage_policy(&self, value: &str) -> Result<(), String>;
    fn query_auto_backup_settings(&self) -> Result<Option<GraphqlAutoBackupSettingsRecord>, String>;
    fn upsert_auto_backup_settings(&self, value: &str) -> Result<(), String>;
    fn query_system_channel_settings(
        &self,
    ) -> Result<Option<GraphqlSystemChannelSettingsRecord>, String>;
    fn upsert_system_channel_settings(&self, value: &str) -> Result<(), String>;
    fn query_is_initialized(&self) -> Result<bool, String>;
    fn query_user_profile(&self, user_id: i64) -> Result<Option<GraphqlUserProfileRecord>, String>;
    fn query_user_projects(&self, user_id: i64) -> Result<Vec<GraphqlUserProjectMembershipRecord>, String>;
    fn query_project_roles(
        &self,
        user_id: i64,
        project_id: i64,
    ) -> Result<Vec<GraphqlRoleSummaryRecord>, String>;
    fn query_user(&self, user_id: i64) -> Result<Option<GraphqlUserRecord>, String>;
    fn query_user_roles(&self, user_id: i64) -> Result<Vec<GraphqlRoleSummaryRecord>, String>;
    fn create_user(
        &self,
        email: &str,
        status: &str,
        prefer_language: &str,
        password_hash: &str,
        first_name: &str,
        last_name: &str,
        avatar: Option<&str>,
        is_owner: bool,
        scopes_json: &str,
        project_ids: &[i64],
        role_ids: &[i64],
    ) -> Result<i64, String>;
    fn update_user_profile(
        &self,
        user_id: i64,
        first_name: Option<&str>,
        last_name: Option<&str>,
        prefer_language: Option<&str>,
        avatar: Option<&str>,
    ) -> Result<bool, String>;
    fn update_user_status(&self, user_id: i64, status: &str) -> Result<bool, String>;
    fn update_user(
        &self,
        user_id: i64,
        first_name: Option<&str>,
        last_name: Option<&str>,
        prefer_language: Option<&str>,
        avatar: Option<&str>,
        scopes_json: Option<&str>,
        role_ids: Option<&[i64]>,
    ) -> Result<bool, String>;
    fn upsert_provider_quota_statuses(
        &self,
        next_check_at: &str,
        quota_error_message: &str,
    ) -> Result<usize, String>;
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

    fn query_default_data_storage(&self) -> Result<Option<GraphqlDefaultDataStorageRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_system_json_setting_seaorm(
                &connection,
                crate::foundation::shared::SYSTEM_KEY_DEFAULT_DATA_STORAGE,
            )
            .await
            .map(|value| value.map(|value| GraphqlDefaultDataStorageRecord { value }))
        })
    }

    fn upsert_default_data_storage(&self, value: &str) -> Result<(), String> {
        let db = self.db.clone();
        let value = value.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            upsert_system_json_setting_seaorm(
                &connection,
                crate::foundation::shared::SYSTEM_KEY_DEFAULT_DATA_STORAGE,
                &value,
            )
            .await
        })
    }

    fn query_data_storage_status(&self, id: i64) -> Result<Option<GraphqlDataStorageStatusRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_data_storage_status_seaorm(&connection, id).await
        })
    }

    fn query_data_storage_config(&self, id: i64) -> Result<Option<GraphqlDataStorageConfigRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_data_storage_config_seaorm(&connection, id).await
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

    fn query_is_initialized(&self) -> Result<bool, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_system_json_setting_seaorm(&connection, "initialized")
                .await
                .map(|value| value.map_or(false, |v| v.eq_ignore_ascii_case("true")))
                .map_err(|error| error.to_string())
        })
    }

    fn query_user_profile(&self, user_id: i64) -> Result<Option<GraphqlUserProfileRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_user_profile_seaorm(&connection, user_id).await
        })
    }

    fn query_user_projects(&self, user_id: i64) -> Result<Vec<GraphqlUserProjectMembershipRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_user_projects_seaorm(&connection, user_id).await
        })
    }

    fn query_project_roles(
        &self,
        user_id: i64,
        project_id: i64,
    ) -> Result<Vec<GraphqlRoleSummaryRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_project_roles_seaorm(&connection, user_id, project_id).await
        })
    }

    fn query_user(&self, user_id: i64) -> Result<Option<GraphqlUserRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_user_seaorm(&connection, user_id).await
        })
    }

    fn query_user_roles(&self, user_id: i64) -> Result<Vec<GraphqlRoleSummaryRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_user_roles_seaorm(&connection, user_id).await
        })
    }

    fn create_user(
        &self,
        email: &str,
        status: &str,
        prefer_language: &str,
        password_hash: &str,
        first_name: &str,
        last_name: &str,
        avatar: Option<&str>,
        is_owner: bool,
        scopes_json: &str,
        project_ids: &[i64],
        role_ids: &[i64],
    ) -> Result<i64, String> {
        let db = self.db.clone();
        let email = email.to_owned();
        let status = status.to_owned();
        let prefer_language = prefer_language.to_owned();
        let password_hash = password_hash.to_owned();
        let first_name = first_name.to_owned();
        let last_name = last_name.to_owned();
        let avatar = avatar.map(ToOwned::to_owned);
        let scopes_json = scopes_json.to_owned();
        let project_ids = project_ids.to_vec();
        let role_ids = role_ids.to_vec();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            create_user_seaorm(
                &connection,
                email.as_str(),
                status.as_str(),
                prefer_language.as_str(),
                password_hash.as_str(),
                first_name.as_str(),
                last_name.as_str(),
                avatar.as_deref(),
                is_owner,
                scopes_json.as_str(),
                &project_ids,
                &role_ids,
            )
            .await
        })
    }

    fn update_user_profile(
        &self,
        user_id: i64,
        first_name: Option<&str>,
        last_name: Option<&str>,
        prefer_language: Option<&str>,
        avatar: Option<&str>,
    ) -> Result<bool, String> {
        let db = self.db.clone();
        let first_name = first_name.map(ToOwned::to_owned);
        let last_name = last_name.map(ToOwned::to_owned);
        let prefer_language = prefer_language.map(ToOwned::to_owned);
        let avatar = avatar.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_user_profile_seaorm(
                &connection,
                user_id,
                first_name.as_deref(),
                last_name.as_deref(),
                prefer_language.as_deref(),
                avatar.as_deref(),
            )
            .await
        })
    }

    fn update_user_status(&self, user_id: i64, status: &str) -> Result<bool, String> {
        let db = self.db.clone();
        let status = status.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_user_status_seaorm(&connection, user_id, status.as_str()).await
        })
    }

    fn update_user(
        &self,
        user_id: i64,
        first_name: Option<&str>,
        last_name: Option<&str>,
        prefer_language: Option<&str>,
        avatar: Option<&str>,
        scopes_json: Option<&str>,
        role_ids: Option<&[i64]>,
    ) -> Result<bool, String> {
        let db = self.db.clone();
        let first_name = first_name.map(ToOwned::to_owned);
        let last_name = last_name.map(ToOwned::to_owned);
        let prefer_language = prefer_language.map(ToOwned::to_owned);
        let avatar = avatar.map(ToOwned::to_owned);
        let scopes_json = scopes_json.map(ToOwned::to_owned);
        let role_ids = role_ids.map(|ids| ids.to_vec());
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_user_seaorm(
                &connection,
                user_id,
                first_name.as_deref(),
                last_name.as_deref(),
                prefer_language.as_deref(),
                avatar.as_deref(),
                scopes_json.as_deref(),
                role_ids.as_deref(),
            )
            .await
        })
    }

    fn upsert_provider_quota_statuses(
        &self,
        next_check_at: &str,
        quota_error_message: &str,
    ) -> Result<usize, String> {
        let db = self.db.clone();
        let next_check_at = next_check_at.to_owned();
        let quota_error_message = quota_error_message.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            upsert_provider_quota_statuses_seaorm(
                &connection,
                next_check_at.as_str(),
                quota_error_message.as_str(),
            )
            .await
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

async fn query_data_storage_status_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    id: i64,
) -> Result<Option<GraphqlDataStorageStatusRecord>, String> {
    data_storages::Entity::find()
        .filter(data_storages::Column::Id.eq(id))
        .filter(data_storages::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<data_storages::GraphqlStatus>()
        .one(db)
        .await
        .map(|row| {
            row.map(|row| GraphqlDataStorageStatusRecord {
                id: row.id,
                status: row.status,
            })
        })
        .map_err(|error| error.to_string())
}

async fn query_data_storage_config_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    id: i64,
) -> Result<Option<GraphqlDataStorageConfigRecord>, String> {
    data_storages::Entity::find()
        .filter(data_storages::Column::Id.eq(id))
        .filter(data_storages::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<data_storages::StorageConfig>()
        .one(db)
        .await
        .map(|row| {
            row.map(|row| GraphqlDataStorageConfigRecord {
                storage_type: row.storage_type,
                settings: row.settings,
                status: row.status,
            })
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

async fn query_user_profile_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Option<GraphqlUserProfileRecord>, String> {
    users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<users::GraphqlProfile>()
        .one(db)
        .await
        .map(|row| {
            row.map(|row| GraphqlUserProfileRecord {
                id: row.id,
                email: row.email,
                first_name: row.first_name,
                last_name: row.last_name,
                is_owner: row.is_owner,
                prefer_language: row.prefer_language,
                avatar: row.avatar,
                scopes: row.scopes,
            })
        })
        .map_err(|error| error.to_string())
}

async fn query_user_projects_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<GraphqlUserProjectMembershipRecord>, String> {
    user_projects::Entity::find()
        .filter(user_projects::Column::UserId.eq(user_id))
        .find_also_related(projects::Entity)
        .order_by_asc(user_projects::Column::ProjectId)
        .all(db)
        .await
        .map_err(|error| error.to_string())
        .map(|rows| {
            rows.into_iter()
                .filter_map(|(membership, project)| {
                    project.and_then(|project| {
                        (project.deleted_at == 0).then_some(GraphqlUserProjectMembershipRecord {
                            project_id: membership.project_id,
                            is_owner: membership.is_owner,
                            scopes: membership.scopes,
                        })
                    })
                })
                .collect()
        })
}

async fn query_project_roles_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
    project_id: i64,
) -> Result<Vec<GraphqlRoleSummaryRecord>, String> {
    roles_for_user_seaorm(db, user_id)
        .await
        .map(|roles| {
            roles.into_iter()
                .filter(|role| role.project_id == project_id)
                .map(|role| GraphqlRoleSummaryRecord {
                    id: role.id,
                    name: role.name,
                    scopes: role.scopes,
                })
                .collect()
        })
}

async fn query_user_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Option<GraphqlUserRecord>, String> {
    users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<users::GraphqlUserListItem>()
        .one(db)
        .await
        .map(|row| {
            row.map(|row| GraphqlUserRecord {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                email: row.email,
                status: row.status,
                first_name: row.first_name,
                last_name: row.last_name,
                is_owner: row.is_owner,
                prefer_language: row.prefer_language,
                avatar: None,
                scopes: row.scopes,
            })
        })
        .map_err(|error| error.to_string())
}

async fn query_user_roles_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<GraphqlRoleSummaryRecord>, String> {
    roles_for_user_seaorm(db, user_id)
        .await
        .map(|roles| {
            roles.into_iter()
                .map(|role| GraphqlRoleSummaryRecord {
                    id: role.id,
                    name: role.name,
                    scopes: role.scopes,
                })
                .collect()
        })
}

async fn create_user_seaorm(
    db: &DatabaseConnection,
    email: &str,
    status: &str,
    prefer_language: &str,
    password_hash: &str,
    first_name: &str,
    last_name: &str,
    avatar: Option<&str>,
    is_owner: bool,
    scopes_json: &str,
    project_ids: &[i64],
    role_ids: &[i64],
) -> Result<i64, String> {
    let txn = db.begin().await.map_err(|error| error.to_string())?;

    let created = users::Entity::insert(users::ActiveModel {
        email: Set(email.to_owned()),
        status: Set(status.to_owned()),
        prefer_language: Set(prefer_language.to_owned()),
        password: Set(password_hash.to_owned()),
        first_name: Set(first_name.to_owned()),
        last_name: Set(last_name.to_owned()),
        avatar: Set(avatar.map(ToOwned::to_owned)),
        is_owner: Set(is_owner),
        scopes: Set(scopes_json.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(&txn)
    .await
    .map_err(|error| format!("failed to create user: {error}"))?;

    let user_id = created.last_insert_id;

    for &project_id in project_ids {
        if let Err(error) = user_projects::Entity::insert(user_projects::ActiveModel {
            user_id: Set(user_id),
            project_id: Set(project_id),
            is_owner: Set(false),
            scopes: Set("[]".to_owned()),
            ..Default::default()
        })
        .exec(&txn)
        .await
        {
            let _ = txn.rollback().await;
            return Err(format!("failed to assign user project membership: {error}"));
        }
    }

    for &role_id in role_ids {
        if let Err(error) = user_roles::Entity::insert(user_roles::ActiveModel {
            user_id: Set(user_id),
            role_id: Set(role_id),
            ..Default::default()
        })
        .exec(&txn)
        .await
        {
            let _ = txn.rollback().await;
            return Err(format!("failed to assign user role: {error}"));
        }
    }

    txn.commit().await.map_err(|error| error.to_string())?;
    Ok(user_id)
}

async fn update_user_profile_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
    first_name: Option<&str>,
    last_name: Option<&str>,
    prefer_language: Option<&str>,
    avatar: Option<&str>,
) -> Result<bool, String> {
    let existing = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Ok(false);
    };

    let mut active_model: users::ActiveModel = existing.into();
    if let Some(first_name) = first_name {
        active_model.first_name = Set(first_name.to_owned());
    }
    if let Some(last_name) = last_name {
        active_model.last_name = Set(last_name.to_owned());
    }
    if let Some(prefer_language) = prefer_language {
        active_model.prefer_language = Set(prefer_language.to_owned());
    }
    if let Some(avatar) = avatar {
        active_model.avatar = Set(Some(avatar.to_owned()));
    }
    active_model.deleted_at = Set(0_i64);
    active_model.update(db).await.map_err(|error| error.to_string())?;
    Ok(true)
}

async fn update_user_status_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
    status: &str,
) -> Result<bool, String> {
    let existing = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Ok(false);
    };

    let mut active_model: users::ActiveModel = existing.into();
    active_model.status = Set(status.to_owned());
    active_model.deleted_at = Set(0_i64);
    active_model.update(db).await.map_err(|error| error.to_string())?;
    Ok(true)
}

async fn update_user_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
    first_name: Option<&str>,
    last_name: Option<&str>,
    prefer_language: Option<&str>,
    avatar: Option<&str>,
    scopes_json: Option<&str>,
    role_ids: Option<&[i64]>,
) -> Result<bool, String> {
    let txn = db.begin().await.map_err(|error| error.to_string())?;

    let existing = users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.eq(0_i64))
        .one(&txn)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        let _ = txn.rollback().await;
        return Ok(false);
    };

    let mut active_model: users::ActiveModel = existing.into();
    if let Some(first_name) = first_name {
        active_model.first_name = Set(first_name.to_owned());
    }
    if let Some(last_name) = last_name {
        active_model.last_name = Set(last_name.to_owned());
    }
    if let Some(prefer_language) = prefer_language {
        active_model.prefer_language = Set(prefer_language.to_owned());
    }
    if let Some(avatar) = avatar {
        active_model.avatar = Set(Some(avatar.to_owned()));
    }
    if let Some(scopes_json) = scopes_json {
        active_model.scopes = Set(scopes_json.to_owned());
    }
    active_model.deleted_at = Set(0_i64);
    if let Err(error) = active_model.update(&txn).await {
        let _ = txn.rollback().await;
        return Err(format!("failed to update user: {error}"));
    }

    if let Some(role_ids) = role_ids {
        if let Err(error) = user_roles::Entity::delete_many()
            .filter(user_roles::Column::UserId.eq(user_id))
            .exec(&txn)
            .await
        {
            let _ = txn.rollback().await;
            return Err(format!("failed to clear existing user roles: {error}"));
        }

        for &role_id in role_ids {
            if let Err(error) = user_roles::Entity::insert(user_roles::ActiveModel {
                user_id: Set(user_id),
                role_id: Set(role_id),
                ..Default::default()
            })
            .exec(&txn)
            .await
            {
                let _ = txn.rollback().await;
                return Err(format!("failed to replace user role assignments: {error}"));
            }
        }
    }

    txn.commit().await.map_err(|error| error.to_string())?;
    Ok(true)
}

async fn upsert_provider_quota_statuses_seaorm(
    db: &DatabaseConnection,
    next_check_at: &str,
    quota_error_message: &str,
) -> Result<usize, String> {
    let channels = channels::Entity::find()
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .filter(channels::Column::Status.eq("enabled"))
        .all(db)
        .await
        .map_err(|error| error.to_string())?;

    let mut updated = 0_usize;
    let quota_data = serde_json::json!({"error": quota_error_message}).to_string();

    for channel in channels {
        let provider_type = match channel.type_field.as_str() {
            "claudecode" => Some("claudecode"),
            "codex" => Some("codex"),
            _ => None,
        };
        let Some(provider_type) = provider_type else {
            continue;
        };

        let existing = provider_quota_statuses::Entity::find()
            .filter(provider_quota_statuses::Column::ChannelId.eq(channel.id))
            .one(db)
            .await
            .map_err(|error| error.to_string())?;

        if let Some(existing) = existing {
            let mut active_model: provider_quota_statuses::ActiveModel = existing.into();
            active_model.provider_type = Set(provider_type.to_owned());
            active_model.status = Set("unknown".to_owned());
            active_model.quota_data = Set(quota_data.clone());
            active_model.next_reset_at = Set(None);
            active_model.ready = Set(false);
            active_model.next_check_at = Set(next_check_at.to_owned());
            active_model.deleted_at = Set(0_i64);
            active_model.update(db).await.map_err(|error| error.to_string())?;
        } else {
            provider_quota_statuses::Entity::insert(provider_quota_statuses::ActiveModel {
                channel_id: Set(channel.id),
                provider_type: Set(provider_type.to_owned()),
                status: Set("unknown".to_owned()),
                quota_data: Set(quota_data.clone()),
                next_reset_at: Set(None),
                ready: Set(false),
                next_check_at: Set(next_check_at.to_owned()),
                deleted_at: Set(0_i64),
                ..Default::default()
            })
            .exec(db)
            .await
            .map_err(|error| error.to_string())?;
        }

        updated += 1;
    }

    Ok(updated)
}

async fn roles_for_user_seaorm(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<roles::Assignment>, String> {
    let links = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .all(db)
        .await
        .map_err(|error| error.to_string())?;

    let mut roles_for_user = Vec::new();
    for link in links {
        let Some(role) = roles::Entity::find_by_id(link.role_id)
            .filter(roles::Column::DeletedAt.eq(0_i64))
            .into_partial_model::<roles::Assignment>()
            .one(db)
            .await
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        roles_for_user.push(role);
    }
    roles_for_user.sort_by_key(|role| role.id);
    Ok(roles_for_user)
}
