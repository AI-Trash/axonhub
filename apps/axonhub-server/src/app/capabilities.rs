use std::sync::Arc;

use axonhub_http::{
    AdminCapability, AdminGraphqlCapability, IdentityCapability, OpenAiV1Capability,
    OpenApiGraphqlCapability, ProviderEdgeAdminCapability, RequestContextCapability,
    SystemBootstrapCapability,
};

use super::services::{
    AdminApplicationService, AdminGraphqlApplicationService, IdentityApplicationService,
    OpenAiV1ApplicationService, OpenApiGraphqlApplicationService, RequestContextApplicationService,
    SystemBootstrapApplicationService,
};
use crate::foundation::{
    admin::SeaOrmAdminService,
    graphql::{SeaOrmAdminGraphqlService, SeaOrmOpenApiGraphqlService},
    identity_service::SeaOrmIdentityService,
    openai_v1::SeaOrmOpenAiV1Service,
    ports::{
        AdminGraphqlRepository, AdminRepository, IdentityRepository, OpenAiV1Repository,
        OpenApiGraphqlRepository, RequestContextRepository, SystemBootstrapRepository,
    },
    provider_edge::SqliteProviderEdgeAdminService,
    request_context_service::SeaOrmRequestContextService,
    seaorm::SeaOrmConnectionFactory,
    system::SeaOrmBootstrapService,
};

const SQLITE_POSTGRES_AND_MYSQL_DIALECT_HINT: &str =
    "Rust replacement for this surface is currently supported only on sqlite3, postgres, and mysql.";

pub(crate) struct ServerCapabilities {
    pub(crate) system_bootstrap: SystemBootstrapCapability,
    pub(crate) identity: IdentityCapability,
    pub(crate) request_context: RequestContextCapability,
    pub(crate) openai_v1: OpenAiV1Capability,
    pub(crate) admin: AdminCapability,
    pub(crate) admin_graphql: AdminGraphqlCapability,
    pub(crate) openapi_graphql: OpenApiGraphqlCapability,
    pub(crate) provider_edge_admin: ProviderEdgeAdminCapability,
}

enum PersistenceProfile {
    Sqlite { db: SeaOrmConnectionFactory },
    Postgres { db: SeaOrmConnectionFactory },
    MySql { db: SeaOrmConnectionFactory },
    Unsupported,
}

impl PersistenceProfile {
    fn resolve(dialect: &str, dsn: &str) -> Self {
        if dialect.eq_ignore_ascii_case("sqlite3") {
            return Self::Sqlite {
                db: SeaOrmConnectionFactory::sqlite(dsn.to_owned()),
            };
        }

        if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
            return Self::Postgres {
                db: SeaOrmConnectionFactory::postgres(dsn.to_owned()),
            };
        }

        if dialect.eq_ignore_ascii_case("mysql") {
            return Self::MySql {
                db: SeaOrmConnectionFactory::mysql(dsn.to_owned()),
            };
        }

        Self::Unsupported
    }
}

fn supported_seaorm_dialect_message(surface: &str) -> String {
    format!(
        "{surface} is not available for the configured dialect yet. {SQLITE_POSTGRES_AND_MYSQL_DIALECT_HINT}"
    )
}

pub(crate) fn build_server_capabilities(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
    version: &str,
) -> ServerCapabilities {
    let profile = PersistenceProfile::resolve(dialect, dsn);

    ServerCapabilities {
        system_bootstrap: build_system_bootstrap_capability_from_profile(&profile, version),
        identity: build_identity_capability_from_profile(&profile, allow_no_auth),
        request_context: build_request_context_capability_from_profile(&profile, allow_no_auth),
        openai_v1: build_openai_v1_capability_from_profile(&profile),
        admin: build_admin_capability_from_profile(&profile),
        admin_graphql: build_admin_graphql_capability_from_profile(&profile),
        openapi_graphql: build_openapi_graphql_capability_from_profile(&profile),
        provider_edge_admin: build_provider_edge_admin_capability(dialect, dsn),
    }
}

pub(crate) fn build_system_bootstrap_capability(
    dialect: &str,
    dsn: &str,
    version: &str,
) -> SystemBootstrapCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_system_bootstrap_capability_from_profile(&profile, version)
}

pub(crate) fn build_identity_capability(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
) -> IdentityCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_identity_capability_from_profile(&profile, allow_no_auth)
}

