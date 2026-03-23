use axonhub_http::{
    ContextResolveError, ProjectContext, RequestContextPort, ThreadContext, TraceContext,
};

use super::{
    identity_service::IdentityAuthService, request_context::TraceContextStore,
    shared::SqliteFoundation,
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
