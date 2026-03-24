use axonhub_http::{
    ContextResolveError, ProjectContext, RequestContextPort, ThreadContext, TraceContext,
};
use postgres::{Client as PostgresClient, NoTls};

use super::{
    identity_service::{query_project_postgres, IdentityAuthService},
    request_context::TraceContextStore,
    shared::SqliteFoundation,
    system::{ensure_identity_tables_postgres, ensure_trace_tables_postgres},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RequestContextService {
    identity_auth: IdentityAuthService,
    trace_contexts: TraceContextStore,
}

impl RequestContextService {
    pub fn new(identity_auth: IdentityAuthService, trace_contexts: TraceContextStore) -> Self {
        Self {
            identity_auth,
            trace_contexts,
        }
    }

    pub fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        self.identity_auth.resolve_project(project_id)
    }

    pub fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        self.trace_contexts
            .get_or_create_thread(project_id, thread_id.trim())
            .map(Some)
            .map_err(|_| ContextResolveError::Internal)
    }

    pub fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        self.trace_contexts
            .get_or_create_trace(project_id, trace_id.trim(), thread_db_id)
            .map(Some)
            .map_err(|_| ContextResolveError::Internal)
    }
}

pub struct SqliteRequestContextService {
    request_contexts: RequestContextService,
}

impl SqliteRequestContextService {
    pub fn new(foundation: Arc<SqliteFoundation>, allow_no_auth: bool) -> Self {
        Self {
            request_contexts: foundation.request_context_service(allow_no_auth),
        }
    }
}

impl RequestContextPort for SqliteRequestContextService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        self.request_contexts.resolve_project(project_id)
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        self.request_contexts.resolve_thread(project_id, thread_id)
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        self.request_contexts
            .resolve_trace(project_id, trace_id, thread_db_id)
    }
}

pub struct PostgresRequestContextService {
    dsn: String,
}

impl PostgresRequestContextService {
    pub fn new(dsn: impl Into<String>) -> Self {
        Self { dsn: dsn.into() }
    }

    fn run_blocking<T, F>(&self, operation: F) -> Result<T, ContextResolveError>
    where
        T: Send + 'static,
        F: FnOnce(String) -> Result<T, ContextResolveError> + Send + 'static,
    {
        let dsn = self.dsn.clone();

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(move || operation(dsn))
                .join()
                .unwrap_or_else(|_| panic!("postgres request-context worker thread panicked"))
        } else {
            operation(dsn)
        }
    }

    fn connect(dsn: &str) -> Result<PostgresClient, ContextResolveError> {
        let mut client =
            PostgresClient::connect(dsn, NoTls).map_err(|_| ContextResolveError::Internal)?;
        ensure_identity_tables_postgres(&mut client).map_err(|_| ContextResolveError::Internal)?;
        ensure_trace_tables_postgres(&mut client).map_err(|_| ContextResolveError::Internal)?;
        Ok(client)
    }
}

impl RequestContextPort for PostgresRequestContextService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        self.run_blocking(move |dsn| {
            let mut client = Self::connect(&dsn)?;
            match query_project_postgres(&mut client, project_id) {
                Ok(project) if project.status == "active" => Ok(Some(ProjectContext {
                    id: project.id,
                    name: project.name,
                    status: project.status,
                })),
                Ok(_) => Ok(None),
                Err(axonhub_http::ApiKeyAuthError::Invalid) => Ok(None),
                Err(
                    axonhub_http::ApiKeyAuthError::Missing
                    | axonhub_http::ApiKeyAuthError::Internal,
                ) => Err(ContextResolveError::Internal),
            }
        })
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        let thread_id = thread_id.trim().to_owned();

        self.run_blocking(move |dsn| {
            let mut client = Self::connect(&dsn)?;
            if let Some(row) = client
                .query_opt(
                    "SELECT id, thread_id, project_id FROM threads WHERE thread_id = $1 LIMIT 1",
                    &[&thread_id],
                )
                .map_err(|_| ContextResolveError::Internal)?
            {
                let existing = ThreadContext {
                    id: row.get(0),
                    thread_id: row.get(1),
                    project_id: row.get(2),
                };
                if existing.project_id == project_id {
                    return Ok(Some(existing));
                }
                return Err(ContextResolveError::Internal);
            }

            let insert_params: [&(dyn postgres::types::ToSql + Sync); 2] =
                [&project_id, &thread_id];
            let row = client
                .query_one(
                    "INSERT INTO threads (project_id, thread_id) VALUES ($1, $2) RETURNING id",
                    &insert_params,
                )
                .map_err(|_| ContextResolveError::Internal)?;

            Ok(Some(ThreadContext {
                id: row.get(0),
                thread_id,
                project_id,
            }))
        })
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        let trace_id = trace_id.trim().to_owned();

        self.run_blocking(move |dsn| {
            let mut client = Self::connect(&dsn)?;
            if let Some(row) = client
                .query_opt(
                    "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = $1 LIMIT 1",
                    &[&trace_id],
                )
                .map_err(|_| ContextResolveError::Internal)?
            {
                let existing = TraceContext {
                    id: row.get(0),
                    trace_id: row.get(1),
                    project_id: row.get(2),
                    thread_id: row.get(3),
                };
                if existing.project_id == project_id
                    && (thread_db_id.is_none() || existing.thread_id == thread_db_id)
                {
                    return Ok(Some(existing));
                }
                return Err(ContextResolveError::Internal);
            }

            let insert_params: [&(dyn postgres::types::ToSql + Sync); 3] =
                [&project_id, &trace_id, &thread_db_id];
            let row = client
                .query_one(
                    "INSERT INTO traces (project_id, trace_id, thread_id) VALUES ($1, $2, $3) RETURNING id",
                    &insert_params,
                )
                .map_err(|_| ContextResolveError::Internal)?;

            Ok(Some(TraceContext {
                id: row.get(0),
                trace_id,
                project_id,
                thread_id: thread_db_id,
            }))
        })
    }
}