pub(crate) fn build_request_context_capability(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
) -> RequestContextCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_request_context_capability_from_profile(&profile, allow_no_auth)
}

pub(crate) fn build_openai_v1_capability(dialect: &str, dsn: &str) -> OpenAiV1Capability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_openai_v1_capability_from_profile(&profile)
}

pub(crate) fn build_admin_capability(dialect: &str, dsn: &str) -> AdminCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_admin_capability_from_profile(&profile)
}

pub(crate) fn build_admin_graphql_capability(dialect: &str, dsn: &str) -> AdminGraphqlCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_admin_graphql_capability_from_profile(&profile)
}

pub(crate) fn build_openapi_graphql_capability(
    dialect: &str,
    dsn: &str,
) -> OpenApiGraphqlCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn);
    build_openapi_graphql_capability_from_profile(&profile)
}

fn build_system_bootstrap_capability_from_profile(
    profile: &PersistenceProfile,
    version: &str,
) -> SystemBootstrapCapability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn SystemBootstrapRepository> =
                Arc::new(SeaOrmBootstrapService::new(db.clone(), version.to_owned()));
            SystemBootstrapCapability::Available {
                system: Arc::new(SystemBootstrapApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => SystemBootstrapCapability::Unsupported {
            message: supported_seaorm_dialect_message("DB-backed admin system status/bootstrap"),
        },
    }
}

fn build_identity_capability_from_profile(
    profile: &PersistenceProfile,
    allow_no_auth: bool,
) -> IdentityCapability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn IdentityRepository> =
                Arc::new(SeaOrmIdentityService::new(db.clone(), allow_no_auth));
            IdentityCapability::Available {
                identity: Arc::new(IdentityApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => IdentityCapability::Unsupported {
            message: supported_seaorm_dialect_message("DB-backed identity auth"),
        },
    }
}

fn build_request_context_capability_from_profile(
    profile: &PersistenceProfile,
    allow_no_auth: bool,
) -> RequestContextCapability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn RequestContextRepository> =
                Arc::new(SeaOrmRequestContextService::new(db.clone(), allow_no_auth));
            RequestContextCapability::Available {
                request_context: Arc::new(RequestContextApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => RequestContextCapability::Unsupported {
            message: supported_seaorm_dialect_message("DB-backed request context resolution"),
        },
    }
}

fn build_openai_v1_capability_from_profile(profile: &PersistenceProfile) -> OpenAiV1Capability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn OpenAiV1Repository> =
                Arc::new(SeaOrmOpenAiV1Service::new(db.clone()));
            OpenAiV1Capability::Available {
                openai: Arc::new(OpenAiV1ApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => OpenAiV1Capability::Unsupported {
            message: supported_seaorm_dialect_message("OpenAI `/v1` inference"),
        },
    }
}

fn build_admin_capability_from_profile(profile: &PersistenceProfile) -> AdminCapability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn AdminRepository> =
                Arc::new(SeaOrmAdminService::new(db.clone()));
            AdminCapability::Available {
                admin: Arc::new(AdminApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => AdminCapability::Unsupported {
            message: supported_seaorm_dialect_message("DB-backed admin read routes"),
        },
    }
}

fn build_admin_graphql_capability_from_profile(
    profile: &PersistenceProfile,
) -> AdminGraphqlCapability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn AdminGraphqlRepository> =
                Arc::new(SeaOrmAdminGraphqlService::new(db.clone()));
            AdminGraphqlCapability::Available {
                graphql: Arc::new(AdminGraphqlApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => AdminGraphqlCapability::Unsupported {
            message: supported_seaorm_dialect_message("DB-backed admin GraphQL"),
        },
    }
}

fn build_openapi_graphql_capability_from_profile(
    profile: &PersistenceProfile,
) -> OpenApiGraphqlCapability {
    match profile {
        PersistenceProfile::Sqlite { db }
        | PersistenceProfile::Postgres { db }
        | PersistenceProfile::MySql { db } => {
            let repository: Arc<dyn OpenApiGraphqlRepository> =
                Arc::new(SeaOrmOpenApiGraphqlService::new(db.clone()));
            OpenApiGraphqlCapability::Available {
                graphql: Arc::new(OpenApiGraphqlApplicationService::new(repository)),
            }
        }
        PersistenceProfile::Unsupported => OpenApiGraphqlCapability::Unsupported {
            message: supported_seaorm_dialect_message("DB-backed OpenAPI GraphQL"),
        },
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
