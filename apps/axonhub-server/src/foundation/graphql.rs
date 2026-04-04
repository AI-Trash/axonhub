use std::future::Future;
use std::pin::Pin;

use async_graphql::{Enum, InputObject, SimpleObject};
use axonhub_db_entity::{api_keys, channels, models, projects, roles};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, ProjectContext, TraceContext,
};
use getrandom::getrandom;
use hex::encode as hex_encode;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set,
};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use super::{
    admin::{
        default_auto_backup_settings, default_storage_policy, default_system_channel_settings,
        parse_graphql_resource_id, AutoSyncFrequencySetting, BackupFrequencySetting,
        ProbeFrequencySetting, StoredAutoBackupSettings, StoredChannelProbeData,
        StoredCircuitBreakerStatus, StoredProviderQuotaStatus, StoredProxyPreset,
        StoredStoragePolicy, StoredSystemChannelSettings,
    },
    admin_operational::SeaOrmOperationalService,
    authz::{
        authorize_user_system_scope, is_valid_scope, require_owner_bypass,
        require_service_api_key_write_access, require_user_project_scope, scope_strings,
        serialize_scope_slugs, LLM_API_KEY_SCOPES, SCOPE_READ_CHANNELS, SCOPE_READ_SETTINGS,
        SCOPE_READ_PROMPTS, SCOPE_WRITE_API_KEYS, SCOPE_WRITE_CHANNELS,
        SCOPE_WRITE_PROJECTS, SCOPE_WRITE_PROMPTS, SCOPE_WRITE_ROLES, SCOPE_WRITE_SETTINGS,
    },
    circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker},
    openai_v1::{parse_model_card, StoredChannelSummary, StoredModelRecord, StoredRequestSummary},
    ports::{AdminGraphqlRepository, OpenApiGraphqlRepository},
    prompt_protection::{
        default_prompt_protection_connection_json, normalize_prompt_protection_status,
        prompt_protection_rule_from_record, prompt_protection_rule_json,
        validate_prompt_protection_rule, PromptProtectionAction, PromptProtectionScope,
        StoredPromptProtectionSettings,
    },
    repositories::graphql::{
        AdminGraphqlSubsetRepository, GraphqlAutoBackupSettingsRecord,
        GraphqlDefaultDataStorageRecord, GraphqlRoleSummaryRecord,
        GraphqlStoragePolicyRecord, GraphqlSystemChannelSettingsRecord,
        OpenApiGraphqlMutationRepository,
        SeaOrmAdminGraphqlSubsetRepository, SeaOrmOpenApiGraphqlMutationRepository,
    },
    repositories::prompt_protection::{
        bulk_set_prompt_protection_rule_status_seaorm,
        bulk_soft_delete_prompt_protection_rules_seaorm, create_prompt_protection_rule_seaorm,
        list_prompt_protection_rules_seaorm, load_prompt_protection_rule_seaorm,
        prompt_protection_rule_name_exists_seaorm, set_prompt_protection_rule_status_seaorm,
        soft_delete_prompt_protection_rule_seaorm, update_prompt_protection_rule_seaorm,
    },
    shared::{
        format_unix_timestamp, graphql_gid, i64_to_i32,
    },
    system::hash_password,
};
use serde_json::json;

pub struct SeaOrmAdminGraphqlService {
    repository: SeaOrmAdminGraphqlSubsetRepository,
    circuit_breaker: SharedCircuitBreaker,
}

pub struct SeaOrmOpenApiGraphqlService {
    repository: SeaOrmOpenApiGraphqlMutationRepository,
}



#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlSystemStatus {
    pub(crate) is_initialized: bool,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "CleanupOption", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCleanupOption {
    pub(crate) resource_type: String,
    pub(crate) enabled: bool,
    pub(crate) cleanup_days: i32,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "StoragePolicy", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlStoragePolicy {
    pub(crate) store_chunks: bool,
    pub(crate) store_request_body: bool,
    pub(crate) store_response_body: bool,
    pub(crate) cleanup_options: Vec<AdminGraphqlCleanupOption>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CleanupOptionInput")]
pub(crate) struct AdminGraphqlCleanupOptionInput {
    pub(crate) resource_type: String,
    pub(crate) enabled: bool,
    pub(crate) cleanup_days: i32,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateStoragePolicyInput")]
pub(crate) struct AdminGraphqlUpdateStoragePolicyInput {
    pub(crate) store_chunks: Option<bool>,
    pub(crate) store_request_body: Option<bool>,
    pub(crate) store_response_body: Option<bool>,
    pub(crate) cleanup_options: Option<Vec<AdminGraphqlCleanupOptionInput>>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AutoBackupSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlAutoBackupSettings {
    pub(crate) enabled: bool,
    pub(crate) frequency: BackupFrequencySetting,
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: i32,
    pub(crate) include_channels: bool,
    pub(crate) include_models: bool,
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: bool,
    pub(crate) include_model_prices: bool,
    pub(crate) retention_days: i32,
    pub(crate) last_backup_at: Option<String>,
    pub(crate) last_backup_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateAutoBackupSettingsInput")]
pub(crate) struct AdminGraphqlUpdateAutoBackupSettingsInput {
    pub(crate) enabled: Option<bool>,
    pub(crate) frequency: Option<BackupFrequencySetting>,
    #[serde(rename = "dataStorageID")]
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: Option<i32>,
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
    #[serde(rename = "includeAPIKeys")]
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: Option<bool>,
    pub(crate) include_model_prices: Option<bool>,
    pub(crate) retention_days: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "TriggerBackupPayload", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTriggerBackupPayload {
    pub(crate) success: bool,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbeSetting", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelProbeSetting {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemChannelSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlSystemChannelSettings {
    pub(crate) probe: AdminGraphqlChannelProbeSetting,
    pub(crate) auto_sync: AdminGraphqlChannelModelAutoSyncSetting,
    pub(crate) query_all_channel_models: bool,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelModelAutoSyncSetting", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelModelAutoSyncSetting {
    pub(crate) frequency: AutoSyncFrequencySetting,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateChannelProbeSettingInput")]
pub(crate) struct AdminGraphqlUpdateChannelProbeSettingInput {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateSystemChannelSettingsInput")]
pub(crate) struct AdminGraphqlUpdateSystemChannelSettingsInput {
    pub(crate) probe: Option<AdminGraphqlUpdateChannelProbeSettingInput>,
    pub(crate) auto_sync: Option<AdminGraphqlUpdateChannelModelAutoSyncSettingInput>,
    pub(crate) query_all_channel_models: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateChannelModelAutoSyncSettingInput")]
pub(crate) struct AdminGraphqlUpdateChannelModelAutoSyncSettingInput {
    pub(crate) frequency: AutoSyncFrequencySetting,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProxyPreset", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProxyPreset {
    pub(crate) name: Option<String>,
    pub(crate) url: String,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "SaveProxyPresetInput")]
pub(crate) struct AdminGraphqlSaveProxyPresetInput {
    pub(crate) name: Option<String>,
    pub(crate) url: String,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserAgentPassThroughSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserAgentPassThroughSettings {
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateDefaultDataStorageInput")]
pub(crate) struct AdminGraphqlUpdateDefaultDataStorageInput {
    #[serde(rename = "dataStorageID")]
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: String,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbePoint", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelProbePoint {
    pub(crate) timestamp: i64,
    pub(crate) total_request_count: i32,
    pub(crate) success_request_count: i32,
    pub(crate) avg_tokens_per_second: Option<f64>,
    pub(crate) avg_time_to_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbeData", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelProbeData {
    pub(crate) channel_id: String,
    pub(crate) points: Vec<AdminGraphqlChannelProbePoint>,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "GetChannelProbeDataInput")]
pub(crate) struct AdminGraphqlGetChannelProbeDataInput {
    pub(crate) channel_ids: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProviderQuotaStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProviderQuotaStatus {
    pub(crate) id: String,
    pub(crate) channel_id: String,
    pub(crate) provider_type: String,
    pub(crate) status: String,
    pub(crate) ready: bool,
    pub(crate) next_reset_at: Option<String>,
    pub(crate) next_check_at: String,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "CircuitBreakerStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCircuitBreakerStatus {
    pub(crate) channel_id: String,
    pub(crate) model_id: String,
    pub(crate) state: String,
    pub(crate) consecutive_failures: i32,
    pub(crate) next_probe_at_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum PromptProtectionRuleStatus {
    Enabled,
    Disabled,
    Archived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum AdminGraphqlPromptProtectionAction {
    Mask,
    Reject,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum AdminGraphqlPromptProtectionScope {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "PromptProtectionSettingsInput")]
pub(crate) struct AdminGraphqlPromptProtectionSettingsInput {
    pub(crate) action: AdminGraphqlPromptProtectionAction,
    pub(crate) replacement: Option<String>,
    pub(crate) scopes: Option<Vec<AdminGraphqlPromptProtectionScope>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreatePromptProtectionRuleInput")]
pub(crate) struct AdminGraphqlCreatePromptProtectionRuleInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) pattern: String,
    pub(crate) settings: AdminGraphqlPromptProtectionSettingsInput,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdatePromptProtectionRuleInput")]
pub(crate) struct AdminGraphqlUpdatePromptProtectionRuleInput {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) pattern: Option<String>,
    pub(crate) status: Option<PromptProtectionRuleStatus>,
    pub(crate) settings: Option<AdminGraphqlPromptProtectionSettingsInput>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Channel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannel {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) channel_type: String,
    pub(crate) base_url: String,
    pub(crate) status: String,
    pub(crate) supported_models: Vec<String>,
    pub(crate) ordering_weight: i32,
    pub(crate) provider_quota_status: Option<AdminGraphqlProviderQuotaStatus>,
    pub(crate) circuit_breaker_status: Option<AdminGraphqlCircuitBreakerStatus>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Model", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModel {
    pub(crate) id: String,
    pub(crate) developer: String,
    pub(crate) model_id: String,
    pub(crate) model_type: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) remark: String,
    pub(crate) context_length: Option<i32>,
    pub(crate) max_output_tokens: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Request", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRequestSummaryObject {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) trace_id: Option<String>,
    pub(crate) channel_id: Option<String>,
    pub(crate) model_id: String,
    pub(crate) format: String,
    pub(crate) status: String,
    pub(crate) source: String,
    pub(crate) external_id: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Trace", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTrace {
    pub(crate) id: String,
    pub(crate) trace_id: String,
    pub(crate) project_id: String,
    pub(crate) thread_id: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "APIKey", rename_fields = "camelCase")]
pub(crate) struct OpenApiGraphqlApiKey {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone)]
struct GraphqlProjectRecord {
    id: i64,
    name: String,
    description: String,
    status: String,
}

#[derive(Debug, Clone)]
struct GraphqlRoleRecord {
    id: i64,
    name: String,
    level: String,
    project_id: i64,
    scopes: String,
}

#[derive(Debug, Clone)]
struct GraphqlApiKeyRecord {
    id: i64,
    project_id: i64,
    key: String,
    name: String,
    key_type: String,
    status: String,
    scopes: String,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AdminApiKey", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlApiKey {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) key_type: String,
    pub(crate) status: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "BackupPayload", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlBackupPayload {
    pub(crate) success: bool,
    pub(crate) data: Option<String>,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RestorePayload", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRestorePayload {
    pub(crate) success: bool,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum CreateLlmApiKeyError {
    InvalidName,
    PermissionDenied,
    Internal(String),
}

impl SeaOrmAdminGraphqlService {
    pub fn new(db: super::seaorm::SeaOrmConnectionFactory) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory(&db);
        Self {
            repository: SeaOrmAdminGraphqlSubsetRepository::new(db),
            circuit_breaker,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_circuit_breaker_policy(
        db: super::seaorm::SeaOrmConnectionFactory,
        policy: CircuitBreakerPolicy,
    ) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory_and_policy(&db, policy);
        Self {
            repository: SeaOrmAdminGraphqlSubsetRepository::new(db),
            circuit_breaker,
        }
    }
}

impl SeaOrmOpenApiGraphqlService {
    pub fn new(db: super::seaorm::SeaOrmConnectionFactory) -> Self {
        Self {
            repository: SeaOrmOpenApiGraphqlMutationRepository::new(db),
        }
    }
}

impl AdminGraphqlPort for SeaOrmAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let repository = self.repository.clone();
        let circuit_breaker = self.circuit_breaker.clone();
        Box::pin(async move {
            let payload = request;
            match execute_admin_graphql_seaorm_request(
                repository,
                circuit_breaker,
                payload,
                project_id,
                user,
            )
            .await
            {
                Ok(result) => result,
                Err(message) => GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": null,
                        "errors": [{"message": format!("Failed to execute GraphQL request: {message}")}],
                    }),
                },
            }
        })
    }
}

impl AdminGraphqlRepository for SeaOrmAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        <Self as AdminGraphqlPort>::execute_graphql(self, request, project_id, user)
    }
}

impl OpenApiGraphqlPort for SeaOrmOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let repository = self.repository.clone();
        Box::pin(async move {
            let payload = request;
            match execute_openapi_graphql_seaorm_request(repository, payload, owner_api_key).await {
                Ok(result) => result,
                Err(message) => GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": null,
                        "errors": [{"message": format!("Failed to execute GraphQL request: {message}")}],
                    }),
                },
            }
        })
    }
}

