use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;
use sea_query::TableCreateStatement;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        manager
            .create_table(realtime_sessions_table_statement(backend).to_owned())
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_realtime_sessions_session_id")
                    .table(RealtimeSessions::Table)
                    .col(RealtimeSessions::SessionId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_realtime_sessions_project_status")
                    .table(RealtimeSessions::Table)
                    .col(RealtimeSessions::ProjectId)
                    .col(RealtimeSessions::Status)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_realtime_sessions_trace_id")
                    .table(RealtimeSessions::Table)
                    .col(RealtimeSessions::TraceId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(operational_runs_table_statement(backend).to_owned())
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_operational_runs_operation_type_started_at")
                    .table(OperationalRuns::Table)
                    .col(OperationalRuns::OperationType)
                    .col(OperationalRuns::StartedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_operational_runs_status_started_at")
                    .table(OperationalRuns::Table)
                    .col(OperationalRuns::Status)
                    .col(OperationalRuns::StartedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_operational_runs_channel_id")
                    .table(OperationalRuns::Table)
                    .col(OperationalRuns::ChannelId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(OperationalRuns::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(RealtimeSessions::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

pub(crate) fn realtime_sessions_table_statement(
    backend: DatabaseBackend,
) -> TableCreateStatement {
    let mut opened_at = timestamp_column(backend, RealtimeSessions::OpenedAt, false);
    opened_at.not_null().default(Expr::current_timestamp());

    let mut last_activity_at = timestamp_column(backend, RealtimeSessions::LastActivityAt, false);
    last_activity_at
        .not_null()
        .default(Expr::current_timestamp());

    let mut closed_at = timestamp_column(backend, RealtimeSessions::ClosedAt, false);
    closed_at.null();

    let mut expires_at = timestamp_column(backend, RealtimeSessions::ExpiresAt, false);
    expires_at.null();

    Table::create()
        .table(RealtimeSessions::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(RealtimeSessions::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key(),
        )
        .col(ColumnDef::new(RealtimeSessions::ProjectId).big_integer().not_null())
        .col(ColumnDef::new(RealtimeSessions::ThreadId).big_integer().null())
        .col(ColumnDef::new(RealtimeSessions::TraceId).big_integer().null())
        .col(ColumnDef::new(RealtimeSessions::RequestId).big_integer().null())
        .col(ColumnDef::new(RealtimeSessions::ApiKeyId).big_integer().null())
        .col(ColumnDef::new(RealtimeSessions::ChannelId).big_integer().null())
        .col(ColumnDef::new(RealtimeSessions::SessionId).text().not_null())
        .col(ColumnDef::new(RealtimeSessions::Transport).text().not_null())
        .col(ColumnDef::new(RealtimeSessions::Status).text().not_null())
        .col(ColumnDef::new(RealtimeSessions::Metadata).text().not_null())
        .col(opened_at)
        .col(last_activity_at)
        .col(closed_at)
        .col(expires_at)
        .foreign_key(
            ForeignKey::create()
                .name("fk_realtime_sessions_project_id")
                .from(RealtimeSessions::Table, RealtimeSessions::ProjectId)
                .to(Projects::Table, Projects::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_realtime_sessions_thread_id")
                .from(RealtimeSessions::Table, RealtimeSessions::ThreadId)
                .to(Threads::Table, Threads::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_realtime_sessions_trace_id")
                .from(RealtimeSessions::Table, RealtimeSessions::TraceId)
                .to(Traces::Table, Traces::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_realtime_sessions_request_id")
                .from(RealtimeSessions::Table, RealtimeSessions::RequestId)
                .to(Requests::Table, Requests::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_realtime_sessions_api_key_id")
                .from(RealtimeSessions::Table, RealtimeSessions::ApiKeyId)
                .to(ApiKeys::Table, ApiKeys::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_realtime_sessions_channel_id")
                .from(RealtimeSessions::Table, RealtimeSessions::ChannelId)
                .to(Channels::Table, Channels::Id),
        )
        .to_owned()
}

pub(crate) fn operational_runs_table_statement(
    backend: DatabaseBackend,
) -> TableCreateStatement {
    let mut started_at = timestamp_column(backend, OperationalRuns::StartedAt, false);
    started_at.not_null().default(Expr::current_timestamp());

    let mut finished_at = timestamp_column(backend, OperationalRuns::FinishedAt, false);
    finished_at.null();

    Table::create()
        .table(OperationalRuns::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(OperationalRuns::Id)
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key(),
        )
        .col(ColumnDef::new(OperationalRuns::OperationType).text().not_null())
        .col(ColumnDef::new(OperationalRuns::TriggerSource).text().not_null())
        .col(ColumnDef::new(OperationalRuns::Status).text().not_null())
        .col(ColumnDef::new(OperationalRuns::ResultPayload).text().null())
        .col(ColumnDef::new(OperationalRuns::ErrorMessage).text().null())
        .col(ColumnDef::new(OperationalRuns::InitiatedByUserId).big_integer().null())
        .col(ColumnDef::new(OperationalRuns::DataStorageId).big_integer().null())
        .col(ColumnDef::new(OperationalRuns::ChannelId).big_integer().null())
        .col(ColumnDef::new(OperationalRuns::ProjectId).big_integer().null())
        .col(started_at)
        .col(finished_at)
        .foreign_key(
            ForeignKey::create()
                .name("fk_operational_runs_initiated_by_user_id")
                .from(OperationalRuns::Table, OperationalRuns::InitiatedByUserId)
                .to(Users::Table, Users::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_operational_runs_data_storage_id")
                .from(OperationalRuns::Table, OperationalRuns::DataStorageId)
                .to(DataStorages::Table, DataStorages::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_operational_runs_channel_id")
                .from(OperationalRuns::Table, OperationalRuns::ChannelId)
                .to(Channels::Table, Channels::Id),
        )
        .foreign_key(
            ForeignKey::create()
                .name("fk_operational_runs_project_id")
                .from(OperationalRuns::Table, OperationalRuns::ProjectId)
                .to(Projects::Table, Projects::Id),
        )
        .to_owned()
}

fn timestamp_column(backend: DatabaseBackend, iden: impl IntoIden, on_update: bool) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    match backend {
        DatabaseBackend::Sqlite => {
            column.custom(Alias::new("TEXT"));
        }
        DatabaseBackend::Postgres => {
            column.timestamp_with_time_zone();
        }
        DatabaseBackend::MySql => {
            column.timestamp();
            if on_update {
                column.extra("ON UPDATE CURRENT_TIMESTAMP");
            }
        }
    };
    column
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
}

#[derive(DeriveIden)]
enum Traces {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Requests {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Channels {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum DataStorages {
    Table,
    Id,
}

#[derive(DeriveIden)]
pub enum RealtimeSessions {
    Table,
    Id,
    ProjectId,
    ThreadId,
    TraceId,
    RequestId,
    ApiKeyId,
    ChannelId,
    SessionId,
    Transport,
    Status,
    Metadata,
    OpenedAt,
    LastActivityAt,
    ClosedAt,
    ExpiresAt,
}

#[derive(DeriveIden)]
pub enum OperationalRuns {
    Table,
    Id,
    OperationType,
    TriggerSource,
    Status,
    ResultPayload,
    ErrorMessage,
    InitiatedByUserId,
    DataStorageId,
    ChannelId,
    ProjectId,
    StartedAt,
    FinishedAt,
}
