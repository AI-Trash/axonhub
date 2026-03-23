use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_graphql::{
    Context, EmptySubscription, InputObject, Object, Request as AsyncGraphqlRequest,
    Schema, SimpleObject, Variables,
};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, ProjectContext, TraceContext,
};
use rusqlite::params;
use serde_json::Value;

use super::{
    admin::{
        BackupFrequencySetting, ProbeFrequencySetting, SqliteOperationalService,
        StoredAutoBackupSettings, StoredChannelProbeData, StoredProviderQuotaStatus,
        StoredStoragePolicy, StoredSystemChannelSettings,
    },
    authz::{
        api_key_has_scope, scope_strings, serialize_scope_slugs, user_has_project_scope,
        user_has_system_scope, ScopeSlug, LLM_API_KEY_SCOPES, SCOPE_READ_CHANNELS,
        SCOPE_READ_REQUESTS, SCOPE_READ_SETTINGS, SCOPE_WRITE_API_KEYS, SCOPE_WRITE_SETTINGS,
    },
    openai_v1::{parse_model_card, StoredChannelSummary, StoredModelRecord, StoredRequestSummary},
    shared::{format_unix_timestamp, graphql_gid, i64_to_i32, SqliteFoundation},
    system::ensure_identity_tables,
};

pub struct SqliteAdminGraphqlService {
    pub(crate) schema: Arc<AdminGraphqlSchema>,
}

pub struct SqliteOpenApiGraphqlService {
    pub(crate) schema: Arc<OpenApiGraphqlSchema>,
}

pub(crate) type AdminGraphqlSchema = Schema<AdminGraphqlQueryRoot, AdminGraphqlMutationRoot, EmptySubscription>;
pub(crate) type OpenApiGraphqlSchema = Schema<OpenApiGraphqlQueryRoot, OpenApiGraphqlMutationRoot, EmptySubscription>;

#[derive(Clone)]
pub(crate) struct AdminGraphqlRequestContext {
    pub(crate) project_id: Option<i64>,
    pub(crate) user: AuthUserContext,
}

#[derive(Clone)]
pub(crate) struct OpenApiGraphqlRequestContext {
    pub(crate) owner_api_key: AuthApiKeyContext,
}

#[derive(Clone)]
pub(crate) struct AdminGraphqlQueryRoot {
    pub(crate) foundation: Arc<SqliteFoundation>,
    pub(crate) operational: Arc<SqliteOperationalService>,
}

#[derive(Clone)]
pub(crate) struct AdminGraphqlMutationRoot {
    pub(crate) operational: Arc<SqliteOperationalService>,
}

#[derive(Clone)]
pub(crate) struct OpenApiGraphqlQueryRoot;

#[derive(Clone)]
pub(crate) struct OpenApiGraphqlMutationRoot {
    pub(crate) foundation: Arc<SqliteFoundation>,
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
    pub(crate) data_storage_id: i32,
    pub(crate) include_channels: bool,
    pub(crate) include_models: bool,
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
    pub(crate) data_storage_id: Option<i32>,
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
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

impl SqliteAdminGraphqlService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        let operational = Arc::new(SqliteOperationalService::new(foundation.clone()));
        let schema = Schema::build(
            AdminGraphqlQueryRoot {
                foundation,
                operational: operational.clone(),
            },
            AdminGraphqlMutationRoot { operational },
            EmptySubscription,
        )
        .finish();

        Self {
            schema: Arc::new(schema),
        }
    }
}

impl SqliteOpenApiGraphqlService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        let schema = Schema::build(
            OpenApiGraphqlQueryRoot,
            OpenApiGraphqlMutationRoot { foundation },
            EmptySubscription,
        )
        .finish();

        Self {
            schema: Arc::new(schema),
        }
    }
}

impl AdminGraphqlPort for SqliteAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let schema = Arc::clone(&self.schema);
        Box::pin(async move {
            execute_graphql_schema(schema, request, AdminGraphqlRequestContext { project_id, user }).await
        })
    }
}

impl OpenApiGraphqlPort for SqliteOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let schema = Arc::clone(&self.schema);
        Box::pin(async move {
            execute_graphql_schema(schema, request, OpenApiGraphqlRequestContext { owner_api_key }).await
        })
    }
}

