pub use sea_orm_migration::prelude::*;

mod m20260326_000001_bootstrap_control_schema;
mod m20260326_000002_request_context_schema;
mod m20260326_000003_catalog_schema;
mod m20260326_000004_request_ledger_schema;
mod m20260326_000005_operational_schema;

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
        ]
    }
}
