use std::future::Future;
use std::pin::Pin;

use async_graphql::{
    Enum, InputObject, SimpleObject,
};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, ProjectContext, TraceContext,
};
use getrandom::getrandom;
use hex::encode as hex_encode;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use super::{
    admin::{
        BackupFrequencySetting, ProbeFrequencySetting, StoredAutoBackupSettings,
        StoredChannelProbeData, StoredProviderQuotaStatus, StoredStoragePolicy,
        StoredSystemChannelSettings,
    },
    authz::{
        scope_strings, serialize_scope_slugs, user_has_system_scope, LLM_API_KEY_SCOPES,
        SCOPE_READ_CHANNELS, SCOPE_READ_SETTINGS,
    },
    openai_v1::{parse_model_card, StoredChannelSummary, StoredModelRecord, StoredRequestSummary},
    ports::{AdminGraphqlRepository, OpenApiGraphqlRepository},
    seaorm::SeaOrmConnectionFactory,
    shared::{format_unix_timestamp, graphql_gid, i64_to_i32},
};

pub struct SeaOrmAdminGraphqlService {
    db: SeaOrmConnectionFactory,
}

pub struct SeaOrmOpenApiGraphqlService {
    db: SeaOrmConnectionFactory,
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

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "CleanupOptionInput")]
pub(crate) struct AdminGraphqlCleanupOptionInput {
    pub(crate) resource_type: String,
    pub(crate) enabled: bool,
    pub(crate) cleanup_days: i32,
}

