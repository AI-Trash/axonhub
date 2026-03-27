use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        let mut threads_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => threads_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => threads_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => threads_created_at.timestamp(),
        };
        threads_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut threads_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => threads_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => threads_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => threads_updated_at.timestamp(),
        };
        threads_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            threads_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

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
                    .col(threads_created_at)
                    .col(threads_updated_at)
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

        let mut threads_thread_id_index = Index::create();
        threads_thread_id_index
            .name("uk_threads_thread_id")
            .table(Threads::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            threads_thread_id_index.col((Threads::ThreadId, 255));
        } else {
            threads_thread_id_index.col(Threads::ThreadId);
        }
        manager
            .create_index(threads_thread_id_index.to_owned())
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

        let mut traces_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => traces_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => traces_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => traces_created_at.timestamp(),
        };
        traces_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut traces_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => traces_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => traces_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => traces_updated_at.timestamp(),
        };
        traces_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            traces_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

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
                    .col(traces_created_at)
                    .col(traces_updated_at)
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

        let mut traces_trace_id_index = Index::create();
        traces_trace_id_index
            .name("uk_traces_trace_id")
            .table(Traces::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            traces_trace_id_index.col((Traces::TraceId, 255));
        } else {
            traces_trace_id_index.col(Traces::TraceId);
        }
        manager
            .create_index(traces_trace_id_index.to_owned())
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
