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
pub(crate) const SQLITE_LEGACY_DDL_SOURCE_PATH: &str =
    "apps/axonhub-server/src/foundation/shared.rs";
pub(crate) const POSTGRES_LEGACY_DDL_SOURCE_PATH: &str =
    "apps/axonhub-server/src/foundation/system.rs";

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
    pub(crate) sqlite_legacy_source_path: &'static str,
    pub(crate) postgres_legacy_source_path: &'static str,
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
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "request_context",
        tables: REQUEST_CONTEXT_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "catalog",
        tables: CATALOG_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "request_ledger",
        tables: REQUEST_LEDGER_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "operational",
        tables: OPERATIONAL_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
        schema_authority: SchemaAuthority::SeaOrmMigrations,
    },
    TableGroupOwnership {
        group: "persistence_extension",
        tables: PERSISTENCE_EXTENSION_TABLES,
        sqlite_legacy_source_path: SQLITE_LEGACY_DDL_SOURCE_PATH,
        postgres_legacy_source_path: POSTGRES_LEGACY_DDL_SOURCE_PATH,
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
        purpose: "Dialect-specific DDL or data backfill that SeaORM cannot express consistently across sqlite, postgres, and mysql.",
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
    supported_runtime_dialects: &["sqlite", "postgres"],
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
        current_schema_ownership_contract, table_group, EntityWorkflow, ProductionSchemaSyncPolicy,
        RawSqlUsage, SchemaAuthority,
    };
    use crate::foundation::{
        authz::scope_strings,
        seaorm::SeaOrmConnectionFactory,
        shared::SYSTEM_KEY_INITIALIZED,
        system::{SqliteBootstrapService, SqliteFoundation},
    };
    use axonhub_http::{InitializeSystemRequest, SystemBootstrapPort};
    use axonhub_db_entity::{api_keys, data_storages, projects, roles, systems, user_projects};
    use sea_orm::{ColumnTrait, DatabaseBackend, EntityTrait, QueryFilter};
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
        assert_eq!(contract.supported_runtime_dialects, ["sqlite", "postgres"]);
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
                group.sqlite_legacy_source_path,
                "apps/axonhub-server/src/foundation/shared.rs"
            );
            assert_eq!(
                group.postgres_legacy_source_path,
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

        foundation.seaorm().run_sync(move |db| async move {
            let connection = db.connect_migrated().await.unwrap();

            for key in contract.bootstrap.preserved_system_keys {
                let value = systems::Entity::find()
                    .filter(systems::Column::Key.eq(key))
                    .filter(systems::Column::DeletedAt.eq(0_i64))
                    .into_partial_model::<systems::KeyValue>()
                    .one(&connection)
                    .await
                    .unwrap();
                assert!(value.is_some(), "missing system key {key}");
            }

            let initialized_value = systems::Entity::find()
                .filter(systems::Column::Key.eq(SYSTEM_KEY_INITIALIZED))
                .filter(systems::Column::DeletedAt.eq(0_i64))
                .into_partial_model::<systems::KeyValue>()
                .one(&connection)
                .await
                .unwrap()
                .map(|row| row.value)
                .expect("initialized system key exists");
            assert_eq!(initialized_value, "true");

            let storage_id = systems::Entity::find()
                .filter(systems::Column::Key.eq(contract.bootstrap.preserved_system_keys[4]))
                .filter(systems::Column::DeletedAt.eq(0_i64))
                .into_partial_model::<systems::KeyValue>()
                .one(&connection)
                .await
                .unwrap()
                .map(|row| row.value)
                .expect("default storage key exists")
                .parse::<i64>()
                .unwrap();
            let primary_storage = data_storages::Entity::find_by_id(storage_id)
                .filter(data_storages::Column::DeletedAt.eq(0_i64))
                .one(&connection)
                .await
                .unwrap()
                .expect("primary storage exists");
            assert_eq!(primary_storage.name, contract.bootstrap.primary_data_storage_name);
            assert_eq!(primary_storage.description, contract.bootstrap.primary_data_storage_description);
            assert_eq!(primary_storage.type_field, contract.bootstrap.primary_data_storage_type);
            assert_eq!(primary_storage.status, contract.bootstrap.primary_data_storage_status);
            assert_eq!(primary_storage.settings, contract.bootstrap.primary_data_storage_settings_json);

            let default_project = projects::Entity::find()
                .filter(projects::Column::Name.eq(contract.bootstrap.default_project_name))
                .filter(projects::Column::DeletedAt.eq(0_i64))
                .one(&connection)
                .await
                .unwrap()
                .expect("default project exists");
            assert_eq!(default_project.name, contract.bootstrap.default_project_name);
            assert_eq!(default_project.description, contract.bootstrap.default_project_description);
            assert_eq!(default_project.status, contract.bootstrap.default_project_status);

            for role in contract.bootstrap.default_project_roles {
                let role_row = roles::Entity::find()
                    .filter(roles::Column::Name.eq(role.name))
                    .filter(roles::Column::ProjectId.eq(default_project.id))
                    .filter(roles::Column::DeletedAt.eq(0_i64))
                    .one(&connection)
                    .await
                    .unwrap()
                    .expect("seeded role exists");
                let stored_scopes: Vec<String> = serde_json::from_str(&role_row.scopes).unwrap();
                assert_eq!(stored_scopes, scope_strings(role.scopes));
            }

            for api_key in contract.bootstrap.default_api_keys {
                let row = api_keys::Entity::find()
                    .filter(api_keys::Column::Key.eq(api_key.value))
                    .filter(api_keys::Column::DeletedAt.eq(0_i64))
                    .one(&connection)
                    .await
                    .unwrap()
                    .expect("seeded api key exists");
                let scopes: Vec<String> = serde_json::from_str(&row.scopes).unwrap();
                assert_eq!(row.name, api_key.name);
                assert_eq!(row.type_field, api_key.key_type);
                assert_eq!(row.status, "enabled");
                assert_eq!(scopes, scope_strings(api_key.scopes));
            }

            let owner_membership_count = user_projects::Entity::find()
                .filter(user_projects::Column::UserId.eq(1_i64))
                .filter(user_projects::Column::ProjectId.eq(default_project.id))
                .filter(user_projects::Column::IsOwner.eq(true))
                .count(&connection)
                .await
                .unwrap();
            assert_eq!(owner_membership_count, 1);
        });

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

        foundation.seaorm().run_sync(|db| async move {
            let connection = db.connect_migrated().await.unwrap();
            let prompts_count = axonhub_db_entity::prompts::Entity::find()
                .count(&connection)
                .await
                .unwrap();
            assert_eq!(prompts_count, 0);
        });

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(
                SeaOrmConnectionFactory::sqlite(db_path.display().to_string()).connect_migrated(),
            )
            .unwrap();

        foundation.seaorm().run_sync(|db| async move {
            let connection = db.connect_migrated().await.unwrap();
            for table_count in [
                axonhub_db_entity::prompts::Entity::find().count(&connection).await.unwrap(),
                axonhub_db_entity::prompt_protection_rules::Entity::find().count(&connection).await.unwrap(),
                axonhub_db_entity::channel_model_prices::Entity::find().count(&connection).await.unwrap(),
                axonhub_db_entity::channel_model_price_versions::Entity::find().count(&connection).await.unwrap(),
                axonhub_db_entity::channel_override_templates::Entity::find().count(&connection).await.unwrap(),
            ] {
                assert!(table_count >= 0);
            }

            let initialized_value = systems::Entity::find()
                .filter(systems::Column::Key.eq(SYSTEM_KEY_INITIALIZED))
                .filter(systems::Column::DeletedAt.eq(0_i64))
                .into_partial_model::<systems::KeyValue>()
                .one(&connection)
                .await
                .unwrap()
                .map(|row| row.value)
                .expect("initialized system key exists");
            assert_eq!(initialized_value, "true");

            let default_project_count = projects::Entity::find()
                .filter(projects::Column::Name.eq("Default Project"))
                .filter(projects::Column::DeletedAt.eq(0_i64))
                .count(&connection)
                .await
                .unwrap();
            assert_eq!(default_project_count, 1);
        });

        std::fs::remove_file(db_path).ok();
    }
}

#[cfg(test)]
pub(crate) fn schema_ownership_contract_limits_raw_sql_usage_inner() {
    tests::schema_ownership_contract_limits_raw_sql_usage_inner();
}
