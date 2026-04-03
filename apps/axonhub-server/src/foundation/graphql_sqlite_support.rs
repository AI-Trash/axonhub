use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_graphql::{
    Context, EmptySubscription, Object, Request as AsyncGraphqlRequest, Schema, Variables,
};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, TraceContext,
};
use sea_orm::{ConnectionTrait, DatabaseBackend};

use super::{
    admin::parse_graphql_resource_id,
    admin::SqliteOperationalService,
    authz::{
        authorize_user_system_scope, require_owner_bypass,
        require_service_api_key_write_access, require_user_project_scope, scope_strings,
        serialize_scope_slugs, ScopeSlug, LLM_API_KEY_SCOPES, SCOPE_READ_CHANNELS,
        SCOPE_READ_REQUESTS, SCOPE_READ_ROLES, SCOPE_READ_SETTINGS, SCOPE_READ_USERS,
        SCOPE_WRITE_SETTINGS, SCOPE_WRITE_USERS,
    },
    circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker},
    graphql::*,
    repositories::graphql::query_all_graphql,
    shared::{graphql_gid, SqliteFoundation},
    system::{ensure_identity_tables, hash_password},
};

pub(crate) struct SqliteAdminGraphqlService {
    pub(crate) schema: Arc<AdminGraphqlSchema>,
}

pub(crate) struct SqliteOpenApiGraphqlService {
    pub(crate) schema: Arc<OpenApiGraphqlSchema>,
}

pub(crate) type AdminGraphqlSchema =
    Schema<AdminGraphqlQueryRoot, AdminGraphqlMutationRoot, EmptySubscription>;
pub(crate) type OpenApiGraphqlSchema =
    Schema<OpenApiGraphqlQueryRoot, OpenApiGraphqlMutationRoot, EmptySubscription>;

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
    pub(crate) circuit_breaker: SharedCircuitBreaker,
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

