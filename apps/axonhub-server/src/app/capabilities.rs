use std::sync::Arc;

use axonhub_http::{
    AdminCapability, AdminGraphqlCapability, IdentityCapability, OpenAiV1Capability,
    OpenApiGraphqlCapability, ProviderEdgeAdminCapability, RequestContextCapability,
    SystemBootstrapCapability,
};

use crate::foundation::{
    admin::SqliteAdminService,
    graphql::{SqliteAdminGraphqlService, SqliteOpenApiGraphqlService},
    identity_service::SqliteIdentityService,
    openai_v1::SqliteOpenAiV1Service,
    provider_edge::SqliteProviderEdgeAdminService,
    request_context_service::SqliteRequestContextService,
    shared::SqliteFoundation,
    system::SqliteBootstrapService,
};

const SYSTEM_BOOTSTRAP_UNSUPPORTED_MESSAGE: &str =
    "DB-backed admin system status/bootstrap is not available for the configured dialect yet. Use the legacy Go backend for this route.";

pub(crate) fn build_system_bootstrap_capability(
    dialect: &str,
    dsn: &str,
    version: &str,
) -> SystemBootstrapCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return SystemBootstrapCapability::Available {
            system: Arc::new(SqliteBootstrapService::new(foundation, version.to_owned())),
        };
    }

    SystemBootstrapCapability::Unsupported {
        message: SYSTEM_BOOTSTRAP_UNSUPPORTED_MESSAGE.to_owned(),
    }
}

pub(crate) fn build_identity_capability(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
) -> IdentityCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return IdentityCapability::Available {
            identity: Arc::new(SqliteIdentityService::new(foundation, allow_no_auth)),
        };
    }

    IdentityCapability::Unsupported {
        message: "DB-backed identity auth is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

pub(crate) fn build_request_context_capability(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
) -> RequestContextCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return RequestContextCapability::Available {
            request_context: Arc::new(SqliteRequestContextService::new(foundation, allow_no_auth)),
        };
    }

    RequestContextCapability::Unsupported {
        message: "DB-backed request context resolution is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

pub(crate) fn build_openai_v1_capability(dialect: &str, dsn: &str) -> OpenAiV1Capability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return OpenAiV1Capability::Available {
            openai: Arc::new(SqliteOpenAiV1Service::new(foundation)),
        };
    }

    OpenAiV1Capability::Unsupported {
        message: "OpenAI `/v1` inference is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

pub(crate) fn build_admin_capability(dialect: &str, dsn: &str) -> AdminCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AdminCapability::Available {
            admin: Arc::new(SqliteAdminService::new(foundation)),
        };
    }

    AdminCapability::Unsupported {
        message: "DB-backed admin read routes are not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

pub(crate) fn build_admin_graphql_capability(dialect: &str, dsn: &str) -> AdminGraphqlCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AdminGraphqlCapability::Available {
            graphql: Arc::new(SqliteAdminGraphqlService::new(foundation)),
        };
    }

    AdminGraphqlCapability::Unsupported {
        message: "DB-backed admin GraphQL is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

pub(crate) fn build_openapi_graphql_capability(
    dialect: &str,
    dsn: &str,
) -> OpenApiGraphqlCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return OpenApiGraphqlCapability::Available {
            graphql: Arc::new(SqliteOpenApiGraphqlService::new(foundation)),
        };
    }

    OpenApiGraphqlCapability::Unsupported {
        message: "DB-backed OpenAPI GraphQL is not available for the configured dialect yet. Use the legacy Go backend for this route.".to_owned(),
    }
}

pub(crate) fn build_provider_edge_admin_capability(
    dialect: &str,
    _dsn: &str,
) -> ProviderEdgeAdminCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        return ProviderEdgeAdminCapability::Available {
            provider_edge: Arc::new(SqliteProviderEdgeAdminService::new()),
        };
    }

    ProviderEdgeAdminCapability::Unsupported {
        message: "Provider-edge admin OAuth helpers are not available for the configured dialect yet. Use the legacy Go backend for these routes.".to_owned(),
    }
}
