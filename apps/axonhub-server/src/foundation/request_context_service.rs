use axonhub_http::{
    ContextResolveError, ProjectContext, RequestContextPort, ThreadContext, TraceContext,
};

#[cfg(test)]
pub(crate) use super::request_context_sqlite_support::SqliteRequestContextService;
use super::{
    ports::RequestContextRepository,
    repositories::request_context::{SeaOrmTraceContextRepository, TraceContextRepository},
    seaorm::SeaOrmConnectionFactory,
};

pub struct SeaOrmRequestContextService {
    repository: SeaOrmTraceContextRepository,
}

impl SeaOrmRequestContextService {
    pub fn new(db: SeaOrmConnectionFactory, _allow_no_auth: bool) -> Self {
        Self {
            repository: SeaOrmTraceContextRepository::new(db),
        }
    }
}

impl RequestContextPort for SeaOrmRequestContextService {
    fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        self.repository.resolve_project(project_id)
    }

    fn resolve_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> Result<Option<ThreadContext>, ContextResolveError> {
        let thread_id = thread_id.trim().to_owned();
        if let Some(existing) = self.repository.query_thread(&thread_id)? {
            if existing.project_id == project_id {
                return Ok(Some(existing));
            }
            return Err(ContextResolveError::Internal);
        }

        let id = self.repository.insert_thread(project_id, &thread_id)?;
        Ok(Some(ThreadContext {
            id,
            thread_id,
            project_id,
        }))
    }

    fn resolve_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> Result<Option<TraceContext>, ContextResolveError> {
        let trace_id = trace_id.trim().to_owned();
        if let Some(existing) = self.repository.query_trace(&trace_id)? {
            if existing.project_id == project_id
                && (thread_db_id.is_none() || existing.thread_id == thread_db_id)
            {
                return Ok(Some(existing));
            }
            return Err(ContextResolveError::Internal);
        }

        let id = self
            .repository
            .insert_trace(project_id, &trace_id, thread_db_id)?;
        Ok(Some(TraceContext {
            id,
            trace_id,
            project_id,
            thread_id: thread_db_id,
        }))
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
