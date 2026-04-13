use std::sync::Arc;

use axonhub_http::{
    AdminCapability, AdminGraphqlCapability, IdentityCapability, OauthProviderAdminCapability,
    OpenAiV1Capability, OpenApiGraphqlCapability, RequestContextCapability,
    SystemBootstrapCapability,
};

use super::services::{
    AdminApplicationService, AdminGraphqlApplicationService, IdentityApplicationService,
    OpenAiV1ApplicationService, OpenApiGraphqlApplicationService, RequestContextApplicationService,
    SystemBootstrapApplicationService,
};
use crate::foundation::{
    admin::oauth::SqliteOauthProviderAdminService,
    admin::SeaOrmAdminService,
    graphql::{SeaOrmAdminGraphqlService, SeaOrmOpenApiGraphqlService},
    identity_service::SeaOrmIdentityService,
    openai_v1::SeaOrmOpenAiV1Service,
    ports::{
        AdminGraphqlRepository, AdminRepository, IdentityRepository, OpenAiV1Repository,
        OpenApiGraphqlRepository, RequestContextRepository, SystemBootstrapRepository,
    },
    request_context_service::SeaOrmRequestContextService,
    seaorm::SeaOrmConnectionFactory,
    system::SeaOrmBootstrapService,
};

const SQLITE_AND_POSTGRES_DIALECT_HINT: &str =
    "Rust replacement for this surface is currently supported only on sqlite3 and postgres.";

pub(crate) struct ServerCapabilities {
    pub(crate) system_bootstrap: SystemBootstrapCapability,
    pub(crate) identity: IdentityCapability,
    pub(crate) request_context: RequestContextCapability,
    pub(crate) openai_v1: OpenAiV1Capability,
    pub(crate) admin: AdminCapability,
    pub(crate) admin_graphql: AdminGraphqlCapability,
    pub(crate) openapi_graphql: OpenApiGraphqlCapability,
    pub(crate) oauth_provider_admin: OauthProviderAdminCapability,
}

enum PersistenceProfile {
    Sqlite { db: SeaOrmConnectionFactory },
    Postgres { db: SeaOrmConnectionFactory },
    Unsupported,
}

impl PersistenceProfile {
    fn resolve(dialect: &str, dsn: &str, db_debug: bool) -> Self {
        if dialect.eq_ignore_ascii_case("sqlite3") {
            return Self::Sqlite {
                db: SeaOrmConnectionFactory::sqlite_with_debug(dsn.to_owned(), db_debug),
            };
        }

        if dialect.eq_ignore_ascii_case("postgres") || dialect.eq_ignore_ascii_case("postgresql") {
            return Self::Postgres {
                db: SeaOrmConnectionFactory::postgres_with_debug(dsn.to_owned(), db_debug),
            };
        }

        Self::Unsupported
    }
}

fn supported_seaorm_dialect_message(surface: &str) -> String {
    format!(
        "{surface} is not available for the configured dialect yet. {SQLITE_AND_POSTGRES_DIALECT_HINT}"
    )
}

pub(crate) fn build_server_capabilities(
    dialect: &str,
    dsn: &str,
    db_debug: bool,
    allow_no_auth: bool,
    version: &str,
) -> ServerCapabilities {
    let profile = PersistenceProfile::resolve(dialect, dsn, db_debug);

    ServerCapabilities {
        system_bootstrap: build_system_bootstrap_capability_from_profile(&profile, version),
        identity: build_identity_capability_from_profile(&profile, allow_no_auth),
        request_context: build_request_context_capability_from_profile(&profile, allow_no_auth),
        openai_v1: build_openai_v1_capability_from_profile(&profile),
        admin: build_admin_capability_from_profile(&profile),
        admin_graphql: build_admin_graphql_capability_from_profile(&profile),
        openapi_graphql: build_openapi_graphql_capability_from_profile(&profile),
        oauth_provider_admin: build_oauth_provider_admin_capability(dialect, dsn),
    }
}

pub(crate) fn build_system_bootstrap_capability(
    dialect: &str,
    dsn: &str,
    version: &str,
) -> SystemBootstrapCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_system_bootstrap_capability_from_profile(&profile, version)
}

pub(crate) fn build_identity_capability(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
) -> IdentityCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_identity_capability_from_profile(&profile, allow_no_auth)
}

pub(crate) fn build_request_context_capability(
    dialect: &str,
    dsn: &str,
    allow_no_auth: bool,
) -> RequestContextCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_request_context_capability_from_profile(&profile, allow_no_auth)
}

pub(crate) fn build_openai_v1_capability(dialect: &str, dsn: &str) -> OpenAiV1Capability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_openai_v1_capability_from_profile(&profile)
}

pub(crate) fn build_admin_capability(dialect: &str, dsn: &str) -> AdminCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_admin_capability_from_profile(&profile)
}

pub(crate) fn build_admin_graphql_capability(dialect: &str, dsn: &str) -> AdminGraphqlCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_admin_graphql_capability_from_profile(&profile)
}

pub(crate) fn build_openapi_graphql_capability(
    dialect: &str,
    dsn: &str,
) -> OpenApiGraphqlCapability {
    let profile = PersistenceProfile::resolve(dialect, dsn, false);
    build_openapi_graphql_capability_from_profile(&profile)
}

fn build_system_bootstrap_capability_from_profile(
    profile: &PersistenceProfile,
    version: &str,
) -> SystemBootstrapCapability {
    match profile {
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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
        PersistenceProfile::Sqlite { db } | PersistenceProfile::Postgres { db } => {
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

pub(crate) fn build_oauth_provider_admin_capability(
    _dialect: &str,
    _dsn: &str,
) -> OauthProviderAdminCapability {
    if let Some(oauth_provider_admin) = SqliteOauthProviderAdminService::from_env() {
        return OauthProviderAdminCapability::Available {
            oauth_provider_admin: Arc::new(oauth_provider_admin),
        };
    }

    OauthProviderAdminCapability::Unsupported {
        message: "OAuth provider admin helpers are unavailable until secure runtime configuration is present. Set the required OAuth provider environment variables to enable these routes."
            .to_owned(),
    }
}
