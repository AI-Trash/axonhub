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
    use super::Migrator;
    use crate::m20260402_000007_operational_runtime_schema::{
        operational_runs_table_statement, realtime_sessions_table_statement,
    };
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    use sea_orm_migration::{prelude::{PostgresQueryBuilder, TableCreateStatement}, MigratorTrait};

    fn table_sql(statement: TableCreateStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    async fn table_exists(db: &sea_orm::DatabaseConnection, table: &str) -> bool {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = ? AND name = ?".to_owned(),
                vec!["table".into(), table.into()],
            ))
            .await
            .unwrap()
            .unwrap();

        row.try_get_by_index::<i64>(0).unwrap() == 1
    }

    async fn column_exists(db: &sea_orm::DatabaseConnection, table: &str, column: &str) -> bool {
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("PRAGMA table_info({table})"),
                vec![],
            ))
            .await
            .unwrap();

        rows.into_iter().any(|row| {
            row.try_get_by_index::<String>(1)
                .map(|name| name == column)
                .unwrap_or(false)
        })
    }

    #[tokio::test]
    async fn migrator_creates_persistence_extension_tables_on_sqlite() {
        let db = Database::connect("sqlite::memory:").await.unwrap();

        Migrator::up(&db, None).await.unwrap();

        for table in [
            "prompts",
            "prompt_protection_rules",
            "channel_model_prices",
            "channel_model_price_versions",
            "channel_override_templates",
        ] {
            assert!(table_exists(&db, table).await, "missing table {table}");
        }

        assert!(column_exists(&db, "prompts", "project_id").await);
        assert!(column_exists(&db, "prompts", "settings").await);
        assert!(column_exists(&db, "prompt_protection_rules", "pattern").await);
        assert!(column_exists(&db, "channel_model_prices", "reference_id").await);
        assert!(column_exists(&db, "channel_model_price_versions", "effective_start_at").await);
        assert!(column_exists(&db, "channel_model_price_versions", "effective_end_at").await);
        assert!(
            column_exists(
                &db,
                "channel_override_templates",
                "header_override_operations"
            )
            .await
        );
        assert!(
            column_exists(
                &db,
                "channel_override_templates",
                "body_override_operations"
            )
            .await
        );

        for table in ["realtime_sessions", "operational_runs"] {
            assert!(table_exists(&db, table).await, "missing table {table}");
        }

        assert!(column_exists(&db, "realtime_sessions", "session_id").await);
        assert!(column_exists(&db, "realtime_sessions", "transport").await);
        assert!(column_exists(&db, "realtime_sessions", "metadata").await);
        assert!(column_exists(&db, "operational_runs", "operation_type").await);
        assert!(column_exists(&db, "operational_runs", "trigger_source").await);
        assert!(column_exists(&db, "operational_runs", "result_payload").await);
        assert!(column_exists(&db, "users", "token_version").await);
    }

    #[test]
    fn postgres_operational_runtime_schema_statements_include_expected_contract() {
        let realtime_sessions_sql =
            table_sql(realtime_sessions_table_statement(DatabaseBackend::Postgres));
        let operational_runs_sql =
            table_sql(operational_runs_table_statement(DatabaseBackend::Postgres));

        assert!(realtime_sessions_sql.contains("realtime_sessions"));
        assert!(realtime_sessions_sql.contains("session_id"));
        assert!(realtime_sessions_sql.contains("transport"));
        assert!(realtime_sessions_sql.contains("timestamp with time zone"));
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