impl OpenApiGraphqlRepository for SeaOrmOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        <Self as OpenApiGraphqlPort>::execute_graphql(self, request, owner_api_key)
    }
}

async fn execute_admin_graphql_seaorm_request(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    circuit_breaker: SharedCircuitBreaker,
    payload: GraphqlRequestPayload,
    _project_id: Option<i64>,
    user: AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let query = payload.query.trim();

    if query.contains("storagePolicy") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("storagePolicy");
        }

        return query_storage_policy_seaorm(&repository);
    }

    if query.contains("autoBackupSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("autoBackupSettings");
        }

        return query_auto_backup_settings_seaorm(&repository);
    }

    if query.contains("defaultDataStorageID") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("defaultDataStorageID");
        }

        return query_default_data_storage_id_seaorm(&repository);
    }

    if query.contains("systemChannelSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("systemChannelSettings");
        }

        return query_system_channel_settings_seaorm(&repository);
    }

    if query.contains("proxyPresets") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("proxyPresets");
        }

        return query_proxy_presets_seaorm(&repository);
    }

    if query.contains("userAgentPassThroughSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("userAgentPassThroughSettings");
        }

        return query_user_agent_pass_through_settings_seaorm(&repository);
    }

    if query.contains("channels") {
        if authorize_user_system_scope(&user, SCOPE_READ_CHANNELS).is_err() {
            return graphql_permission_denied("channels");
        }

        return query_channels_seaorm(&repository, &circuit_breaker);
    }

    if query.contains("updateStoragePolicy") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateStoragePolicy");
        }

        return update_storage_policy_seaorm(&repository, payload.variables);
    }

    if query.contains("updateAutoBackupSettings") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("updateAutoBackupSettings");
        }

        return update_auto_backup_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("updateDefaultDataStorage") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateDefaultDataStorage");
        }

        return update_default_data_storage_seaorm(&repository, payload.variables);
    }

    if query.contains("updateSystemChannelSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateSystemChannelSettings");
        }

        return update_system_channel_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("saveProxyPreset") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("saveProxyPreset");
        }

        return save_proxy_preset_seaorm(&repository, payload.variables);
    }

    if query.contains("deleteProxyPreset") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("deleteProxyPreset");
        }

        return delete_proxy_preset_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUserAgentPassThroughSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateUserAgentPassThroughSettings");
        }

        return update_user_agent_pass_through_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("triggerAutoBackup") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("triggerAutoBackup");
        }

        return trigger_auto_backup_seaorm(&repository, user.id);
    }

    if query.contains("backup") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("backup");
        }

        return backup_seaorm(&repository, payload.variables);
    }

    if query.contains("restore") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("restore");
        }

        return restore_seaorm(&repository, payload.variables, user.id);
    }

    if query.contains("checkProviderQuotas") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("checkProviderQuotas");
        }

        return check_provider_quotas_seaorm(&repository, user.id);
    }

    if query.contains("resetProviderQuota") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("resetProviderQuota");
        }

        return reset_provider_quota_seaorm(&repository, payload.variables, user.id);
    }

    if query.contains("triggerGcCleanup") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return Ok(GraphqlExecutionResult {
                status: 200,
                body: json!({
                    "data": {"triggerGcCleanup": Value::Null},
                    "errors": [{"message": "permission denied: requires write:settings scope"}],
                }),
            });
        }

        return trigger_gc_cleanup_seaorm(&repository, user.id);
    }

    if query.contains("createUser") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_WRITE_USERS).is_err() {
            return graphql_permission_denied("createUser");
        }

        return create_user_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUserStatus") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_WRITE_USERS).is_err() {
            return graphql_permission_denied("updateUserStatus");
        }

        return update_user_status_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUser") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_WRITE_USERS).is_err() {
            return graphql_permission_denied("updateUser");
        }

        return update_user_seaorm(&repository, payload.variables);
    }

    if query.contains("createPromptProtectionRule") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("createPromptProtectionRule");
        }

        return create_prompt_protection_rule_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updatePromptProtectionRuleStatus") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("updatePromptProtectionRuleStatus");
        }

        return update_prompt_protection_rule_status_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("bulkDeletePromptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkDeletePromptProtectionRules");
        }

        return bulk_delete_prompt_protection_rules_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("bulkEnablePromptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkEnablePromptProtectionRules");
        }

        return bulk_set_prompt_protection_rules_status_graphql_seaorm(
            &repository,
            payload.variables,
            "enabled",
            "bulkEnablePromptProtectionRules",
        );
    }

    if query.contains("bulkDisablePromptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkDisablePromptProtectionRules");
        }

        return bulk_set_prompt_protection_rules_status_graphql_seaorm(
            &repository,
            payload.variables,
            "disabled",
            "bulkDisablePromptProtectionRules",
        );
    }

    if query.contains("deletePromptProtectionRule") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("deletePromptProtectionRule");
        }

        return delete_prompt_protection_rule_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updatePromptProtectionRule") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("updatePromptProtectionRule");
        }

        return update_prompt_protection_rule_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updateMe") {
        return update_me_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createProject") {
        return create_project_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateProject") {
        return update_project_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createRole") {
        return create_role_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateRole") {
        return update_role_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createAPIKey") {
        return create_api_key_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateAPIKey") {
        return update_api_key_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createChannel") {
        return create_channel_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateChannel") {
        return update_channel_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createModel") {
        return create_model_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateModel") {
        return update_model_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("queryModels") {
        if authorize_user_system_scope(&user, SCOPE_READ_CHANNELS).is_err() {
            return graphql_permission_denied("queryModels");
        }

        let models = repository
            .query_model_statuses()?
            .into_iter()
            .map(|row| {
                serde_json::json!({
                    "id": graphql_gid("model", row.id),
                    "status": row.status,
                })
            })
            .collect::<Vec<_>>();

        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({"data": {"queryModels": models}}),
        });
    }

    if query.contains("promptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_READ_PROMPTS).is_err() {
            return graphql_permission_denied("promptProtectionRules");
        }

        return query_prompt_protection_rules_graphql_seaorm(&repository);
    }

    if query.contains("systemStatus") {
        let is_initialized = repository.query_is_initialized()?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({"data": {"systemStatus": {"isInitialized": is_initialized}}}),
        });
    }

    if let Some(field) = first_graphql_field_name(query) {
        return graphql_not_implemented_for_route("/admin/graphql", field.as_str());
    }

    Err("unsupported graphql subset query".to_owned())
}

fn graphql_permission_denied(field: &str) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {field: Value::Null},
            "errors": [{"message": "permission denied"}],
        }),
    })
}

fn graphql_owner_denied(field: &str) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {field: Value::Null},
            "errors": [{"message": "permission denied: owner access required"}],
        }),
    })
}

fn graphql_field_error(field: &str, message: impl Into<String>) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {field: Value::Null},
            "errors": [{"message": message.into()}],
        }),
    })
}

fn graphql_not_implemented_for_route(
    route_family: &str,
    field: &str,
) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 501,
        body: json!({
            "error": "not_implemented",
            "status": 501,
            "route_family": route_family,
            "method": "POST",
            "path": route_family,
            "message": format!("GraphQL field `{field}` is not supported"),
        }),
    })
}

fn query_storage_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let policy = load_storage_policy_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "storagePolicy": storage_policy_json(&policy),
            }
        }),
    })
}

fn query_auto_backup_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_auto_backup_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "autoBackupSettings": auto_backup_settings_json(&settings),
            }
        }),
    })
}

fn query_default_data_storage_id_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let current = load_default_data_storage_id_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "defaultDataStorageID": current.map(|id| graphql_gid("DataStorage", id)),
            }
        }),
    })
}

fn query_system_channel_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_system_channel_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "systemChannelSettings": system_channel_settings_json(&settings),
            }
        }),
    })
}

fn query_proxy_presets_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let presets = SeaOrmOperationalService::new(repository.db()).proxy_presets()?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "proxyPresets": presets.into_iter().map(|preset| proxy_preset_json(&preset)).collect::<Vec<_>>(),
            }
        }),
    })
}

fn query_user_agent_pass_through_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let enabled = SeaOrmOperationalService::new(repository.db()).user_agent_pass_through()?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "userAgentPassThroughSettings": {
                    "enabled": enabled,
                },
            }
        }),
    })
}

fn query_channels_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    circuit_breaker: &SharedCircuitBreaker,
) -> Result<GraphqlExecutionResult, String> {
    let channels = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        channels::Entity::find()
            .filter(channels::Column::DeletedAt.eq(0_i64))
            .all(&connection)
            .await
            .map_err(|error| error.to_string())
    })?;

    let quota_by_channel = SeaOrmOperationalService::new(repository.db())
        .provider_quota_statuses()
        .unwrap_or_default()
        .into_iter()
        .map(|status| (status.channel_id, AdminGraphqlProviderQuotaStatus::from(status)))
        .collect::<std::collections::HashMap<_, _>>();

    let items = channels
        .into_iter()
        .map(|channel| {
            let mut gql = AdminGraphqlChannel {
                id: graphql_gid("channel", channel.id),
                name: channel.name,
                channel_type: channel.type_field,
                base_url: channel.base_url.unwrap_or_default(),
                status: channel.status,
                supported_models: serde_json::from_str(&channel.supported_models).unwrap_or_default(),
                ordering_weight: channel.ordering_weight,
                provider_quota_status: quota_by_channel.get(&channel.id).cloned(),
                circuit_breaker_status: stored_circuit_breaker_status(channel.id, circuit_breaker)
                    .map(AdminGraphqlCircuitBreakerStatus::from),
            };
            if gql.provider_quota_status.is_none() {
                gql.provider_quota_status = None;
            }
            channel_json(&gql)
        })
        .collect::<Vec<_>>();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"channels": items}}),
    })
}

fn update_storage_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateStoragePolicyInput>(
        variables,
        "input",
        "storage policy input is required",
    )?;
    let mut policy = load_storage_policy_seaorm(repository)?;
    if let Some(store_chunks) = input.store_chunks {
        policy.store_chunks = store_chunks;
    }
    if let Some(store_request_body) = input.store_request_body {
        policy.store_request_body = store_request_body;
    }
    if let Some(store_response_body) = input.store_response_body {
        policy.store_response_body = store_response_body;
    }
    if let Some(cleanup_options) = input.cleanup_options {
        policy.cleanup_options = cleanup_options
            .into_iter()
            .map(|option| super::admin::StoredCleanupOption {
                resource_type: option.resource_type,
                enabled: option.enabled,
                cleanup_days: option.cleanup_days,
            })
            .collect();
    }
    SeaOrmOperationalService::new(repository.db()).update_storage_policy(policy)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateStoragePolicy": true}}),
    })
}

fn update_auto_backup_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateAutoBackupSettingsInput>(
        variables,
        "input",
        "auto backup input is required",
    )?;
    let mut settings = load_auto_backup_settings_seaorm(repository)?;
    if let Some(enabled) = input.enabled {
        settings.enabled = enabled;
    }
    if let Some(frequency) = input.frequency {
        settings.frequency = frequency;
    }
    if let Some(data_storage_id) = input.data_storage_id {
        settings.data_storage_id = i64::from(data_storage_id);
    }
    if let Some(include_channels) = input.include_channels {
        settings.include_channels = include_channels;
    }
    if let Some(include_models) = input.include_models {
        settings.include_models = include_models;
    }
    if let Some(include_api_keys) = input.include_api_keys {
        settings.include_api_keys = include_api_keys;
    }
    if let Some(include_model_prices) = input.include_model_prices {
        settings.include_model_prices = include_model_prices;
    }
    if let Some(retention_days) = input.retention_days {
        settings.retention_days = retention_days.max(0);
    }
    if settings.enabled && settings.data_storage_id <= 0 {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateAutoBackupSettings": Value::Null},
                "errors": [{"message": "dataStorageID is required when auto backup is enabled"}],
            }),
        });
    }
    SeaOrmOperationalService::new(repository.db()).update_auto_backup_settings(settings)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateAutoBackupSettings": true}}),
    })
}

fn update_default_data_storage_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateDefaultDataStorageInput>(
        variables,
        "input",
        "default data storage input is required",
    )?;
    let data_storage_id = match parse_graphql_resource_id(input.data_storage_id.as_str(), "DataStorage") {
        Ok(id) => id,
        Err(_) => {
            return Ok(GraphqlExecutionResult {
                status: 200,
                body: json!({
                    "data": {"updateDefaultDataStorage": Value::Null},
                    "errors": [{"message": "invalid dataStorageID"}],
                }),
            })
        }
    };
    let current = repository.query_data_storage_status(data_storage_id)?;
    let Some(current) = current else {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateDefaultDataStorage": Value::Null},
                "errors": [{"message": "data storage not found"}],
            }),
        });
    };
    if current.id <= 0 {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateDefaultDataStorage": Value::Null},
                "errors": [{"message": "data storage not found"}],
            }),
        });
    }
    if !current.status.eq_ignore_ascii_case("active") {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateDefaultDataStorage": Value::Null},
                "errors": [{"message": "data storage is not active"}],
            }),
        });
    }

    repository.upsert_default_data_storage(data_storage_id.to_string().as_str())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateDefaultDataStorage": true}}),
    })
}

