#![cfg_attr(not(test), allow(dead_code))]

use crate::foundation::{
    authz::{
        ScopeSlug, DEFAULT_SERVICE_API_KEY_SCOPES, DEFAULT_USER_API_KEY_SCOPES,
        NO_AUTH_API_KEY_SCOPES, PROJECT_ADMIN_SCOPES, PROJECT_DEVELOPER_SCOPES,
        PROJECT_VIEWER_SCOPES,
    },
    shared::{
        DEFAULT_PROJECT_DESCRIPTION, DEFAULT_PROJECT_NAME, DEFAULT_SERVICE_API_KEY_NAME,
        DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_NAME, DEFAULT_USER_API_KEY_VALUE,
        NO_AUTH_API_KEY_NAME, NO_AUTH_API_KEY_VALUE, PRIMARY_DATA_STORAGE_DESCRIPTION,
        PRIMARY_DATA_STORAGE_NAME, PRIMARY_DATA_STORAGE_SETTINGS_JSON, SYSTEM_KEY_BRAND_NAME,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_INITIALIZED, SYSTEM_KEY_SECRET_KEY,
        SYSTEM_KEY_VERSION,
    },
};

pub(crate) const SCHEMA_MIGRATION_CRATE_PATH: &str = "crates/axonhub-db-migration";
pub(crate) const SCHEMA_ENTITY_CRATE_PATH: &str = "crates/axonhub-db-entity";
pub(crate) const CURRENT_PERSISTENCE_ADAPTER_PATH: &str = "apps/axonhub-server/src/foundation";
pub(crate) const LEGACY_DDL_REFERENCE_PATH: &str = "apps/axonhub-server/src/foundation/system.rs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaAuthority {
    SeaOrmMigrations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityWorkflow {
    FollowMigrationSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductionSchemaSyncPolicy {
    Forbidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawSqlUsage {
    MigrationDialectStep,
    BootstrapSeedData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepositoryStructure {
    pub(crate) migration_crate_path: &'static str,
    pub(crate) entity_crate_path: &'static str,
    pub(crate) current_repository_adapter_path: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TableGroupOwnership {
    pub(crate) group: &'static str,
    pub(crate) tables: &'static [&'static str],
    pub(crate) legacy_reference_path: &'static str,
    pub(crate) schema_authority: SchemaAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ApiKeySeedContract {
    pub(crate) name: &'static str,
    pub(crate) value: &'static str,
    pub(crate) key_type: &'static str,
    pub(crate) scopes: &'static [ScopeSlug],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoleSeedContract {
    pub(crate) name: &'static str,
    pub(crate) scopes: &'static [ScopeSlug],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BootstrapSemanticsContract {
    pub(crate) preserved_system_keys: &'static [&'static str],
    pub(crate) primary_data_storage_name: &'static str,
    pub(crate) primary_data_storage_description: &'static str,
    pub(crate) primary_data_storage_type: &'static str,
    pub(crate) primary_data_storage_status: &'static str,
    pub(crate) primary_data_storage_settings_json: &'static str,
    pub(crate) default_project_name: &'static str,
    pub(crate) default_project_description: &'static str,
    pub(crate) default_project_status: &'static str,
    pub(crate) default_project_roles: &'static [RoleSeedContract],
    pub(crate) default_api_keys: &'static [ApiKeySeedContract],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawSqlBoundaryRule {
    pub(crate) location: &'static str,
    pub(crate) usage: RawSqlUsage,
    pub(crate) purpose: &'static str,
    pub(crate) allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SchemaOwnershipContract {
    pub(crate) schema_authority: SchemaAuthority,
    pub(crate) entity_workflow: EntityWorkflow,
    pub(crate) production_schema_sync_policy: ProductionSchemaSyncPolicy,
    pub(crate) repository_structure: RepositoryStructure,
    pub(crate) runtime_database: &'static str,
    pub(crate) table_groups: &'static [TableGroupOwnership],
    pub(crate) bootstrap: BootstrapSemanticsContract,
    pub(crate) raw_sql_boundaries: &'static [RawSqlBoundaryRule],
}

const BOOTSTRAP_CONTROL_TABLES: &[&str] = &[
    "systems",
    "data_storages",
    "users",
    "projects",
    "user_projects",
    "roles",
    "user_roles",
    "api_keys",
];
const REQUEST_CONTEXT_TABLES: &[&str] = &["threads", "traces"];
const CATALOG_TABLES: &[&str] = &["channels", "models"];
const REQUEST_LEDGER_TABLES: &[&str] = &["requests", "request_executions", "usage_logs"];
const OPERATIONAL_TABLES: &[&str] = &[
    "channel_probes",
    "provider_quota_statuses",
    "realtime_sessions",
    "operational_runs",
];
const PERSISTENCE_EXTENSION_TABLES: &[&str] = &[
    "prompts",
    "prompt_protection_rules",
    "channel_model_prices",
    "channel_model_price_versions",
    "channel_override_templates",
];

const PRESERVED_SYSTEM_KEYS: &[&str] = &[
    SYSTEM_KEY_INITIALIZED,
    SYSTEM_KEY_VERSION,
    SYSTEM_KEY_SECRET_KEY,
    SYSTEM_KEY_BRAND_NAME,
    SYSTEM_KEY_DEFAULT_DATA_STORAGE,
];

const DEFAULT_PROJECT_ROLE_CONTRACTS: &[RoleSeedContract] = &[
    RoleSeedContract {
        name: "Admin",
        scopes: PROJECT_ADMIN_SCOPES,
    },
    RoleSeedContract {
        name: "Developer",
        scopes: PROJECT_DEVELOPER_SCOPES,
    },
    RoleSeedContract {
        name: "Viewer",
        scopes: PROJECT_VIEWER_SCOPES,
    },
];

const DEFAULT_API_KEY_CONTRACTS: &[ApiKeySeedContract] = &[
    ApiKeySeedContract {
        name: DEFAULT_USER_API_KEY_NAME,
        value: DEFAULT_USER_API_KEY_VALUE,
        key_type: "user",
        scopes: DEFAULT_USER_API_KEY_SCOPES,
    },
    ApiKeySeedContract {
        name: DEFAULT_SERVICE_API_KEY_NAME,
        value: DEFAULT_SERVICE_API_KEY_VALUE,
        key_type: "service_account",
        scopes: DEFAULT_SERVICE_API_KEY_SCOPES,
    },
    ApiKeySeedContract {
        name: NO_AUTH_API_KEY_NAME,
        value: NO_AUTH_API_KEY_VALUE,
        key_type: "noauth",
        scopes: NO_AUTH_API_KEY_SCOPES,
    },
];

const CURRENT_TABLE_GROUPS: &[TableGroupOwnership] = &[
    TableGroupOwnership {
        group: "bootstrap_control",
        tables: BOOTSTRAP_CONTROL_TABLES,
        legacy_reference_path: LEGACY_DDL_REFERENCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "request_context",
        tables: REQUEST_CONTEXT_TABLES,
        legacy_reference_path: LEGACY_DDL_REFERENCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "catalog",
        tables: CATALOG_TABLES,
        legacy_reference_path: LEGACY_DDL_REFERENCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "request_ledger",
        tables: REQUEST_LEDGER_TABLES,
        legacy_reference_path: LEGACY_DDL_REFERENCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "operational",
        tables: OPERATIONAL_TABLES,
        legacy_reference_path: LEGACY_DDL_REFERENCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "persistence_extension",
        tables: PERSISTENCE_EXTENSION_TABLES,
        legacy_reference_path: LEGACY_DDL_REFERENCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
];

const CURRENT_BOOTSTRAP_CONTRACT: BootstrapSemanticsContract = BootstrapSemanticsContract {
    preserved_system_keys: PRESERVED_SYSTEM_KEYS,
    primary_data_storage_name: PRIMARY_DATA_STORAGE_NAME,
    primary_data_storage_description: PRIMARY_DATA_STORAGE_DESCRIPTION,
    primary_data_storage_type: "database",
    primary_data_storage_status: "active",
    primary_data_storage_settings_json: PRIMARY_DATA_STORAGE_SETTINGS_JSON,
    default_project_name: DEFAULT_PROJECT_NAME,
    default_project_description: DEFAULT_PROJECT_DESCRIPTION,
    default_project_status: "active",
    default_project_roles: DEFAULT_PROJECT_ROLE_CONTRACTS,
    default_api_keys: DEFAULT_API_KEY_CONTRACTS,
};

const RAW_SQL_BOUNDARY_RULES: &[RawSqlBoundaryRule] = &[
    RawSqlBoundaryRule {
        location: SCHEMA_MIGRATION_CRATE_PATH,
        usage: RawSqlUsage::MigrationDialectStep,
        purpose: "PostgreSQL migration DDL or data backfill that SeaORM cannot express directly.",
        allowed: true,
    },
    RawSqlBoundaryRule {
        location: "runtime schema sync",
        usage: RawSqlUsage::MigrationDialectStep,
        purpose: "Ad-hoc schema creation or entity-first sync is not allowed to define production truth once SeaORM migrations own the schema.",
        allowed: false,
    },
];

const CURRENT_SCHEMA_OWNERSHIP_CONTRACT: SchemaOwnershipContract = SchemaOwnershipContract {
    schema_authority: SchemaAuthority::SeaOrmMigrations,
    entity_workflow: EntityWorkflow::FollowMigrationSchema,
    production_schema_sync_policy: ProductionSchemaSyncPolicy::Forbidden,
    repository_structure: RepositoryStructure {
        migration_crate_path: SCHEMA_MIGRATION_CRATE_PATH,
        entity_crate_path: SCHEMA_ENTITY_CRATE_PATH,
        current_repository_adapter_path: CURRENT_PERSISTENCE_ADAPTER_PATH,
    },
    runtime_database: "postgres",
    table_groups: CURRENT_TABLE_GROUPS,
    bootstrap: CURRENT_BOOTSTRAP_CONTRACT,
    raw_sql_boundaries: RAW_SQL_BOUNDARY_RULES,
};

pub(crate) fn current_schema_ownership_contract() -> &'static SchemaOwnershipContract {
    &CURRENT_SCHEMA_OWNERSHIP_CONTRACT
}

pub(crate) fn table_group(group: &str) -> Option<&'static TableGroupOwnership> {
    CURRENT_SCHEMA_OWNERSHIP_CONTRACT
        .table_groups
        .iter()
        .find(|candidate| candidate.group == group)
}

#[cfg(test)]
mod tests {
    use super::{
        current_schema_ownership_contract, table_group, EntityWorkflow, ProductionSchemaSyncPolicy,
        RawSqlUsage, SchemaAuthority,
    };
    use crate::foundation::seaorm::SeaOrmConnectionFactory;
    #[test]
    fn schema_ownership_contract_is_migration_first() {
        let contract = current_schema_ownership_contract();

        assert_eq!(contract.schema_authority, SchemaAuthority::SeaOrmMigrations);
        assert_eq!(
            contract.entity_workflow,
            EntityWorkflow::FollowMigrationSchema
        );
        assert_eq!(
            contract.production_schema_sync_policy,
            ProductionSchemaSyncPolicy::Forbidden
        );
        assert_eq!(
            contract.repository_structure.migration_crate_path,
            "crates/axonhub-db-migration"
        );
        assert_eq!(
            contract.repository_structure.entity_crate_path,
            "crates/axonhub-db-entity"
        );
        assert_eq!(contract.runtime_database, "postgres");
    }

    #[test]
    fn schema_ownership_runtime_contract_matches_connection_factory_variants() {
        assert_eq!(
            SeaOrmConnectionFactory::postgres("postgres://localhost/axonhub".to_owned())
                .runtime_dsn(),
            "postgres://localhost/axonhub"
        );
    }

    #[test]
    fn schema_ownership_contract_maps_current_foundation_tables() {
        let bootstrap = table_group("bootstrap_control").expect("bootstrap_control group");
        let request_context = table_group("request_context").expect("request_context group");
        let catalog = table_group("catalog").expect("catalog group");
        let request_ledger = table_group("request_ledger").expect("request_ledger group");
        let operational = table_group("operational").expect("operational group");
        let persistence_extension =
            table_group("persistence_extension").expect("persistence_extension group");

        assert_eq!(
            bootstrap.tables,
            [
                "systems",
                "data_storages",
                "users",
                "projects",
                "user_projects",
                "roles",
                "user_roles",
                "api_keys",
            ]
        );
        assert_eq!(request_context.tables, ["threads", "traces"]);
        assert_eq!(catalog.tables, ["channels", "models"]);
        assert_eq!(
            request_ledger.tables,
            ["requests", "request_executions", "usage_logs"]
        );
        assert_eq!(
            operational.tables,
            [
                "channel_probes",
                "provider_quota_statuses",
                "realtime_sessions",
                "operational_runs",
            ]
        );
        assert_eq!(
            persistence_extension.tables,
            [
                "prompts",
                "prompt_protection_rules",
                "channel_model_prices",
                "channel_model_price_versions",
                "channel_override_templates",
            ]
        );

        for group in [
            bootstrap,
            request_context,
            catalog,
            request_ledger,
            operational,
            persistence_extension,
        ] {
            assert_eq!(
                group.legacy_reference_path,
                "apps/axonhub-server/src/foundation/system.rs"
            );
            assert_eq!(group.schema_authority, SchemaAuthority::SeaOrmMigrations);
        }
    }

    pub(crate) fn schema_ownership_contract_limits_raw_sql_usage_inner() {
        let contract = current_schema_ownership_contract();

        assert_eq!(contract.raw_sql_boundaries.len(), 2);
        assert_eq!(
            contract.raw_sql_boundaries[0].usage,
            RawSqlUsage::MigrationDialectStep
        );
        assert!(contract.raw_sql_boundaries[0].allowed);
        assert_eq!(
            contract.raw_sql_boundaries[1].usage,
            RawSqlUsage::MigrationDialectStep
        );
        assert!(!contract.raw_sql_boundaries[1].allowed);
        assert_eq!(
            contract.raw_sql_boundaries[1].location,
            "runtime schema sync"
        );
        assert!(contract.raw_sql_boundaries.iter().all(|boundary| {
            boundary.location != "apps/axonhub-server/src/foundation/system.rs"
        }));
    }

    #[test]
    fn schema_ownership_contract_limits_raw_sql_usage() {
        schema_ownership_contract_limits_raw_sql_usage_inner();
    }
}

#[cfg(test)]
pub(crate) fn schema_ownership_contract_limits_raw_sql_usage_inner() {
    tests::schema_ownership_contract_limits_raw_sql_usage_inner();
}
