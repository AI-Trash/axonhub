use axonhub_http::{
    ContextResolveError, ProjectContext, RequestContextPort, ThreadContext, TraceContext,
};

#[cfg(test)]
use std::sync::Arc;

use super::{
    ports::RequestContextRepository,
    repositories::request_context::{
        validate_trace_thread_association, SeaOrmTraceContextRepository, TraceContextRepository,
    },
    request_context::{normalize_context_key, thread_belongs_to_project},
    seaorm::SeaOrmConnectionFactory,
};

#[cfg(test)]
use super::{
    identity_service::SeaOrmIdentityService,
    request_context::sqlite_test_support::TraceContextStore,
    system::sqlite_test_support::SqliteFoundation,
};

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct RequestContextService {
    identity_auth: SeaOrmIdentityService,
    trace_contexts: TraceContextStore,
}

#[cfg(test)]
impl RequestContextService {
    pub fn new(identity_auth: SeaOrmIdentityService, trace_contexts: TraceContextStore) -> Self {
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

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    pub(crate) use super::SqliteRequestContextService;
}

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
        let thread_id = normalize_context_key(thread_id);
        if let Some(existing) = self.repository.query_thread(&thread_id)? {
            if thread_belongs_to_project(&existing, project_id) {
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
        let trace_id = normalize_context_key(trace_id);
        let thread = match thread_db_id {
            Some(thread_db_id) => Some(
                self.repository
                    .query_thread_by_db_id(thread_db_id)?
                    .ok_or(ContextResolveError::Internal)?,
            ),
            None => None,
        };

        if let Some(existing) = self.repository.query_trace(&trace_id)? {
            validate_trace_thread_association(
                project_id,
                thread.as_ref(),
                Some(&existing),
                thread_db_id,
            )?;
            return Ok(Some(existing));
        }

        validate_trace_thread_association(project_id, thread.as_ref(), None, thread_db_id)?;

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
