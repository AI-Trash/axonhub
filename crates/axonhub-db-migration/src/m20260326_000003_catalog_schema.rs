use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        let mut channels_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => channels_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => channels_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => channels_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        channels_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut channels_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => channels_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => channels_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => channels_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        channels_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            channels_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut channels_status = ColumnDef::new(Channels::Status);
        channels_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_status.default("disabled");
        }

        let mut channels_disabled_api_keys = ColumnDef::new(Channels::DisabledApiKeys);
        channels_disabled_api_keys.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_disabled_api_keys.default("[]");
        }

        let mut channels_manual_models = ColumnDef::new(Channels::ManualModels);
        channels_manual_models.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_manual_models.default("[]");
        }

        let mut channels_auto_sync_model_pattern = ColumnDef::new(Channels::AutoSyncModelPattern);
        channels_auto_sync_model_pattern.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_auto_sync_model_pattern.default("");
        }

        let mut channels_tags = ColumnDef::new(Channels::Tags);
        channels_tags.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_tags.default("[]");
        }

        let mut channels_policies = ColumnDef::new(Channels::Policies);
        channels_policies.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_policies.default("{\"stream\":\"unlimited\"}");
        }

        let mut channels_settings = ColumnDef::new(Channels::Settings);
        channels_settings.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channels_settings.default("{}");
        }

        manager
            .create_table(
                Table::create()
                    .table(Channels::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Channels::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(channels_created_at)
                    .col(channels_updated_at)
                    .col(
                        ColumnDef::new(Channels::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Channels::Type).text().not_null())
                    .col(ColumnDef::new(Channels::BaseUrl).text().null())
                    .col(ColumnDef::new(Channels::Name).text().not_null())
                    .col(channels_status)
                    .col(ColumnDef::new(Channels::Credentials).text().not_null())
                    .col(channels_disabled_api_keys)
                    .col(ColumnDef::new(Channels::SupportedModels).text().not_null())
                    .col(channels_manual_models)
                    .col(
                        ColumnDef::new(Channels::AutoSyncSupportedModels)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(channels_auto_sync_model_pattern)
                    .col(channels_tags)
                    .col(ColumnDef::new(Channels::DefaultTestModel).text().not_null())
                    .col(channels_policies)
                    .col(channels_settings)
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

        let mut channels_name_index = Index::create();
        channels_name_index
            .name("uk_channels_name_deleted_at")
            .table(Channels::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            channels_name_index
                .col((Channels::Name, 255))
                .col(Channels::DeletedAt);
        } else {
            channels_name_index
                .col(Channels::Name)
                .col(Channels::DeletedAt);
        }
        manager.create_index(channels_name_index.to_owned()).await?;

        let mut models_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => models_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => models_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => models_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        models_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut models_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => models_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => models_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => models_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        models_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            models_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut models_type = ColumnDef::new(Models::Type);
        models_type.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            models_type.default("chat");
        }

        let mut models_status = ColumnDef::new(Models::Status);
        models_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            models_status.default("disabled");
        }

        manager
            .create_table(
                Table::create()
                    .table(Models::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Models::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(models_created_at)
                    .col(models_updated_at)
                    .col(
                        ColumnDef::new(Models::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Models::Developer).text().not_null())
                    .col(ColumnDef::new(Models::ModelId).text().not_null())
                    .col(models_type)
                    .col(ColumnDef::new(Models::Name).text().not_null())
                    .col(ColumnDef::new(Models::Icon).text().not_null())
                    .col(ColumnDef::new(Models::Group).text().not_null())
                    .col(ColumnDef::new(Models::ModelCard).text().not_null())
                    .col(ColumnDef::new(Models::Settings).text().not_null())
                    .col(models_status)
                    .col(ColumnDef::new(Models::Remark).text().null())
                    .to_owned(),
            )
            .await?;

        let mut models_name_index = Index::create();
        models_name_index
            .name("uk_models_name_deleted_at")
            .table(Models::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            models_name_index
                .col((Models::Name, 255))
                .col(Models::DeletedAt);
        } else {
            models_name_index.col(Models::Name).col(Models::DeletedAt);
        }
        manager.create_index(models_name_index.to_owned()).await?;

        let mut models_model_id_index = Index::create();
        models_model_id_index
            .name("uk_models_model_id_deleted_at")
            .table(Models::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            models_model_id_index
                .col((Models::ModelId, 255))
                .col(Models::DeletedAt);
        } else {
            models_model_id_index
                .col(Models::ModelId)
                .col(Models::DeletedAt);
        }
        manager
            .create_index(models_model_id_index.to_owned())
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