fn update_system_channel_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let mut settings = load_system_channel_settings_seaorm(repository)?;
    if let Some(probe) = variables.get("input").and_then(|input| input.get("probe")) {
        let enabled = probe
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(|| "invalid probe.enabled: expected boolean".to_owned())?;
        let frequency = parse_probe_frequency_graphql_value(
            probe
                .get("frequency")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid probe.frequency: expected string".to_owned())?,
        )?;
        settings.probe = super::admin::StoredChannelProbeSettings { enabled, frequency };
    }
    if let Some(auto_sync) = variables.get("input").and_then(|input| input.get("autoSync")) {
        let frequency = parse_auto_sync_frequency_graphql_value(
            auto_sync
                .get("frequency")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid autoSync.frequency: expected string".to_owned())?,
        )?;
        settings.auto_sync = super::admin::StoredChannelModelAutoSyncSettings { frequency };
    }
    if let Some(query_all_channel_models) = variables
        .get("input")
        .and_then(|input| input.get("queryAllChannelModels"))
    {
        settings.query_all_channel_models = query_all_channel_models
            .as_bool()
            .ok_or_else(|| "invalid queryAllChannelModels: expected boolean".to_owned())?;
    }
    SeaOrmOperationalService::new(repository.db()).update_system_channel_settings(settings)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateSystemChannelSettings": true}}),
    })
}

fn save_proxy_preset_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = variables
        .get("input")
        .ok_or_else(|| "input is required".to_owned())?;
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "invalid url: expected non-empty string".to_owned())?;
    let preset = StoredProxyPreset {
        name: input.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_owned(),
        url: url.to_owned(),
        username: input.get("username").and_then(Value::as_str).unwrap_or_default().trim().to_owned(),
        password: input.get("password").and_then(Value::as_str).unwrap_or_default().to_owned(),
    };
    SeaOrmOperationalService::new(repository.db()).save_proxy_preset(preset)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"saveProxyPreset": true}}),
    })
}

fn delete_proxy_preset_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let url = variables
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "invalid url: expected non-empty string".to_owned())?;
    SeaOrmOperationalService::new(repository.db()).delete_proxy_preset(url)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"deleteProxyPreset": true}}),
    })
}

fn update_user_agent_pass_through_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let enabled = variables
        .get("input")
        .and_then(|input| input.get("enabled"))
        .and_then(Value::as_bool)
        .ok_or_else(|| "invalid enabled: expected boolean".to_owned())?;
    SeaOrmOperationalService::new(repository.db()).set_user_agent_pass_through(enabled)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateUserAgentPassThroughSettings": true}}),
    })
}

fn trigger_auto_backup_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let message = SeaOrmOperationalService::new(repository.db()).trigger_backup_now(Some(user_id))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "triggerAutoBackup": {
                    "success": true,
                    "message": message,
                }
            }
        }),
    })
}

fn check_provider_quotas_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    SeaOrmOperationalService::new(repository.db())
        .run_provider_quota_check_tick(true, std::time::Duration::from_secs(20 * 60), Some(user_id))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"checkProviderQuotas": true}}),
    })
}

fn reset_provider_quota_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let channel_id = variables
        .get("channelID")
        .and_then(Value::as_str)
        .ok_or_else(|| "channel id is required".to_owned())
        .and_then(|value| parse_graphql_resource_id(value, "channel"))?;
    let updated = SeaOrmOperationalService::new(repository.db())
        .reset_provider_quota_status(channel_id, Some(user_id))?;
    if !updated {
        return graphql_field_error("resetProviderQuota", "provider quota channel not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"resetProviderQuota": true}}),
    })
}

fn trigger_gc_cleanup_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    SeaOrmOperationalService::new(repository.db()).run_gc_cleanup_now(false, Some(user_id))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"triggerGcCleanup": true}}),
    })
}

fn backup_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlBackupInput>(
        variables,
        "input",
        "backup input is required",
    )?;

    let mut settings = default_auto_backup_settings();
    if let Some(include_channels) = input.include_channels {
        settings.include_channels = include_channels;
    }
    if let Some(include_models) = input.include_models {
        settings.include_models = include_models;
    }
    if let Some(include_api_keys) = input.include_api_keys {
        settings.include_api_keys = include_api_keys;
    }
    if let Some(include_model_prices) = input.include_model_prices {
        settings.include_model_prices = include_model_prices;
    }

    let payload = SeaOrmOperationalService::new(repository.db()).build_backup_payload(&settings)?;
    let data = serde_json::to_string(&payload)
        .map_err(|error| format!("failed to serialize backup payload: {error}"))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "backup": {
                    "success": true,
                    "data": data,
                    "message": "Backup completed successfully",
                }
            }
        }),
    })
}

fn restore_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlRestoreInput>(
        variables,
        "input",
        "restore input is required",
    )?;
    let message = SeaOrmOperationalService::new(repository.db()).restore_backup(
        input.payload.as_bytes(),
        super::admin_operational::RestoreOptions {
            include_channels: input.include_channels.unwrap_or(true),
            include_models: input.include_models.unwrap_or(true),
            include_api_keys: input.include_api_keys.unwrap_or(true),
            include_model_prices: input.include_model_prices.unwrap_or(true),
            overwrite_existing: input.overwrite_existing.unwrap_or(false),
        },
        Some(user_id),
    )?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "restore": {
                    "success": true,
                    "message": message,
                }
            }
        }),
    })
}

fn create_project_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_PROJECTS).is_err() {
        return graphql_permission_denied("createProject");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlCreateProjectInput>(
        variables,
        "input",
        "project input is required",
    )?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createProject", "project name is required");
    }
    let description = input.description.unwrap_or_default();
    let status = normalize_project_status(input.status.as_deref())?.to_owned();

    let record = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        if projects::Entity::find()
            .filter(projects::Column::Name.eq(name.clone()))
            .filter(projects::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("project already exists".to_owned());
        }
        let created = projects::Entity::insert(projects::ActiveModel {
            name: Set(name.clone()),
            description: Set(description.clone()),
            status: Set(status.clone()),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(&connection)
        .await
        .map_err(|error| error.to_string())?;
        load_project_record(&connection, created.last_insert_id).await?
            .ok_or_else(|| "project not found".to_owned())
    })?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"createProject": project_json(&record)}}),
    })
}

fn update_project_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_PROJECTS).is_err() {
        return graphql_permission_denied("updateProject");
    }
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "project id is required".to_owned())?;
    let project_id = parse_graphql_resource_id(id, "project")
        .map_err(|_| "invalid project id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateProjectInput>(
        variables,
        "input",
        "project input is required",
    )?;
    if input.name.is_none() && input.description.is_none() && input.status.is_none() {
        return graphql_field_error("updateProject", "no fields to update");
    }

    let record = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let existing = projects::Entity::find_by_id(project_id)
            .filter(projects::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?;
        let Some(existing) = existing else {
            return Err("project not found".to_owned());
        };

        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            if let Some(other) = projects::Entity::find()
                .filter(projects::Column::Name.eq(name))
                .filter(projects::Column::DeletedAt.eq(0_i64))
                .one(&connection)
                .await
                .map_err(|error| error.to_string())?
            {
                if other.id != project_id {
                    return Err("project already exists".to_owned());
                }
            }
        }

        let mut active: projects::ActiveModel = existing.into();
        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            active.name = Set(name.to_owned());
        }
        if let Some(description) = input.description {
            active.description = Set(description);
        }
        if let Some(status) = input.status.as_deref() {
            active.status = Set(normalize_project_status(Some(status))?.to_owned());
        }
        active.deleted_at = Set(0_i64);
        active.update(&connection).await.map_err(|error| error.to_string())?;
        load_project_record(&connection, project_id).await?
            .ok_or_else(|| "project not found".to_owned())
    });

    match record {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateProject": project_json(&record)}}),
        }),
        Err(message) if message == "project not found" => graphql_field_error("updateProject", message),
        Err(message) if message == "project already exists" => graphql_field_error("updateProject", message),
        Err(message) => Err(message),
    }
}

fn create_role_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreateRoleInput>(
        variables,
        "input",
        "role input is required",
    )?;
    let level = normalize_role_level(input.level.as_deref())?;
    let project_id = parse_role_project_id(input.project_id.as_deref(), level)?;
    authorize_role_write(user, level, project_id, "createRole")?;
    validate_scope_list(input.scopes.as_deref().unwrap_or(&[]), "createRole")?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createRole", "role name is required");
    }
    let level = level.to_owned();
    let scopes_json = serde_json::to_string(&input.scopes.unwrap_or_default())
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;

    let record = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        if roles::Entity::find()
            .filter(roles::Column::Name.eq(name.clone()))
            .filter(roles::Column::ProjectId.eq(project_id))
            .filter(roles::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("role already exists".to_owned());
        }
        let created = roles::Entity::insert(roles::ActiveModel {
            name: Set(name.clone()),
            level: Set(level.clone()),
            project_id: Set(project_id),
            scopes: Set(scopes_json.clone()),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(&connection)
        .await
        .map_err(|error| error.to_string())?;
        load_role_record(&connection, created.last_insert_id).await?
            .ok_or_else(|| "role not found".to_owned())
    });

    match record {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createRole": role_json(&record)}}),
        }),
        Err(message) if message == "role already exists" => graphql_field_error("createRole", message),
        Err(message) => Err(message),
    }
}

fn update_role_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "role id is required".to_owned())?;
    let role_id = parse_graphql_resource_id(id, "role").map_err(|_| "invalid role id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateRoleInput>(
        variables,
        "input",
        "role input is required",
    )?;
    if input.name.is_none() && input.level.is_none() && input.project_id.is_none() && input.scopes.is_none() {
        return graphql_field_error("updateRole", "no fields to update");
    }

    let user_owned = user.clone();
    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let existing = roles::Entity::find_by_id(role_id)
            .filter(roles::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?;
        let Some(existing) = existing else {
            return Err("role not found".to_owned());
        };

        let level = normalize_role_level(input.level.as_deref().or(Some(existing.level.as_str())))?;
        let project_id = parse_role_project_id(input.project_id.as_deref(), level)
            .unwrap_or(existing.project_id);
        authorize_role_write(&user_owned, level, project_id, "updateRole")?;
        if let Some(scopes) = input.scopes.as_deref() {
            validate_scope_list(scopes, "updateRole")?;
        }

        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            if let Some(other) = roles::Entity::find()
                .filter(roles::Column::Name.eq(name))
                .filter(roles::Column::ProjectId.eq(project_id))
                .filter(roles::Column::DeletedAt.eq(0_i64))
                .one(&connection)
                .await
                .map_err(|error| error.to_string())?
            {
                if other.id != role_id {
                    return Err("role already exists".to_owned());
                }
            }
        }

        let mut active: roles::ActiveModel = existing.into();
        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            active.name = Set(name.to_owned());
        }
        active.level = Set(level.to_owned());
        active.project_id = Set(project_id);
        if let Some(scopes) = input.scopes {
            active.scopes = Set(
                serde_json::to_string(&scopes)
                    .map_err(|error| format!("failed to serialize scopes: {error}"))?,
            );
        }
        active.deleted_at = Set(0_i64);
        active.update(&connection).await.map_err(|error| error.to_string())?;
        load_role_record(&connection, role_id).await?
            .ok_or_else(|| "role not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateRole": role_json(&record)}}),
        }),
        Err(message) if message == "permission denied" => graphql_permission_denied("updateRole"),
        Err(message) if message == "role not found" || message == "role already exists" => {
            graphql_field_error("updateRole", message)
        }
        Err(message) => Err(message),
    }
}

fn create_api_key_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreateApiKeyInput>(
        variables,
        "input",
        "api key input is required",
    )?;
    let project_id = parse_graphql_resource_id(input.project_id.as_str(), "project")
        .map_err(|_| "invalid project id".to_owned())?;
    if require_user_project_scope(user, project_id, SCOPE_WRITE_API_KEYS).is_err() {
        return graphql_permission_denied("createAPIKey");
    }
    validate_scope_list(input.scopes.as_deref().unwrap_or(&[]), "createAPIKey")?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createAPIKey", "api key name is required");
    }
    let key = input.key.unwrap_or(generate_llm_api_key()?);
    let key_type = normalize_api_key_type(input.key_type.as_deref())?.to_owned();
    let status = normalize_api_key_status(input.status.as_deref())?.to_owned();
    let scopes_json = serde_json::to_string(&input.scopes.unwrap_or_default())
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let owner_user_id = user.id;

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        if projects::Entity::find_by_id(project_id)
            .filter(projects::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("project not found".to_owned());
        }
        if api_keys::Entity::find()
            .filter(api_keys::Column::Key.eq(key.clone()))
            .filter(api_keys::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("api key already exists".to_owned());
        }

        let created = api_keys::Entity::insert(api_keys::ActiveModel {
            user_id: Set(owner_user_id),
            project_id: Set(project_id),
            key: Set(key.clone()),
            name: Set(name.clone()),
            type_field: Set(key_type.clone()),
            status: Set(status.clone()),
            scopes: Set(scopes_json.clone()),
            profiles: Set(input.profiles_json.unwrap_or_else(|| "{}".to_owned())),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(&connection)
        .await
        .map_err(|error| error.to_string())?;
        load_api_key_record(&connection, created.last_insert_id).await?
            .ok_or_else(|| "api key not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createAPIKey": api_key_json(&record)}}),
        }),
        Err(message)
            if message == "project not found" || message == "api key already exists" =>
        {
            graphql_field_error("createAPIKey", message)
        }
        Err(message) => Err(message),
    }
}

