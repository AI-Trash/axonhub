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

use super::openai_v1::{list_enabled_model_records_seaorm, query_system_channel_settings_seaorm};

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

#[derive(Debug, Clone)]
pub(crate) struct GraphqlProjectRecord {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlRoleRecord {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) level: String,
    pub(crate) project_id: i64,
    pub(crate) scopes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlApiKeyRecord {
    pub(crate) id: i64,
    pub(crate) project_id: i64,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) key_type: String,
    pub(crate) status: String,
    pub(crate) scopes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlChannelRecord {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) channel_type: String,
    pub(crate) base_url: String,
    pub(crate) status: String,
    pub(crate) supported_models: String,
    pub(crate) ordering_weight: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphqlModelRecord {
    pub(crate) id: i64,
    pub(crate) developer: String,
    pub(crate) model_id: String,
    pub(crate) model_type: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) remark: String,
    pub(crate) model_card_json: String,
}

pub(crate) trait AdminGraphqlSubsetRepository: Send + Sync {
    fn query_channels(&self) -> Result<Vec<GraphqlChannelRecord>, String>;
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
    fn query_role(&self, role_id: i64) -> Result<Option<GraphqlRoleRecord>, String>;
    fn query_api_key(&self, api_key_id: i64) -> Result<Option<GraphqlApiKeyRecord>, String>;
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
    fn create_project(&self, name: &str, description: &str, status: &str) -> Result<GraphqlProjectRecord, String>;
    fn update_project(
        &self,
        project_id: i64,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
    ) -> Result<GraphqlProjectRecord, String>;
    fn create_role(
        &self,
        name: &str,
        level: &str,
        project_id: i64,
        scopes_json: &str,
    ) -> Result<GraphqlRoleRecord, String>;
    fn update_role(
        &self,
        role_id: i64,
        name: Option<&str>,
        level: &str,
        project_id: i64,
        scopes_json: Option<&str>,
    ) -> Result<GraphqlRoleRecord, String>;
    fn create_api_key(
        &self,
        owner_user_id: i64,
        project_id: i64,
        key: &str,
        name: &str,
        key_type: &str,
        status: &str,
        scopes_json: &str,
        profiles_json: &str,
    ) -> Result<GraphqlApiKeyRecord, String>;
    fn update_api_key(
        &self,
        api_key_id: i64,
        name: Option<&str>,
        status: Option<&str>,
        scopes_json: Option<&str>,
    ) -> Result<GraphqlApiKeyRecord, String>;
    fn create_channel(
        &self,
        channel_type: &str,
        base_url: &str,
        name: &str,
        status: &str,
        credentials_json: &str,
        supported_models: &str,
        auto_sync_supported_models: bool,
        default_test_model: &str,
        settings_json: &str,
        tags: &str,
        ordering_weight: i32,
        error_message: &str,
        remark: &str,
    ) -> Result<GraphqlChannelRecord, String>;
    fn update_channel(
        &self,
        channel_id: i64,
        name: Option<&str>,
        base_url: Option<&str>,
        status: Option<&str>,
        supported_models: Option<&str>,
        auto_sync_supported_models: Option<bool>,
        default_test_model: Option<&str>,
        credentials_json: Option<&str>,
        settings_json: Option<&str>,
        tags: Option<&str>,
        ordering_weight: Option<i32>,
        error_message: Option<&str>,
        remark: Option<&str>,
    ) -> Result<GraphqlChannelRecord, String>;
    fn create_model(
        &self,
        developer: &str,
        model_id: &str,
        model_type: &str,
        name: &str,
        icon: &str,
        group: &str,
        model_card_json: &str,
        settings_json: &str,
        status: &str,
        remark: Option<&str>,
    ) -> Result<GraphqlModelRecord, String>;
    fn update_model(
        &self,
        model_id: i64,
        name: Option<&str>,
        icon: Option<&str>,
        group: Option<&str>,
        model_card_json: Option<&str>,
        settings_json: Option<&str>,
        status: Option<&str>,
        remark: Option<Option<&str>>,
    ) -> Result<GraphqlModelRecord, String>;
    fn query_prompt_protection_rules(
        &self,
    ) -> Result<Vec<super::prompt_protection::StoredPromptProtectionRuleRecord>, String>;
    fn create_prompt_protection_rule(
        &self,
        name: &str,
        description: &str,
        pattern: &str,
        status: &str,
        settings_json: &str,
    ) -> Result<Option<super::prompt_protection::StoredPromptProtectionRuleRecord>, String>;
    fn update_prompt_protection_rule(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        pattern: Option<&str>,
        status: Option<&str>,
        settings_json: Option<&str>,
    ) -> Result<Option<super::prompt_protection::StoredPromptProtectionRuleRecord>, String>;
    fn set_prompt_protection_rule_status(&self, id: i64, status: &str) -> Result<bool, String>;
    fn delete_prompt_protection_rule(&self, id: i64) -> Result<bool, String>;
    fn bulk_delete_prompt_protection_rules(&self, ids: &[i64]) -> Result<(), String>;
    fn bulk_set_prompt_protection_rules_status(&self, ids: &[i64], status: &str) -> Result<(), String>;
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

