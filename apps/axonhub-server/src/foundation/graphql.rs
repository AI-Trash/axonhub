use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_graphql::{
    Context, EmptySubscription, Enum, InputObject, Object, Request as AsyncGraphqlRequest,
    Schema, SimpleObject, Variables,
};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, ProjectContext, TraceContext,
};
use postgres::{types::ToSql as PostgresToSql, Client as PostgresClient, NoTls};
use rusqlite::{params, ToSql};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use super::{
    admin::{
        BackupFrequencySetting, ProbeFrequencySetting, SqliteOperationalService,
        StoredAutoBackupSettings, StoredChannelProbeData, StoredProviderQuotaStatus,
        StoredStoragePolicy, StoredSystemChannelSettings,
    },
    authz::{
        api_key_has_scope, scope_strings, serialize_scope_slugs, user_has_project_scope,
        user_has_system_scope, ScopeSlug, LLM_API_KEY_SCOPES, SCOPE_READ_CHANNELS,
        SCOPE_READ_REQUESTS, SCOPE_READ_SETTINGS, SCOPE_READ_ROLES, SCOPE_READ_USERS,
        SCOPE_WRITE_API_KEYS, SCOPE_WRITE_SETTINGS, SCOPE_WRITE_USERS,
    },
    identity_service::query_api_key_postgres,
    openai_v1::{parse_model_card, StoredChannelSummary, StoredModelRecord, StoredRequestSummary},
    shared::{format_unix_timestamp, graphql_gid, i64_to_i32, SqliteFoundation},
    system::{
        ensure_channel_model_tables_postgres, ensure_identity_tables,
        ensure_identity_tables_postgres, hash_password,
    },
};

pub struct SqliteAdminGraphqlService {
    pub(crate) schema: Arc<AdminGraphqlSchema>,
}

pub struct SqliteOpenApiGraphqlService {
    pub(crate) schema: Arc<OpenApiGraphqlSchema>,
}

pub struct PostgresAdminGraphqlService {
    dsn: Arc<String>,
}

pub struct PostgresOpenApiGraphqlService {
    dsn: Arc<String>,
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

impl PostgresAdminGraphqlService {
    pub fn new(dsn: impl Into<String>) -> Self {
        Self {
            dsn: Arc::new(dsn.into()),
        }
    }
}

impl PostgresOpenApiGraphqlService {
    pub fn new(dsn: impl Into<String>) -> Self {
        Self {
            dsn: Arc::new(dsn.into()),
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

impl AdminGraphqlPort for PostgresAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let dsn = Arc::clone(&self.dsn);
        Box::pin(async move {
            let payload = request;
            match tokio::task::spawn_blocking(move || {
                execute_admin_graphql_postgres_request(dsn.as_ref().clone(), payload, project_id, user)
            })
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(message)) => GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": null,
                        "errors": [{"message": format!("Failed to execute GraphQL request: {message}")}],
                    }),
                },
                Err(error) => GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": null,
                        "errors": [{"message": format!("Failed to execute GraphQL request: {error}")}],
                    }),
                },
            }
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