#[derive(Debug, Clone, InputObject)]
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

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateAutoBackupSettingsInput")]
pub(crate) struct AdminGraphqlUpdateAutoBackupSettingsInput {
    pub(crate) enabled: Option<bool>,
    pub(crate) frequency: Option<BackupFrequencySetting>,
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: Option<i32>,
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
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
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateChannelProbeSettingInput")]
pub(crate) struct AdminGraphqlUpdateChannelProbeSettingInput {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "UpdateSystemChannelSettingsInput")]
pub(crate) struct AdminGraphqlUpdateSystemChannelSettingsInput {
    pub(crate) probe: Option<AdminGraphqlUpdateChannelProbeSettingInput>,
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
    pub fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl SeaOrmOpenApiGraphqlService {
    pub fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl AdminGraphqlPort for SeaOrmAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let db = self.db.clone();
        Box::pin(async move {
            let payload = request;
            match db.run_sync(move |db| async move {
                execute_admin_graphql_seaorm_request(db, payload, project_id, user).await
            }) {
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
        let db = self.db.clone();
        Box::pin(async move {
            let payload = request;
            match db.run_sync(move |db| async move {
                execute_openapi_graphql_seaorm_request(db, payload, owner_api_key).await
            }) {
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

fn graphql_sql<'a>(
    backend: DatabaseBackend,
    sqlite: &'a str,
    postgres: &'a str,
    mysql: &'a str,
) -> &'a str {
    match backend {
        DatabaseBackend::Sqlite => sqlite,
        DatabaseBackend::Postgres => postgres,
        DatabaseBackend::MySql => mysql,
    }
}

async fn query_one_graphql(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<sea_orm::QueryResult>, String> {
    db.query_one(Statement::from_sql_and_values(
        backend,
        graphql_sql(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
    .map_err(|error| error.to_string())
}

pub(crate) async fn query_all_graphql(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>, String> {
    db.query_all(Statement::from_sql_and_values(
        backend,
        graphql_sql(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
    .map_err(|error| error.to_string())
}

async fn execute_admin_graphql_seaorm_request(
    db: SeaOrmConnectionFactory,
    payload: GraphqlRequestPayload,
    _project_id: Option<i64>,
    user: AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let query = payload.query.trim();

    if query.contains("allScopes") {
        if !user_has_system_scope(&user, SCOPE_READ_SETTINGS) {
            return Ok(GraphqlExecutionResult {
                status: 200,
                body: serde_json::json!({
                    "data": {"allScopes": Value::Null},
                    "errors": [{"message": "permission denied"}],
                }),
            });
        }

        let all_scopes = vec![
            admin_scope_info("read_settings", "Read system settings", &["system"]),
            admin_scope_info("write_settings", "Write system settings", &["system"]),
            admin_scope_info("read_channels", "Read channel configurations", &["system"]),
            admin_scope_info("write_channels", "Write channel configurations", &["system"]),
            admin_scope_info("read_requests", "Read request data", &["system", "project"]),
            admin_scope_info("write_requests", "Write request data", &["system", "project"]),
            admin_scope_info("read_users", "Read user data", &["system"]),
            admin_scope_info("write_users", "Write user data", &["system"]),
            admin_scope_info("read_api_keys", "Read API keys", &["system"]),
            admin_scope_info("write_api_keys", "Write API keys", &["system"]),
        ];

        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({"data": {"allScopes": all_scopes}}),
        });
    }

    if query.contains("queryModels") {
        if !user_has_system_scope(&user, SCOPE_READ_CHANNELS) {
            return Ok(GraphqlExecutionResult {
                status: 200,
                body: serde_json::json!({
                    "data": {"queryModels": Value::Null},
                    "errors": [{"message": "permission denied"}],
                }),
            });
        }

        let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
        let models = query_all_graphql(
            &connection,
            db.backend(),
            "SELECT id, status FROM models WHERE deleted_at = 0 ORDER BY id ASC",
            "SELECT id, status FROM models WHERE deleted_at = 0 ORDER BY id ASC",
            "SELECT id, status FROM models WHERE deleted_at = 0 ORDER BY id ASC",
            vec![],
        )
        .await?
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "id": graphql_gid("model", row.try_get_by_index::<i64>(0).unwrap_or_default()),
                "status": row.try_get_by_index::<String>(1).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();

        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({"data": {"queryModels": models}}),
        });
    }

    Err("unsupported graphql subset query".to_owned())
}

fn generate_llm_api_key() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom(&mut bytes).map_err(|error| error.to_string())?;
    Ok(format!("ah-{}", hex_encode(bytes)))
}

async fn create_llm_api_key_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    owner_api_key: &AuthApiKeyContext,
    trimmed_name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
    let owner_record = query_one_graphql(
        db,
        backend,
        "SELECT id, user_id, key, name, type, status, project_id, scopes FROM api_keys WHERE key = ? AND deleted_at = 0 LIMIT 1",
        "SELECT id, user_id, key, name, type, status, project_id, scopes FROM api_keys WHERE key = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT id, user_id, `key`, name, type, status, project_id, scopes FROM api_keys WHERE `key` = ? AND deleted_at = 0 LIMIT 1",
        vec![owner_api_key.key.as_str().into()],
    )
    .await
    .map_err(CreateLlmApiKeyError::Internal)?
    .ok_or_else(|| CreateLlmApiKeyError::Internal("failed to load owner api key".to_owned()))?;

    let owner_key_type = owner_record
        .try_get_by_index::<String>(4)
        .map_err(|error| CreateLlmApiKeyError::Internal(error.to_string()))?;
    let owner_user_id = owner_record
        .try_get_by_index::<i64>(1)
        .map_err(|error| CreateLlmApiKeyError::Internal(error.to_string()))?;
    let owner_project_id = owner_record
        .try_get_by_index::<i64>(6)
        .map_err(|error| CreateLlmApiKeyError::Internal(error.to_string()))?;

    if owner_key_type != "service_account" || owner_project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let generated_key = generate_llm_api_key().map_err(CreateLlmApiKeyError::Internal)?;
    let scopes = scope_strings(LLM_API_KEY_SCOPES);
    let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}")))?;

    db.execute(Statement::from_sql_and_values(
        backend,
        graphql_sql(
            backend,
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at) VALUES (?, ?, ?, ?, 'user', 'enabled', ?, '{}', 0)",
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at) VALUES ($1, $2, $3, $4, 'user', 'enabled', $5, '{}', 0)",
            "INSERT INTO api_keys (user_id, project_id, `key`, name, type, status, scopes, profiles, deleted_at) VALUES (?, ?, ?, ?, 'user', 'enabled', ?, '{}', 0)",
        ),
        vec![
            owner_user_id.into(),
            owner_api_key.project.id.into(),
            generated_key.as_str().into(),
            trimmed_name.into(),
            scopes_json.into(),
        ],
    ))
    .await
    .map_err(|error| CreateLlmApiKeyError::Internal(error.to_string()))?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
    })
}

async fn execute_openapi_graphql_seaorm_request(
    db: SeaOrmConnectionFactory,
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
        let name = extract_create_llm_api_key_name(&payload.query)
            .ok_or_else(|| "api key name is required".to_owned())?;
        let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
        return create_llm_api_key_seaorm(&connection, db.backend(), &owner_api_key, &name)
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

    Ok(GraphqlExecutionResult {
        status: 200,
        body: serde_json::json!({
            "data": null,
            "errors": [{"message": "unsupported openapi graphql pilot query"}],
        }),
    })
}

pub(crate) fn extract_create_llm_api_key_name(query: &str) -> Option<String> {
    let marker = "createLLMAPIKey(name:";
    let start = query.find(marker)? + marker.len();
    let remainder = query.get(start..)?.trim_start();
    let first_quote = remainder.find('"')? + 1;
    let after_first = remainder.get(first_quote..)?;
    let end_quote = after_first.find('"')?;
    Some(after_first[..end_quote].to_owned())
}

pub(crate) fn admin_scope_info(scope: &str, description: &str, levels: &[&str]) -> Value {
    serde_json::json!({
        "scope": scope,
        "description": description,
        "levels": levels,
    })
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

#[derive(Debug, Clone, InputObject)]
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

#[derive(Debug, Clone, InputObject)]
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
    #[graphql(name = "projectIDs")]
    pub(crate) project_ids: Option<Vec<String>>,
    #[graphql(name = "roleIDs")]
    pub(crate) role_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, InputObject)]
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
    #[graphql(name = "roleIDs")]
    pub(crate) role_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ModelIdentityWithStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModelIdentityWithStatus {
    pub(crate) id: String,
    pub(crate) status: String,
}
