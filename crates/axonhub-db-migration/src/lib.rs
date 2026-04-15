pub use sea_orm_migration::prelude::*;

mod m20260326_000001_bootstrap_control_schema;
mod m20260326_000002_request_context_schema;
mod m20260326_000003_catalog_schema;
mod m20260326_000004_request_ledger_schema;
mod m20260326_000005_operational_schema;
mod m20260327_000006_persistence_extension_schema;
mod m20260402_000007_operational_runtime_schema;
mod m20260413_000008_identity_token_version;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260326_000001_bootstrap_control_schema::Migration),
            Box::new(m20260326_000002_request_context_schema::Migration),
            Box::new(m20260326_000003_catalog_schema::Migration),
            Box::new(m20260326_000004_request_ledger_schema::Migration),
            Box::new(m20260326_000005_operational_schema::Migration),
            Box::new(m20260327_000006_persistence_extension_schema::Migration),
            Box::new(m20260402_000007_operational_runtime_schema::Migration),
            Box::new(m20260413_000008_identity_token_version::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use crate::m20260402_000007_operational_runtime_schema::{
        operational_runs_table_statement, realtime_sessions_table_statement,
    };
    use sea_orm_migration::prelude::{PostgresQueryBuilder, TableCreateStatement};

    fn table_sql(statement: TableCreateStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn postgres_operational_runtime_schema_statements_include_expected_contract() {
        let realtime_sessions_sql = table_sql(realtime_sessions_table_statement());
        let operational_runs_sql = table_sql(operational_runs_table_statement());

        assert!(realtime_sessions_sql.contains("realtime_sessions"));
        assert!(realtime_sessions_sql.contains("session_id"));
        assert!(realtime_sessions_sql.contains("transport"));
        assert!(realtime_sessions_sql.contains("TEXT"));
        assert!(realtime_sessions_sql.contains("project_id"));
        assert!(realtime_sessions_sql.contains("projects"));

        assert!(operational_runs_sql.contains("operational_runs"));
        assert!(operational_runs_sql.contains("operation_type"));
        assert!(operational_runs_sql.contains("result_payload"));
        assert!(operational_runs_sql.contains("trigger_source"));
        assert!(operational_runs_sql.contains("started_at"));
        assert!(operational_runs_sql.contains("initiated_by_user_id"));
        assert!(operational_runs_sql.contains("users"));
    }
}