impl SqliteAdminGraphqlService {
    pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
        let operational = Arc::new(SqliteOperationalService::new(foundation.clone()));
        let circuit_breaker = SharedCircuitBreaker::new(CircuitBreakerPolicy::default());
        let schema = Schema::build(
            AdminGraphqlQueryRoot {
                foundation,
                operational: operational.clone(),
                circuit_breaker,
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
            execute_graphql_schema(
                schema,
                request,
                AdminGraphqlRequestContext { project_id, user },
            )
            .await
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
            execute_graphql_schema(schema, request, OpenApiGraphqlRequestContext { owner_api_key })
                .await
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

async fn list_traces_by_project_graphql(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    project_id: i64,
) -> Result<Vec<TraceContext>, String> {
    query_all_graphql(
        db,
        backend,
        "SELECT id, trace_id, project_id, thread_id FROM traces WHERE project_id = ? ORDER BY id DESC",
        "SELECT id, trace_id, project_id, thread_id FROM traces WHERE project_id = $1 ORDER BY id DESC",
        "SELECT id, trace_id, project_id, thread_id FROM traces WHERE project_id = ? ORDER BY id DESC",
        vec![project_id.into()],
    )
    .await?
    .into_iter()
    .map(|row: sea_orm::QueryResult| {
        Ok::<TraceContext, String>(TraceContext {
            id: row.try_get_by_index(0).map_err(|error| error.to_string())?,
            trace_id: row.try_get_by_index(1).map_err(|error| error.to_string())?,
            project_id: row.try_get_by_index(2).map_err(|error| error.to_string())?,
            thread_id: row.try_get_by_index(3).map_err(|error| error.to_string())?,
        })
    })
    .collect()
}

#[Object]
impl AdminGraphqlQueryRoot {
    async fn system_status(
        &self,
        _ctx: &Context<'_>,
    ) -> async_graphql::Result<AdminGraphqlSystemStatus> {
        let is_initialized = self
            .foundation
            .system_settings()
            .is_initialized()
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to check system status: {error}"))
            })?;

        Ok(AdminGraphqlSystemStatus { is_initialized })
    }

    async fn channels(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<AdminGraphqlChannel>> {
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
                gql.circuit_breaker_status = stored_circuit_breaker_status(channel.id, &self.circuit_breaker)
                    .map(AdminGraphqlCircuitBreakerStatus::from);
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

    async fn models(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<AdminGraphqlModel>> {
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

    async fn traces(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<AdminGraphqlTrace>> {
        let project_id = require_admin_graphql_project_id(ctx)?;
        require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
        let db = self.foundation.seaorm();
        let traces = db
            .run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                list_traces_by_project_graphql(&connection, db.backend(), project_id).await
            })
            .map_err(|error| async_graphql::Error::new(format!("failed to list traces: {error}")))?;

        Ok(traces.into_iter().map(AdminGraphqlTrace::from).collect())
    }

    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlUserInfo> {
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        let user_id = request_context.user.id;
        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let user_row = connection
            .query_row(
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
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to query user: {error}")))?;

        let (id, email, first_name, last_name, is_owner_i64, prefer_language, avatar, scopes_json) =
            user_row;
        let is_owner = is_owner_i64 != 0;
        let avatar = if avatar.is_empty() { None } else { Some(avatar) };
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles = connection
            .prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
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

        let project_rows = connection
            .prepare(
                "SELECT p.id, p.name, up.is_owner, up.scopes FROM projects p JOIN user_projects up ON up.project_id = p.id WHERE up.user_id = ?1 AND p.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare projects: {error}")))?
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
            let project_roles = connection
                .prepare(
                    "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.project_id = ?2 AND r.deleted_at = 0",
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to prepare project roles: {error}")))?
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
        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let rows = connection
            .prepare("SELECT id, name, scopes FROM roles WHERE deleted_at = 0 ORDER BY id ASC")
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
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

        let edges = rows
            .into_iter()
            .map(|node| AdminGraphqlRoleEdge {
                cursor: None,
                node: Some(node),
            })
            .collect();

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

    async fn all_scopes(
        &self,
        ctx: &Context<'_>,
        level: Option<String>,
    ) -> async_graphql::Result<Vec<AdminGraphqlScopeInfo>> {
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
            Some("system") => all_scopes
                .into_iter()
                .filter(|s| s.levels.contains(&"system".to_string()))
                .collect(),
            Some("project") => all_scopes
                .into_iter()
                .filter(|s| s.levels.contains(&"project".to_string()))
                .collect(),
            Some(invalid) => {
                return Err(async_graphql::Error::new(format!("invalid level: {}", invalid)))
            }
            None => all_scopes,
        };
        Ok(result)
    }

    async fn query_models(
        &self,
        ctx: &Context<'_>,
        _input: AdminGraphqlQueryModelsInput,
    ) -> async_graphql::Result<Vec<AdminGraphqlModelIdentityWithStatus>> {
        require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let rows = connection
            .prepare("SELECT id, status FROM models WHERE deleted_at = 0")
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare models: {error}")))?
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
        let connection = self
            .foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let user_rows = connection
            .prepare(
                "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE deleted_at = 0 ORDER BY id ASC",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare users: {error}")))?
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
                Ok((
                    id,
                    email,
                    first_name,
                    last_name,
                    is_owner,
                    prefer_language,
                    status,
                    created_at,
                    updated_at,
                    scopes,
                ))
            })
            .map_err(|error| async_graphql::Error::new(format!("failed to query users: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| async_graphql::Error::new(format!("failed to parse users: {error}")))?;

        let mut edges = Vec::new();
        for (
            id,
            email,
            first_name,
            last_name,
            is_owner,
            prefer_language,
            status,
            created_at,
            updated_at,
            scopes,
        ) in user_rows.into_iter()
        {
            let roles_vec = connection
                .prepare(
                    "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
                )
                .map_err(|error| {
                    async_graphql::Error::new(format!(
                        "failed to prepare roles for user {}: {error}",
                        id
                    ))
                })?
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
                .map_err(|error| {
                    async_graphql::Error::new(format!(
                        "failed to query roles for user {}: {error}",
                        id
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    async_graphql::Error::new(format!(
                        "failed to parse roles for user {}: {error}",
                        id
                    ))
                })?;

            let roles_connection = AdminGraphqlRoleConnection {
                edges: roles_vec
                    .into_iter()
                    .map(|node| AdminGraphqlRoleEdge {
                        cursor: None,
                        node: Some(node),
                    })
                    .collect(),
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

    async fn reset_provider_quota(
        &self,
        ctx: &Context<'_>,
        #[graphql(name = "channelID")] channel_id: String,
    ) -> async_graphql::Result<bool> {
        require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
        let channel_id = parse_graphql_resource_id(channel_id.as_str(), "channel")?;
        self.operational
            .reset_provider_quota_status(channel_id)
            .map_err(async_graphql::Error::new)
    }

    async fn trigger_gc_cleanup(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        if require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS).is_err() {
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

        if input.first_name.is_none()
            && input.last_name.is_none()
            && input.prefer_language.is_none()
            && input.avatar.is_none()
        {
            return Err(async_graphql::Error::new("no fields to update"));
        }

        let foundation = &self.operational.foundation;
        let connection = foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        if let Some(first_name) = &input.first_name {
            let rows_affected = connection
                .execute(
                    "UPDATE users SET first_name = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (first_name.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
        }
        if let Some(last_name) = &input.last_name {
            let rows_affected = connection
                .execute(
                    "UPDATE users SET last_name = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (last_name.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
        }
        if let Some(prefer_language) = &input.prefer_language {
            let rows_affected = connection
                .execute(
                    "UPDATE users SET prefer_language = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (prefer_language.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
        }
        if let Some(avatar) = &input.avatar {
            let rows_affected = connection
                .execute(
                    "UPDATE users SET avatar = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (avatar.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
        }

        let user_row = connection
            .query_row(
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
            )
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to query updated user: {error}"))
            })?;

        let (id, email, first_name, last_name, is_owner_i64, prefer_language, avatar, scopes_json) =
            user_row;
        let is_owner = is_owner_i64 != 0;
        let avatar = if avatar.is_empty() { None } else { Some(avatar) };
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles = connection
            .prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
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

        let project_rows = connection
            .prepare(
                "SELECT p.id, p.name, up.is_owner, up.scopes FROM projects p JOIN user_projects up ON up.project_id = p.id WHERE up.user_id = ?1 AND p.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare projects: {error}")))?
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
            let project_roles = connection
                .prepare(
                    "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.project_id = ?2 AND r.deleted_at = 0",
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to prepare project roles: {error}")))?
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
        let user_id_str = id_parts
            .last()
            .ok_or_else(|| async_graphql::Error::new("invalid user id format"))?;
        let user_id: i64 = user_id_str
            .parse()
            .map_err(|_| async_graphql::Error::new("invalid user id"))?;

        let foundation = &self.operational.foundation;
        let connection = foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

        let status_str = match status {
            UserStatus::Activated => "activated",
            UserStatus::Deactivated => "deactivated",
        };
        let rows_affected = connection
            .execute(
                "UPDATE users SET status = ? WHERE id = ? AND deleted_at = 0",
                (status_str, user_id),
            )
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to update user status: {error}"))
            })?;

        if rows_affected == 0 {
            return Err(async_graphql::Error::new("user not found"));
        }

        let user_row = connection
            .query_row(
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
            )
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to query updated user: {error}"))
            })?;

        let (
            id,
            email,
            first_name,
            last_name,
            is_owner_i64,
            prefer_language,
            status,
            created_at,
            updated_at,
            scopes_json,
        ) = user_row;
        let is_owner = is_owner_i64 != 0;
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles_vec = connection
            .prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
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
            edges: roles_vec
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect(),
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
        let mut connection = foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        let transaction = connection.transaction().map_err(|error| {
            async_graphql::Error::new(format!("failed to start user transaction: {error}"))
        })?;

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

        transaction
            .execute(
                "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
                (
                    input.email,
                    status,
                    input.prefer_language.unwrap_or_else(|| "en".to_string()),
                    hashed_password,
                    input.first_name.unwrap_or_else(|| "".to_string()),
                    input.last_name.unwrap_or_else(|| "".to_string()),
                    input.avatar.unwrap_or_else(|| "".to_string()),
                    input.is_owner.unwrap_or(false) as i64,
                    scopes_json,
                ),
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to create user: {error}")))?;

        let user_id = transaction.last_insert_rowid();

        if let Some(project_ids) = input.project_ids {
            for project_gid in project_ids {
                let parts: Vec<&str> = project_gid.split('/').collect();
                if let Some(id_str) = parts.last() {
                    if let Ok(project_id) = id_str.parse::<i64>() {
                        let is_owner = false;
                        transaction
                            .execute(
                                "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES (?1, ?2, ?3, ?4)",
                                (user_id, project_id, if is_owner { 1 } else { 0 }, "[]"),
                            )
                            .map_err(|error| {
                                async_graphql::Error::new(format!(
                                    "failed to assign user project membership: {error}"
                                ))
                            })?;
                    }
                }
            }
        }

        if let Some(role_ids) = input.role_ids {
            for role_gid in role_ids {
                let parts: Vec<&str> = role_gid.split('/').collect();
                if let Some(id_str) = parts.last() {
                    if let Ok(role_id) = id_str.parse::<i64>() {
                        transaction
                            .execute(
                                "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                                (user_id, role_id),
                            )
                            .map_err(|error| {
                                async_graphql::Error::new(format!(
                                    "failed to assign user role: {error}"
                                ))
                            })?;
                    }
                }
            }
        }

        let user_row = transaction
            .query_row(
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
            )
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to query created user: {error}"))
            })?;

        let (
            id,
            email,
            first_name,
            last_name,
            is_owner_i64,
            prefer_language,
            status,
            created_at,
            updated_at,
            scopes_json,
        ) = user_row;
        let is_owner = is_owner_i64 != 0;
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles_vec = transaction
            .prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
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
            edges: roles_vec
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect(),
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

        transaction.commit().map_err(|error| {
            async_graphql::Error::new(format!("failed to commit user transaction: {error}"))
        })?;

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
        let user_id_str = id_parts
            .last()
            .ok_or_else(|| async_graphql::Error::new("invalid user id format"))?;
        let user_id: i64 = user_id_str
            .parse()
            .map_err(|_| async_graphql::Error::new("invalid user id"))?;

        let foundation = &self.operational.foundation;
        let mut connection = foundation
            .open_connection(true)
            .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
        let transaction = connection.transaction().map_err(|error| {
            async_graphql::Error::new(format!("failed to start user transaction: {error}"))
        })?;

        if input.first_name.is_none()
            && input.last_name.is_none()
            && input.prefer_language.is_none()
            && input.avatar.is_none()
            && input.scopes.is_none()
            && input.role_ids.is_none()
        {
            return Err(async_graphql::Error::new("no fields to update"));
        }

        let mut touched = false;
        if let Some(first_name) = &input.first_name {
            let rows_affected = transaction
                .execute(
                    "UPDATE users SET first_name = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (first_name.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
            touched = true;
        }
        if let Some(last_name) = &input.last_name {
            let rows_affected = transaction
                .execute(
                    "UPDATE users SET last_name = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (last_name.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
            touched = true;
        }
        if let Some(prefer_language) = &input.prefer_language {
            let rows_affected = transaction
                .execute(
                    "UPDATE users SET prefer_language = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (prefer_language.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
            touched = true;
        }
        if let Some(avatar) = &input.avatar {
            let rows_affected = transaction
                .execute(
                    "UPDATE users SET avatar = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (avatar.as_str(), user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
            touched = true;
        }
        if let Some(scopes) = &input.scopes {
            let scopes_json = serde_json::to_string(scopes).unwrap_or_default();
            let rows_affected = transaction
                .execute(
                    "UPDATE users SET scopes = ?1 WHERE id = ?2 AND deleted_at = 0",
                    (scopes_json, user_id),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
            if rows_affected == 0 {
                return Err(async_graphql::Error::new("user not found"));
            }
            touched = true;
        }
        if !touched {
            return Err(async_graphql::Error::new("no fields to update"));
        }

        if let Some(role_ids) = &input.role_ids {
            transaction
                .execute("DELETE FROM user_roles WHERE user_id = ?", [user_id])
                .map_err(|error| {
                    async_graphql::Error::new(format!(
                        "failed to clear existing user roles: {error}"
                    ))
                })?;

            for role_gid in role_ids {
                let parts: Vec<&str> = role_gid.split('/').collect();
                if let Some(id_str) = parts.last() {
                    if let Ok(role_id) = id_str.parse::<i64>() {
                        transaction
                            .execute(
                                "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                                (user_id, role_id),
                            )
                            .map_err(|error| {
                                async_graphql::Error::new(format!(
                                    "failed to replace user role assignments: {error}"
                                ))
                            })?;
                    }
                }
            }
        }

        let user_row = transaction
            .query_row(
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
            )
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to query updated user: {error}"))
            })?;

        let (
            id,
            email,
            first_name,
            last_name,
            is_owner_i64,
            prefer_language,
            status,
            created_at,
            updated_at,
            scopes_json,
        ) = user_row;
        let is_owner = is_owner_i64 != 0;
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

        let roles_vec = transaction
            .prepare(
                "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
            )
            .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
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
            edges: roles_vec
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect(),
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

        transaction.commit().map_err(|error| {
            async_graphql::Error::new(format!("failed to commit user transaction: {error}"))
        })?;

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
    if authorize_user_system_scope(&request_context.user, scope).is_ok() {
        Ok(())
    } else {
        Err(async_graphql::Error::new("permission denied"))
    }
}

pub(crate) fn require_admin_owner(ctx: &Context<'_>) -> async_graphql::Result<()> {
    let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
    if require_owner_bypass(&request_context.user).is_ok() {
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
    if require_user_project_scope(&request_context.user, project_id, scope).is_ok() {
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
    if require_service_api_key_write_access(owner_api_key).is_err() {
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
        .map_err(|error| {
            CreateLlmApiKeyError::Internal(format!(
                "failed to load owner api key: {error:?}"
            ))
        })?;
    if owner_record.key_type != "service_account" || owner_record.project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let connection = foundation.open_connection(true).map_err(|error| {
        CreateLlmApiKeyError::Internal(format!("failed to open database: {error}"))
    })?;
    ensure_identity_tables(&connection).map_err(|error| {
        CreateLlmApiKeyError::Internal(format!("failed to ensure identity schema: {error}"))
    })?;

    let generated_key: String = connection
        .query_row("SELECT 'ah-' || lower(hex(randomblob(32)))", [], |row| row.get(0))
        .map_err(|error| {
            CreateLlmApiKeyError::Internal(format!("failed to generate api key: {error}"))
        })?;
    let scopes = scope_strings(LLM_API_KEY_SCOPES);
    let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES).map_err(|error| {
        CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}"))
    })?;

    connection
        .execute(
            "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
             VALUES (?1, ?2, ?3, ?4, 'user', 'enabled', ?5, '{}', 0)",
            (
                owner_record.user_id,
                owner_api_key.project.id,
                generated_key.clone(),
                trimmed_name,
                scopes_json,
            ),
        )
        .map_err(|error| {
            CreateLlmApiKeyError::Internal(format!("failed to create api key: {error}"))
        })?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
    })
}
