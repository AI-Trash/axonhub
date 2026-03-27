#![cfg_attr(not(test), allow(dead_code))]

use super::{
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

pub(crate) const CANONICAL_SEAORM_MIGRATION_CRATE_PATH: &str = "crates/axonhub-db-migration";
pub(crate) const CANONICAL_SEAORM_ENTITY_CRATE_PATH: &str = "crates/axonhub-db-entity";
pub(crate) const CURRENT_PERSISTENCE_ADAPTER_PATH: &str = "apps/axonhub-server/src/foundation";
pub(crate) const SQLITE_LEGACY_DDL_SOURCE_PATH: &str =
    "apps/axonhub-server/src/foundation/shared.rs";
pub(crate) const POSTGRES_LEGACY_DDL_SOURCE_PATH: &str =
    "apps/axonhub-server/src/foundation/system.rs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalSchemaOwner {
    SeaOrmMigrations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityWorkflow {
    FollowCanonicalSchema,
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
    pub(crate) sqlite_legacy_source_path: &'static str,
    pub(crate) postgres_legacy_source_path: &'static str,
    pub(crate) canonical_owner: CanonicalSchemaOwner,
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
    pub(crate) canonical_owner: CanonicalSchemaOwner,
    pub(crate) entity_workflow: EntityWorkflow,
    pub(crate) production_schema_sync_policy: ProductionSchemaSyncPolicy,
    pub(crate) repository_structure: RepositoryStructure,
    pub(crate) supported_runtime_dialects: &'static [&'static str],
    pub(crate) verified_runtime_dialects: &'static [&'static str],
    pub(crate) future_runtime_dialects: &'static [&'static str],
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
const OPERATIONAL_TABLES: &[&str] = &["channel_probes", "provider_quota_statuses"];
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
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "request_context",
        tables: REQUEST_CONTEXT_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "catalog",
        tables: CATALOG_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "request_ledger",
        tables: REQUEST_LEDGER_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "operational",
        tables: OPERATIONAL_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "persistence_extension",
        tables: PERSISTENCE_EXTENSION_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
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
        location: CANONICAL_SEAORM_MIGRATION_CRATE_PATH,
        usage: RawSqlUsage::MigrationDialectStep,
        purpose: "Dialect-specific DDL or data backfill that SeaORM cannot express consistently across sqlite, postgres, and mysql.",
        allowed: true,
    },
    RawSqlBoundaryRule {
        location: "apps/axonhub-server/src/foundation/system.rs",
        usage: RawSqlUsage::BootstrapSeedData,
        purpose: "Idempotent bootstrap DML that preserves initialization state, default storage, default project, default roles, owner membership, and default API keys for the current slice.",
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
    canonical_owner: CanonicalSchemaOwner::SeaOrmMigrations,
    entity_workflow: EntityWorkflow::FollowCanonicalSchema,
    production_schema_sync_policy: ProductionSchemaSyncPolicy::Forbidden,
    repository_structure: RepositoryStructure {
        migration_crate_path: CANONICAL_SEAORM_MIGRATION_CRATE_PATH,
        entity_crate_path: CANONICAL_SEAORM_ENTITY_CRATE_PATH,
        current_repository_adapter_path: CURRENT_PERSISTENCE_ADAPTER_PATH,
    },
    supported_runtime_dialects: &["sqlite", "postgres", "mysql"],
    verified_runtime_dialects: &["sqlite", "postgres"],
    future_runtime_dialects: &[],
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
        current_schema_ownership_contract, table_group, CanonicalSchemaOwner, EntityWorkflow,
        ProductionSchemaSyncPolicy, RawSqlUsage,
    };
    use crate::foundation::{
        authz::scope_strings,
        seaorm::SeaOrmConnectionFactory,
        shared::{SqliteFoundation, SYSTEM_KEY_INITIALIZED},
        system::SqliteBootstrapService,
    };
    use axonhub_http::{InitializeSystemRequest, SystemBootstrapPort};
    use rusqlite::{params, OptionalExtension};
    use sea_orm::DatabaseBackend;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_sqlite_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("axonhub-{name}-{unique}.db"))
    }

    #[test]
    fn schema_ownership_contract_is_migration_first() {
        let contract = current_schema_ownership_contract();

        assert_eq!(
            contract.canonical_owner,
            CanonicalSchemaOwner::SeaOrmMigrations
        );
        assert_eq!(
            contract.entity_workflow,
            EntityWorkflow::FollowCanonicalSchema
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
        assert_eq!(
            contract.supported_runtime_dialects,
            ["sqlite", "postgres", "mysql"]
        );
        assert_eq!(contract.verified_runtime_dialects, ["sqlite", "postgres"]);
        assert!(contract.future_runtime_dialects.is_empty());
    }

    #[test]
    fn schema_ownership_runtime_contract_matches_connection_factory_variants() {
        assert_eq!(
            SeaOrmConnectionFactory::sqlite(":memory:".to_owned()).backend(),
            DatabaseBackend::Sqlite
        );
        assert_eq!(
            SeaOrmConnectionFactory::postgres("postgres://localhost/axonhub".to_owned()).backend(),
            DatabaseBackend::Postgres
        );
        assert_eq!(
            SeaOrmConnectionFactory::mysql("mysql://localhost:3306/axonhub".to_owned()).backend(),
            DatabaseBackend::MySql
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
            ["channel_probes", "provider_quota_statuses"]
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
                group.sqlite_legacy_source_path,
                "apps/axonhub-server/src/foundation/shared.rs"
            );
            assert_eq!(
                group.postgres_legacy_source_path,
                "apps/axonhub-server/src/foundation/system.rs"
            );
            assert_eq!(
                group.canonical_owner,
                CanonicalSchemaOwner::SeaOrmMigrations
            );
        }
    }

    #[test]
    fn schema_ownership_contract_limits_raw_sql_usage() {
        let contract = current_schema_ownership_contract();

        assert_eq!(contract.raw_sql_boundaries.len(), 3);
        assert_eq!(
            contract.raw_sql_boundaries[0].usage,
            RawSqlUsage::MigrationDialectStep
        );
        assert!(contract.raw_sql_boundaries[0].allowed);
        assert_eq!(
            contract.raw_sql_boundaries[1].usage,
            RawSqlUsage::BootstrapSeedData
        );
        assert!(contract.raw_sql_boundaries[1].allowed);
        assert!(!contract.raw_sql_boundaries[2].allowed);
        assert_eq!(
            contract.raw_sql_boundaries[2].location,
            "runtime schema sync"
        );
    }

    #[test]
    fn sqlite_bootstrap_matches_preserved_schema_contract() {
        let db_path = temp_sqlite_path("schema-ownership-bootstrap");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());
        let contract = current_schema_ownership_contract();

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let connection = foundation.open_connection(false).unwrap();

        for key in contract.bootstrap.preserved_system_keys {
            let value: Option<String> = connection
                .query_row(
                    "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                    [key],
                    |row| row.get(0),
                )
                .optional()
                .unwrap();
            assert!(value.is_some(), "missing system key {key}");
        }

        let initialized_value: String = connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                [SYSTEM_KEY_INITIALIZED],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(initialized_value, "true");

        let storage_id: String = connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                [contract.bootstrap.preserved_system_keys[4]],
                |row| row.get(0),
            )
            .unwrap();
        let primary_storage: (String, String, String, String, String) = connection
            .query_row(
                "SELECT name, description, type, status, settings FROM data_storages WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
                [storage_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            primary_storage.0,
            contract.bootstrap.primary_data_storage_name
        );
        assert_eq!(
            primary_storage.1,
            contract.bootstrap.primary_data_storage_description
        );
        assert_eq!(
            primary_storage.2,
            contract.bootstrap.primary_data_storage_type
        );
        assert_eq!(
            primary_storage.3,
            contract.bootstrap.primary_data_storage_status
        );
        assert_eq!(
            primary_storage.4,
            contract.bootstrap.primary_data_storage_settings_json
        );

        let default_project: (i64, String, String, String) = connection
            .query_row(
                "SELECT id, name, description, status FROM projects WHERE name = ?1 AND deleted_at = 0 LIMIT 1",
                [contract.bootstrap.default_project_name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(default_project.1, contract.bootstrap.default_project_name);
        assert_eq!(
            default_project.2,
            contract.bootstrap.default_project_description
        );
        assert_eq!(default_project.3, contract.bootstrap.default_project_status);

        for role in contract.bootstrap.default_project_roles {
            let scopes_json: String = connection
                .query_row(
                    "SELECT scopes FROM roles WHERE name = ?1 AND project_id = ?2 AND deleted_at = 0 LIMIT 1",
                    params![role.name, default_project.0],
                    |row| row.get(0),
                )
                .unwrap();
            let stored_scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap();
            assert_eq!(stored_scopes, scope_strings(role.scopes));
        }

        for api_key in contract.bootstrap.default_api_keys {
            let row: (String, String, String, String) = connection
                .query_row(
                    "SELECT name, type, scopes, status FROM api_keys WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                    [api_key.value],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            let scopes: Vec<String> = serde_json::from_str(&row.2).unwrap();
            assert_eq!(row.0, api_key.name);
            assert_eq!(row.1, api_key.key_type);
            assert_eq!(row.3, "enabled");
            assert_eq!(scopes, scope_strings(api_key.scopes));
        }

        let owner_membership_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM user_projects WHERE user_id = 1 AND project_id = ?1 AND is_owner = 1",
                [default_project.0],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner_membership_count, 1);

        std::fs::remove_file(db_path).ok();
    }

    #[test]
    fn sqlite_legacy_bootstrap_upgrades_to_persistence_extension_schema() {
        let db_path = temp_sqlite_path("schema-ownership-upgrade");
        let foundation = Arc::new(SqliteFoundation::new(db_path.display().to_string()));
        let bootstrap = SqliteBootstrapService::new(foundation.clone(), "v0.9.20".to_owned());

        bootstrap
            .initialize(&InitializeSystemRequest {
                owner_email: "owner@example.com".to_owned(),
                owner_password: "password123".to_owned(),
                owner_first_name: "System".to_owned(),
                owner_last_name: "Owner".to_owned(),
                brand_name: "AxonHub".to_owned(),
            })
            .unwrap();

        let pre_upgrade_connection = foundation.open_connection(false).unwrap();
        let pre_upgrade_prompts_count: i64 = pre_upgrade_connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                ["prompts"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pre_upgrade_prompts_count, 0);
        drop(pre_upgrade_connection);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(
                SeaOrmConnectionFactory::sqlite(db_path.display().to_string()).connect_migrated(),
            )
            .unwrap();

        let post_upgrade_connection = foundation.open_connection(false).unwrap();
        for table in [
            "prompts",
            "prompt_protection_rules",
            "channel_model_prices",
            "channel_model_price_versions",
            "channel_override_templates",
        ] {
            let table_count: i64 = post_upgrade_connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(table_count, 1, "missing upgraded table {table}");
        }

        let initialized_value: String = post_upgrade_connection
            .query_row(
                "SELECT value FROM systems WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                [SYSTEM_KEY_INITIALIZED],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(initialized_value, "true");

        let default_project_count: i64 = post_upgrade_connection
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE name = 'Default Project' AND deleted_at = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(default_project_count, 1);

        std::fs::remove_file(db_path).ok();
    }
}
