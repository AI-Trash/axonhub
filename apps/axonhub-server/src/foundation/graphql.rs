use std::future::Future;
use std::pin::Pin;

use async_graphql::{Enum, InputObject, SimpleObject};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, ProjectContext, TraceContext,
};
use getrandom::getrandom;
use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use super::{
    admin::{
        default_auto_backup_settings, default_storage_policy, default_system_channel_settings,
        parse_graphql_resource_id, BackupFrequencySetting, ProbeFrequencySetting,
        StoredAutoBackupSettings, StoredChannelProbeData, StoredProviderQuotaStatus,
        StoredStoragePolicy, StoredSystemChannelSettings,
    },
    authz::{
        scope_strings, serialize_scope_slugs, user_has_system_scope, LLM_API_KEY_SCOPES,
        SCOPE_READ_CHANNELS, SCOPE_READ_SETTINGS, SCOPE_WRITE_SETTINGS,
    },
    openai_v1::{parse_model_card, StoredChannelSummary, StoredModelRecord, StoredRequestSummary},
    ports::{AdminGraphqlRepository, OpenApiGraphqlRepository},
    repositories::graphql::{
        AdminGraphqlSubsetRepository, GraphqlAutoBackupSettingsRecord,
        GraphqlDefaultDataStorageRecord, GraphqlRoleSummaryRecord,
        GraphqlStoragePolicyRecord, GraphqlSystemChannelSettingsRecord,
        OpenApiGraphqlMutationRepository,
        SeaOrmAdminGraphqlSubsetRepository, SeaOrmOpenApiGraphqlMutationRepository,
    },
    shared::{
        format_unix_timestamp, graphql_gid, i64_to_i32,
    },
    system::hash_password,
};
use serde_json::json;

pub struct SeaOrmAdminGraphqlService {
    repository: SeaOrmAdminGraphqlSubsetRepository,
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
    pub(crate) query_all_channel_models: bool,
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
    pub(crate) query_all_channel_models: Option<bool>,
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
pub(crate) enum CreateLlmApiKeyError {
    InvalidName,
    PermissionDenied,
    Internal(String),
}

impl SeaOrmAdminGraphqlService {
    pub fn new(db: super::seaorm::SeaOrmConnectionFactory) -> Self {
        Self {
            repository: SeaOrmAdminGraphqlSubsetRepository::new(db),
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
        Box::pin(async move {
            let payload = request;
            match execute_admin_graphql_seaorm_request(repository, payload, project_id, user).await {
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
    payload: GraphqlRequestPayload,
    _project_id: Option<i64>,
    user: AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let query = payload.query.trim();

    if query.contains("storagePolicy") {
        if !user_has_system_scope(&user, SCOPE_READ_SETTINGS) {
            return graphql_permission_denied("storagePolicy");
        }

        return query_storage_policy_seaorm(&repository);
    }

    if query.contains("autoBackupSettings") {
        if !user_has_system_scope(&user, SCOPE_READ_SETTINGS) {
            return graphql_permission_denied("autoBackupSettings");
        }

        return query_auto_backup_settings_seaorm(&repository);
    }

    if query.contains("defaultDataStorageID") {
        if !user_has_system_scope(&user, SCOPE_READ_SETTINGS) {
            return graphql_permission_denied("defaultDataStorageID");
        }

        return query_default_data_storage_id_seaorm(&repository);
    }

    if query.contains("systemChannelSettings") {
        if !user_has_system_scope(&user, SCOPE_READ_SETTINGS) {
            return graphql_permission_denied("systemChannelSettings");
        }

        return query_system_channel_settings_seaorm(&repository);
    }

    if query.contains("updateStoragePolicy") {
        if !user_has_system_scope(&user, SCOPE_WRITE_SETTINGS) {
            return graphql_permission_denied("updateStoragePolicy");
        }

        return update_storage_policy_seaorm(&repository, payload.variables);
    }

    if query.contains("updateAutoBackupSettings") {
        if !user.is_owner {
            return graphql_owner_denied("updateAutoBackupSettings");
        }

        return update_auto_backup_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("updateDefaultDataStorage") {
        if !user_has_system_scope(&user, SCOPE_WRITE_SETTINGS) {
            return graphql_permission_denied("updateDefaultDataStorage");
        }

        return update_default_data_storage_seaorm(&repository, payload.variables);
    }

    if query.contains("updateSystemChannelSettings") {
        if !user_has_system_scope(&user, SCOPE_WRITE_SETTINGS) {
            return graphql_permission_denied("updateSystemChannelSettings");
        }

        return update_system_channel_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("createUser") {
        if !user_has_system_scope(&user, super::authz::SCOPE_WRITE_USERS) {
            return graphql_permission_denied("createUser");
        }

        return create_user_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUserStatus") {
        if !user_has_system_scope(&user, super::authz::SCOPE_WRITE_USERS) {
            return graphql_permission_denied("updateUserStatus");
        }

        return update_user_status_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUser") {
        if !user_has_system_scope(&user, super::authz::SCOPE_WRITE_USERS) {
            return graphql_permission_denied("updateUser");
        }

        return update_user_seaorm(&repository, payload.variables);
    }

    if query.contains("updateMe") {
        return update_me_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("queryModels") {
        if !user_has_system_scope(&user, SCOPE_READ_CHANNELS) {
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
    let value = serde_json::to_string(&policy).map_err(|error| error.to_string())?;
    repository.upsert_storage_policy(value.as_str())?;
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
    let value = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    repository.upsert_auto_backup_settings(value.as_str())?;
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
    if let Some(query_all_channel_models) = variables
        .get("input")
        .and_then(|input| input.get("queryAllChannelModels"))
    {
        settings.query_all_channel_models = query_all_channel_models
            .as_bool()
            .ok_or_else(|| "invalid queryAllChannelModels: expected boolean".to_owned())?;
    }
    let value = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
    repository.upsert_system_channel_settings(value.as_str())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateSystemChannelSettings": true}}),
    })
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

fn probe_frequency_graphql_name(value: ProbeFrequencySetting) -> &'static str {
    match value {
        ProbeFrequencySetting::OneMinute => "ONE_MINUTE",
        ProbeFrequencySetting::FiveMinutes => "FIVE_MINUTES",
        ProbeFrequencySetting::ThirtyMinutes => "THIRTY_MINUTES",
        ProbeFrequencySetting::OneHour => "ONE_HOUR",
    }
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
            .get("error")
            .and_then(Value::as_str)
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

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ModelIdentityWithStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModelIdentityWithStatus {
    pub(crate) id: String,
    pub(crate) status: String,
}
