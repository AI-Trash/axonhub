use axonhub_http::{
    ContextResolveError, ProjectContext, RequestContextPort, ThreadContext, TraceContext,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, ExecResult, Statement};

use super::{
    identity_service::query_project_seaorm,
    ports::RequestContextRepository,
    seaorm::SeaOrmConnectionFactory,
};
#[cfg(test)]
pub(crate) use super::request_context_sqlite_support::SqliteRequestContextService;

pub struct SeaOrmRequestContextService {
    db: SeaOrmConnectionFactory,
}

impl SeaOrmRequestContextService {
    pub fn new(db: SeaOrmConnectionFactory, _allow_no_auth: bool) -> Self {
        Self { db }
    }
}

impl RequestContextPort for SeaOrmRequestContextService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            match query_project_seaorm(&connection, db.backend(), project_id).await {
                Ok(project) if project.status == "active" => Ok(Some(ProjectContext {
                    id: project.id,
                    name: project.name,
                    status: project.status,
                })),
                Ok(_) => Ok(None),
                Err(axonhub_http::ApiKeyAuthError::Invalid) => Ok(None),
                Err(axonhub_http::ApiKeyAuthError::Missing | axonhub_http::ApiKeyAuthError::Internal) => {
                    Err(ContextResolveError::Internal)
                }
            }
        })
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        let db = self.db.clone();
        let thread_id = thread_id.trim().to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            let backend = db.backend();
            if let Some(existing) = query_thread_seaorm(&connection, backend, &thread_id)
                .await
                .map_err(|_| ContextResolveError::Internal)?
            {
                if existing.project_id == project_id {
                    return Ok(Some(existing));
                }
                return Err(ContextResolveError::Internal);
            }

            let id = insert_thread_seaorm(&connection, backend, project_id, &thread_id)
                .await
                .map_err(|_| ContextResolveError::Internal)?;
            Ok(Some(ThreadContext { id, thread_id, project_id }))
        })
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        let db = self.db.clone();
        let trace_id = trace_id.trim().to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ContextResolveError::Internal)?;
            let backend = db.backend();
            if let Some(existing) = query_trace_seaorm(&connection, backend, &trace_id)
                .await
                .map_err(|_| ContextResolveError::Internal)?
            {
                if existing.project_id == project_id
                    && (thread_db_id.is_none() || existing.thread_id == thread_db_id)
                {
                    return Ok(Some(existing));
                }
                return Err(ContextResolveError::Internal);
            }

            let id = insert_trace_seaorm(&connection, backend, project_id, &trace_id, thread_db_id)
                .await
                .map_err(|_| ContextResolveError::Internal)?;
            Ok(Some(TraceContext { id, trace_id, project_id, thread_id: thread_db_id }))
        })
    }
}

impl RequestContextRepository for SeaOrmRequestContextService {
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

fn request_context_sql<'a>(
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

async fn query_one_request_context(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<sea_orm::QueryResult>, sea_orm::DbErr> {
    db.query_one(Statement::from_sql_and_values(
        backend,
        request_context_sql(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
}

fn inserted_id(result: &ExecResult, backend: DatabaseBackend) -> Result<i64, sea_orm::DbErr> {
    let id = result.last_insert_id();
    if id == 0 {
        Err(sea_orm::DbErr::Custom(format!(
            "missing inserted id for request-context {backend:?} operation"
        )))
    } else {
        Ok(id as i64)
    }
}

async fn query_thread_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    thread_id: &str,
) -> Result<Option<ThreadContext>, sea_orm::DbErr> {
    query_one_request_context(
        db,
        backend,
        "SELECT id, thread_id, project_id FROM threads WHERE thread_id = ? LIMIT 1",
        "SELECT id, thread_id, project_id FROM threads WHERE thread_id = $1 LIMIT 1",
        "SELECT id, thread_id, project_id FROM threads WHERE thread_id = ? LIMIT 1",
        vec![thread_id.into()],
    )
    .await?
    .map(|row| {
        Ok(ThreadContext {
            id: row.try_get_by_index(0)?,
            thread_id: row.try_get_by_index(1)?,
            project_id: row.try_get_by_index(2)?,
        })
    })
    .transpose()
}

async fn insert_thread_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    project_id: i64,
    thread_id: &str,
) -> Result<i64, sea_orm::DbErr> {
    match backend {
        DatabaseBackend::Sqlite => {
            let result = db
                .execute(Statement::from_sql_and_values(
                    backend,
                    "INSERT INTO threads (project_id, thread_id) VALUES (?, ?)",
                    vec![project_id.into(), thread_id.into()],
                ))
                .await?;
            inserted_id(&result, backend)
        }
        DatabaseBackend::Postgres => {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    backend,
                    "INSERT INTO threads (project_id, thread_id) VALUES ($1, $2) RETURNING id",
                    vec![project_id.into(), thread_id.into()],
                ))
                .await?
                .ok_or_else(|| sea_orm::DbErr::RecordNotFound("thread insert returning id".to_owned()))?;
            row.try_get_by_index(0)
        }
        DatabaseBackend::MySql => {
            let result = db
                .execute(Statement::from_sql_and_values(
                    backend,
                    "INSERT INTO threads (project_id, thread_id) VALUES (?, ?)",
                    vec![project_id.into(), thread_id.into()],
                ))
                .await?;
            inserted_id(&result, backend)
        }
    }
}

async fn query_trace_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    trace_id: &str,
) -> Result<Option<TraceContext>, sea_orm::DbErr> {
    query_one_request_context(
        db,
        backend,
        "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = ? LIMIT 1",
        "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = $1 LIMIT 1",
        "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = ? LIMIT 1",
        vec![trace_id.into()],
    )
    .await?
    .map(|row| {
        Ok(TraceContext {
            id: row.try_get_by_index(0)?,
            trace_id: row.try_get_by_index(1)?,
            project_id: row.try_get_by_index(2)?,
            thread_id: row.try_get_by_index(3)?,
        })
    })
    .transpose()
}

async fn insert_trace_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    project_id: i64,
    trace_id: &str,
    thread_db_id: Option<i64>,
) -> Result<i64, sea_orm::DbErr> {
    match backend {
        DatabaseBackend::Sqlite => {
            let result = db
                .execute(Statement::from_sql_and_values(
                    backend,
                    "INSERT INTO traces (project_id, trace_id, thread_id) VALUES (?, ?, ?)",
                    vec![project_id.into(), trace_id.into(), thread_db_id.into()],
                ))
                .await?;
            inserted_id(&result, backend)
        }
        DatabaseBackend::Postgres => {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    backend,
                    "INSERT INTO traces (project_id, trace_id, thread_id) VALUES ($1, $2, $3) RETURNING id",
                    vec![project_id.into(), trace_id.into(), thread_db_id.into()],
                ))
                .await?
                .ok_or_else(|| sea_orm::DbErr::RecordNotFound("trace insert returning id".to_owned()))?;
            row.try_get_by_index(0)
        }
        DatabaseBackend::MySql => {
            let result = db
                .execute(Statement::from_sql_and_values(
                    backend,
                    "INSERT INTO traces (project_id, trace_id, thread_id) VALUES (?, ?, ?)",
                    vec![project_id.into(), trace_id.into(), thread_db_id.into()],
                ))
                .await?;
            inserted_id(&result, backend)
        }
    }
}