fn update_api_key_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "api key id is required".to_owned())?;
    let api_key_id = parse_graphql_resource_id(id, "api_key")
        .or_else(|_| parse_graphql_resource_id(id, "apiKey"))
        .map_err(|_| "invalid api key id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateApiKeyInput>(
        variables,
        "input",
        "api key input is required",
    )?;
    if input.name.is_none() && input.status.is_none() && input.scopes.is_none() {
        return graphql_field_error("updateAPIKey", "no fields to update");
    }
    if let Some(scopes) = input.scopes.as_deref() {
        validate_scope_list(scopes, "updateAPIKey")?;
    }

    let user_owned = user.clone();
    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let existing = api_keys::Entity::find_by_id(api_key_id)
            .filter(api_keys::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?;
        let Some(existing) = existing else {
            return Err("api key not found".to_owned());
        };
        if require_user_project_scope(&user_owned, existing.project_id, SCOPE_WRITE_API_KEYS).is_err() {
            return Err("permission denied".to_owned());
        }
        let mut active: api_keys::ActiveModel = existing.into();
        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            active.name = Set(name.to_owned());
        }
        if let Some(status) = input.status.as_deref() {
            active.status = Set(normalize_api_key_status(Some(status))?.to_owned());
        }
        if let Some(scopes) = input.scopes {
            active.scopes = Set(
                serde_json::to_string(&scopes)
                    .map_err(|error| format!("failed to serialize scopes: {error}"))?,
            );
        }
        active.deleted_at = Set(0_i64);
        active.update(&connection).await.map_err(|error| error.to_string())?;
        load_api_key_record(&connection, api_key_id).await?
            .ok_or_else(|| "api key not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateAPIKey": api_key_json(&record)}}),
        }),
        Err(message) if message == "permission denied" => graphql_permission_denied("updateAPIKey"),
        Err(message) if message == "api key not found" => graphql_field_error("updateAPIKey", message),
        Err(message) => Err(message),
    }
}

fn create_channel_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("createChannel");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlCreateChannelInput>(
        variables,
        "input",
        "channel input is required",
    )?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createChannel", "channel name is required");
    }
    let channel_type = input.channel_type.trim().to_owned();
    if channel_type.is_empty() {
        return graphql_field_error("createChannel", "channel type is required");
    }
    let status = normalize_enable_status(input.status.as_deref(), "channel")?.to_owned();
    let supported_models = serde_json::to_string(&input.supported_models.unwrap_or_default())
        .map_err(|error| format!("failed to serialize supported models: {error}"))?;
    let tags = serde_json::to_string(&input.tags.unwrap_or_default())
        .map_err(|error| format!("failed to serialize tags: {error}"))?;
    let credentials_json = normalize_json_blob(input.credentials_json.as_deref())?;
    let settings_json = normalize_json_blob(input.settings_json.as_deref())?;

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let backend = connection.get_database_backend();
        if super::repositories::common::query_one(
            &connection,
            backend,
            "SELECT id FROM channels WHERE name = ? AND deleted_at = 0 LIMIT 1",
            "SELECT id FROM channels WHERE name = $1 AND deleted_at = 0 LIMIT 1",
            "SELECT id FROM channels WHERE name = ? AND deleted_at = 0 LIMIT 1",
            vec![name.clone().into()],
        )
        .await
        .map_err(|error| error.to_string())?
        .is_some()
        {
            return Err("channel already exists".to_owned());
        }

        let created = super::repositories::common::execute(
            &connection,
            backend,
            "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
            "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 0)",
            "INSERT INTO channels (type, base_url, name, status, credentials, supported_models, auto_sync_supported_models, default_test_model, settings, tags, ordering_weight, error_message, remark, deleted_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
            vec![
                channel_type.clone().into(),
                input.base_url.unwrap_or_default().into(),
                name.clone().into(),
                status.into(),
                credentials_json.into(),
                supported_models.into(),
                input.auto_sync_supported_models.unwrap_or(false).into(),
                input.default_test_model.unwrap_or_default().into(),
                settings_json.into(),
                tags.into(),
                input.ordering_weight.unwrap_or(100).into(),
                input.error_message.unwrap_or_default().into(),
                input.remark.unwrap_or_default().into(),
            ],
        )
        .await
        .map_err(|error| error.to_string())?;
        load_channel_record(&connection, created.last_insert_id() as i64).await?
            .ok_or_else(|| "channel not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createChannel": channel_json(&record)}}),
        }),
        Err(message) if message == "channel already exists" => graphql_field_error("createChannel", message),
        Err(message) => Err(message),
    }
}

fn update_channel_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("updateChannel");
    }
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "channel id is required".to_owned())?;
    let channel_id = parse_graphql_resource_id(id, "channel")
        .map_err(|_| "invalid channel id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateChannelInput>(
        variables,
        "input",
        "channel input is required",
    )?;
    if input.name.is_none()
        && input.base_url.is_none()
        && input.status.is_none()
        && input.supported_models.is_none()
        && input.auto_sync_supported_models.is_none()
        && input.default_test_model.is_none()
        && input.credentials_json.is_none()
        && input.settings_json.is_none()
        && input.tags.is_none()
        && input.ordering_weight.is_none()
        && input.error_message.is_none()
        && input.remark.is_none()
    {
        return graphql_field_error("updateChannel", "no fields to update");
    }

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let Some(current) = load_channel_record(&connection, channel_id).await? else {
            return Err("channel not found".to_owned());
        };
        let backend = connection.get_database_backend();
        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            if let Some(other) = super::repositories::common::query_one(
                &connection,
                backend,
                "SELECT id FROM channels WHERE name = ? AND deleted_at = 0 LIMIT 1",
                "SELECT id FROM channels WHERE name = $1 AND deleted_at = 0 LIMIT 1",
                "SELECT id FROM channels WHERE name = ? AND deleted_at = 0 LIMIT 1",
                vec![name.to_owned().into()],
            )
            .await
            .map_err(|error| error.to_string())?
            {
                let other_id = other.try_get_by_index::<i64>(0).map_err(|error| error.to_string())?;
                if other_id != channel_id {
                    return Err("channel already exists".to_owned());
                }
            }
        }

        let next_name = input
            .name
            .as_ref()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.name);
        let next_base_url = input.base_url.unwrap_or(current.base_url);
        let next_status = input
            .status
            .as_deref()
            .map(|value| normalize_enable_status(Some(value), "channel"))
            .transpose()?
            .map(str::to_owned)
            .unwrap_or(current.status);
        let next_supported_models = input
            .supported_models
            .map(|value| serde_json::to_string(&value).map_err(|error| format!("failed to serialize supported models: {error}")))
            .transpose()?
            .unwrap_or_else(|| serde_json::to_string(&current.supported_models).unwrap_or_else(|_| "[]".to_owned()));
        let next_auto_sync = input.auto_sync_supported_models.unwrap_or(false);
        let next_default_test_model = input.default_test_model.unwrap_or_default();
        let next_credentials = normalize_json_blob(input.credentials_json.as_deref())?;
        let next_settings = normalize_json_blob(input.settings_json.as_deref())?;
        let next_tags = input
            .tags
            .map(|value| serde_json::to_string(&value).map_err(|error| format!("failed to serialize tags: {error}")))
            .transpose()?
            .unwrap_or_else(|| serde_json::to_string(&Vec::<String>::new()).unwrap());
        let next_ordering_weight = input.ordering_weight.unwrap_or(current.ordering_weight);
        let next_error_message = input.error_message.unwrap_or_default();
        let next_remark = input.remark.unwrap_or_default();

        super::repositories::common::execute(
            &connection,
            backend,
            "UPDATE channels SET type = ?, base_url = ?, name = ?, status = ?, credentials = ?, supported_models = ?, auto_sync_supported_models = ?, default_test_model = ?, settings = ?, tags = ?, ordering_weight = ?, error_message = ?, remark = ?, deleted_at = 0 WHERE id = ?",
            "UPDATE channels SET type = $1, base_url = $2, name = $3, status = $4, credentials = $5, supported_models = $6, auto_sync_supported_models = $7, default_test_model = $8, settings = $9, tags = $10, ordering_weight = $11, error_message = $12, remark = $13, deleted_at = 0 WHERE id = $14",
            "UPDATE channels SET type = ?, base_url = ?, name = ?, status = ?, credentials = ?, supported_models = ?, auto_sync_supported_models = ?, default_test_model = ?, settings = ?, tags = ?, ordering_weight = ?, error_message = ?, remark = ?, deleted_at = 0 WHERE id = ?",
            vec![
                current.channel_type.into(),
                next_base_url.into(),
                next_name.into(),
                next_status.into(),
                next_credentials.into(),
                next_supported_models.into(),
                next_auto_sync.into(),
                next_default_test_model.into(),
                next_settings.into(),
                next_tags.into(),
                next_ordering_weight.into(),
                next_error_message.into(),
                next_remark.into(),
                channel_id.into(),
            ],
        )
        .await
        .map_err(|error| error.to_string())?;
        load_channel_record(&connection, channel_id).await?
            .ok_or_else(|| "channel not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateChannel": channel_json(&record)}}),
        }),
        Err(message) if message == "channel not found" || message == "channel already exists" => {
            graphql_field_error("updateChannel", message)
        }
        Err(message) => Err(message),
    }
}

fn create_model_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("createModel");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlCreateModelInput>(
        variables,
        "input",
        "model input is required",
    )?;
    let developer = input.developer.trim().to_owned();
    let model_id = input.model_id.trim().to_owned();
    let model_type = input.model_type.trim().to_owned();
    let name = input.name.trim().to_owned();
    if developer.is_empty() || model_id.is_empty() || model_type.is_empty() || name.is_empty() {
        return graphql_field_error("createModel", "developer, modelID, type, and name are required");
    }
    let status = normalize_enable_status(input.status.as_deref(), "model")?.to_owned();
    let model_card_json = normalize_json_blob(input.model_card_json.as_deref())?;
    let settings_json = normalize_json_blob(input.settings_json.as_deref())?;

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        if models::Entity::find()
            .filter(models::Column::Developer.eq(developer.clone()))
            .filter(models::Column::ModelId.eq(model_id.clone()))
            .filter(models::Column::TypeField.eq(model_type.clone()))
            .filter(models::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("model already exists".to_owned());
        }
        let created = models::Entity::insert(models::ActiveModel {
            developer: Set(developer.clone()),
            model_id: Set(model_id.clone()),
            type_field: Set(model_type.clone()),
            name: Set(name.clone()),
            icon: Set(input.icon.unwrap_or_default()),
            group_name: Set(input.group.unwrap_or_default()),
            model_card: Set(model_card_json),
            settings: Set(settings_json),
            status: Set(status),
            remark: Set(input.remark),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(&connection)
        .await
        .map_err(|error| error.to_string())?;
        load_model_record(&connection, created.last_insert_id).await?
            .ok_or_else(|| "model not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createModel": model_json(&record)}}),
        }),
        Err(message) if message == "model already exists" => graphql_field_error("createModel", message),
        Err(message) => Err(message),
    }
}

fn update_model_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("updateModel");
    }
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "model id is required".to_owned())?;
    let model_id = parse_graphql_resource_id(id, "model").map_err(|_| "invalid model id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateModelInput>(
        variables,
        "input",
        "model input is required",
    )?;
    if input.name.is_none()
        && input.icon.is_none()
        && input.group.is_none()
        && input.model_card_json.is_none()
        && input.settings_json.is_none()
        && input.status.is_none()
        && input.remark.is_none()
    {
        return graphql_field_error("updateModel", "no fields to update");
    }

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let existing = models::Entity::find_by_id(model_id)
            .filter(models::Column::DeletedAt.eq(0_i64))
            .one(&connection)
            .await
            .map_err(|error| error.to_string())?;
        let Some(existing) = existing else {
            return Err("model not found".to_owned());
        };

        let mut active: models::ActiveModel = existing.into();
        if let Some(name) = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            active.name = Set(name.to_owned());
        }
        if let Some(icon) = input.icon {
            active.icon = Set(icon);
        }
        if let Some(group) = input.group {
            active.group_name = Set(group);
        }
        if let Some(model_card_json) = input.model_card_json.as_deref() {
            active.model_card = Set(normalize_json_blob(Some(model_card_json))?);
        }
        if let Some(settings_json) = input.settings_json.as_deref() {
            active.settings = Set(normalize_json_blob(Some(settings_json))?);
        }
        if let Some(status) = input.status.as_deref() {
            active.status = Set(normalize_enable_status(Some(status), "model")?.to_owned());
        }
        if let Some(remark) = input.remark {
            active.remark = Set(Some(remark));
        }
        active.deleted_at = Set(0_i64);
        active.update(&connection).await.map_err(|error| error.to_string())?;
        load_model_record(&connection, model_id).await?
            .ok_or_else(|| "model not found".to_owned())
    });

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateModel": model_json(&record)}}),
        }),
        Err(message) if message == "model not found" => graphql_field_error("updateModel", message),
        Err(message) => Err(message),
    }
}

