pub use sea_orm_migration::prelude::*;

mod m20260326_000001_bootstrap_control_schema;
mod m20260326_000002_request_context_schema;
mod m20260326_000003_catalog_schema;
mod m20260326_000004_request_ledger_schema;
mod m20260326_000005_operational_schema;
mod m20260327_000006_persistence_extension_schema;

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
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::Migrator;
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn table_exists(db: &sea_orm::DatabaseConnection, table: &str) -> bool {
        let row = db
            .query_one(Statement::from_sql_and_values(
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
            .query_all(Statement::from_sql_and_values(
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
    }
}
