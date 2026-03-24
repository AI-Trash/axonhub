use std::sync::Arc;

use axonhub_http::{
    AdminCapability, AdminGraphqlCapability, IdentityCapability, OpenAiV1Capability,
    OpenApiGraphqlCapability, ProviderEdgeAdminCapability, RequestContextCapability,
    SystemBootstrapCapability,
};

use crate::foundation::{
    admin::{PostgresAdminService, SqliteAdminService},
    graphql::{
        PostgresAdminGraphqlService, PostgresOpenApiGraphqlService, SqliteAdminGraphqlService,
        SqliteOpenApiGraphqlService,
    },
    identity_service::{PostgresIdentityService, SqliteIdentityService},
    openai_v1::{PostgresOpenAiV1Service, SqliteOpenAiV1Service},
    provider_edge::SqliteProviderEdgeAdminService,
    request_context_service::{PostgresRequestContextService, SqliteRequestContextService},
    shared::SqliteFoundation,
    system::{PostgresBootstrapService, SqliteBootstrapService},
};

const SQLITE_AND_POSTGRES_DIALECT_HINT: &str =
    "Rust replacement for this surface is currently supported only on sqlite3 and postgres.";

fn sqlite_and_postgres_message(surface: &str) -> String {
    format!(
        "{surface} is not available for the configured dialect yet. {SQLITE_AND_POSTGRES_DIALECT_HINT}"
    )
}

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

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        return SystemBootstrapCapability::Available {
            system: Arc::new(PostgresBootstrapService::new(
                dsn.to_owned(),
                version.to_owned(),
            )),
        };
    }

    SystemBootstrapCapability::Unsupported {
        message: sqlite_and_postgres_message("DB-backed admin system status/bootstrap"),
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

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        return IdentityCapability::Available {
            identity: Arc::new(PostgresIdentityService::new(dsn.to_owned(), allow_no_auth)),
        };
    }

    IdentityCapability::Unsupported {
        message: sqlite_and_postgres_message("DB-backed identity auth"),
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

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        let _ = allow_no_auth;
        return RequestContextCapability::Available {
            request_context: Arc::new(PostgresRequestContextService::new(dsn.to_owned())),
        };
    }

    RequestContextCapability::Unsupported {
        message: sqlite_and_postgres_message("DB-backed request context resolution"),
    }
}

pub(crate) fn build_openai_v1_capability(dialect: &str, dsn: &str) -> OpenAiV1Capability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return OpenAiV1Capability::Available {
            openai: Arc::new(SqliteOpenAiV1Service::new(foundation)),
        };
    }

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        return OpenAiV1Capability::Available {
            openai: Arc::new(PostgresOpenAiV1Service::new(dsn.to_owned())),
        };
    }

    OpenAiV1Capability::Unsupported {
        message: sqlite_and_postgres_message("OpenAI `/v1` inference"),
    }
}

pub(crate) fn build_admin_capability(dialect: &str, dsn: &str) -> AdminCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AdminCapability::Available {
            admin: Arc::new(SqliteAdminService::new(foundation)),
        };
    }

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        return AdminCapability::Available {
            admin: Arc::new(PostgresAdminService::new(dsn.to_owned())),
        };
    }

    AdminCapability::Unsupported {
        message: sqlite_and_postgres_message("DB-backed admin read routes"),
    }
}

pub(crate) fn build_admin_graphql_capability(dialect: &str, dsn: &str) -> AdminGraphqlCapability {
    if dialect.eq_ignore_ascii_case("sqlite3") {
        let foundation = Arc::new(SqliteFoundation::new(dsn.to_owned()));
        return AdminGraphqlCapability::Available {
            graphql: Arc::new(SqliteAdminGraphqlService::new(foundation)),
        };
    }

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        return AdminGraphqlCapability::Available {
            graphql: Arc::new(PostgresAdminGraphqlService::new(dsn.to_owned())),
        };
    }

    AdminGraphqlCapability::Unsupported {
        message: sqlite_and_postgres_message("DB-backed admin GraphQL"),
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

    if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
        return OpenApiGraphqlCapability::Available {
            graphql: Arc::new(PostgresOpenApiGraphqlService::new(dsn.to_owned())),
        };
    }

    OpenApiGraphqlCapability::Unsupported {
        message: sqlite_and_postgres_message("DB-backed OpenAPI GraphQL"),
    }
}

pub(crate) fn build_provider_edge_admin_capability(
    _dialect: &str,
    _dsn: &str,
) -> ProviderEdgeAdminCapability {
    if let Some(provider_edge) = SqliteProviderEdgeAdminService::from_env() {
        return ProviderEdgeAdminCapability::Available {
            provider_edge: Arc::new(provider_edge),
        };
    }

    ProviderEdgeAdminCapability::Unsupported {
        message: "Provider-edge admin OAuth helpers are unavailable until secure runtime configuration is present. Set all required AXONHUB_PROVIDER_EDGE_* environment variables to enable these routes."
            .to_owned(),
    }
}