fn create_user_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreateUserInput>(
        variables,
        "input",
        "user input is required",
    )?;

    let password_hash = hash_password(input.password.as_str())
        .map_err(|error| format!("failed to hash password: {error}"))?;
    let status = match input.status {
        Some(UserStatus::Activated) => "activated",
        Some(UserStatus::Deactivated) => "deactivated",
        None => "activated",
    };
    let scopes_json = serde_json::to_string(&input.scopes.unwrap_or_default())
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let project_ids = parse_graphql_id_list(input.project_ids, "project")?;
    let role_ids = parse_graphql_id_list(input.role_ids, "role")?;

    let user_id = repository.create_user(
        input.email.as_str(),
        status,
        input.prefer_language.unwrap_or_else(|| "en".to_owned()).as_str(),
        password_hash.as_str(),
        input.first_name.unwrap_or_default().as_str(),
        input.last_name.unwrap_or_default().as_str(),
        input.avatar.as_deref(),
        input.is_owner.unwrap_or(false),
        scopes_json.as_str(),
        &project_ids,
        &role_ids,
    )?;

    let user = load_graphql_user(repository, user_id)?.ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"createUser": admin_user_json(&user)}}),
    })
}

fn update_user_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "user id is required".to_owned())?;
    let user_id = parse_graphql_resource_id(id, "user")
        .map_err(|_| "invalid user id".to_owned())?;
    let status_value = variables
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "status is required".to_owned())?;
    let status = match status_value {
        "activated" => "activated",
        "deactivated" => "deactivated",
        _ => return Err(format!("invalid status: {status_value}")),
    };

    if !repository.update_user_status(user_id, status)? {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateUserStatus": Value::Null},
                "errors": [{"message": "user not found"}],
            }),
        });
    }

    let user = load_graphql_user(repository, user_id)?.ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateUserStatus": admin_user_json(&user)}}),
    })
}

fn update_user_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "user id is required".to_owned())?;
    let user_id = parse_graphql_resource_id(id, "user")
        .map_err(|_| "invalid user id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateUserInput>(
        variables,
        "input",
        "user input is required",
    )?;

    if input.first_name.is_none()
        && input.last_name.is_none()
        && input.prefer_language.is_none()
        && input.avatar.is_none()
        && input.scopes.is_none()
        && input.role_ids.is_none()
    {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateUser": Value::Null},
                "errors": [{"message": "no fields to update"}],
            }),
        });
    }

    let scopes_json = input
        .scopes
        .as_ref()
        .map(|scopes| serde_json::to_string(scopes))
        .transpose()
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let role_ids = input
        .role_ids
        .map(Some)
        .map(|ids| parse_graphql_id_list(ids, "role"))
        .transpose()?;

    if !repository.update_user(
        user_id,
        input.first_name.as_deref(),
        input.last_name.as_deref(),
        input.prefer_language.as_deref(),
        input.avatar.as_deref(),
        scopes_json.as_deref(),
        role_ids.as_deref(),
    )? {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateUser": Value::Null},
                "errors": [{"message": "user not found"}],
            }),
        });
    }

    let user = load_graphql_user(repository, user_id)?.ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateUser": admin_user_json(&user)}}),
    })
}

fn update_me_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateMeInput>(
        variables,
        "input",
        "user input is required",
    )?;

    if input.first_name.is_none()
        && input.last_name.is_none()
        && input.prefer_language.is_none()
        && input.avatar.is_none()
    {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateMe": Value::Null},
                "errors": [{"message": "no fields to update"}],
            }),
        });
    }

    if !repository.update_user_profile(
        user.id,
        input.first_name.as_deref(),
        input.last_name.as_deref(),
        input.prefer_language.as_deref(),
        input.avatar.as_deref(),
    )? {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateMe": Value::Null},
                "errors": [{"message": "user not found"}],
            }),
        });
    }

    let profile = load_graphql_user_profile(repository, user.id)?
        .ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateMe": admin_user_profile_json(&profile)}}),
    })
}

fn query_prompt_protection_rules_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let rules = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        list_prompt_protection_rules_seaorm(&connection)
            .await?
            .into_iter()
            .map(prompt_protection_rule_from_record)
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_prompt_protection_openai_error)
    })?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "promptProtectionRules": default_prompt_protection_connection_json(
                    rules.iter().map(prompt_protection_rule_json).collect(),
                )
            }
        }),
    })
}

fn create_prompt_protection_rule_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreatePromptProtectionRuleInput>(
        variables,
        "input",
        "prompt protection rule input is required",
    )?;
    let settings = stored_prompt_protection_settings_from_input(input.settings);
    if let Err(message) = validate_prompt_protection_rule(
        input.name.as_str(),
        input.pattern.as_str(),
        &settings,
        "prompt protection rule",
    ) {
        return graphql_field_error("createPromptProtectionRule", message);
    }
    let name = input.name.trim().to_owned();
    let description = input.description.unwrap_or_default();
    let pattern = input.pattern.trim().to_owned();
    let settings_json = serde_json::to_string(&settings)
        .map_err(|error| format!("failed to serialize prompt protection settings: {error}"))?;

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        if prompt_protection_rule_name_exists_seaorm(&connection, name.as_str(), None).await? {
            return Err("prompt protection rule already exists".to_owned());
        }
        let id = create_prompt_protection_rule_seaorm(
            &connection,
            name.as_str(),
            description.as_str(),
            pattern.as_str(),
            "disabled",
            settings_json.as_str(),
        )
        .await?;
        load_prompt_protection_rule_seaorm(&connection, id)
            .await?
            .ok_or_else(|| "prompt protection rule not found".to_owned())
            .and_then(|record| prompt_protection_rule_from_record(record).map_err(map_prompt_protection_openai_error))
    });

    match result {
        Ok(rule) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createPromptProtectionRule": prompt_protection_rule_json(&rule)}}),
        }),
        Err(message)
            if message == "prompt protection rule already exists"
                || message == "prompt protection rule not found"
                || message.starts_with("invalid ")
                || message.contains("requires replacement")
                || message.contains("must target at least one scope") =>
        {
            graphql_field_error("createPromptProtectionRule", message)
        }
        Err(message) => Err(message),
    }
}

fn update_prompt_protection_rule_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt protection rule id is required".to_owned())?;
    let rule_id = parse_graphql_resource_id(id, "promptProtectionRule")
        .or_else(|_| parse_graphql_resource_id(id, "prompt_protection_rule"))
        .map_err(|_| "invalid prompt protection rule id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdatePromptProtectionRuleInput>(
        variables,
        "input",
        "prompt protection rule input is required",
    )?;
    if input.name.is_none()
        && input.description.is_none()
        && input.pattern.is_none()
        && input.status.is_none()
        && input.settings.is_none()
    {
        return graphql_field_error("updatePromptProtectionRule", "no fields to update");
    }

    let result = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        let existing = load_prompt_protection_rule_seaorm(&connection, rule_id)
            .await?
            .ok_or_else(|| "prompt protection rule not found".to_owned())?;
        let existing_rule = prompt_protection_rule_from_record(existing).map_err(map_prompt_protection_openai_error)?;
        let next_name = input.name.clone().unwrap_or(existing_rule.name).trim().to_owned();
        let next_description = input.description.clone().unwrap_or(existing_rule.description);
        let next_pattern = input.pattern.clone().unwrap_or(existing_rule.pattern).trim().to_owned();
        let next_status = normalize_prompt_protection_status(input.status.map(prompt_protection_rule_status_name))?
            .to_owned();
        let next_settings = input
            .settings
            .map(stored_prompt_protection_settings_from_input)
            .unwrap_or(existing_rule.settings);
        validate_prompt_protection_rule(
            next_name.as_str(),
            next_pattern.as_str(),
            &next_settings,
            "prompt protection rule",
        )?;
        if prompt_protection_rule_name_exists_seaorm(&connection, next_name.as_str(), Some(rule_id)).await? {
            return Err("prompt protection rule already exists".to_owned());
        }
        let settings_json = serde_json::to_string(&next_settings)
            .map_err(|error| format!("failed to serialize prompt protection settings: {error}"))?;
        update_prompt_protection_rule_seaorm(
            &connection,
            rule_id,
            Some(next_name.as_str()),
            Some(next_description.as_str()),
            Some(next_pattern.as_str()),
            Some(next_status.as_str()),
            Some(settings_json.as_str()),
        )
        .await?;
        load_prompt_protection_rule_seaorm(&connection, rule_id)
            .await?
            .ok_or_else(|| "prompt protection rule not found".to_owned())
            .and_then(|record| prompt_protection_rule_from_record(record).map_err(map_prompt_protection_openai_error))
    });

    match result {
        Ok(rule) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updatePromptProtectionRule": prompt_protection_rule_json(&rule)}}),
        }),
        Err(message)
            if message == "prompt protection rule not found"
                || message == "prompt protection rule already exists"
                || message.starts_with("invalid ")
                || message.contains("requires replacement")
                || message.contains("must target at least one scope") =>
        {
            graphql_field_error("updatePromptProtectionRule", message)
        }
        Err(message) => Err(message),
    }
}

fn update_prompt_protection_rule_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt protection rule id is required".to_owned())?;
    let rule_id = parse_graphql_resource_id(id, "promptProtectionRule")
        .or_else(|_| parse_graphql_resource_id(id, "prompt_protection_rule"))
        .map_err(|_| "invalid prompt protection rule id".to_owned())?;
    let status = variables
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "status is required".to_owned())?;
    let status = normalize_prompt_protection_status(Some(status))?.to_owned();
    let updated = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        set_prompt_protection_rule_status_seaorm(&connection, rule_id, status.as_str()).await
    })?;
    if !updated {
        return graphql_field_error("updatePromptProtectionRuleStatus", "prompt protection rule not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updatePromptProtectionRuleStatus": true}}),
    })
}

fn delete_prompt_protection_rule_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt protection rule id is required".to_owned())?;
    let rule_id = parse_graphql_resource_id(id, "promptProtectionRule")
        .or_else(|_| parse_graphql_resource_id(id, "prompt_protection_rule"))
        .map_err(|_| "invalid prompt protection rule id".to_owned())?;
    let deleted = repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        soft_delete_prompt_protection_rule_seaorm(&connection, rule_id).await
    })?;
    if !deleted {
        return graphql_field_error("deletePromptProtectionRule", "prompt protection rule not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"deletePromptProtectionRule": true}}),
    })
}

fn bulk_delete_prompt_protection_rules_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let ids = parse_graphql_prompt_protection_rule_ids(&variables)?;
    repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        bulk_soft_delete_prompt_protection_rules_seaorm(&connection, &ids)
            .await
            .map(|_| ())
    })?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"bulkDeletePromptProtectionRules": true}}),
    })
}

fn bulk_set_prompt_protection_rules_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    status: &str,
    field: &str,
) -> Result<GraphqlExecutionResult, String> {
    let ids = parse_graphql_prompt_protection_rule_ids(&variables)?;
    let status = status.to_owned();
    repository.db().run_sync(move |factory| async move {
        let connection = factory.connect_migrated().await.map_err(|error| error.to_string())?;
        bulk_set_prompt_protection_rule_status_seaorm(&connection, &ids, status.as_str())
            .await
            .map(|_| ())
    })?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {field: true}}),
    })
}

fn load_storage_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredStoragePolicy, String> {
    deserialize_setting_or_default(repository.query_storage_policy()?, default_storage_policy)
}

fn load_auto_backup_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredAutoBackupSettings, String> {
    deserialize_setting_or_default(repository.query_auto_backup_settings()?, default_auto_backup_settings)
}

fn load_system_channel_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredSystemChannelSettings, String> {
    deserialize_setting_or_default(
        repository.query_system_channel_settings()?,
        default_system_channel_settings,
    )
}

fn load_default_data_storage_id_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Option<i64>, String> {
    let Some(record) = repository.query_default_data_storage()? else {
        return Ok(None);
    };

    let Ok(id) = record.value.trim().parse::<i64>() else {
        return Ok(None);
    };
    if id <= 0 {
        return Ok(None);
    }

    let Some(storage) = repository.query_data_storage_status(id)? else {
        return Ok(None);
    };
    if !storage.status.eq_ignore_ascii_case("active") {
        return Ok(None);
    }

    Ok(Some(id))
}