async fn execute_graphql_schema<Query, Mutation, Data>(
    schema: Arc<Schema<Query, Mutation, EmptySubscription>>,
    payload: GraphqlRequestPayload,
    context_data: Data,
) -> GraphqlExecutionResult
where
    Query: async_graphql::ObjectType + Send + Sync + 'static,
    Mutation: async_graphql::ObjectType + Send + Sync + 'static,
    Data: Send + Sync + Clone + 'static,
{
    let mut request = AsyncGraphqlRequest::new(payload.query)
        .variables(Variables::from_json(payload.variables))
        .data(context_data);
    if let Some(operation_name) = payload
        .operation_name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        request = request.operation_name(operation_name);
    }

    let response = schema.execute(request).await;
    let body = serde_json::to_value(response).unwrap_or_else(|error| {
        serde_json::json!({
            "data": null,
            "errors": [{"message": format!("Failed to serialize GraphQL response: {error}")}]
        })
    });

    GraphqlExecutionResult { status: 200, body }
}

#[Object]
impl AdminGraphqlQueryRoot {
    async fn system_status(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlSystemStatus> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        let is_initialized = self
            .foundation
            .system_settings()
            .is_initialized()
            .map_err(|error| async_graphql::Error::new(format!("failed to check system status: {error}")))?;

        Ok(AdminGraphqlSystemStatus { is_initialized })
    }

    async fn channels(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlChannel>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        let quota_by_channel = self
            .operational
            .provider_quota_statuses()
            .unwrap_or_default()
            .into_iter()
            .map(|status| (status.channel_id, AdminGraphqlProviderQuotaStatus::from(status)))
            .collect::<HashMap<_, _>>();
        let channels = self
            .foundation
            .channel_models()
            .list_channels()
            .map_err(|error| async_graphql::Error::new(format!("failed to list channels: {error}")))?;

        Ok(channels
            .into_iter()
            .map(|channel| {
                let mut gql = AdminGraphqlChannel::from(channel.clone());
                gql.provider_quota_status = quota_by_channel.get(&channel.id).cloned();
                gql
            })
            .collect())
    }

    async fn storage_policy(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlStoragePolicy> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        self.operational
            .storage_policy()
            .map(AdminGraphqlStoragePolicy::from)
            .map_err(async_graphql::Error::new)
    }

    async fn auto_backup_settings(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlAutoBackupSettings> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        self.operational
            .auto_backup_settings()
            .map(AdminGraphqlAutoBackupSettings::from)
            .map_err(async_graphql::Error::new)
    }

    async fn system_channel_settings(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlSystemChannelSettings> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        self.operational
            .system_channel_settings()
            .map(AdminGraphqlSystemChannelSettings::from)
            .map_err(async_graphql::Error::new)
    }

