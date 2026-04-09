use axonhub_db_entity::{projects, threads, traces};
use axonhub_http::{ContextResolveError, ProjectContext, ThreadContext, TraceContext};
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use crate::foundation::request_context::trace_matches_project_and_thread;
use crate::foundation::seaorm::SeaOrmConnectionFactory;

pub(crate) trait TraceContextRepository: Send + Sync {
    fn resolve_project(&self, project_id: i64) -> Result<Option<ProjectContext>, ContextResolveError>;
    fn query_thread(&self, thread_id: &str) -> Result<Option<ThreadContext>, ContextResolveError>;
    fn insert_thread(&self, project_id: i64, thread_id: &str) -> Result<i64, ContextResolveError>;
    fn query_trace(&self, trace_id: &str) -> Result<Option<TraceContext>, ContextResolveError>;
    fn query_thread_by_db_id(
        &self,
        thread_db_id: i64,
    ) -> Result<Option<ThreadContext>, ContextResolveError>;
    fn insert_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<i64, ContextResolveError>;
}

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmTraceContextRepository {
    db: SeaOrmConnectionFactory,
}

impl SeaOrmTraceContextRepository {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl TraceContextRepository for SeaOrmTraceContextRepository {
    fn resolve_project(&self, project_id: i64) -> Result<Option<ProjectContext>, ContextResolveError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            projects::Entity::find_by_id(project_id)
                .filter(projects::Column::DeletedAt.eq(0_i64))
                .into_partial_model::<projects::ContextSummary>()
                .one(&connection)
                .await
                .map_err(|_| ContextResolveError::Internal)
                .map(|project| {
                    project.filter(|project| project.status == "active").map(|project| ProjectContext {
                        id: project.id,
                        name: project.name,
                        status: project.status,
                    })
                })
        })
    }

    fn query_thread(&self, thread_id: &str) -> Result<Option<ThreadContext>, ContextResolveError> {
        let db = self.db.clone();
        let thread_id = thread_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            query_thread_seaorm(&connection, &thread_id)
                .await
                .map_err(|_| ContextResolveError::Internal)
        })
    }

    fn insert_thread(&self, project_id: i64, thread_id: &str) -> Result<i64, ContextResolveError> {
        let db = self.db.clone();
        let thread_id = thread_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            insert_thread_seaorm(&connection, project_id, &thread_id)
                .await
                .map_err(|_| ContextResolveError::Internal)
        })
    }

    fn query_trace(&self, trace_id: &str) -> Result<Option<TraceContext>, ContextResolveError> {
        let db = self.db.clone();
        let trace_id = trace_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            query_trace_seaorm(&connection, &trace_id)
                .await
                .map_err(|_| ContextResolveError::Internal)
        })
    }

    fn insert_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<i64, ContextResolveError> {
        let db = self.db.clone();
        let trace_id = trace_id.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            insert_trace_seaorm(&connection, project_id, &trace_id, thread_db_id)
                .await
                .map_err(|_| ContextResolveError::Internal)
        })
    }

    fn query_thread_by_db_id(
        &self,
        thread_db_id: i64,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            query_thread_by_db_id_seaorm(&connection, thread_db_id)
                .await
                .map_err(|_| ContextResolveError::Internal)
        })
    }
}

async fn query_thread_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    thread_id: &str,
) -> Result<Option<ThreadContext>, sea_orm::DbErr> {
    threads::Entity::find()
        .filter(threads::Column::ThreadId.eq(thread_id))
        .into_partial_model::<threads::ResolveContext>()
        .one(db)
        .await
        .map(|thread| {
            thread.map(|thread| ThreadContext {
                id: thread.id,
                thread_id: thread.thread_id,
                project_id: thread.project_id,
            })
        })
}

async fn insert_thread_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    project_id: i64,
    thread_id: &str,
) -> Result<i64, sea_orm::DbErr> {
    let inserted = threads::Entity::insert(threads::ActiveModel {
        project_id: Set(project_id),
        thread_id: Set(thread_id.to_owned()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(inserted.last_insert_id)
}

async fn query_thread_by_db_id_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    thread_db_id: i64,
) -> Result<Option<ThreadContext>, sea_orm::DbErr> {
    threads::Entity::find_by_id(thread_db_id)
        .into_partial_model::<threads::ResolveContext>()
        .one(db)
        .await
        .map(|thread| {
            thread.map(|thread| ThreadContext {
                id: thread.id,
                thread_id: thread.thread_id,
                project_id: thread.project_id,
            })
        })
}

async fn query_trace_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    trace_id: &str,
) -> Result<Option<TraceContext>, sea_orm::DbErr> {
    traces::Entity::find()
        .filter(traces::Column::TraceId.eq(trace_id))
        .into_partial_model::<traces::ResolveContext>()
        .one(db)
        .await
        .map(|trace| {
            trace.map(|trace| TraceContext {
                id: trace.id,
                trace_id: trace.trace_id,
                project_id: trace.project_id,
                thread_id: trace.thread_id,
            })
        })
}

pub(crate) fn validate_trace_thread_association(
    project_id: i64,
    thread: Option<&ThreadContext>,
    trace: Option<&TraceContext>,
    requested_thread_db_id: Option<i64>,
) -> Result<(), ContextResolveError> {
    if let Some(thread) = thread {
        if thread.project_id != project_id {
            return Err(ContextResolveError::Internal);
        }
    }

    if let Some(trace) = trace {
        if !trace_matches_project_and_thread(trace, project_id, requested_thread_db_id) {
            return Err(ContextResolveError::Internal);
        }
    }

    Ok(())
}

async fn insert_trace_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    project_id: i64,
    trace_id: &str,
    thread_db_id: Option<i64>,
) -> Result<i64, sea_orm::DbErr> {
    let inserted = traces::Entity::insert(traces::ActiveModel {
        project_id: Set(project_id),
        trace_id: Set(trace_id.to_owned()),
        thread_id: Set(thread_db_id),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(inserted.last_insert_id)
}

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use axonhub_db_entity::traces;
    use axonhub_http::{ThreadContext, TraceContext};
    use rusqlite::{
        params, params_from_iter, Connection as SqlConnection, Error as SqlError,
        OptionalExtension, Result as SqlResult,
    };
    use sea_orm::{
        ColumnTrait, DatabaseBackend, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
        QueryTrait,
    };

    use crate::foundation::{
        request_context::{normalize_context_key, thread_belongs_to_project},
        system::sqlite_test_support::{ensure_trace_tables, SqliteConnectionFactory},
    };

    use super::validate_trace_thread_association;

    #[derive(Debug, Clone)]
    pub struct TraceContextStore {
        connection_factory: SqliteConnectionFactory,
    }

    impl TraceContextStore {
        pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
            Self { connection_factory }
        }

        pub fn ensure_schema(&self) -> SqlResult<()> {
            let connection = self.connection_factory.open(true)?;
            ensure_trace_tables(&connection)
        }

        pub fn get_or_create_thread(
            &self,
            project_id: i64,
            thread_id: &str,
        ) -> SqlResult<ThreadContext> {
            let connection = self.connection_factory.open(true)?;
            ensure_trace_tables(&connection)?;
            get_or_create_thread(&connection, project_id, thread_id)
        }

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
            Value::String(Some(inner)) => Ok(inner.to_string().into()),
            Value::Char(Some(inner)) => Ok(inner.to_string().into()),
            Value::Bytes(Some(inner)) => Ok(inner.to_vec().into()),
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
}