fn load_graphql_user_profile(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<Option<AdminGraphqlUserInfo>, String> {
    let Some(profile) = repository.query_user_profile(user_id)? else {
        return Ok(None);
    };
    let roles = repository
        .query_user_roles(user_id)?
        .into_iter()
        .map(role_record_to_graphql_role)
        .collect::<Result<Vec<_>, _>>()?;
    let projects = repository
        .query_user_projects(user_id)?
        .into_iter()
        .map(|membership| {
            let roles = repository
                .query_project_roles(user_id, membership.project_id)?
                .into_iter()
                .map(role_record_to_graphql_role)
                .collect::<Result<Vec<_>, _>>()?;
            let scopes = parse_scope_json(membership.scopes.as_str());
            Ok::<AdminGraphqlUserProjectInfo, String>(AdminGraphqlUserProjectInfo {
                project_id: graphql_gid("project", membership.project_id),
                is_owner: membership.is_owner,
                scopes,
                roles,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(AdminGraphqlUserInfo {
        id: graphql_gid("user", profile.id),
        email: profile.email,
        first_name: profile.first_name,
        last_name: profile.last_name,
        is_owner: profile.is_owner,
        prefer_language: profile.prefer_language,
        avatar: profile.avatar.filter(|value| !value.is_empty()),
        scopes: parse_scope_json(profile.scopes.as_str()),
        roles,
        projects,
    }))
}

fn load_graphql_user(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<Option<AdminGraphqlUser>, String> {
    let Some(user) = repository.query_user(user_id)? else {
        return Ok(None);
    };
    let roles = repository
        .query_user_roles(user_id)?
        .into_iter()
        .map(role_record_to_graphql_role)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(AdminGraphqlUser {
        id: graphql_gid("user", user.id),
        created_at: user.created_at,
        updated_at: user.updated_at,
        email: user.email,
        status: user.status,
        first_name: user.first_name,
        last_name: user.last_name,
        is_owner: user.is_owner,
        prefer_language: user.prefer_language,
        scopes: parse_scope_json(user.scopes.as_str()),
        roles: AdminGraphqlRoleConnection {
            edges: roles
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect(),
            page_info: empty_page_info(),
        },
    }))
}

fn deserialize_setting_or_default<T, Record, F>(
    record: Option<Record>,
    default_factory: F,
) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
    Record: IntoGraphqlSettingValue,
    F: FnOnce() -> T,
{
    let Some(record) = record else {
        return Ok(default_factory());
    };
    serde_json::from_str(record.setting_value())
        .map_err(|error| format!("failed to decode stored admin setting: {error}"))
}

trait IntoGraphqlSettingValue {
    fn setting_value(&self) -> &str;
}

impl IntoGraphqlSettingValue for GraphqlStoragePolicyRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlAutoBackupSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlDefaultDataStorageRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlSystemChannelSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

fn parse_graphql_variable_input<T>(
    variables: Value,
    key: &str,
    missing_message: &str,
) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let input = variables
        .get(key)
        .cloned()
        .ok_or_else(|| missing_message.to_owned())?;
    serde_json::from_value(input).map_err(|error| format!("invalid {key}: {error}"))
}

fn storage_policy_json(policy: &StoredStoragePolicy) -> Value {
    json!({
        "storeChunks": policy.store_chunks,
        "storeRequestBody": policy.store_request_body,
        "storeResponseBody": policy.store_response_body,
        "cleanupOptions": policy.cleanup_options.iter().map(|option| {
            json!({
                "resourceType": option.resource_type,
                "enabled": option.enabled,
                "cleanupDays": option.cleanup_days,
            })
        }).collect::<Vec<_>>(),
    })
}

fn auto_backup_settings_json(settings: &StoredAutoBackupSettings) -> Value {
    json!({
        "enabled": settings.enabled,
        "frequency": settings.frequency,
        "dataStorageID": settings.data_storage_id,
        "includeChannels": settings.include_channels,
        "includeModels": settings.include_models,
        "includeAPIKeys": settings.include_api_keys,
        "includeModelPrices": settings.include_model_prices,
        "retentionDays": settings.retention_days,
        "lastBackupAt": settings.last_backup_at.map(format_unix_timestamp),
        "lastBackupError": if settings.last_backup_error.trim().is_empty() {
            Value::Null
        } else {
            Value::String(settings.last_backup_error.clone())
        },
    })
}

fn system_channel_settings_json(settings: &StoredSystemChannelSettings) -> Value {
    json!({
        "probe": {
            "enabled": settings.probe.enabled,
            "frequency": probe_frequency_graphql_name(settings.probe.frequency),
        },
        "autoSync": {
            "frequency": auto_sync_frequency_graphql_name(settings.auto_sync.frequency),
        },
        "queryAllChannelModels": settings.query_all_channel_models,
    })
}

fn parse_probe_frequency_graphql_value(value: &str) -> Result<ProbeFrequencySetting, String> {
    match value {
        "ONE_MINUTE" => Ok(ProbeFrequencySetting::OneMinute),
        "FIVE_MINUTES" => Ok(ProbeFrequencySetting::FiveMinutes),
        "THIRTY_MINUTES" => Ok(ProbeFrequencySetting::ThirtyMinutes),
        "ONE_HOUR" => Ok(ProbeFrequencySetting::OneHour),
        other => Err(format!("invalid probe.frequency: {other}")),
    }
}

fn parse_auto_sync_frequency_graphql_value(value: &str) -> Result<AutoSyncFrequencySetting, String> {
    match value {
        "ONE_HOUR" => Ok(AutoSyncFrequencySetting::OneHour),
        "SIX_HOURS" => Ok(AutoSyncFrequencySetting::SixHours),
        "ONE_DAY" => Ok(AutoSyncFrequencySetting::OneDay),
        other => Err(format!("invalid autoSync.frequency: {other}")),
    }
}

fn probe_frequency_graphql_name(value: ProbeFrequencySetting) -> &'static str {
    match value {
        ProbeFrequencySetting::OneMinute => "ONE_MINUTE",
        ProbeFrequencySetting::FiveMinutes => "FIVE_MINUTES",
        ProbeFrequencySetting::ThirtyMinutes => "THIRTY_MINUTES",
        ProbeFrequencySetting::OneHour => "ONE_HOUR",
    }
}

fn auto_sync_frequency_graphql_name(value: AutoSyncFrequencySetting) -> &'static str {
    match value {
        AutoSyncFrequencySetting::OneHour => "ONE_HOUR",
        AutoSyncFrequencySetting::SixHours => "SIX_HOURS",
        AutoSyncFrequencySetting::OneDay => "ONE_DAY",
    }
}

fn proxy_preset_json(preset: &StoredProxyPreset) -> Value {
    json!({
        "name": if preset.name.trim().is_empty() { Value::Null } else { Value::String(preset.name.clone()) },
        "url": preset.url,
        "username": if preset.username.trim().is_empty() { Value::Null } else { Value::String(preset.username.clone()) },
        "password": if preset.password.trim().is_empty() { Value::Null } else { Value::String(preset.password.clone()) },
    })
}

fn empty_page_info() -> AdminGraphqlPageInfo {
    AdminGraphqlPageInfo {
        has_next_page: false,
        has_previous_page: false,
        start_cursor: None,
        end_cursor: None,
    }
}

fn parse_scope_json(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn role_record_to_graphql_role(record: GraphqlRoleSummaryRecord) -> Result<AdminGraphqlRoleInfo, String> {
    Ok(AdminGraphqlRoleInfo {
        id: graphql_gid("role", record.id),
        name: record.name,
        scopes: parse_scope_json(record.scopes.as_str()),
    })
}

fn admin_user_json(user: &AdminGraphqlUser) -> Value {
    json!({
        "id": user.id,
        "createdAt": user.created_at,
        "updatedAt": user.updated_at,
        "email": user.email,
        "status": user.status,
        "firstName": user.first_name,
        "lastName": user.last_name,
        "isOwner": user.is_owner,
        "preferLanguage": user.prefer_language,
        "scopes": user.scopes,
        "roles": {
            "edges": user.roles.edges.iter().map(|edge| {
                json!({
                    "cursor": edge.cursor,
                    "node": edge.node.as_ref().map(|node| {
                        json!({
                            "id": node.id,
                            "name": node.name,
                            "scopes": node.scopes,
                        })
                    })
                })
            }).collect::<Vec<_>>(),
            "pageInfo": {
                "hasNextPage": user.roles.page_info.has_next_page,
                "hasPreviousPage": user.roles.page_info.has_previous_page,
                "startCursor": user.roles.page_info.start_cursor,
                "endCursor": user.roles.page_info.end_cursor,
            }
        }
    })
}

fn admin_user_profile_json(user: &AdminGraphqlUserInfo) -> Value {
    json!({
        "id": user.id,
        "email": user.email,
        "firstName": user.first_name,
        "lastName": user.last_name,
        "isOwner": user.is_owner,
        "preferLanguage": user.prefer_language,
        "avatar": user.avatar,
        "scopes": user.scopes,
        "roles": user.roles.iter().map(|role| {
            json!({
                "id": role.id,
                "name": role.name,
                "scopes": role.scopes,
            })
        }).collect::<Vec<_>>(),
        "projects": user.projects.iter().map(|project| {
            json!({
                "projectID": project.project_id,
                "isOwner": project.is_owner,
                "scopes": project.scopes,
                "roles": project.roles.iter().map(|role| {
                    json!({
                        "id": role.id,
                        "name": role.name,
                        "scopes": role.scopes,
                    })
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>(),
    })
}

fn parse_graphql_id_list(ids: Option<Vec<String>>, expected_type: &str) -> Result<Vec<i64>, String> {
    ids.unwrap_or_default()
        .into_iter()
        .map(|value| parse_graphql_resource_id(value.as_str(), expected_type))
        .collect()
}

fn generate_llm_api_key() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom(&mut bytes).map_err(|error| error.to_string())?;
    Ok(format!("ah-{}", hex_encode(bytes)))
}

async fn create_llm_api_key_seaorm(
    repository: &SeaOrmOpenApiGraphqlMutationRepository,
    owner_api_key: &AuthApiKeyContext,
    trimmed_name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
    if trimmed_name.is_empty() {
        return Err(CreateLlmApiKeyError::InvalidName);
    }

    require_service_api_key_write_access(owner_api_key)
        .map_err(|_| CreateLlmApiKeyError::PermissionDenied)?;

    let owner_record = repository
        .query_owner_api_key(owner_api_key.key.as_str())
        .map_err(CreateLlmApiKeyError::Internal)?
    .ok_or_else(|| CreateLlmApiKeyError::Internal("failed to load owner api key".to_owned()))?;

    let owner_key_type = owner_record.key_type;
    let owner_user_id = owner_record.user_id;
    let owner_project_id = owner_record.project_id;

    if owner_key_type != "service_account" || owner_project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let generated_key = generate_llm_api_key().map_err(CreateLlmApiKeyError::Internal)?;
    let scopes = scope_strings(LLM_API_KEY_SCOPES);
    let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}")))?;

    repository
        .insert_llm_api_key(
            owner_user_id,
            owner_api_key,
            generated_key.as_str(),
            trimmed_name,
            scopes_json.as_str(),
        )
        .map_err(CreateLlmApiKeyError::Internal)?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
    })
}

async fn execute_openapi_graphql_seaorm_request(
    repository: SeaOrmOpenApiGraphqlMutationRepository,
    payload: GraphqlRequestPayload,
    owner_api_key: AuthApiKeyContext,
) -> Result<GraphqlExecutionResult, String> {
    let query = payload.query.trim();

    if query.contains("serviceAccountProject") {
        let project = GraphqlProject::from(owner_api_key.project);
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({
                "data": {
                    "serviceAccountProject": {
                        "id": project.id,
                        "name": project.name,
                        "status": project.status,
                    }
                }
            }),
        });
    }

    if query.contains("createLLMAPIKey") {
        let name = extract_create_llm_api_key_name(&payload.query, &payload.variables)
            .ok_or_else(|| "api key name is required".to_owned())?;
        return create_llm_api_key_seaorm(&repository, &owner_api_key, &name)
            .await
            .map(|api_key| GraphqlExecutionResult {
                status: 200,
                body: serde_json::json!({
                    "data": {
                        "createLLMAPIKey": {
                            "key": api_key.key,
                            "name": api_key.name,
                            "scopes": api_key.scopes,
                        }
                    }
                }),
            })
            .or_else(|error| {
                let message = match error {
                    CreateLlmApiKeyError::InvalidName => "api key name is required".to_owned(),
                    CreateLlmApiKeyError::PermissionDenied => "permission denied".to_owned(),
                    CreateLlmApiKeyError::Internal(message) => message,
                };
                Ok(GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": {"createLLMAPIKey": Value::Null},
                        "errors": [{"message": message}],
                    }),
                })
            });
    }

    if let Some(field) = first_graphql_field_name(query) {
        return graphql_not_implemented_for_route("/openapi/v1/graphql", field.as_str());
    }

    Err("unsupported openapi graphql query".to_owned())
}