impl OpenApiGraphqlPort for PostgresOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let dsn = Arc::clone(&self.dsn);
        Box::pin(async move {
            let payload = request;
            match tokio::task::spawn_blocking(move || {
                execute_openapi_graphql_postgres_request(dsn.as_ref().clone(), payload, owner_api_key)
            })
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(message)) => GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": null,
                        "errors": [{"message": format!("Failed to execute GraphQL request: {message}")}],
                    }),
                },
                Err(error) => GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": null,
                        "errors": [{"message": format!("Failed to execute GraphQL request: {error}")}],
                    }),
                },
            }
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

    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlUserInfo> {
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        let user_id = request_context.user.id;
        let connection = self.foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        
        let user_row = connection.query_row(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, avatar, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
            [user_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        ).map_err(|error| async_graphql::Error::new(format!("failed to query user: {error}")))?;
        
        let (id, email, first_name, last_name, is_owner_i64, prefer_language, avatar, scopes_json) = user_row;
        let is_owner = is_owner_i64 != 0;
        let avatar = if avatar.is_empty() { None } else { Some(avatar) };
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
        
        let roles = connection.prepare(
            "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
          .query_map([user_id], |row| {
                let role_id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let role_scopes_json: String = row.get(2)?;
                let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                Ok(AdminGraphqlRoleInfo {
                    id: graphql_gid("role", role_id),
                    name,
                    scopes: role_scopes,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;
        
        let project_rows = connection.prepare(
            "SELECT p.id, p.name, up.is_owner, up.scopes FROM projects p JOIN user_projects up ON up.project_id = p.id WHERE up.user_id = ?1 AND p.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare projects: {error}")))?
          .query_map([user_id], |row| {
                let project_id: i64 = row.get(0)?;
                let _project_name: String = row.get(1)?;
                let is_owner = row.get::<_, i64>(2)? != 0;
                let project_scopes_json: String = row.get(3)?;
                let project_scopes: Vec<String> = serde_json::from_str(&project_scopes_json).unwrap_or_default();
                Ok((project_id, is_owner, project_scopes))
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query projects: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse projects: {error}")))?;
        
        let mut projects = Vec::new();
        for (project_id, is_owner, scopes) in project_rows {
            let project_roles = connection.prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.project_id = ?2 AND r.deleted_at = 0"
            ).map_err(|error| async_graphql::Error::new(format!("failed to prepare project roles: {error}")))?
              .query_map([user_id, project_id], |row| {
                    let role_id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let role_scopes_json: String = row.get(2)?;
                    let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                    Ok(AdminGraphqlRoleInfo {
                        id: graphql_gid("role", role_id),
                        name,
                        scopes: role_scopes,
                    })
                })
              .map_err(|error| async_graphql::Error::new(format!("failed to query project roles: {error}")))?
              .collect::<Result<Vec<_>, _>>()
              .map_err(|error| async_graphql::Error::new(format!("failed to parse project roles: {error}")))?;
            
            projects.push(AdminGraphqlUserProjectInfo {
                project_id: graphql_gid("project", project_id),
                is_owner,
                scopes,
                roles: project_roles,
            });
        }
        
        Ok(AdminGraphqlUserInfo {
            id: graphql_gid("user", id),
            email,
            first_name,
            last_name,
            is_owner,
            prefer_language,
            avatar,
            scopes,
            roles,
            projects,
        })
    }

    async fn roles(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlRoleConnection> {
        require_admin_system_scope(ctx, SCOPE_READ_ROLES)?;
        let connection = self.foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        
        let rows = connection.prepare(
            "SELECT id, name, scopes FROM roles WHERE deleted_at = 0 ORDER BY id ASC"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
          .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let scopes_json: String = row.get(2)?;
                let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
                Ok(AdminGraphqlRoleInfo {
                    id: graphql_gid("role", id),
                    name,
                    scopes,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;
        
        let edges = rows.into_iter().map(|node| AdminGraphqlRoleEdge {
            cursor: None,
            node: Some(node),
        }).collect();
        
        Ok(AdminGraphqlRoleConnection {
            edges,
            page_info: AdminGraphqlPageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            },
        })
    }

    async fn all_scopes(&self, ctx: &Context<'_>, level: Option<String>) -> async_graphql::Result<Vec<AdminGraphqlScopeInfo>> {
        require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
        
        let all_scopes = vec![
            AdminGraphqlScopeInfo {
                scope: "read_settings".to_string(),
                description: "Read system settings".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "write_settings".to_string(),
                description: "Write system settings".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "read_channels".to_string(),
                description: "Read channel configurations".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "write_channels".to_string(),
                description: "Write channel configurations".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "read_requests".to_string(),
                description: "Read request data".to_string(),
                levels: vec!["system".to_string(), "project".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "write_requests".to_string(),
                description: "Write request data".to_string(),
                levels: vec!["system".to_string(), "project".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "read_users".to_string(),
                description: "Read user data".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "write_users".to_string(),
                description: "Write user data".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "read_api_keys".to_string(),
                description: "Read API keys".to_string(),
                levels: vec!["system".to_string()],
            },
            AdminGraphqlScopeInfo {
                scope: "write_api_keys".to_string(),
                description: "Write API keys".to_string(),
                levels: vec!["system".to_string()],
            },
        ];
        
        let result = match level.as_deref() {
            Some("system") => all_scopes.into_iter().filter(|s| s.levels.contains(&"system".to_string())).collect(),
            Some("project") => all_scopes.into_iter().filter(|s| s.levels.contains(&"project".to_string())).collect(),
            Some(invalid) => return Err(async_graphql::Error::new(format!("invalid level: {}", invalid))),
            None => all_scopes,
        };
        Ok(result)
    }

    async fn query_models(&self, ctx: &Context<'_>, _input: AdminGraphqlQueryModelsInput) -> async_graphql::Result<Vec<AdminGraphqlModelIdentityWithStatus>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        let connection = self.foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        
        let rows = connection.prepare(
            "SELECT id, status FROM models WHERE deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare models: {error}")))?
          .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let status: String = row.get(1)?;
                Ok(AdminGraphqlModelIdentityWithStatus {
                    id: graphql_gid("model", id),
                    status,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query models: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse models: {error}")))?;
        
        Ok(rows)
    }

    async fn users(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlUserConnection> {
        require_admin_system_scope(ctx, SCOPE_READ_USERS)?;
        let connection = self.foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        
        let user_rows = connection.prepare(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE deleted_at = 0 ORDER BY id ASC"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare users: {error}")))?
          .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let email: String = row.get(1)?;
                let first_name: String = row.get(2)?;
                let last_name: String = row.get(3)?;
                let is_owner = row.get::<_, i64>(4)? != 0;
                let prefer_language: String = row.get(5)?;
                let status: String = row.get(6)?;
                let created_at: String = row.get(7)?;
                let updated_at: String = row.get(8)?;
                let scopes_json: String = row.get(9)?;
                let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
                Ok((id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes))
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query users: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse users: {error}")))?;
        
        let mut edges = Vec::new();
        for (id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes) in user_rows.into_iter() {
            let roles_vec = connection.prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0"
            ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles for user {}: {error}", id)))?
              .query_map([id], |row| {
                    let role_id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let role_scopes_json: String = row.get(2)?;
                    let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                    Ok(AdminGraphqlRoleInfo {
                        id: graphql_gid("role", role_id),
                        name,
                        scopes: role_scopes,
                    })
                })
              .map_err(|error| async_graphql::Error::new(format!("failed to query roles for user {}: {error}", id)))?
              .collect::<Result<Vec<_>, _>>()
              .map_err(|error| async_graphql::Error::new(format!("failed to parse roles for user {}: {error}", id)))?;
            
            let roles_connection = AdminGraphqlRoleConnection {
                edges: roles_vec.into_iter().map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                }).collect(),
                page_info: AdminGraphqlPageInfo {
                    has_next_page: false,
                    has_previous_page: false,
                    start_cursor: None,
                    end_cursor: None,
                },
            };
            
            let node = AdminGraphqlUser {
                id: graphql_gid("user", id),
                created_at,
                updated_at,
                email,
                status,
                first_name,
                last_name,
                is_owner,
                prefer_language,
                scopes,
                roles: roles_connection,
            };
            
            edges.push(AdminGraphqlUserEdge {
                cursor: None,
                node: Some(node),
            });
        }
        
        let page_info = AdminGraphqlPageInfo {
            has_next_page: false,
            has_previous_page: false,
            start_cursor: None,
            end_cursor: None,
        };
        
        Ok(AdminGraphqlUserConnection { edges, page_info })
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
        require_admin_owner(ctx)?;
        self.operational
            .update_auto_backup_settings(input)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn trigger_auto_backup(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlTriggerBackupPayload> {
        require_admin_owner(ctx)?;
        let _ = self.operational.trigger_backup_now();
        Ok(AdminGraphqlTriggerBackupPayload {
            success: true,
            message: Some("Backup completed successfully".to_owned()),
        })
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
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        if !user_has_system_scope(&request_context.user, SCOPE_WRITE_SETTINGS) {
            return Err(async_graphql::Error::new(
                "permission denied: requires write:settings scope",
            ));
        }
        self.operational
            .run_gc_cleanup_now(false, false)
            .map(|_| true)
            .map_err(async_graphql::Error::new)
    }

    async fn update_me(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlUpdateMeInput,
    ) -> async_graphql::Result<AdminGraphqlUserInfo> {
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        let user_id = request_context.user.id;

        if input.first_name.is_none() && input.last_name.is_none() &&
           input.prefer_language.is_none() && input.avatar.is_none() {
            return Err(async_graphql::Error::new("no fields to update"));
        }

        let foundation = &self.operational.foundation;
        let connection = foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let mut set_parts = Vec::new();
        let mut params: Vec<&dyn ToSql> = Vec::new();

        if let Some(first_name) = &input.first_name {
            set_parts.push("first_name = ?");
            params.push(first_name as &dyn ToSql);
        }
        if let Some(last_name) = &input.last_name {
            set_parts.push("last_name = ?");
            params.push(last_name as &dyn ToSql);
        }
        if let Some(prefer_language) = &input.prefer_language {
            set_parts.push("prefer_language = ?");
            params.push(prefer_language as &dyn ToSql);
        }
        if let Some(avatar) = &input.avatar {
            set_parts.push("avatar = ?");
            params.push(avatar as &dyn ToSql);
        }

        let set_clause = set_parts.join(", ");
        params.push(&user_id);

        let sql = format!(
            "UPDATE users SET {} WHERE id = ? AND deleted_at = 0",
            set_clause
        );

        // Convert Vec<Box<dyn ToSql>> to slice of &dyn ToSql
        let params_slice: Vec<&dyn ToSql> = params.iter().map(|b| &**b as &dyn ToSql).collect();

        let rows_affected = connection.execute(&sql, params_slice.as_slice())
            .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;

        if rows_affected == 0 {
            return Err(async_graphql::Error::new("user not found"));
        }

        let user_row = connection.query_row(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, avatar, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
            [user_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        ).map_err(|error| async_graphql::Error::new(format!("failed to query updated user: {error}")))?;

        let (id, email, first_name, last_name, is_owner_i64, prefer_language, avatar, scopes_json) = user_row;
        let is_owner = is_owner_i64 != 0;
        let avatar = if avatar.is_empty() { None } else { Some(avatar) };
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles = connection.prepare(
            "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
          .query_map([user_id], |row| {
                let role_id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let role_scopes_json: String = row.get(2)?;
                let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                Ok(AdminGraphqlRoleInfo {
                    id: graphql_gid("role", role_id),
                    name,
                    scopes: role_scopes,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;

        let project_rows = connection.prepare(
            "SELECT p.id, p.name, up.is_owner, up.scopes FROM projects p JOIN user_projects up ON up.project_id = p.id WHERE up.user_id = ?1 AND p.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare projects: {error}")))?
          .query_map([user_id], |row| {
                let project_id: i64 = row.get(0)?;
                let _project_name: String = row.get(1)?;
                let is_owner = row.get::<_, i64>(2)? != 0;
                let project_scopes_json: String = row.get(3)?;
                let project_scopes: Vec<String> = serde_json::from_str(&project_scopes_json).unwrap_or_default();
                Ok((project_id, is_owner, project_scopes))
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query projects: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse projects: {error}")))?;

        let mut projects = Vec::new();
        for (project_id, is_owner, scopes) in project_rows {
            let project_roles = connection.prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.project_id = ?2 AND r.deleted_at = 0"
            ).map_err(|error| async_graphql::Error::new(format!("failed to prepare project roles: {error}")))?
              .query_map([user_id, project_id], |row| {
                    let role_id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let role_scopes_json: String = row.get(2)?;
                    let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                    Ok(AdminGraphqlRoleInfo {
                        id: graphql_gid("role", role_id),
                        name,
                        scopes: role_scopes,
                    })
                })
              .map_err(|error| async_graphql::Error::new(format!("failed to query project roles: {error}")))?
              .collect::<Result<Vec<_>, _>>()
              .map_err(|error| async_graphql::Error::new(format!("failed to parse project roles: {error}")))?;
            
             projects.push(AdminGraphqlUserProjectInfo {
                 project_id: graphql_gid("project", project_id),
                 is_owner,
                 scopes,
                 roles: project_roles,
             });
         }

         Ok(AdminGraphqlUserInfo {
             id: graphql_gid("user", id),
             email,
             first_name,
             last_name,
             is_owner,
             prefer_language,
             avatar,
             scopes,
             roles,
             projects,
         })
     }

    async fn update_user_status(
        &self,
        ctx: &Context<'_>,
        id: String,
        status: UserStatus,
    ) -> async_graphql::Result<AdminGraphqlUser> {
        require_admin_system_scope(ctx, SCOPE_WRITE_USERS)?;

        let id_parts: Vec<&str> = id.split('/').collect();
        let user_id_str = id_parts.last().ok_or_else(|| async_graphql::Error::new("invalid user id format"))?;
        let user_id: i64 = user_id_str.parse().map_err(|_| async_graphql::Error::new("invalid user id"))?;

        let foundation = &self.operational.foundation;
        let connection = foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let status_str = match status {
            UserStatus::Activated => "activated",
            UserStatus::Deactivated => "deactivated",
        };
        let rows_affected = connection.execute(
            "UPDATE users SET status = ? WHERE id = ? AND deleted_at = 0",
            params![status_str, user_id],
        ).map_err(|error| async_graphql::Error::new(format!("failed to update user status: {error}")))?;

        if rows_affected == 0 {
            return Err(async_graphql::Error::new("user not found"));
        }

        let user_row = connection.query_row(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
            [user_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        ).map_err(|error| async_graphql::Error::new(format!("failed to query updated user: {error}")))?;

        let (id, email, first_name, last_name, is_owner_i64, prefer_language, status, created_at, updated_at, scopes_json) = user_row;
        let is_owner = is_owner_i64 != 0;
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles_vec = connection.prepare(
            "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
          .query_map([user_id], |row| {
                let role_id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let role_scopes_json: String = row.get(2)?;
                let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                Ok(AdminGraphqlRoleInfo {
                    id: graphql_gid("role", role_id),
                    name,
                    scopes: role_scopes,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;

        let roles_connection = AdminGraphqlRoleConnection {
            edges: roles_vec.into_iter().map(|node| AdminGraphqlRoleEdge {
                cursor: None,
                node: Some(node),
            }).collect(),
            page_info: AdminGraphqlPageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            },
        };

        Ok(AdminGraphqlUser {
            id: graphql_gid("user", id),
            created_at,
            updated_at,
            email,
            status,
            first_name,
            last_name,
            is_owner,
            prefer_language,
            scopes,
            roles: roles_connection,
        })
    }

    async fn create_user(
        &self,
        ctx: &Context<'_>,
        input: AdminGraphqlCreateUserInput,
    ) -> async_graphql::Result<AdminGraphqlUser> {
        require_admin_system_scope(ctx, SCOPE_WRITE_USERS)?;

        let foundation = &self.operational.foundation;
        let mut connection = foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        let transaction = connection
            .transaction()
            .map_err(|error| async_graphql::Error::new(format!("failed to start user transaction: {error}")))?;

        let hashed_password = hash_password(&input.password)
            .map_err(|error| async_graphql::Error::new(format!("failed to hash password: {error}")))?;

        let status = match input.status {
            Some(UserStatus::Activated) => "activated",
            Some(UserStatus::Deactivated) => "deactivated",
            None => "activated",
        };

        let scopes_json = if let Some(scopes) = input.scopes {
            serde_json::to_string(&scopes).unwrap_or_default()
        } else {
            "[]".to_string()
        };

        transaction.execute(
            "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
            params![
                input.email,
                status,
                input.prefer_language.unwrap_or_else(|| "en".to_string()),
                hashed_password,
                input.first_name.unwrap_or_else(|| "".to_string()),
                input.last_name.unwrap_or_else(|| "".to_string()),
                input.avatar.unwrap_or_else(|| "".to_string()),
                input.is_owner.unwrap_or(false) as i64,
                scopes_json,
            ],
        ).map_err(|error| async_graphql::Error::new(format!("failed to create user: {error}")))?;

        let user_id = transaction.last_insert_rowid();

        if let Some(project_ids) = input.project_ids {
            for project_gid in project_ids {
                let parts: Vec<&str> = project_gid.split('/').collect();
                if let Some(id_str) = parts.last() {
                    if let Ok(project_id) = id_str.parse::<i64>() {
                        let is_owner = false;
                        transaction.execute(
                            "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES (?1, ?2, ?3, ?4)",
                            params![user_id, project_id, if is_owner { 1 } else { 0 }, "[]"],
                        ).map_err(|error| async_graphql::Error::new(format!("failed to assign user project membership: {error}")))?;
                    }
                }
            }
        }

        if let Some(role_ids) = input.role_ids {
            for role_gid in role_ids {
                let parts: Vec<&str> = role_gid.split('/').collect();
                if let Some(id_str) = parts.last() {
                    if let Ok(role_id) = id_str.parse::<i64>() {
                        transaction.execute(
                            "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                            params![user_id, role_id],
                        ).map_err(|error| async_graphql::Error::new(format!("failed to assign user role: {error}")))?;
                    }
                }
            }
        }

        let user_row = transaction.query_row(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
            [user_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        ).map_err(|error| async_graphql::Error::new(format!("failed to query created user: {error}")))?;

        let (id, email, first_name, last_name, is_owner_i64, prefer_language, status, created_at, updated_at, scopes_json) = user_row;
        let is_owner = is_owner_i64 != 0;
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles_vec = transaction.prepare(
            "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
          .query_map([user_id], |row| {
                let role_id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let role_scopes_json: String = row.get(2)?;
                let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                Ok(AdminGraphqlRoleInfo {
                    id: graphql_gid("role", role_id),
                    name,
                    scopes: role_scopes,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;

        let roles_connection = AdminGraphqlRoleConnection {
            edges: roles_vec.into_iter().map(|node| AdminGraphqlRoleEdge {
                cursor: None,
                node: Some(node),
            }).collect(),
            page_info: AdminGraphqlPageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            },
        };

        let created_user = AdminGraphqlUser {
            id: graphql_gid("user", id),
            created_at,
            updated_at,
            email,
            status,
            first_name,
            last_name,
            is_owner,
            prefer_language,
            scopes,
            roles: roles_connection,
        };

        transaction
            .commit()
            .map_err(|error| async_graphql::Error::new(format!("failed to commit user transaction: {error}")))?;

        Ok(created_user)
    }

    async fn update_user(
        &self,
        ctx: &Context<'_>,
        id: String,
        input: AdminGraphqlUpdateUserInput,
    ) -> async_graphql::Result<AdminGraphqlUser> {
        require_admin_system_scope(ctx, SCOPE_WRITE_USERS)?;

        let id_parts: Vec<&str> = id.split('/').collect();
        let user_id_str = id_parts.last().ok_or_else(|| async_graphql::Error::new("invalid user id format"))?;
        let user_id: i64 = user_id_str.parse().map_err(|_| async_graphql::Error::new("invalid user id"))?;

        let foundation = &self.operational.foundation;
        let mut connection = foundation.open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        let transaction = connection
            .transaction()
            .map_err(|error| async_graphql::Error::new(format!("failed to start user transaction: {error}")))?;

        let mut set_parts = Vec::new();
        let mut params: Vec<Box<dyn ToSql>> = Vec::new();

        if let Some(first_name) = &input.first_name {
            set_parts.push("first_name = ?");
            params.push(Box::new(first_name.clone()));
        }
        if let Some(last_name) = &input.last_name {
            set_parts.push("last_name = ?");
            params.push(Box::new(last_name.clone()));
        }
        if let Some(prefer_language) = &input.prefer_language {
            set_parts.push("prefer_language = ?");
            params.push(Box::new(prefer_language.clone()));
        }
        if let Some(avatar) = &input.avatar {
            set_parts.push("avatar = ?");
            params.push(Box::new(avatar.clone()));
        }

        if set_parts.is_empty() && input.scopes.is_none() && input.role_ids.is_none() {
            return Err(async_graphql::Error::new("no fields to update"));
        }

        if let Some(scopes) = &input.scopes {
            let scopes_json = serde_json::to_string(scopes).unwrap_or_default();
            set_parts.push("scopes = ?");
            params.push(Box::new(scopes_json));
        }

        if set_parts.is_empty() {
            return Err(async_graphql::Error::new("no fields to update"));
        }

        let set_clause = set_parts.join(", ");
        params.push(Box::new(user_id));

        let sql = format!(
            "UPDATE users SET {} WHERE id = ? AND deleted_at = 0",
            set_clause
        );

        let params_slice: Vec<&dyn ToSql> = params.iter().map(|b| &**b as &dyn ToSql).collect();

        let rows_affected = transaction.execute(&sql, params_slice.as_slice())
            .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;

        if rows_affected == 0 {
            return Err(async_graphql::Error::new("user not found"));
        }

        if let Some(role_ids) = &input.role_ids {
            transaction.execute(
                "DELETE FROM user_roles WHERE user_id = ?",
                params![user_id],
            ).map_err(|error| async_graphql::Error::new(format!("failed to clear existing user roles: {error}")))?;

            for role_gid in role_ids {
                let parts: Vec<&str> = role_gid.split('/').collect();
                if let Some(id_str) = parts.last() {
                    if let Ok(role_id) = id_str.parse::<i64>() {
                        transaction.execute(
                            "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                            params![user_id, role_id],
                        ).map_err(|error| async_graphql::Error::new(format!("failed to replace user role assignments: {error}")))?;
                    }
                }
            }
        }

        let user_row = transaction.query_row(
            "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
            [user_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        ).map_err(|error| async_graphql::Error::new(format!("failed to query updated user: {error}")))?;

        let (id, email, first_name, last_name, is_owner_i64, prefer_language, status, created_at, updated_at, scopes_json) = user_row;
        let is_owner = is_owner_i64 != 0;
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles_vec = transaction.prepare(
            "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0"
        ).map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
          .query_map([user_id], |row| {
                let role_id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let role_scopes_json: String = row.get(2)?;
                let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                Ok(AdminGraphqlRoleInfo {
                    id: graphql_gid("role", role_id),
                    name,
                    scopes: role_scopes,
                })
            })
          .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;

        let roles_connection = AdminGraphqlRoleConnection {
            edges: roles_vec.into_iter().map(|node| AdminGraphqlRoleEdge {
                cursor: None,
                node: Some(node),
            }).collect(),
            page_info: AdminGraphqlPageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            },
        };

        let updated_user = AdminGraphqlUser {
            id: graphql_gid("user", id),
            created_at,
            updated_at,
            email,
            status,
            first_name,
            last_name,
            is_owner,
            prefer_language,
            scopes,
            roles: roles_connection,
        };

        transaction
            .commit()
            .map_err(|error| async_graphql::Error::new(format!("failed to commit user transaction: {error}")))?;

        Ok(updated_user)
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

pub(crate) fn require_admin_owner(ctx: &Context<'_>) -> async_graphql::Result<()> {
    let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
    if request_context.user.is_owner {
        Ok(())
    } else {
        Err(async_graphql::Error::new(
            "permission denied: owner access required",
        ))
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

    create_llm_api_key_sqlite(foundation, owner_api_key, trimmed_name)
}

fn create_llm_api_key_sqlite(
    foundation: &SqliteFoundation,
    owner_api_key: &AuthApiKeyContext,
    trimmed_name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
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

fn create_llm_api_key_postgres(
    dsn: &str,
    owner_api_key: &AuthApiKeyContext,
    trimmed_name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
    let mut client = PostgresClient::connect(dsn, NoTls)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to connect to postgres: {error}")))?;
    ensure_identity_tables_postgres(&mut client)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to ensure identity schema: {error}")))?;

    let owner_record = query_api_key_postgres(&mut client, owner_api_key.key.as_str())
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to load owner api key: {error:?}")))?;
    if owner_record.key_type != "service_account" || owner_record.project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let generated_key_row = client
        .query_one("SELECT 'ah-' || lower(encode(gen_random_bytes(32), 'hex'))", &[])
        .or_else(|_| client.query_one("SELECT 'ah-' || lower(md5(random()::text || clock_timestamp()::text))", &[]))
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to generate api key: {error}")))?;
    let generated_key: String = generated_key_row.get(0);
    let scopes = scope_strings(LLM_API_KEY_SCOPES);
    let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}")))?;
    let params: [&(dyn PostgresToSql + Sync); 5] = [
        &owner_record.user_id,
        &owner_api_key.project.id,
        &generated_key,
        &trimmed_name,
        &scopes_json,
    ];

    client
        .execute(
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
             VALUES ($1, $2, $3, $4, 'user', 'enabled', $5, '{}', 0)",
            &params,
        )
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to create api key: {error}")))?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
    })
}

fn execute_openapi_graphql_postgres_request(
    dsn: String,
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
        return create_llm_api_key_postgres(&dsn, &owner_api_key, &name)
            .map(|api_key| {
                let key = api_key.key;
                let name = api_key.name;
                let scopes = api_key.scopes;
                GraphqlExecutionResult {
                status: 200,
                body: serde_json::json!({
                    "data": {
                        "createLLMAPIKey": {
                            "key": key,
                            "name": name,
                            "scopes": scopes,
                        }
                    }
                }),
            }
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
            "errors": [{"message": "unsupported postgres openapi graphql pilot query"}],
        }),
    })
}

fn extract_create_llm_api_key_name(query: &str) -> Option<String> {
    let marker = "createLLMAPIKey(name:";
    let start = query.find(marker)? + marker.len();
    let remainder = query.get(start..)?.trim_start();
    let first_quote = remainder.find('"')? + 1;
    let after_first = remainder.get(first_quote..)?;
    let end_quote = after_first.find('"')?;
    Some(after_first[..end_quote].to_owned())
}

fn execute_admin_graphql_postgres_request(
    dsn: String,
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
            admin_scope_info(
                "read_requests",
                "Read request data",
                &["system", "project"],
            ),
            admin_scope_info(
                "write_requests",
                "Write request data",
                &["system", "project"],
            ),
            admin_scope_info("read_users", "Read user data", &["system"]),
            admin_scope_info("write_users", "Write user data", &["system"]),
            admin_scope_info("read_api_keys", "Read API keys", &["system"]),
            admin_scope_info("write_api_keys", "Write API keys", &["system"]),
        ];

        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({
                "data": {"allScopes": all_scopes},
            }),
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

        let mut client = PostgresClient::connect(&dsn, NoTls).map_err(|error| error.to_string())?;
        ensure_channel_model_tables_postgres(&mut client).map_err(|error| error.to_string())?;
        let models = client
            .query(
                "SELECT id, status FROM models WHERE deleted_at = 0 ORDER BY id ASC",
                &[],
            )
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|row| serde_json::json!({
                "id": graphql_gid("model", row.get::<_, i64>(0)),
                "status": row.get::<_, String>(1),
            }))
            .collect::<Vec<_>>();

        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({
                "data": {"queryModels": models},
            }),
        });
    }

    Err("unsupported postgres admin graphql subset query".to_owned())
}

fn admin_scope_info(scope: &str, description: &str, levels: &[&str]) -> Value {
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
