use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.has_column("users", "token_version").await? {
            return Ok(());
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(
                        ColumnDef::new(Users::TokenVersion)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !manager.has_column("users", "token_version").await? {
            return Ok(());
        }

        match manager.get_database_backend() {
            DatabaseBackend::Sqlite => Ok(()),
            _ => manager
                .alter_table(
                    Table::alter()
                        .table(Users::Table)
                        .drop_column(Users::TokenVersion)
                        .to_owned(),
                )
                .await,
        }
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    TokenVersion,
}
