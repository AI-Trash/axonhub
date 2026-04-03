use axonhub_db_entity::traces;
use axonhub_http::TraceContext;
#[cfg(test)]
use axonhub_http::{ContextResolveError, ProjectContext, RequestContextPort, ThreadContext};
use rusqlite::{
    params, params_from_iter, Connection as SqlConnection, Error as SqlError, OptionalExtension,
    Result as SqlResult,
};
use sea_orm::{
    ColumnTrait, DatabaseBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
};

use super::sqlite_support::{ensure_trace_tables, SqliteConnectionFactory};
#[cfg(test)]
use super::{
    identity_service::IdentityAuthService,
    ports::RequestContextRepository,
    repositories::request_context::validate_trace_thread_association,
    request_context::{normalize_context_key, thread_belongs_to_project},
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
        let statement_definition = list_traces_by_project_query_statement(project_id);
        let mut statement = connection.prepare(statement_definition.sql.as_str())?;
        let rows = statement.query_map(
            params_from_iter(rusqlite_values(&statement_definition)?),
            |row| {
                Ok(TraceContext {
                    id: row.get(0)?,
                    trace_id: row.get(1)?,
                    project_id: row.get(2)?,
                    thread_id: row.get(3)?,
                })
            },
        )?;
        rows.collect()
    }
}

fn list_traces_by_project_query_statement(project_id: i64) -> sea_orm::Statement {
    traces::Entity::find()
        .filter(traces::Column::ProjectId.eq(project_id))
        .select_only()
        .column(traces::Column::Id)
        .column(traces::Column::TraceId)
        .column(traces::Column::ProjectId)
        .column(traces::Column::ThreadId)
        .order_by_desc(traces::Column::Id)
        .build(DatabaseBackend::Sqlite)
}

fn rusqlite_values(statement: &sea_orm::Statement) -> SqlResult<Vec<rusqlite::types::Value>> {
    statement
        .values
        .as_ref()
        .map(|values| {
            values
                .0
                .iter()
                .map(sea_value_to_rusqlite)
                .collect::<SqlResult<Vec<_>>>()
        })
        .transpose()
        .map(|values| values.unwrap_or_default())
}

fn sea_value_to_rusqlite(value: &sea_orm::Value) -> SqlResult<rusqlite::types::Value> {
    use sea_orm::Value;

    match value {
        Value::Bool(Some(inner)) => Ok((*inner as i64).into()),
        Value::TinyInt(Some(inner)) => Ok(i64::from(*inner).into()),
        Value::SmallInt(Some(inner)) => Ok(i64::from(*inner).into()),
        Value::Int(Some(inner)) => Ok(i64::from(*inner).into()),
        Value::BigInt(Some(inner)) => Ok((*inner).into()),
        Value::TinyUnsigned(Some(inner)) => Ok(i64::from(*inner).into()),
        Value::SmallUnsigned(Some(inner)) => Ok(i64::from(*inner).into()),
        Value::Unsigned(Some(inner)) => Ok(i64::from(*inner).into()),
        Value::BigUnsigned(Some(inner)) => i64::try_from(*inner)
            .map(Into::into)
            .map_err(|error| SqlError::ToSqlConversionFailure(Box::new(error))),
        Value::Float(Some(inner)) => Ok(f64::from(*inner).into()),
        Value::Double(Some(inner)) => Ok((*inner).into()),
        Value::String(Some(inner)) => Ok((**inner).clone().into()),
        Value::Char(Some(inner)) => Ok(inner.to_string().into()),
        Value::Bytes(Some(inner)) => Ok((**inner).clone().into()),
        Value::Bool(None)
        | Value::TinyInt(None)
        | Value::SmallInt(None)
        | Value::Int(None)
        | Value::BigInt(None)
        | Value::TinyUnsigned(None)
        | Value::SmallUnsigned(None)
        | Value::Unsigned(None)
        | Value::BigUnsigned(None)
        | Value::Float(None)
        | Value::Double(None)
        | Value::String(None)
        | Value::Char(None)
        | Value::Bytes(None) => Ok(rusqlite::types::Value::Null),
        _ => Err(SqlError::ToSqlConversionFailure(Box::new(
            std::io::Error::other(format!("unsupported SeaORM sqlite value: {value:?}")),
        ))),
    }
}

#[cfg(test)]
pub(crate) fn get_or_create_thread(
    connection: &SqlConnection,
    project_id: i64,
    thread_id: &str,
) -> SqlResult<ThreadContext> {
    let thread_id = normalize_context_key(thread_id);
    let existing = connection
        .query_row(
            "SELECT id, thread_id, project_id FROM threads WHERE thread_id = ?1 LIMIT 1",
            [thread_id.as_str()],
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
        if thread_belongs_to_project(&thread, project_id) {
            return Ok(thread);
        }
        return Err(SqlError::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO threads (project_id, thread_id) VALUES (?1, ?2)",
        params![project_id, thread_id.as_str()],
    )?;

    Ok(ThreadContext {
        id: connection.last_insert_rowid(),
        thread_id,
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
    let trace_id = normalize_context_key(trace_id);
    let thread = thread_db_id
        .map(|thread_db_id| {
            connection
                .query_row(
                    "SELECT id, thread_id, project_id FROM threads WHERE id = ?1 LIMIT 1",
                    [thread_db_id],
                    |row| {
                        Ok(ThreadContext {
                            id: row.get(0)?,
                            thread_id: row.get(1)?,
                            project_id: row.get(2)?,
                        })
                    },
                )
                .optional()
        })
        .transpose()?
        .flatten();

    if thread_db_id.is_some() && thread.as_ref().is_none() {
        return Err(SqlError::InvalidQuery);
    }

    let existing = connection
        .query_row(
            "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = ?1 LIMIT 1",
            [trace_id.as_str()],
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
        validate_trace_thread_association(project_id, thread.as_ref(), Some(&trace), thread_db_id)
            .map_err(|_| SqlError::InvalidQuery)?;
        return Ok(trace);
    }

    validate_trace_thread_association(project_id, thread.as_ref(), None, thread_db_id)
        .map_err(|_| SqlError::InvalidQuery)?;

    connection.execute(
        "INSERT INTO traces (project_id, trace_id, thread_id) VALUES (?1, ?2, ?3)",
        params![project_id, trace_id.as_str(), thread_db_id],
    )?;

    Ok(TraceContext {
        id: connection.last_insert_rowid(),
        trace_id,
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
            .get_or_create_thread(project_id, thread_id)
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
            .get_or_create_trace(project_id, trace_id, thread_db_id)
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
