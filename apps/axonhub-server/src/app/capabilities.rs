use std::sync::Arc;
use std::time::Duration;

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
    admin::oauth::OauthProviderAdminService,
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

pub(crate) fn build_server_capabilities(
    dsn: &str,
    db_debug: bool,
    allow_no_auth: bool,
    version: &str,
    llm_request_timeout: Option<Duration>,
) -> ServerCapabilities {
    let db = SeaOrmConnectionFactory::postgres_with_debug(dsn.to_owned(), db_debug);

    ServerCapabilities {
        system_bootstrap: build_system_bootstrap_capability_from_db(&db, version),
        identity: build_identity_capability_from_db(&db, allow_no_auth),
        request_context: build_request_context_capability_from_db(&db, allow_no_auth),
        openai_v1: build_openai_v1_capability_from_db(&db, llm_request_timeout),
        admin: build_admin_capability_from_db(&db),
        admin_graphql: build_admin_graphql_capability_from_db(&db),
        openapi_graphql: build_openapi_graphql_capability_from_db(&db),
        oauth_provider_admin: build_oauth_provider_admin_capability(),
    }
}

pub(crate) fn build_system_bootstrap_capability(
    dsn: &str,
    version: &str,
) -> SystemBootstrapCapability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_system_bootstrap_capability_from_db(&db, version)
}

pub(crate) fn build_identity_capability(dsn: &str, allow_no_auth: bool) -> IdentityCapability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_identity_capability_from_db(&db, allow_no_auth)
}

pub(crate) fn build_request_context_capability(
    dsn: &str,
    allow_no_auth: bool,
) -> RequestContextCapability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_request_context_capability_from_db(&db, allow_no_auth)
}

pub(crate) fn build_openai_v1_capability(dsn: &str) -> OpenAiV1Capability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_openai_v1_capability_from_db(&db, None)
}

pub(crate) fn build_admin_capability(dsn: &str) -> AdminCapability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_admin_capability_from_db(&db)
}

pub(crate) fn build_admin_graphql_capability(dsn: &str) -> AdminGraphqlCapability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_admin_graphql_capability_from_db(&db)
}

pub(crate) fn build_openapi_graphql_capability(dsn: &str) -> OpenApiGraphqlCapability {
    let db = SeaOrmConnectionFactory::postgres(dsn.to_owned());
    build_openapi_graphql_capability_from_db(&db)
}

fn build_system_bootstrap_capability_from_db(
    db: &SeaOrmConnectionFactory,
    version: &str,
) -> SystemBootstrapCapability {
    let repository: Arc<dyn SystemBootstrapRepository> =
        Arc::new(SeaOrmBootstrapService::new(db.clone(), version.to_owned()));
    SystemBootstrapCapability::Available {
        system: Arc::new(SystemBootstrapApplicationService::new(repository)),
    }
}

fn build_identity_capability_from_db(
    db: &SeaOrmConnectionFactory,
    allow_no_auth: bool,
) -> IdentityCapability {
    let repository: Arc<dyn IdentityRepository> =
        Arc::new(SeaOrmIdentityService::new(db.clone(), allow_no_auth));
    IdentityCapability::Available {
        identity: Arc::new(IdentityApplicationService::new(repository)),
    }
}

fn build_request_context_capability_from_db(
    db: &SeaOrmConnectionFactory,
    allow_no_auth: bool,
) -> RequestContextCapability {
    let repository: Arc<dyn RequestContextRepository> =
        Arc::new(SeaOrmRequestContextService::new(db.clone(), allow_no_auth));
    RequestContextCapability::Available {
        request_context: Arc::new(RequestContextApplicationService::new(repository)),
    }
}

fn build_openai_v1_capability_from_db(
    db: &SeaOrmConnectionFactory,
    llm_request_timeout: Option<Duration>,
) -> OpenAiV1Capability {
    let repository: Arc<dyn OpenAiV1Repository> = Arc::new(
        SeaOrmOpenAiV1Service::new_with_upstream_request_timeout(db.clone(), llm_request_timeout),
    );
    OpenAiV1Capability::Available {
        openai: Arc::new(OpenAiV1ApplicationService::new(repository)),
    }
}

fn build_admin_capability_from_db(db: &SeaOrmConnectionFactory) -> AdminCapability {
    let repository: Arc<dyn AdminRepository> = Arc::new(SeaOrmAdminService::new(db.clone()));
    AdminCapability::Available {
        admin: Arc::new(AdminApplicationService::new(repository)),
    }
}

fn build_admin_graphql_capability_from_db(db: &SeaOrmConnectionFactory) -> AdminGraphqlCapability {
    let repository: Arc<dyn AdminGraphqlRepository> =
        Arc::new(SeaOrmAdminGraphqlService::new(db.clone()));
    AdminGraphqlCapability::Available {
        graphql: Arc::new(AdminGraphqlApplicationService::new(repository)),
    }
}

fn build_openapi_graphql_capability_from_db(
    db: &SeaOrmConnectionFactory,
) -> OpenApiGraphqlCapability {
    let repository: Arc<dyn OpenApiGraphqlRepository> =
        Arc::new(SeaOrmOpenApiGraphqlService::new(db.clone()));
    OpenApiGraphqlCapability::Available {
        graphql: Arc::new(OpenApiGraphqlApplicationService::new(repository)),
    }
}

pub(crate) fn build_oauth_provider_admin_capability() -> OauthProviderAdminCapability {
    if let Some(oauth_provider_admin) = OauthProviderAdminService::from_env() {
        return OauthProviderAdminCapability::Available {
            oauth_provider_admin: Arc::new(oauth_provider_admin),
        };
    }

    OauthProviderAdminCapability::Unsupported {
        message: "OAuth provider admin helpers are unavailable until secure runtime configuration is present. Set the required OAuth provider environment variables to enable these routes."
            .to_owned(),
    }
}