    pub(crate) fn db(&self) -> SeaOrmConnectionFactory {
        self.db.clone()
    }
}

impl SeaOrmOpenApiGraphqlMutationRepository {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl AdminGraphqlSubsetRepository for SeaOrmAdminGraphqlSubsetRepository {
    fn query_channels(&self) -> Result<Vec<GraphqlChannelRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_channels_seaorm(&connection).await
        })
    }

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

    fn query_role(&self, role_id: i64) -> Result<Option<GraphqlRoleRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            load_role_record_seaorm(&connection, role_id).await
        })
    }

    fn query_api_key(&self, api_key_id: i64) -> Result<Option<GraphqlApiKeyRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            load_api_key_record_seaorm(&connection, api_key_id).await
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

    fn create_project(&self, name: &str, description: &str, status: &str) -> Result<GraphqlProjectRecord, String> {
        let db = self.db.clone();
        let name = name.to_owned();
        let description = description.to_owned();
        let status = status.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            create_project_seaorm(&connection, &name, &description, &status).await
        })
    }

    fn update_project(
        &self,
        project_id: i64,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
    ) -> Result<GraphqlProjectRecord, String> {
        let db = self.db.clone();
        let name = name.map(ToOwned::to_owned);
        let description = description.map(ToOwned::to_owned);
        let status = status.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_project_seaorm(
                &connection,
                project_id,
                name.as_deref(),
                description.as_deref(),
                status.as_deref(),
            )
            .await
        })
    }

    fn create_role(
        &self,
        name: &str,
        level: &str,
        project_id: i64,
        scopes_json: &str,
    ) -> Result<GraphqlRoleRecord, String> {
        let db = self.db.clone();
        let name = name.to_owned();
        let level = level.to_owned();
        let scopes_json = scopes_json.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            create_role_seaorm(&connection, &name, &level, project_id, &scopes_json).await
        })
    }

    fn update_role(
        &self,
        role_id: i64,
        name: Option<&str>,
        level: &str,
        project_id: i64,
        scopes_json: Option<&str>,
    ) -> Result<GraphqlRoleRecord, String> {
        let db = self.db.clone();
        let name = name.map(ToOwned::to_owned);
        let level = level.to_owned();
        let scopes_json = scopes_json.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_role_seaorm(
                &connection,
                role_id,
                name.as_deref(),
                &level,
                project_id,
                scopes_json.as_deref(),
            )
            .await
        })
    }

    fn create_api_key(
        &self,
        owner_user_id: i64,
        project_id: i64,
        key: &str,
        name: &str,
        key_type: &str,
        status: &str,
        scopes_json: &str,
        profiles_json: &str,
    ) -> Result<GraphqlApiKeyRecord, String> {
        let db = self.db.clone();
        let key = key.to_owned();
        let name = name.to_owned();
        let key_type = key_type.to_owned();
        let status = status.to_owned();
        let scopes_json = scopes_json.to_owned();
        let profiles_json = profiles_json.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            create_api_key_seaorm(
                &connection,
                owner_user_id,
                project_id,
                &key,
                &name,
                &key_type,
                &status,
                &scopes_json,
                &profiles_json,
            )
            .await
        })
    }

    fn update_api_key(
        &self,
        api_key_id: i64,
        name: Option<&str>,
        status: Option<&str>,
        scopes_json: Option<&str>,
    ) -> Result<GraphqlApiKeyRecord, String> {
        let db = self.db.clone();
        let name = name.map(ToOwned::to_owned);
        let status = status.map(ToOwned::to_owned);
        let scopes_json = scopes_json.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_api_key_seaorm(
                &connection,
                api_key_id,
                name.as_deref(),
                status.as_deref(),
                scopes_json.as_deref(),
            )
            .await
        })
    }

    fn create_channel(
        &self,
        channel_type: &str,
        base_url: &str,
        name: &str,
        status: &str,
        credentials_json: &str,
        supported_models: &str,
        auto_sync_supported_models: bool,
        default_test_model: &str,
        settings_json: &str,
        tags: &str,
        ordering_weight: i32,
        error_message: &str,
        remark: &str,
    ) -> Result<GraphqlChannelRecord, String> {
        let db = self.db.clone();
        let channel_type = channel_type.to_owned();
        let base_url = base_url.to_owned();
        let name = name.to_owned();
        let status = status.to_owned();
        let credentials_json = credentials_json.to_owned();
        let supported_models = supported_models.to_owned();
        let default_test_model = default_test_model.to_owned();
        let settings_json = settings_json.to_owned();
        let tags = tags.to_owned();
        let error_message = error_message.to_owned();
        let remark = remark.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            create_channel_seaorm(
                &connection,
                &channel_type,
                &base_url,
                &name,
                &status,
                &credentials_json,
                &supported_models,
                auto_sync_supported_models,
                &default_test_model,
                &settings_json,
                &tags,
                ordering_weight,
                &error_message,
                &remark,
            )
            .await
        })
    }

    fn update_channel(
        &self,
        channel_id: i64,
        name: Option<&str>,
        base_url: Option<&str>,
        status: Option<&str>,
        supported_models: Option<&str>,
        auto_sync_supported_models: Option<bool>,
        default_test_model: Option<&str>,
        credentials_json: Option<&str>,
        settings_json: Option<&str>,
        tags: Option<&str>,
        ordering_weight: Option<i32>,
        error_message: Option<&str>,
        remark: Option<&str>,
    ) -> Result<GraphqlChannelRecord, String> {
        let db = self.db.clone();
        let name = name.map(ToOwned::to_owned);
        let base_url = base_url.map(ToOwned::to_owned);
        let status = status.map(ToOwned::to_owned);
        let supported_models = supported_models.map(ToOwned::to_owned);
        let default_test_model = default_test_model.map(ToOwned::to_owned);
        let credentials_json = credentials_json.map(ToOwned::to_owned);
        let settings_json = settings_json.map(ToOwned::to_owned);
        let tags = tags.map(ToOwned::to_owned);
        let error_message = error_message.map(ToOwned::to_owned);
        let remark = remark.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_channel_seaorm(
                &connection,
                channel_id,
                name.as_deref(),
                base_url.as_deref(),
                status.as_deref(),
                supported_models.as_deref(),
                auto_sync_supported_models,
                default_test_model.as_deref(),
                credentials_json.as_deref(),
                settings_json.as_deref(),
                tags.as_deref(),
                ordering_weight,
                error_message.as_deref(),
                remark.as_deref(),
            )
            .await
        })
    }

    fn create_model(
        &self,
        developer: &str,
        model_id: &str,
        model_type: &str,
        name: &str,
        icon: &str,
        group: &str,
        model_card_json: &str,
        settings_json: &str,
        status: &str,
        remark: Option<&str>,
    ) -> Result<GraphqlModelRecord, String> {
        let db = self.db.clone();
        let developer = developer.to_owned();
        let model_id = model_id.to_owned();
        let model_type = model_type.to_owned();
        let name = name.to_owned();
        let icon = icon.to_owned();
        let group = group.to_owned();
        let model_card_json = model_card_json.to_owned();
        let settings_json = settings_json.to_owned();
        let status = status.to_owned();
        let remark = remark.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            create_model_seaorm(
                &connection,
                &developer,
                &model_id,
                &model_type,
                &name,
                &icon,
                &group,
                &model_card_json,
                &settings_json,
                &status,
                remark.as_deref(),
            )
            .await
        })
    }

    fn update_model(
        &self,
        model_id: i64,
        name: Option<&str>,
        icon: Option<&str>,
        group: Option<&str>,
        model_card_json: Option<&str>,
        settings_json: Option<&str>,
        status: Option<&str>,
        remark: Option<Option<&str>>,
    ) -> Result<GraphqlModelRecord, String> {
        let db = self.db.clone();
        let name = name.map(ToOwned::to_owned);
        let icon = icon.map(ToOwned::to_owned);
        let group = group.map(ToOwned::to_owned);
        let model_card_json = model_card_json.map(ToOwned::to_owned);
        let settings_json = settings_json.map(ToOwned::to_owned);
        let status = status.map(ToOwned::to_owned);
        let remark = remark.map(|value| value.map(ToOwned::to_owned));
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            update_model_seaorm(
                &connection,
                model_id,
                name.as_deref(),
                icon.as_deref(),
                group.as_deref(),
                model_card_json.as_deref(),
                settings_json.as_deref(),
                status.as_deref(),
                remark.as_ref().map(|value| value.as_deref()),
            )
            .await
        })
    }

    fn query_prompt_protection_rules(
        &self,
    ) -> Result<Vec<super::prompt_protection::StoredPromptProtectionRuleRecord>, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            super::prompt_protection::list_prompt_protection_rules_seaorm(&connection).await
        })
    }

    fn create_prompt_protection_rule(
        &self,
        name: &str,
        description: &str,
        pattern: &str,
        status: &str,
        settings_json: &str,
    ) -> Result<Option<super::prompt_protection::StoredPromptProtectionRuleRecord>, String> {
        let db = self.db.clone();
        let name = name.to_owned();
        let description = description.to_owned();
        let pattern = pattern.to_owned();
        let status = status.to_owned();
        let settings_json = settings_json.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            if super::prompt_protection::prompt_protection_rule_name_exists_seaorm(&connection, &name, None).await? {
                return Err("prompt protection rule already exists".to_owned());
            }
            let id = super::prompt_protection::create_prompt_protection_rule_seaorm(
                &connection,
                &name,
                &description,
                &pattern,
                &status,
                &settings_json,
            )
            .await?;
            super::prompt_protection::load_prompt_protection_rule_seaorm(&connection, id).await
        })
    }

    fn update_prompt_protection_rule(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        pattern: Option<&str>,
        status: Option<&str>,
        settings_json: Option<&str>,
    ) -> Result<Option<super::prompt_protection::StoredPromptProtectionRuleRecord>, String> {
        let db = self.db.clone();
        let name = name.map(ToOwned::to_owned);
        let description = description.map(ToOwned::to_owned);
        let pattern = pattern.map(ToOwned::to_owned);
        let status = status.map(ToOwned::to_owned);
        let settings_json = settings_json.map(ToOwned::to_owned);
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            if let Some(ref name) = name {
                if super::prompt_protection::prompt_protection_rule_name_exists_seaorm(&connection, name, Some(id)).await? {
                    return Err("prompt protection rule already exists".to_owned());
                }
            }
            let updated = super::prompt_protection::update_prompt_protection_rule_seaorm(
                &connection,
                id,
                name.as_deref(),
                description.as_deref(),
                pattern.as_deref(),
                status.as_deref(),
                settings_json.as_deref(),
            )
            .await?;
            if !updated {
                return Ok(None);
            }
            super::prompt_protection::load_prompt_protection_rule_seaorm(&connection, id).await
        })
    }

    fn set_prompt_protection_rule_status(&self, id: i64, status: &str) -> Result<bool, String> {
        let db = self.db.clone();
        let status = status.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            super::prompt_protection::set_prompt_protection_rule_status_seaorm(&connection, id, &status).await
        })
    }

    fn delete_prompt_protection_rule(&self, id: i64) -> Result<bool, String> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            super::prompt_protection::soft_delete_prompt_protection_rule_seaorm(&connection, id).await
        })
    }

    fn bulk_delete_prompt_protection_rules(&self, ids: &[i64]) -> Result<(), String> {
        let db = self.db.clone();
        let ids = ids.to_vec();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            super::prompt_protection::bulk_soft_delete_prompt_protection_rules_seaorm(&connection, &ids)
                .await
                .map(|_| ())
        })
    }

    fn bulk_set_prompt_protection_rules_status(&self, ids: &[i64], status: &str) -> Result<(), String> {
        let db = self.db.clone();
        let ids = ids.to_vec();
        let status = status.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            super::prompt_protection::bulk_set_prompt_protection_rule_status_seaorm(&connection, &ids, &status)
                .await
                .map(|_| ())
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

async fn query_model_statuses_seaorm(
    db: &impl sea_orm::ConnectionTrait,
) -> Result<Vec<GraphqlModelStatusRecord>, String> {
    let settings = query_system_channel_settings_seaorm(db)
        .await
        .map_err(|error| match error {
            axonhub_http::OpenAiV1Error::InvalidRequest { message }
            | axonhub_http::OpenAiV1Error::Internal { message } => message,
            axonhub_http::OpenAiV1Error::Upstream { status, body } => {
                format!("unexpected upstream error while listing models: {status} {body}")
            }
        })?;

    list_enabled_model_records_seaorm(
        db,
        db.get_database_backend(),
        settings.query_all_channel_models,
        None,
    )
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|row| GraphqlModelStatusRecord {
                id: row.id,
                status: "enabled".to_owned(),
            })
            .collect()
    })
    .map_err(|error| match error {
        axonhub_http::OpenAiV1Error::InvalidRequest { message }
        | axonhub_http::OpenAiV1Error::Internal { message } => message,
        axonhub_http::OpenAiV1Error::Upstream { status, body } => {
            format!("unexpected upstream error while listing models: {status} {body}")
        }
    })
}