pub(crate) fn extract_create_llm_api_key_name(query: &str, variables: &Value) -> Option<String> {
    if let Some(value) = variables
        .get("name")
        .or_else(|| variables.get("input").and_then(|input| input.get("name")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_owned());
    }

    let marker = "createLLMAPIKey(name:";
    let start = query.find(marker)? + marker.len();
    let remainder = query.get(start..)?.trim_start();
    let first_quote = remainder.find('"')? + 1;
    let after_first = remainder.get(first_quote..)?;
    let end_quote = after_first.find('"')?;
    let name = after_first[..end_quote].trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn first_graphql_field_name(query: &str) -> Option<String> {
    let trimmed = query.trim();
    let body = if let Some(start) = trimmed.find('{') {
        trimmed.get(start + 1..)?
    } else {
        trimmed
    };

    let token = body
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .find(|token| !token.is_empty())?;

    Some(token.to_owned())
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Project", rename_fields = "camelCase")]
pub(crate) struct GraphqlProject {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
}

impl From<ProjectContext> for GraphqlProject {
    fn from(value: ProjectContext) -> Self {
        Self {
            id: graphql_gid("project", value.id),
            name: value.name,
            status: value.status,
        }
    }
}

impl From<StoredChannelSummary> for AdminGraphqlChannel {
    fn from(value: StoredChannelSummary) -> Self {
        Self {
            id: graphql_gid("channel", value.id),
            name: value.name,
            channel_type: value.channel_type,
            base_url: value.base_url,
            status: value.status,
            supported_models: value.supported_models,
            ordering_weight: i64_to_i32(value.ordering_weight),
            provider_quota_status: None,
            circuit_breaker_status: None,
        }
    }
}

impl From<StoredModelRecord> for AdminGraphqlModel {
    fn from(value: StoredModelRecord) -> Self {
        let parsed = parse_model_card(value.model_card_json.as_str());
        Self {
            id: graphql_gid("model", value.id),
            developer: value.developer,
            model_id: value.model_id,
            model_type: value.model_type,
            name: value.name,
            icon: value.icon,
            remark: value.remark,
            context_length: parsed.context_length.map(i64_to_i32),
            max_output_tokens: parsed.max_output_tokens.map(i64_to_i32),
        }
    }
}

impl From<StoredRequestSummary> for AdminGraphqlRequestSummaryObject {
    fn from(value: StoredRequestSummary) -> Self {
        Self {
            id: graphql_gid("request", value.id),
            project_id: graphql_gid("project", value.project_id),
            trace_id: value.trace_id.map(|id| graphql_gid("trace", id)),
            channel_id: value.channel_id.map(|id| graphql_gid("channel", id)),
            model_id: value.model_id,
            format: value.format,
            status: value.status,
            source: value.source,
            external_id: value.external_id,
        }
    }
}

impl From<TraceContext> for AdminGraphqlTrace {
    fn from(value: TraceContext) -> Self {
        Self {
            id: graphql_gid("trace", value.id),
            trace_id: value.trace_id,
            project_id: graphql_gid("project", value.project_id),
            thread_id: value.thread_id.map(|id| graphql_gid("thread", id)),
        }
    }
}

impl From<StoredStoragePolicy> for AdminGraphqlStoragePolicy {
    fn from(value: StoredStoragePolicy) -> Self {
        Self {
            store_chunks: value.store_chunks,
            store_request_body: value.store_request_body,
            store_response_body: value.store_response_body,
            cleanup_options: value
                .cleanup_options
                .into_iter()
                .map(|option| AdminGraphqlCleanupOption {
                    resource_type: option.resource_type,
                    enabled: option.enabled,
                    cleanup_days: option.cleanup_days,
                })
                .collect(),
        }
    }
}

impl From<StoredAutoBackupSettings> for AdminGraphqlAutoBackupSettings {
    fn from(value: StoredAutoBackupSettings) -> Self {
        Self {
            enabled: value.enabled,
            frequency: value.frequency,
            data_storage_id: i64_to_i32(value.data_storage_id),
            include_channels: value.include_channels,
            include_models: value.include_models,
            include_api_keys: value.include_api_keys,
            include_model_prices: value.include_model_prices,
            retention_days: value.retention_days,
            last_backup_at: value.last_backup_at.map(format_unix_timestamp),
            last_backup_error: if value.last_backup_error.trim().is_empty() {
                None
            } else {
                Some(value.last_backup_error)
            },
        }
    }
}

impl From<StoredSystemChannelSettings> for AdminGraphqlSystemChannelSettings {
    fn from(value: StoredSystemChannelSettings) -> Self {
        Self {
            probe: AdminGraphqlChannelProbeSetting {
                enabled: value.probe.enabled,
                frequency: value.probe.frequency,
            },
            auto_sync: AdminGraphqlChannelModelAutoSyncSetting {
                frequency: value.auto_sync.frequency,
            },
            query_all_channel_models: value.query_all_channel_models,
        }
    }
}

impl From<StoredChannelProbeData> for AdminGraphqlChannelProbeData {
    fn from(value: StoredChannelProbeData) -> Self {
        Self {
            channel_id: graphql_gid("channel", value.channel_id),
            points: value
                .points
                .into_iter()
                .map(|point| AdminGraphqlChannelProbePoint {
                    timestamp: point.timestamp,
                    total_request_count: point.total_request_count,
                    success_request_count: point.success_request_count,
                    avg_tokens_per_second: point.avg_tokens_per_second,
                    avg_time_to_first_token_ms: point.avg_time_to_first_token_ms,
                })
                .collect(),
        }
    }
}

impl From<StoredProviderQuotaStatus> for AdminGraphqlProviderQuotaStatus {
    fn from(value: StoredProviderQuotaStatus) -> Self {
        let quota_data = serde_json::from_str::<Value>(value.quota_data_json.as_str()).unwrap_or(Value::Null);
        let message = quota_data
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| quota_data.get("error").and_then(Value::as_str))
            .map(ToOwned::to_owned);
        Self {
            id: graphql_gid("provider_quota_status", value.id),
            channel_id: graphql_gid("channel", value.channel_id),
            provider_type: value.provider_type,
            status: value.status,
            ready: value.ready,
            next_reset_at: value.next_reset_at.map(format_unix_timestamp),
            next_check_at: format_unix_timestamp(value.next_check_at),
            message,
        }
    }
}

impl From<StoredCircuitBreakerStatus> for AdminGraphqlCircuitBreakerStatus {
    fn from(value: StoredCircuitBreakerStatus) -> Self {
        Self {
            channel_id: graphql_gid("channel", value.channel_id),
            model_id: value.model_id,
            state: value.state,
            consecutive_failures: value.consecutive_failures,
            next_probe_at_seconds: value.next_probe_at_seconds,
        }
    }
}

pub(crate) fn stored_circuit_breaker_status(
    channel_id: i64,
    circuit_breaker: &SharedCircuitBreaker,
) -> Option<StoredCircuitBreakerStatus> {
    let snapshot = circuit_breaker.channel_status(channel_id).active?;
    Some(StoredCircuitBreakerStatus {
        channel_id: snapshot.channel_id,
        model_id: snapshot.model_id,
        state: snapshot.state.as_str().to_owned(),
        consecutive_failures: i32::try_from(snapshot.consecutive_failures).unwrap_or(i32::MAX),
        next_probe_at_seconds: snapshot.next_probe_in_seconds,
    })
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateMeInput")]
pub(crate) struct AdminGraphqlUpdateMeInput {
    pub(crate) first_name: Option<String>,
    pub(crate) last_name: Option<String>,
    pub(crate) prefer_language: Option<String>,
    pub(crate) avatar: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PageInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPageInfo {
    pub(crate) has_next_page: bool,
    pub(crate) has_previous_page: bool,
    pub(crate) start_cursor: Option<String>,
    pub(crate) end_cursor: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RoleInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRoleInfo {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RoleEdge", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRoleEdge {
    pub(crate) cursor: Option<String>,
    pub(crate) node: Option<AdminGraphqlRoleInfo>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RoleConnection", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRoleConnection {
    pub(crate) edges: Vec<AdminGraphqlRoleEdge>,
    pub(crate) page_info: AdminGraphqlPageInfo,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserProjectInfo")]
pub(crate) struct AdminGraphqlUserProjectInfo {
    #[graphql(name = "projectID")]
    pub(crate) project_id: String,
    #[graphql(name = "isOwner")]
    pub(crate) is_owner: bool,
    pub(crate) scopes: Vec<String>,
    pub(crate) roles: Vec<AdminGraphqlRoleInfo>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserInfo {
    pub(crate) id: String,
    pub(crate) email: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) is_owner: bool,
    pub(crate) prefer_language: String,
    pub(crate) avatar: Option<String>,
    pub(crate) scopes: Vec<String>,
    pub(crate) roles: Vec<AdminGraphqlRoleInfo>,
    pub(crate) projects: Vec<AdminGraphqlUserProjectInfo>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "User", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUser {
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) email: String,
    pub(crate) status: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) is_owner: bool,
    pub(crate) prefer_language: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) roles: AdminGraphqlRoleConnection,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserEdge", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserEdge {
    pub(crate) cursor: Option<String>,
    pub(crate) node: Option<AdminGraphqlUser>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserConnection", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserConnection {
    pub(crate) edges: Vec<AdminGraphqlUserEdge>,
    pub(crate) page_info: AdminGraphqlPageInfo,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ScopeInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlScopeInfo {
    pub(crate) scope: String,
    pub(crate) description: String,
    pub(crate) levels: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum UserStatus {
    Activated,
    Deactivated,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "QueryModelsInput")]
pub(crate) struct AdminGraphqlQueryModelsInput {
    pub(crate) first: Option<i32>,
    pub(crate) last: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateUserInput")]
pub(crate) struct AdminGraphqlCreateUserInput {
    pub(crate) email: String,
    pub(crate) status: Option<UserStatus>,
    pub(crate) prefer_language: Option<String>,
    pub(crate) password: String,
    pub(crate) first_name: Option<String>,
    pub(crate) last_name: Option<String>,
    pub(crate) avatar: Option<String>,
    pub(crate) is_owner: Option<bool>,
    pub(crate) scopes: Option<Vec<String>>,
    #[serde(rename = "projectIDs")]
    #[graphql(name = "projectIDs")]
    pub(crate) project_ids: Option<Vec<String>>,
    #[serde(rename = "roleIDs")]
    #[graphql(name = "roleIDs")]
    pub(crate) role_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateUserInput")]
pub(crate) struct AdminGraphqlUpdateUserInput {
    #[graphql(name = "firstName")]
    pub(crate) first_name: Option<String>,
    #[graphql(name = "lastName")]
    pub(crate) last_name: Option<String>,
    #[graphql(name = "preferLanguage")]
    pub(crate) prefer_language: Option<String>,
    #[graphql(name = "avatar")]
    pub(crate) avatar: Option<String>,
    #[graphql(name = "scopes")]
    pub(crate) scopes: Option<Vec<String>>,
    #[serde(rename = "roleIDs")]
    #[graphql(name = "roleIDs")]
    pub(crate) role_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateProjectInput")]
pub(crate) struct AdminGraphqlCreateProjectInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateProjectInput")]
pub(crate) struct AdminGraphqlUpdateProjectInput {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateRoleInput")]
pub(crate) struct AdminGraphqlCreateRoleInput {
    pub(crate) name: String,
    pub(crate) level: Option<String>,
    #[serde(rename = "projectID")]
    #[graphql(name = "projectID")]
    pub(crate) project_id: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateRoleInput")]
pub(crate) struct AdminGraphqlUpdateRoleInput {
    pub(crate) name: Option<String>,
    pub(crate) level: Option<String>,
    #[serde(rename = "projectID")]
    #[graphql(name = "projectID")]
    pub(crate) project_id: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateAPIKeyInput")]
pub(crate) struct AdminGraphqlCreateApiKeyInput {
    #[serde(rename = "projectID")]
    #[graphql(name = "projectID")]
    pub(crate) project_id: String,
    pub(crate) key: Option<String>,
    pub(crate) name: String,
    #[serde(rename = "keyType")]
    #[graphql(name = "keyType")]
    pub(crate) key_type: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
    #[serde(rename = "profilesJSON")]
    #[graphql(name = "profilesJSON")]
    pub(crate) profiles_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateAPIKeyInput")]
pub(crate) struct AdminGraphqlUpdateApiKeyInput {
    pub(crate) name: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateChannelInput")]
pub(crate) struct AdminGraphqlCreateChannelInput {
    pub(crate) name: String,
    #[serde(rename = "channelType")]
    #[graphql(name = "channelType")]
    pub(crate) channel_type: String,
    #[serde(rename = "baseURL")]
    #[graphql(name = "baseURL")]
    pub(crate) base_url: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(rename = "credentialsJSON")]
    #[graphql(name = "credentialsJSON")]
    pub(crate) credentials_json: Option<String>,
    #[serde(rename = "supportedModels")]
    #[graphql(name = "supportedModels")]
    pub(crate) supported_models: Option<Vec<String>>,
    #[serde(rename = "autoSyncSupportedModels")]
    #[graphql(name = "autoSyncSupportedModels")]
    pub(crate) auto_sync_supported_models: Option<bool>,
    #[serde(rename = "defaultTestModel")]
    #[graphql(name = "defaultTestModel")]
    pub(crate) default_test_model: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    #[serde(rename = "orderingWeight")]
    #[graphql(name = "orderingWeight")]
    pub(crate) ordering_weight: Option<i32>,
    #[serde(rename = "errorMessage")]
    #[graphql(name = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateChannelInput")]
pub(crate) struct AdminGraphqlUpdateChannelInput {
    pub(crate) name: Option<String>,
    #[serde(rename = "baseURL")]
    #[graphql(name = "baseURL")]
    pub(crate) base_url: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(rename = "credentialsJSON")]
    #[graphql(name = "credentialsJSON")]
    pub(crate) credentials_json: Option<String>,
    #[serde(rename = "supportedModels")]
    #[graphql(name = "supportedModels")]
    pub(crate) supported_models: Option<Vec<String>>,
    #[serde(rename = "autoSyncSupportedModels")]
    #[graphql(name = "autoSyncSupportedModels")]
    pub(crate) auto_sync_supported_models: Option<bool>,
    #[serde(rename = "defaultTestModel")]
    #[graphql(name = "defaultTestModel")]
    pub(crate) default_test_model: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    #[serde(rename = "orderingWeight")]
    #[graphql(name = "orderingWeight")]
    pub(crate) ordering_weight: Option<i32>,
    #[serde(rename = "errorMessage")]
    #[graphql(name = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateModelInput")]
pub(crate) struct AdminGraphqlCreateModelInput {
    pub(crate) developer: String,
    #[serde(rename = "modelID")]
    #[graphql(name = "modelID")]
    pub(crate) model_id: String,
    #[serde(rename = "modelType")]
    #[graphql(name = "modelType")]
    pub(crate) model_type: String,
    pub(crate) name: String,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(rename = "modelCardJSON")]
    #[graphql(name = "modelCardJSON")]
    pub(crate) model_card_json: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateModelInput")]
pub(crate) struct AdminGraphqlUpdateModelInput {
    pub(crate) name: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(rename = "modelCardJSON")]
    #[graphql(name = "modelCardJSON")]
    pub(crate) model_card_json: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "BackupInput")]
pub(crate) struct AdminGraphqlBackupInput {
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
    #[serde(rename = "includeAPIKeys")]
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: Option<bool>,
    pub(crate) include_model_prices: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "RestoreInput")]
pub(crate) struct AdminGraphqlRestoreInput {
    pub(crate) payload: String,
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
    #[serde(rename = "includeAPIKeys")]
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: Option<bool>,
    pub(crate) include_model_prices: Option<bool>,
    pub(crate) overwrite_existing: Option<bool>,
}

fn stored_prompt_protection_settings_from_input(
    input: AdminGraphqlPromptProtectionSettingsInput,
) -> StoredPromptProtectionSettings {
    StoredPromptProtectionSettings {
        action: match input.action {
            AdminGraphqlPromptProtectionAction::Mask => PromptProtectionAction::Mask,
            AdminGraphqlPromptProtectionAction::Reject => PromptProtectionAction::Reject,
        },
        replacement: input.replacement,
        scopes: input
            .scopes
            .unwrap_or_default()
            .into_iter()
            .map(|scope| match scope {
                AdminGraphqlPromptProtectionScope::System => PromptProtectionScope::System,
                AdminGraphqlPromptProtectionScope::Developer => PromptProtectionScope::Developer,
                AdminGraphqlPromptProtectionScope::User => PromptProtectionScope::User,
                AdminGraphqlPromptProtectionScope::Assistant => PromptProtectionScope::Assistant,
                AdminGraphqlPromptProtectionScope::Tool => PromptProtectionScope::Tool,
            })
            .collect(),
    }
}

fn prompt_protection_rule_status_name(status: PromptProtectionRuleStatus) -> &'static str {
    match status {
        PromptProtectionRuleStatus::Enabled => "enabled",
        PromptProtectionRuleStatus::Disabled => "disabled",
        PromptProtectionRuleStatus::Archived => "archived",
    }
}

fn map_prompt_protection_openai_error(error: axonhub_http::OpenAiV1Error) -> String {
    match error {
        axonhub_http::OpenAiV1Error::Internal { message }
        | axonhub_http::OpenAiV1Error::InvalidRequest { message } => message,
        axonhub_http::OpenAiV1Error::Upstream { .. } => "unexpected upstream error".to_owned(),
    }
}

fn parse_graphql_prompt_protection_rule_ids(variables: &Value) -> Result<Vec<i64>, String> {
    let ids = variables
        .get("ids")
        .cloned()
        .ok_or_else(|| "ids are required".to_owned())
        .and_then(|value| {
            serde_json::from_value::<Vec<String>>(value)
                .map_err(|error| format!("invalid ids: {error}"))
        })?;
    parse_graphql_id_list(Some(ids.clone()), "promptProtectionRule")
        .or_else(|_| parse_graphql_id_list(Some(ids), "prompt_protection_rule"))
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ModelIdentityWithStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModelIdentityWithStatus {
    pub(crate) id: String,
    pub(crate) status: String,
}

fn project_json(project: &GraphqlProjectRecord) -> Value {
    json!({
        "id": graphql_gid("project", project.id),
        "name": project.name,
        "description": project.description,
        "status": project.status,
    })
}

fn role_json(role: &GraphqlRoleRecord) -> Value {
    json!({
        "id": graphql_gid("role", role.id),
        "name": role.name,
        "level": role.level,
        "projectID": graphql_gid("project", role.project_id.max(1)),
        "scopes": parse_scope_json(role.scopes.as_str()),
    })
}

fn api_key_json(api_key: &GraphqlApiKeyRecord) -> Value {
    json!({
        "id": graphql_gid("api_key", api_key.id),
        "projectID": graphql_gid("project", api_key.project_id),
        "key": api_key.key,
        "name": api_key.name,
        "keyType": api_key.key_type,
        "status": api_key.status,
        "scopes": parse_scope_json(api_key.scopes.as_str()),
    })
}

fn channel_json(channel: &AdminGraphqlChannel) -> Value {
    json!({
        "id": channel.id,
        "name": channel.name,
        "channelType": channel.channel_type,
        "baseURL": channel.base_url,
        "status": channel.status,
        "supportedModels": channel.supported_models,
        "orderingWeight": channel.ordering_weight,
        "providerQuotaStatus": channel.provider_quota_status.as_ref().map(provider_quota_status_json),
        "circuitBreakerStatus": channel.circuit_breaker_status.as_ref().map(circuit_breaker_status_json),
    })
}

fn provider_quota_status_json(status: &AdminGraphqlProviderQuotaStatus) -> Value {
    json!({
        "id": status.id,
        "channelID": status.channel_id,
        "providerType": status.provider_type,
        "status": status.status,
        "ready": status.ready,
        "nextResetAt": status.next_reset_at,
        "nextCheckAt": status.next_check_at,
        "message": status.message,
    })
}

fn circuit_breaker_status_json(status: &AdminGraphqlCircuitBreakerStatus) -> Value {
    json!({
        "channelID": status.channel_id,
        "modelID": status.model_id,
        "state": status.state,
        "consecutiveFailures": status.consecutive_failures,
        "nextProbeAtSeconds": status.next_probe_at_seconds,
    })
}

fn model_json(model: &AdminGraphqlModel) -> Value {
    json!({
        "id": model.id,
        "developer": model.developer,
        "modelID": model.model_id,
        "modelType": model.model_type,
        "name": model.name,
        "icon": model.icon,
        "remark": model.remark,
        "contextLength": model.context_length,
        "maxOutputTokens": model.max_output_tokens,
    })
}

async fn load_project_record(
    connection: &DatabaseConnection,
    project_id: i64,
) -> Result<Option<GraphqlProjectRecord>, String> {
    projects::Entity::find_by_id(project_id)
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())
        .map(|row| {
            row.map(|value| GraphqlProjectRecord {
                id: value.id,
                name: value.name,
                description: value.description,
                status: value.status,
            })
        })
}

async fn load_role_record(
    connection: &DatabaseConnection,
    role_id: i64,
) -> Result<Option<GraphqlRoleRecord>, String> {
    roles::Entity::find_by_id(role_id)
        .filter(roles::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())
        .map(|row| {
            row.map(|value| GraphqlRoleRecord {
                id: value.id,
                name: value.name,
                level: value.level,
                project_id: value.project_id,
                scopes: value.scopes,
            })
        })
}

async fn load_api_key_record(
    connection: &DatabaseConnection,
    api_key_id: i64,
) -> Result<Option<GraphqlApiKeyRecord>, String> {
    api_keys::Entity::find_by_id(api_key_id)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())
        .map(|row| {
            row.map(|value| GraphqlApiKeyRecord {
                id: value.id,
                project_id: value.project_id,
                key: value.key,
                name: value.name,
                key_type: value.type_field,
                status: value.status,
                scopes: value.scopes,
            })
        })
}

async fn load_channel_record(
    connection: &DatabaseConnection,
    channel_id: i64,
) -> Result<Option<AdminGraphqlChannel>, String> {
    let backend = connection.get_database_backend();
    let sqlite_sql = "SELECT id, name, type, base_url, status, supported_models, ordering_weight FROM channels WHERE id = ? AND deleted_at = 0 LIMIT 1";
    let postgres_sql = "SELECT id, name, type, base_url, status, supported_models, ordering_weight FROM channels WHERE id = $1 AND deleted_at = 0 LIMIT 1";
    let mysql_sql = sqlite_sql;
    let row = super::repositories::common::query_one(
        connection,
        backend,
        sqlite_sql,
        postgres_sql,
        mysql_sql,
        vec![channel_id.into()],
    )
    .await
    .map_err(|error| error.to_string())?;

    let Some(row) = row else {
        return Ok(None);
    };

    let id = row.try_get_by_index::<i64>(0).map_err(|error| error.to_string())?;
    let name = row.try_get_by_index::<String>(1).map_err(|error| error.to_string())?;
    let channel_type = row.try_get_by_index::<String>(2).map_err(|error| error.to_string())?;
    let base_url = row.try_get_by_index::<String>(3).map_err(|error| error.to_string())?;
    let status = row.try_get_by_index::<String>(4).map_err(|error| error.to_string())?;
    let supported_models_json = row.try_get_by_index::<String>(5).map_err(|error| error.to_string())?;
    let ordering_weight = row.try_get_by_index::<i32>(6).map_err(|error| error.to_string())?;

    Ok(Some(AdminGraphqlChannel {
        id: graphql_gid("channel", id),
        name,
        channel_type,
        base_url,
        status,
        supported_models: serde_json::from_str(&supported_models_json).unwrap_or_default(),
        ordering_weight,
        provider_quota_status: None,
        circuit_breaker_status: None,
    }))
}

async fn load_model_record(
    connection: &DatabaseConnection,
    model_id: i64,
) -> Result<Option<AdminGraphqlModel>, String> {
    models::Entity::find_by_id(model_id)
        .filter(models::Column::DeletedAt.eq(0_i64))
        .one(connection)
        .await
        .map_err(|error| error.to_string())
        .map(|row| {
            row.map(|value| {
                let parsed = parse_model_card(value.model_card.as_str());
                AdminGraphqlModel {
                    id: graphql_gid("model", value.id),
                    developer: value.developer,
                    model_id: value.model_id,
                    model_type: value.type_field,
                    name: value.name,
                    icon: value.icon,
                    remark: value.remark.unwrap_or_default(),
                    context_length: parsed.context_length.map(i64_to_i32),
                    max_output_tokens: parsed.max_output_tokens.map(i64_to_i32),
                }
            })
        })
}

fn normalize_project_status(status: Option<&str>) -> Result<&'static str, String> {
    match status.unwrap_or("active").trim().to_ascii_lowercase().as_str() {
        "active" => Ok("active"),
        "disabled" => Ok("disabled"),
        "archived" => Ok("archived"),
        _ => Err("invalid project status".to_owned()),
    }
}

fn normalize_enable_status(status: Option<&str>, resource: &str) -> Result<&'static str, String> {
    match status.unwrap_or("enabled").trim().to_ascii_lowercase().as_str() {
        "enabled" => Ok("enabled"),
        "disabled" => Ok("disabled"),
        "archived" => Ok("archived"),
        _ => Err(format!("invalid {resource} status")),
    }
}

fn normalize_role_level(level: Option<&str>) -> Result<&'static str, String> {
    match level.unwrap_or("system").trim().to_ascii_lowercase().as_str() {
        "system" => Ok("system"),
        "project" => Ok("project"),
        _ => Err("invalid role level".to_owned()),
    }
}

fn normalize_api_key_type(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("user").trim().to_ascii_lowercase().as_str() {
        "user" => Ok("user"),
        "service_account" => Ok("service_account"),
        "noauth" => Ok("noauth"),
        _ => Err("invalid api key type".to_owned()),
    }
}

fn normalize_api_key_status(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("enabled").trim().to_ascii_lowercase().as_str() {
        "enabled" => Ok("enabled"),
        "disabled" => Ok("disabled"),
        "archived" => Ok("archived"),
        _ => Err("invalid api key status".to_owned()),
    }
}

fn normalize_json_blob(value: Option<&str>) -> Result<String, String> {
    let raw = value.unwrap_or("{}").trim();
    if raw.is_empty() {
        return Ok("{}".to_owned());
    }
    let parsed = serde_json::from_str::<Value>(raw).map_err(|error| format!("invalid json payload: {error}"))?;
    serde_json::to_string(&parsed).map_err(|error| format!("failed to normalize json payload: {error}"))
}

fn parse_role_project_id(value: Option<&str>, level: &str) -> Result<i64, String> {
    if level == "system" {
        return Ok(0);
    }
    let value = value.ok_or_else(|| "projectID is required for project roles".to_owned())?;
    parse_graphql_resource_id(value, "project").map_err(|_| "invalid project id".to_owned())
}

fn authorize_role_write(
    user: &AuthUserContext,
    level: &str,
    project_id: i64,
    field: &str,
) -> Result<(), String> {
    if level == "system" {
        if authorize_user_system_scope(user, SCOPE_WRITE_ROLES).is_err() {
            graphql_permission_denied(field)?;
            return Err("permission denied".to_owned());
        }
        return Ok(());
    }

    if require_user_project_scope(user, project_id, SCOPE_WRITE_ROLES).is_err() {
        graphql_permission_denied(field)?;
        return Err("permission denied".to_owned());
    }
    Ok(())
}

fn validate_scope_list(scopes: &[String], field: &str) -> Result<(), String> {
    for scope in scopes {
        if !is_valid_scope(scope.as_str()) {
            graphql_field_error(field, format!("invalid scope: {scope}"))?;
            return Err(format!("invalid scope: {scope}"));
        }
    }
    Ok(())
}
