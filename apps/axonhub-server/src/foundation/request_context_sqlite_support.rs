use axonhub_http::TraceContext;
#[cfg(test)]
use axonhub_http::{ContextResolveError, ProjectContext, RequestContextPort, ThreadContext};
#[cfg(test)]
use rusqlite::{
    params, Connection as SqlConnection, Error as SqlError, OptionalExtension, Result as SqlResult,
};

use super::sqlite_support::{ensure_trace_tables, SqliteConnectionFactory};
#[cfg(test)]
use super::{
    identity_service::IdentityAuthService, ports::RequestContextRepository,
    sqlite_support::SqliteFoundation,
};
#[cfg(test)]
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TraceContextStore {
    connection_factory: SqliteConnectionFactory,
}

impl TraceContextStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)
    }

    #[cfg(test)]
    pub fn get_or_create_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> SqlResult<ThreadContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        get_or_create_thread(&connection, project_id, thread_id)
    }

    #[cfg(test)]
    pub fn get_or_create_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> SqlResult<TraceContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        get_or_create_trace(&connection, project_id, trace_id, thread_db_id)
    }

    pub fn list_traces_by_project(&self, project_id: i64) -> SqlResult<Vec<TraceContext>> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, trace_id, project_id, thread_id
             FROM traces
             WHERE project_id = ?1
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(TraceContext {
                id: row.get(0)?,
                trace_id: row.get(1)?,
                project_id: row.get(2)?,
                thread_id: row.get(3)?,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
pub(crate) fn get_or_create_thread(
    connection: &SqlConnection,
    project_id: i64,
    thread_id: &str,
) -> SqlResult<ThreadContext> {
    let existing = connection
        .query_row(
            "SELECT id, thread_id, project_id FROM threads WHERE thread_id = ?1 LIMIT 1",
            [thread_id],
            |row| {
                Ok(ThreadContext {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    project_id: row.get(2)?,
                })
            },
        )
        .optional()?;

    if let Some(thread) = existing {
        if thread.project_id == project_id {
            return Ok(thread);
        }
        return Err(SqlError::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO threads (project_id, thread_id) VALUES (?1, ?2)",
        params![project_id, thread_id],
    )?;

    Ok(ThreadContext {
        id: connection.last_insert_rowid(),
        thread_id: thread_id.to_owned(),
        project_id,
    })
}

#[cfg(test)]
pub(crate) fn get_or_create_trace(
    connection: &SqlConnection,
    project_id: i64,
    trace_id: &str,
    thread_db_id: Option<i64>,
) -> SqlResult<TraceContext> {
    let existing = connection
        .query_row(
            "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = ?1 LIMIT 1",
            [trace_id],
            |row| {
                Ok(TraceContext {
                    id: row.get(0)?,
                    trace_id: row.get(1)?,
                    project_id: row.get(2)?,
                    thread_id: row.get(3)?,
                })
            },
        )
        .optional()?;

    if let Some(trace) = existing {
        if trace.project_id == project_id
            && (thread_db_id.is_none() || trace.thread_id == thread_db_id)
        {
            return Ok(trace);
        }
        return Err(SqlError::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO traces (project_id, trace_id, thread_id) VALUES (?1, ?2, ?3)",
        params![project_id, trace_id, thread_db_id],
    )?;

    Ok(TraceContext {
        id: connection.last_insert_rowid(),
        trace_id: trace_id.to_owned(),
        project_id,
        thread_id: thread_db_id,
    })
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct RequestContextService {
    identity_auth: IdentityAuthService,
    trace_contexts: TraceContextStore,
}

#[cfg(test)]
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

#[cfg(test)]
pub struct SqliteRequestContextService {
    request_contexts: RequestContextService,
}

#[cfg(test)]
impl SqliteRequestContextService {
    pub fn new(foundation: Arc<SqliteFoundation>, allow_no_auth: bool) -> Self {
        Self {
            request_contexts: foundation.request_context_service(allow_no_auth),
        }
    }
}

#[cfg(test)]
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

#[cfg(test)]
impl RequestContextRepository for SqliteRequestContextService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        <Self as RequestContextPort>::resolve_project(self, project_id)
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        <Self as RequestContextPort>::resolve_thread(self, project_id, thread_id)
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        <Self as RequestContextPort>::resolve_trace(self, project_id, trace_id, thread_db_id)
    }
}
