use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        manager
            .create_table(
                Table::create()
                    .table(ChannelProbes::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ChannelProbes::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ChannelProbes::ChannelId).big_integer().not_null())
                    .col(ColumnDef::new(ChannelProbes::TotalRequestCount).integer().not_null())
                    .col(ColumnDef::new(ChannelProbes::SuccessRequestCount).integer().not_null())
                    .col(ColumnDef::new(ChannelProbes::AvgTokensPerSecond).double().null())
                    .col(ColumnDef::new(ChannelProbes::AvgTimeToFirstTokenMs).double().null())
                    .col(ColumnDef::new(ChannelProbes::Timestamp).big_integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_probes_channel_id")
                            .from(ChannelProbes::Table, ChannelProbes::ChannelId)
                            .to(Channels::Table, Channels::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_channel_probes_channel_timestamp")
                    .table(ChannelProbes::Table)
                    .col(ChannelProbes::ChannelId)
                    .col(ChannelProbes::Timestamp)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        let mut provider_quota_statuses_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => provider_quota_statuses_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => provider_quota_statuses_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => provider_quota_statuses_created_at.timestamp(),
        };
        provider_quota_statuses_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut provider_quota_statuses_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => provider_quota_statuses_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => provider_quota_statuses_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => provider_quota_statuses_updated_at.timestamp(),
        };
        provider_quota_statuses_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            provider_quota_statuses_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut provider_quota_statuses_next_reset_at = ColumnDef::new(ProviderQuotaStatuses::NextResetAt);
        match backend {
            DatabaseBackend::Sqlite => provider_quota_statuses_next_reset_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => provider_quota_statuses_next_reset_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => provider_quota_statuses_next_reset_at.timestamp(),
        };
        provider_quota_statuses_next_reset_at.null();

        let mut provider_quota_statuses_next_check_at = ColumnDef::new(ProviderQuotaStatuses::NextCheckAt);
        match backend {
            DatabaseBackend::Sqlite => provider_quota_statuses_next_check_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => provider_quota_statuses_next_check_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => provider_quota_statuses_next_check_at.timestamp(),
        };
        provider_quota_statuses_next_check_at.not_null();

        manager
            .create_table(
                Table::create()
                    .table(ProviderQuotaStatuses::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(provider_quota_statuses_created_at)
                    .col(provider_quota_statuses_updated_at)
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(ProviderQuotaStatuses::ChannelId).big_integer().not_null())
                    .col(ColumnDef::new(ProviderQuotaStatuses::ProviderType).text().not_null())
                    .col(ColumnDef::new(ProviderQuotaStatuses::Status).text().not_null())
                    .col(ColumnDef::new(ProviderQuotaStatuses::QuotaData).text().not_null())
                    .col(provider_quota_statuses_next_reset_at)
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::Ready)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(provider_quota_statuses_next_check_at)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_provider_quota_statuses_channel_id")
                            .from(ProviderQuotaStatuses::Table, ProviderQuotaStatuses::ChannelId)
                            .to(Channels::Table, Channels::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_provider_quota_statuses_channel_id")
                    .table(ProviderQuotaStatuses::Table)
                    .col(ProviderQuotaStatuses::ChannelId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_provider_quota_statuses_next_check_at")
                    .table(ProviderQuotaStatuses::Table)
                    .col(ProviderQuotaStatuses::NextCheckAt)
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
                    .table(ProviderQuotaStatuses::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ChannelProbes::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum General {
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Channels {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum ChannelProbes {
    Table,
    Id,
    ChannelId,
    TotalRequestCount,
    SuccessRequestCount,
    AvgTokensPerSecond,
    AvgTimeToFirstTokenMs,
    Timestamp,
}

#[derive(DeriveIden)]
enum ProviderQuotaStatuses {
    Table,
    Id,
    DeletedAt,
    ChannelId,
    ProviderType,
    Status,
    QuotaData,
    NextResetAt,
    Ready,
    NextCheckAt,
}
