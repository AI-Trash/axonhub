use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Channels::Table)
                    .if_not_exists()
                    .col(primary_id_column(Channels::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(Channels::DeletedAt))
                    .col(ColumnDef::new(Channels::Type).text().not_null())
                    .col(ColumnDef::new(Channels::BaseUrl).text().null())
                    .col(ColumnDef::new(Channels::Name).text().not_null())
                    .col(
                        ColumnDef::new(Channels::Status)
                            .text()
                            .not_null()
                            .default("disabled"),
                    )
                    .col(ColumnDef::new(Channels::Credentials).text().not_null())
                    .col(
                        ColumnDef::new(Channels::DisabledApiKeys)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(ColumnDef::new(Channels::SupportedModels).text().not_null())
                    .col(
                        ColumnDef::new(Channels::ManualModels)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(
                        ColumnDef::new(Channels::AutoSyncSupportedModels)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Channels::AutoSyncModelPattern)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(Channels::Tags)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(ColumnDef::new(Channels::DefaultTestModel).text().not_null())
                    .col(
                        ColumnDef::new(Channels::Policies)
                            .text()
                            .not_null()
                            .default("{\"stream\":\"unlimited\"}"),
                    )
                    .col(
                        ColumnDef::new(Channels::Settings)
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(
                        ColumnDef::new(Channels::OrderingWeight)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Channels::ErrorMessage).text().null())
                    .col(ColumnDef::new(Channels::Remark).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_channels_name_deleted_at")
                    .table(Channels::Table)
                    .col(Channels::Name)
                    .col(Channels::DeletedAt)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Models::Table)
                    .if_not_exists()
                    .col(primary_id_column(Models::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(Models::DeletedAt))
                    .col(ColumnDef::new(Models::Developer).text().not_null())
                    .col(ColumnDef::new(Models::ModelId).text().not_null())
                    .col(
                        ColumnDef::new(Models::Type)
                            .text()
                            .not_null()
                            .default("chat"),
                    )
                    .col(ColumnDef::new(Models::Name).text().not_null())
                    .col(ColumnDef::new(Models::Icon).text().not_null())
                    .col(ColumnDef::new(Models::Group).text().not_null())
                    .col(ColumnDef::new(Models::ModelCard).text().not_null())
                    .col(ColumnDef::new(Models::Settings).text().not_null())
                    .col(
                        ColumnDef::new(Models::Status)
                            .text()
                            .not_null()
                            .default("disabled"),
                    )
                    .col(ColumnDef::new(Models::Remark).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_models_name_deleted_at")
                    .table(Models::Table)
                    .col(Models::Name)
                    .col(Models::DeletedAt)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_models_model_id_deleted_at")
                    .table(Models::Table)
                    .col(Models::ModelId)
                    .col(Models::DeletedAt)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Models::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Channels::Table).if_exists().to_owned())
            .await
    }
}

fn primary_id_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column.big_integer().not_null().auto_increment().primary_key();
    column
}

fn bigint_default_zero(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column.big_integer().not_null().default(0);
    column
}

fn timestamp_column(iden: impl IntoIden) -> ColumnDef {
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
enum Channels {
    Table,
    Id,
    DeletedAt,
    #[sea_orm(iden = "type")]
    Type,
    BaseUrl,
    Name,
    Status,
    Credentials,
    DisabledApiKeys,
    SupportedModels,
    ManualModels,
    AutoSyncSupportedModels,
    AutoSyncModelPattern,
    Tags,
    DefaultTestModel,
    Policies,
    Settings,
    OrderingWeight,
    ErrorMessage,
    Remark,
}

#[derive(DeriveIden)]
enum Models {
    Table,
    Id,
    DeletedAt,
    Developer,
    ModelId,
    #[sea_orm(iden = "type")]
    Type,
    Name,
    Icon,
    #[sea_orm(iden = "group")]
    Group,
    ModelCard,
    Settings,
    Status,
    Remark,
}