async fn query_channels_seaorm(
    db: &DatabaseConnection,
) -> Result<Vec<GraphqlChannelRecord>, String> {
    channels::Entity::find()
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .all(db)
        .await
        .map_err(|error| error.to_string())
        .map(|rows| rows.into_iter().map(graphql_channel_record_from_model).collect())
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
        avatar: Set(Some(avatar.unwrap_or_default().to_owned())),
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

async fn create_project_seaorm(
    db: &DatabaseConnection,
    name: &str,
    description: &str,
    status: &str,
) -> Result<GraphqlProjectRecord, String> {
    if projects::Entity::find()
        .filter(projects::Column::Name.eq(name))
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("project already exists".to_owned());
    }
    let created = projects::Entity::insert(projects::ActiveModel {
        name: Set(name.to_owned()),
        description: Set(description.to_owned()),
        status: Set(status.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(|error| error.to_string())?;
    load_project_record_seaorm(db, created.last_insert_id)
        .await?
        .ok_or_else(|| "project not found".to_owned())
}

async fn update_project_seaorm(
    db: &DatabaseConnection,
    project_id: i64,
    name: Option<&str>,
    description: Option<&str>,
    status: Option<&str>,
) -> Result<GraphqlProjectRecord, String> {
    let existing = projects::Entity::find_by_id(project_id)
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Err("project not found".to_owned());
    };

    if let Some(name) = name {
        if let Some(other) = projects::Entity::find()
            .filter(projects::Column::Name.eq(name))
            .filter(projects::Column::DeletedAt.eq(0_i64))
            .one(db)
            .await
            .map_err(|error| error.to_string())?
        {
            if other.id != project_id {
                return Err("project already exists".to_owned());
            }
        }
    }

    let mut active: projects::ActiveModel = existing.into();
    if let Some(name) = name {
        active.name = Set(name.to_owned());
    }
    if let Some(description) = description {
        active.description = Set(description.to_owned());
    }
    if let Some(status) = status {
        active.status = Set(status.to_owned());
    }
    active.deleted_at = Set(0_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    load_project_record_seaorm(db, project_id)
        .await?
        .ok_or_else(|| "project not found".to_owned())
}

async fn create_role_seaorm(
    db: &DatabaseConnection,
    name: &str,
    level: &str,
    project_id: i64,
    scopes_json: &str,
) -> Result<GraphqlRoleRecord, String> {
    if roles::Entity::find()
        .filter(roles::Column::Name.eq(name))
        .filter(roles::Column::ProjectId.eq(project_id))
        .filter(roles::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("role already exists".to_owned());
    }
    let created = roles::Entity::insert(roles::ActiveModel {
        name: Set(name.to_owned()),
        level: Set(level.to_owned()),
        project_id: Set(project_id),
        scopes: Set(scopes_json.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(|error| error.to_string())?;
    load_role_record_seaorm(db, created.last_insert_id)
        .await?
        .ok_or_else(|| "role not found".to_owned())
}

async fn update_role_seaorm(
    db: &DatabaseConnection,
    role_id: i64,
    name: Option<&str>,
    level: &str,
    project_id: i64,
    scopes_json: Option<&str>,
) -> Result<GraphqlRoleRecord, String> {
    let existing = roles::Entity::find_by_id(role_id)
        .filter(roles::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Err("role not found".to_owned());
    };

    if let Some(name) = name {
        if let Some(other) = roles::Entity::find()
            .filter(roles::Column::Name.eq(name))
            .filter(roles::Column::ProjectId.eq(project_id))
            .filter(roles::Column::DeletedAt.eq(0_i64))
            .one(db)
            .await
            .map_err(|error| error.to_string())?
        {
            if other.id != role_id {
                return Err("role already exists".to_owned());
            }
        }
    }

    let mut active: roles::ActiveModel = existing.into();
    if let Some(name) = name {
        active.name = Set(name.to_owned());
    }
    active.level = Set(level.to_owned());
    active.project_id = Set(project_id);
    if let Some(scopes_json) = scopes_json {
        active.scopes = Set(scopes_json.to_owned());
    }
    active.deleted_at = Set(0_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    load_role_record_seaorm(db, role_id)
        .await?
        .ok_or_else(|| "role not found".to_owned())
}

async fn create_api_key_seaorm(
    db: &DatabaseConnection,
    owner_user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    status: &str,
    scopes_json: &str,
    profiles_json: &str,
) -> Result<GraphqlApiKeyRecord, String> {
    if projects::Entity::find_by_id(project_id)
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err("project not found".to_owned());
    }
    if api_keys::Entity::find()
        .filter(api_keys::Column::Key.eq(key))
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("api key already exists".to_owned());
    }
    let created = api_keys::Entity::insert(api_keys::ActiveModel {
        user_id: Set(owner_user_id),
        project_id: Set(project_id),
        key: Set(key.to_owned()),
        name: Set(name.to_owned()),
        type_field: Set(key_type.to_owned()),
        status: Set(status.to_owned()),
        scopes: Set(scopes_json.to_owned()),
        profiles: Set(profiles_json.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(|error| error.to_string())?;
    load_api_key_record_seaorm(db, created.last_insert_id)
        .await?
        .ok_or_else(|| "api key not found".to_owned())
}

async fn update_api_key_seaorm(
    db: &DatabaseConnection,
    api_key_id: i64,
    name: Option<&str>,
    status: Option<&str>,
    scopes_json: Option<&str>,
) -> Result<GraphqlApiKeyRecord, String> {
    let existing = api_keys::Entity::find_by_id(api_key_id)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Err("api key not found".to_owned());
    };
    let mut active: api_keys::ActiveModel = existing.into();
    if let Some(name) = name {
        active.name = Set(name.to_owned());
    }
    if let Some(status) = status {
        active.status = Set(status.to_owned());
    }
    if let Some(scopes_json) = scopes_json {
        active.scopes = Set(scopes_json.to_owned());
    }
    active.deleted_at = Set(0_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    load_api_key_record_seaorm(db, api_key_id)
        .await?
        .ok_or_else(|| "api key not found".to_owned())
}

async fn create_channel_seaorm(
    db: &DatabaseConnection,
    channel_type: &str,
    base_url: &str,
    name: &str,
    status: &str,
    credentials_json: &str,
    supported_models: &str,
    auto_sync_supported_models: bool,
    default_test_model: &str,
    settings_json: &str,
    tags: &str,
    ordering_weight: i32,
    error_message: &str,
    remark: &str,
) -> Result<GraphqlChannelRecord, String> {
    if channels::Entity::find()
        .filter(channels::Column::Name.eq(name))
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("channel already exists".to_owned());
    }
    let created = channels::Entity::insert(channels::ActiveModel {
        type_field: Set(channel_type.to_owned()),
        base_url: Set(Some(base_url.to_owned())),
        name: Set(name.to_owned()),
        status: Set(status.to_owned()),
        credentials: Set(credentials_json.to_owned()),
        supported_models: Set(supported_models.to_owned()),
        auto_sync_supported_models: Set(auto_sync_supported_models),
        default_test_model: Set(default_test_model.to_owned()),
        settings: Set(settings_json.to_owned()),
        tags: Set(tags.to_owned()),
        ordering_weight: Set(ordering_weight),
        error_message: Set(Some(error_message.to_owned())),
        remark: Set(Some(remark.to_owned())),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(|error| error.to_string())?;
    load_channel_record_seaorm(db, created.last_insert_id)
        .await?
        .ok_or_else(|| "channel not found".to_owned())
}

async fn update_channel_seaorm(
    db: &DatabaseConnection,
    channel_id: i64,
    name: Option<&str>,
    base_url: Option<&str>,
    status: Option<&str>,
    supported_models: Option<&str>,
    auto_sync_supported_models: Option<bool>,
    default_test_model: Option<&str>,
    credentials_json: Option<&str>,
    settings_json: Option<&str>,
    tags: Option<&str>,
    ordering_weight: Option<i32>,
    error_message: Option<&str>,
    remark: Option<&str>,
) -> Result<GraphqlChannelRecord, String> {
    let existing = channels::Entity::find_by_id(channel_id)
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Err("channel not found".to_owned());
    };
    if let Some(name) = name {
        if let Some(other) = channels::Entity::find()
            .filter(channels::Column::Name.eq(name))
            .filter(channels::Column::DeletedAt.eq(0_i64))
            .one(db)
            .await
            .map_err(|error| error.to_string())?
        {
            if other.id != channel_id {
                return Err("channel already exists".to_owned());
            }
        }
    }
    let mut active: channels::ActiveModel = existing.into();
    if let Some(name) = name {
        active.name = Set(name.to_owned());
    }
    if let Some(base_url) = base_url {
        active.base_url = Set(Some(base_url.to_owned()));
    }
    if let Some(status) = status {
        active.status = Set(status.to_owned());
    }
    if let Some(supported_models) = supported_models {
        active.supported_models = Set(supported_models.to_owned());
    }
    if let Some(auto_sync_supported_models) = auto_sync_supported_models {
        active.auto_sync_supported_models = Set(auto_sync_supported_models);
    }
    if let Some(default_test_model) = default_test_model {
        active.default_test_model = Set(default_test_model.to_owned());
    }
    if let Some(credentials_json) = credentials_json {
        active.credentials = Set(credentials_json.to_owned());
    }
    if let Some(settings_json) = settings_json {
        active.settings = Set(settings_json.to_owned());
    }
    if let Some(tags) = tags {
        active.tags = Set(tags.to_owned());
    }
    if let Some(ordering_weight) = ordering_weight {
        active.ordering_weight = Set(ordering_weight);
    }
    if let Some(error_message) = error_message {
        active.error_message = Set(Some(error_message.to_owned()));
    }
    if let Some(remark) = remark {
        active.remark = Set(Some(remark.to_owned()));
    }
    active.deleted_at = Set(0_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    load_channel_record_seaorm(db, channel_id)
        .await?
        .ok_or_else(|| "channel not found".to_owned())
}

async fn create_model_seaorm(
    db: &DatabaseConnection,
    developer: &str,
    model_id: &str,
    model_type: &str,
    name: &str,
    icon: &str,
    group: &str,
    model_card_json: &str,
    settings_json: &str,
    status: &str,
    remark: Option<&str>,
) -> Result<GraphqlModelRecord, String> {
    if models::Entity::find()
        .filter(models::Column::Developer.eq(developer))
        .filter(models::Column::ModelId.eq(model_id))
        .filter(models::Column::TypeField.eq(model_type))
        .filter(models::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("model already exists".to_owned());
    }
    let created = models::Entity::insert(models::ActiveModel {
        developer: Set(developer.to_owned()),
        model_id: Set(model_id.to_owned()),
        type_field: Set(model_type.to_owned()),
        name: Set(name.to_owned()),
        icon: Set(icon.to_owned()),
        group_name: Set(group.to_owned()),
        model_card: Set(model_card_json.to_owned()),
        settings: Set(settings_json.to_owned()),
        status: Set(status.to_owned()),
        remark: Set(remark.map(ToOwned::to_owned)),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(|error| error.to_string())?;
    load_model_record_seaorm(db, created.last_insert_id)
        .await?
        .ok_or_else(|| "model not found".to_owned())
}

async fn update_model_seaorm(
    db: &DatabaseConnection,
    model_id: i64,
    name: Option<&str>,
    icon: Option<&str>,
    group: Option<&str>,
    model_card_json: Option<&str>,
    settings_json: Option<&str>,
    status: Option<&str>,
    remark: Option<Option<&str>>,
) -> Result<GraphqlModelRecord, String> {
    let existing = models::Entity::find_by_id(model_id)
        .filter(models::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?;
    let Some(existing) = existing else {
        return Err("model not found".to_owned());
    };
    let mut active: models::ActiveModel = existing.into();
    if let Some(name) = name {
        active.name = Set(name.to_owned());
    }
    if let Some(icon) = icon {
        active.icon = Set(icon.to_owned());
    }
    if let Some(group) = group {
        active.group_name = Set(group.to_owned());
    }
    if let Some(model_card_json) = model_card_json {
        active.model_card = Set(model_card_json.to_owned());
    }
    if let Some(settings_json) = settings_json {
        active.settings = Set(settings_json.to_owned());
    }
    if let Some(status) = status {
        active.status = Set(status.to_owned());
    }
    if let Some(remark) = remark {
        active.remark = Set(remark.map(ToOwned::to_owned));
    }
    active.deleted_at = Set(0_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    load_model_record_seaorm(db, model_id)
        .await?
        .ok_or_else(|| "model not found".to_owned())
}

async fn load_project_record_seaorm(
    db: &DatabaseConnection,
    project_id: i64,
) -> Result<Option<GraphqlProjectRecord>, String> {
    projects::Entity::find_by_id(project_id)
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.map(graphql_project_record_from_model))
}

async fn load_role_record_seaorm(db: &DatabaseConnection, role_id: i64) -> Result<Option<GraphqlRoleRecord>, String> {
    roles::Entity::find_by_id(role_id)
        .filter(roles::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.map(graphql_role_record_from_model))
}

async fn load_api_key_record_seaorm(
    db: &DatabaseConnection,
    api_key_id: i64,
) -> Result<Option<GraphqlApiKeyRecord>, String> {
    api_keys::Entity::find_by_id(api_key_id)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.map(graphql_api_key_record_from_model))
}

async fn load_channel_record_seaorm(
    db: &DatabaseConnection,
    channel_id: i64,
) -> Result<Option<GraphqlChannelRecord>, String> {
    channels::Entity::find_by_id(channel_id)
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.map(graphql_channel_record_from_model))
}

async fn load_model_record_seaorm(
    db: &DatabaseConnection,
    model_id: i64,
) -> Result<Option<GraphqlModelRecord>, String> {
    models::Entity::find_by_id(model_id)
        .filter(models::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.map(graphql_model_record_from_model))
}

fn graphql_project_record_from_model(value: projects::Model) -> GraphqlProjectRecord {
    GraphqlProjectRecord { id: value.id, name: value.name, description: value.description, status: value.status }
}

fn graphql_role_record_from_model(value: roles::Model) -> GraphqlRoleRecord {
    GraphqlRoleRecord {
        id: value.id,
        name: value.name,
        level: value.level,
        project_id: value.project_id,
        scopes: value.scopes,
    }
}

fn graphql_api_key_record_from_model(value: api_keys::Model) -> GraphqlApiKeyRecord {
    GraphqlApiKeyRecord {
        id: value.id,
        project_id: value.project_id,
        key: value.key,
        name: value.name,
        key_type: value.type_field,
        status: value.status,
        scopes: value.scopes,
    }
}

fn graphql_channel_record_from_model(value: channels::Model) -> GraphqlChannelRecord {
    GraphqlChannelRecord {
        id: value.id,
        name: value.name,
        channel_type: value.type_field,
        base_url: value.base_url.unwrap_or_default(),
        status: value.status,
        supported_models: value.supported_models,
        ordering_weight: value.ordering_weight,
    }
}

fn graphql_model_record_from_model(value: models::Model) -> GraphqlModelRecord {
    GraphqlModelRecord {
        id: value.id,
        developer: value.developer,
        model_id: value.model_id,
        model_type: value.type_field,
        name: value.name,
        icon: value.icon,
        remark: value.remark.unwrap_or_default(),
        model_card_json: value.model_card,
    }
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
