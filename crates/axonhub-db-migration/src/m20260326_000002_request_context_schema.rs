use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Threads::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Threads::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(created_at_column(General::CreatedAt))
                    .col(updated_at_column(General::UpdatedAt))
                    .col(ColumnDef::new(Threads::ProjectId).big_integer().not_null())
                    .col(ColumnDef::new(Threads::ThreadId).text().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_threads_project_id")
                            .from(Threads::Table, Threads::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_threads_thread_id")
                    .table(Threads::Table)
                    .col(Threads::ThreadId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("threads_by_project_id")
                    .table(Threads::Table)
                    .col(Threads::ProjectId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Traces::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Traces::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(created_at_column(General::CreatedAt))
                    .col(updated_at_column(General::UpdatedAt))
                    .col(ColumnDef::new(Traces::ProjectId).big_integer().not_null())
                    .col(ColumnDef::new(Traces::TraceId).text().not_null())
                    .col(ColumnDef::new(Traces::ThreadId).big_integer().null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_traces_project_id")
                            .from(Traces::Table, Traces::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_traces_thread_id")
                            .from(Traces::Table, Traces::ThreadId)
                            .to(Threads::Table, Threads::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_traces_trace_id")
                    .table(Traces::Table)
                    .col(Traces::TraceId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("traces_by_project_id")
                    .table(Traces::Table)
                    .col(Traces::ProjectId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("traces_by_thread_id")
                    .table(Traces::Table)
                    .col(Traces::ThreadId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Traces::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Threads::Table).if_exists().to_owned())
            .await
    }
}

fn created_at_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column
        .custom(Alias::new("TEXT"))
        .not_null()
        .default(Expr::cust("CURRENT_TIMESTAMP::text"));
    column
}

fn updated_at_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column
        .custom(Alias::new("TEXT"))
        .not_null()
        .default(Expr::cust("CURRENT_TIMESTAMP::text"));
    column
}

#[derive(DeriveIden)]
enum General {
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Threads {
    Table,
    Id,
    ProjectId,
    ThreadId,
}

#[derive(DeriveIden)]
enum Traces {
    Table,
    Id,
    ProjectId,
    TraceId,
    ThreadId,
}