    async fn channel_probe_data(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlGetChannelProbeDataInput,
    ) -> async_graphql::Result<Vec<AdminGraphqlChannelProbeData>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        self.operational
            .channel_probe_data(&input.channel_ids)
            .map(|items| items.into_iter().map(AdminGraphqlChannelProbeData::from).collect())
            .map_err(async_graphql::Error::new)
    }

    async fn models(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlModel>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        let models = self
            .foundation
            .channel_models()
            .list_enabled_model_records()
            .map_err(|error| async_graphql::Error::new(format!("failed to list models: {error}")))?;

        Ok(models.into_iter().map(AdminGraphqlModel::from).collect())
    }

    async fn requests(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlRequestSummaryObject>> {
        let project_id = require_admin_graphql_project_id(ctx)?;
        require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
        let requests = self
            .foundation
            .requests()
            .list_requests_by_project(project_id)
            .map_err(|error| async_graphql::Error::new(format!("failed to list requests: {error}")))?;

        Ok(requests
            .into_iter()
            .map(AdminGraphqlRequestSummaryObject::from)
            .collect())
    }

    async fn traces(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<AdminGraphqlTrace>> {
        let project_id = require_admin_graphql_project_id(ctx)?;
        require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
        let traces = self
            .foundation
            .trace_contexts()
            .list_traces_by_project(project_id)
            .map_err(|error| async_graphql::Error::new(format!("failed to list traces: {error}")))?;

        Ok(traces.into_iter().map(AdminGraphqlTrace::from).collect())
    }
}

#[Object]
impl AdminGraphqlMutationRoot {
    async fn update_storage_policy(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateStoragePolicyInput,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .update_storage_policy(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn update_auto_backup_settings(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateAutoBackupSettingsInput,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .update_auto_backup_settings(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn trigger_auto_backup(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlTriggerBackupPayload> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .trigger_backup_now()
            .map(|message| AdminGraphqlTriggerBackupPayload {
                success: true,
                message: Some(message),
            })
            .map_err(async_graphql::Error::new)
    }

    async fn update_system_channel_settings(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateSystemChannelSettingsInput,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .update_system_channel_settings(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn check_provider_quotas(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .run_provider_quota_check_tick(true, Duration::from_secs(20 * 60))
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn trigger_gc_cleanup(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        self.operational
            .run_gc_cleanup_now(false, false)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }
}

#[Object]
impl OpenApiGraphqlQueryRoot {
    async fn service_account_project(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<GraphqlProject> {
        let request_context = ctx.data_unchecked::<OpenApiGraphqlRequestContext>();
        Ok(GraphqlProject::from(request_context.owner_api_key.project.clone()))
    }
}

#[Object]
impl OpenApiGraphqlMutationRoot {
    #[graphql(name = "createLLMAPIKey")]
    async fn create_llm_api_key(
        &self,
        ctx: &Context<'_>,
        name: String,
    ) -> async_graphql::Result<OpenApiGraphqlApiKey> {
        let request_context = ctx.data_unchecked::<OpenApiGraphqlRequestContext>();
        create_llm_api_key(self.foundation.as_ref(), &request_context.owner_api_key, &name)
            .map_err(|error| match error {
                CreateLlmApiKeyError::InvalidName => {
                    async_graphql::Error::new("api key name is required")
                }
                CreateLlmApiKeyError::PermissionDenied => {
                    async_graphql::Error::new("permission denied")
                }
                CreateLlmApiKeyError::Internal(message) => async_graphql::Error::new(message),
            })
    }
}

pub(crate) fn require_admin_graphql_project_id(ctx: &Context<'_>) -> async_graphql::Result<i64> {
    ctx.data_unchecked::<AdminGraphqlRequestContext>()
        .project_id
        .ok_or_else(|| async_graphql::Error::new("project context is required for this query"))
}

pub(crate) fn require_admin_system_scope(
    ctx: &Context<'_>,
    scope: ScopeSlug,
) -> async_graphql::Result<()> {
    let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
    if user_has_system_scope(&request_context.user, scope) {
        Ok(())
    } else {
        Err(async_graphql::Error::new("permission denied"))
    }
}

pub(crate) fn require_admin_project_scope(
    ctx: &Context<'_>,
    project_id: i64,
    scope: ScopeSlug,
) -> async_graphql::Result<()> {
    let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
    if user_has_system_scope(&request_context.user, scope)
        || user_has_project_scope(&request_context.user, project_id, scope)
    {
        Ok(())
    } else {
        Err(async_graphql::Error::new("permission denied"))
    }
}


pub(crate) fn create_llm_api_key(
    foundation: &SqliteFoundation,
    owner_api_key: &AuthApiKeyContext,
    name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return Err(CreateLlmApiKeyError::InvalidName);
    }
    if !api_key_has_scope(owner_api_key, SCOPE_WRITE_API_KEYS) {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let owner_record = foundation
        .identities()
        .find_api_key_by_value(owner_api_key.key.as_str())
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to load owner api key: {error:?}")))?;
    if owner_record.key_type != "service_account" || owner_record.project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let connection = foundation
        .open_connection(true)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to open database: {error}")))?;
    ensure_identity_tables(&connection)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to ensure identity schema: {error}")))?;

    let generated_key: String = connection
        .query_row("SELECT 'ah-' || lower(hex(randomblob(32)))", [], |row| row.get(0))
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to generate api key: {error}")))?;
    let scopes = scope_strings(LLM_API_KEY_SCOPES);
    let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}")))?;

    connection
        .execute(
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
             VALUES (?1, ?2, ?3, ?4, 'user', 'enabled', ?5, '{}', 0)",
            params![
                owner_record.user_id,
                owner_api_key.project.id,
                generated_key,
                trimmed_name,
                scopes_json,
            ],
        )
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to create api key: {error}")))?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
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
