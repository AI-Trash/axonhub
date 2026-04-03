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
