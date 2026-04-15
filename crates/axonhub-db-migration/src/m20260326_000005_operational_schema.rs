use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    .col(
                        ColumnDef::new(ChannelProbes::TotalRequestCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelProbes::SuccessRequestCount)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ChannelProbes::AvgTokensPerSecond).double().null())
                    .col(
                        ColumnDef::new(ChannelProbes::AvgTimeToFirstTokenMs)
                            .double()
                            .null(),
                    )
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
                    .col(created_at_column(General::CreatedAt))
                    .col(updated_at_column(General::UpdatedAt))
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::ChannelId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::ProviderType)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ProviderQuotaStatuses::Status).text().not_null())
                    .col(ColumnDef::new(ProviderQuotaStatuses::QuotaData).text().not_null())
                    .col(timestamp_column(ProviderQuotaStatuses::NextResetAt).null())
                    .col(
                        ColumnDef::new(ProviderQuotaStatuses::Ready)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(timestamp_column(ProviderQuotaStatuses::NextCheckAt).not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_provider_quota_statuses_channel_id")
                            .from(
                                ProviderQuotaStatuses::Table,
                                ProviderQuotaStatuses::ChannelId,
                            )
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
            .drop_table(
                Table::drop()
                    .table(ChannelProbes::Table)
                    .if_exists()
                    .to_owned(),
            )
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

fn timestamp_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column.custom(Alias::new("TEXT"));
    column
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
