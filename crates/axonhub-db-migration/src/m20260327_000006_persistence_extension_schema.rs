use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        let mut prompts_created_at = timestamp_column(backend, Prompts::CreatedAt, false);
        prompts_created_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut prompts_updated_at = timestamp_column(backend, Prompts::UpdatedAt, true);
        prompts_updated_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut prompts_description = ColumnDef::new(Prompts::Description);
        prompts_description.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            prompts_description.default("");
        }

        let mut prompts_status = ColumnDef::new(Prompts::Status);
        prompts_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            prompts_status.default("disabled");
        }

        manager
            .create_table(
                Table::create()
                    .table(Prompts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Prompts::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(prompts_created_at)
                    .col(prompts_updated_at)
                    .col(
                        ColumnDef::new(Prompts::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Prompts::ProjectId).big_integer().not_null())
                    .col(ColumnDef::new(Prompts::Name).text().not_null())
                    .col(prompts_description)
                    .col(ColumnDef::new(Prompts::Role).text().not_null())
                    .col(ColumnDef::new(Prompts::Content).text().not_null())
                    .col(prompts_status)
                    .col(
                        ColumnDef::new(Prompts::Order)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Prompts::Settings).text().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_prompts_project_id")
                            .from(Prompts::Table, Prompts::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        let mut prompts_project_name_index = Index::create();
        prompts_project_name_index
            .name("prompts_by_project_id_name")
            .table(Prompts::Table)
            .unique()
            .if_not_exists()
            .col(Prompts::ProjectId);
        if matches!(backend, DatabaseBackend::MySql) {
            prompts_project_name_index
                .col((Prompts::Name, 255))
                .col(Prompts::DeletedAt);
        } else {
            prompts_project_name_index
                .col(Prompts::Name)
                .col(Prompts::DeletedAt);
        }
        manager
            .create_index(prompts_project_name_index.to_owned())
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("prompts_by_project_id")
                    .table(Prompts::Table)
                    .col(Prompts::ProjectId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        let mut prompt_protection_rules_created_at =
            timestamp_column(backend, PromptProtectionRules::CreatedAt, false);
        prompt_protection_rules_created_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut prompt_protection_rules_updated_at =
            timestamp_column(backend, PromptProtectionRules::UpdatedAt, true);
        prompt_protection_rules_updated_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut prompt_protection_rules_description =
            ColumnDef::new(PromptProtectionRules::Description);
        prompt_protection_rules_description.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            prompt_protection_rules_description.default("");
        }

        let mut prompt_protection_rules_status = ColumnDef::new(PromptProtectionRules::Status);
        prompt_protection_rules_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            prompt_protection_rules_status.default("disabled");
        }

        manager
            .create_table(
                Table::create()
                    .table(PromptProtectionRules::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PromptProtectionRules::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(prompt_protection_rules_created_at)
                    .col(prompt_protection_rules_updated_at)
                    .col(
                        ColumnDef::new(PromptProtectionRules::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(PromptProtectionRules::Name)
                            .text()
                            .not_null(),
                    )
                    .col(prompt_protection_rules_description)
                    .col(
                        ColumnDef::new(PromptProtectionRules::Pattern)
                            .text()
                            .not_null(),
                    )
                    .col(prompt_protection_rules_status)
                    .col(
                        ColumnDef::new(PromptProtectionRules::Settings)
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        let mut prompt_protection_rules_name_index = Index::create();
        prompt_protection_rules_name_index
            .name("prompt_protection_rules_by_name")
            .table(PromptProtectionRules::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            prompt_protection_rules_name_index
                .col((PromptProtectionRules::Name, 255))
                .col(PromptProtectionRules::DeletedAt);
        } else {
            prompt_protection_rules_name_index
                .col(PromptProtectionRules::Name)
                .col(PromptProtectionRules::DeletedAt);
        }
        manager
            .create_index(prompt_protection_rules_name_index.to_owned())
            .await?;

        let mut channel_model_prices_created_at =
            timestamp_column(backend, ChannelModelPrices::CreatedAt, false);
        channel_model_prices_created_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut channel_model_prices_updated_at =
            timestamp_column(backend, ChannelModelPrices::UpdatedAt, true);
        channel_model_prices_updated_at
            .not_null()
            .default(Expr::current_timestamp());

        manager
            .create_table(
                Table::create()
                    .table(ChannelModelPrices::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ChannelModelPrices::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(channel_model_prices_created_at)
                    .col(channel_model_prices_updated_at)
                    .col(
                        ColumnDef::new(ChannelModelPrices::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ChannelModelPrices::ChannelId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelModelPrices::ModelId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ChannelModelPrices::Price).text().not_null())
                    .col(
                        ColumnDef::new(ChannelModelPrices::ReferenceId)
                            .text()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_model_prices_channel_id")
                            .from(ChannelModelPrices::Table, ChannelModelPrices::ChannelId)
                            .to(Channels::Table, Channels::Id),
                    )
                    .to_owned(),
            )
            .await?;

        let mut channel_model_prices_channel_model_index = Index::create();
        channel_model_prices_channel_model_index
            .name("channel_model_prices_by_channel_id_model_id")
            .table(ChannelModelPrices::Table)
            .unique()
            .if_not_exists()
            .col(ChannelModelPrices::ChannelId);
        if matches!(backend, DatabaseBackend::MySql) {
            channel_model_prices_channel_model_index
                .col((ChannelModelPrices::ModelId, 255))
                .col(ChannelModelPrices::DeletedAt);
        } else {
            channel_model_prices_channel_model_index
                .col(ChannelModelPrices::ModelId)
                .col(ChannelModelPrices::DeletedAt);
        }
        manager
            .create_index(channel_model_prices_channel_model_index.to_owned())
            .await?;

        let mut channel_model_prices_reference_id_index = Index::create();
        channel_model_prices_reference_id_index
            .name("uk_channel_model_prices_reference_id")
            .table(ChannelModelPrices::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            channel_model_prices_reference_id_index.col((ChannelModelPrices::ReferenceId, 255));
        } else {
            channel_model_prices_reference_id_index.col(ChannelModelPrices::ReferenceId);
        }
        manager
            .create_index(channel_model_prices_reference_id_index.to_owned())
            .await?;

        let mut channel_model_price_versions_created_at =
            timestamp_column(backend, ChannelModelPriceVersions::CreatedAt, false);
        channel_model_price_versions_created_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut channel_model_price_versions_updated_at =
            timestamp_column(backend, ChannelModelPriceVersions::UpdatedAt, true);
        channel_model_price_versions_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut channel_model_price_versions_effective_start_at =
            timestamp_column(backend, ChannelModelPriceVersions::EffectiveStartAt, false);
        channel_model_price_versions_effective_start_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut channel_model_price_versions_effective_end_at =
            timestamp_column(backend, ChannelModelPriceVersions::EffectiveEndAt, false);
        channel_model_price_versions_effective_end_at.null();

        manager
            .create_table(
                Table::create()
                    .table(ChannelModelPriceVersions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(channel_model_price_versions_created_at)
                    .col(channel_model_price_versions_updated_at)
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::ChannelId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::ModelId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::ChannelModelPriceId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::Price)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::Status)
                            .text()
                            .not_null(),
                    )
                    .col(channel_model_price_versions_effective_start_at)
                    .col(channel_model_price_versions_effective_end_at)
                    .col(
                        ColumnDef::new(ChannelModelPriceVersions::ReferenceId)
                            .text()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_model_price_versions_channel_model_price_id")
                            .from(
                                ChannelModelPriceVersions::Table,
                                ChannelModelPriceVersions::ChannelModelPriceId,
                            )
                            .to(ChannelModelPrices::Table, ChannelModelPrices::Id),
                    )
                    .to_owned(),
            )
            .await?;

        let mut channel_model_price_versions_reference_id_index = Index::create();
        channel_model_price_versions_reference_id_index
            .name("uk_channel_model_price_versions_reference_id")
            .table(ChannelModelPriceVersions::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            channel_model_price_versions_reference_id_index
                .col((ChannelModelPriceVersions::ReferenceId, 255));
        } else {
            channel_model_price_versions_reference_id_index
                .col(ChannelModelPriceVersions::ReferenceId);
        }
        manager
            .create_index(channel_model_price_versions_reference_id_index.to_owned())
            .await?;

        let mut channel_override_templates_created_at =
            timestamp_column(backend, ChannelOverrideTemplates::CreatedAt, false);
        channel_override_templates_created_at
            .not_null()
            .default(Expr::current_timestamp());
        let mut channel_override_templates_updated_at =
            timestamp_column(backend, ChannelOverrideTemplates::UpdatedAt, true);
        channel_override_templates_updated_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut channel_override_templates_description =
            ColumnDef::new(ChannelOverrideTemplates::Description);
        channel_override_templates_description.text().null();

        let mut channel_override_templates_override_parameters =
            ColumnDef::new(ChannelOverrideTemplates::OverrideParameters);
        channel_override_templates_override_parameters
            .text()
            .not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channel_override_templates_override_parameters.default("{}");
        }

        let mut channel_override_templates_override_headers =
            ColumnDef::new(ChannelOverrideTemplates::OverrideHeaders);
        channel_override_templates_override_headers
            .text()
            .not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channel_override_templates_override_headers.default("[]");
        }

        let mut channel_override_templates_header_override_operations =
            ColumnDef::new(ChannelOverrideTemplates::HeaderOverrideOperations);
        channel_override_templates_header_override_operations
            .text()
            .not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channel_override_templates_header_override_operations.default("[]");
        }

        let mut channel_override_templates_body_override_operations =
            ColumnDef::new(ChannelOverrideTemplates::BodyOverrideOperations);
        channel_override_templates_body_override_operations
            .text()
            .not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            channel_override_templates_body_override_operations.default("[]");
        }

        manager
            .create_table(
                Table::create()
                    .table(ChannelOverrideTemplates::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ChannelOverrideTemplates::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(channel_override_templates_created_at)
                    .col(channel_override_templates_updated_at)
                    .col(
                        ColumnDef::new(ChannelOverrideTemplates::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ChannelOverrideTemplates::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ChannelOverrideTemplates::Name)
                            .text()
                            .not_null(),
                    )
                    .col(channel_override_templates_description)
                    .col(channel_override_templates_override_parameters)
                    .col(channel_override_templates_override_headers)
                    .col(channel_override_templates_header_override_operations)
                    .col(channel_override_templates_body_override_operations)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_channel_override_templates_user_id")
                            .from(
                                ChannelOverrideTemplates::Table,
                                ChannelOverrideTemplates::UserId,
                            )
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;

        let mut channel_override_templates_user_name_index = Index::create();
        channel_override_templates_user_name_index
            .name("channel_override_templates_by_user_name")
            .table(ChannelOverrideTemplates::Table)
            .unique()
            .if_not_exists()
            .col(ChannelOverrideTemplates::UserId);
        if matches!(backend, DatabaseBackend::MySql) {
            channel_override_templates_user_name_index
                .col((ChannelOverrideTemplates::Name, 255))
                .col(ChannelOverrideTemplates::DeletedAt);
        } else {
            channel_override_templates_user_name_index
                .col(ChannelOverrideTemplates::Name)
                .col(ChannelOverrideTemplates::DeletedAt);
        }
        manager
            .create_index(channel_override_templates_user_name_index.to_owned())
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(ChannelOverrideTemplates::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(ChannelModelPriceVersions::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(ChannelModelPrices::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(PromptProtectionRules::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Prompts::Table).if_exists().to_owned())
            .await
    }
}

fn timestamp_column(backend: DatabaseBackend, iden: impl IntoIden, on_update: bool) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    match backend {
        DatabaseBackend::Sqlite => {
            column.custom(Alias::new("TEXT"));
        }
        DatabaseBackend::Postgres => {
            column.custom(Alias::new("TEXT"));
        }
        DatabaseBackend::MySql => {
            column.timestamp();
            if on_update {
                column.extra("ON UPDATE CURRENT_TIMESTAMP");
            }
        }
        _ => unreachable!("unsupported database backend: {:?}", backend),
    };
    column
}

#[derive(DeriveIden)]
enum Prompts {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
    ProjectId,
    Name,
    Description,
    Role,
    Content,
    Status,
    Order,
    Settings,
}

#[derive(DeriveIden)]
enum PromptProtectionRules {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
    Name,
    Description,
    Pattern,
    Status,
    Settings,
}

#[derive(DeriveIden)]
enum ChannelModelPrices {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
    ChannelId,
    ModelId,
    Price,
    ReferenceId,
}

#[derive(DeriveIden)]
enum ChannelModelPriceVersions {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    ChannelId,
    ModelId,
    ChannelModelPriceId,
    Price,
    Status,
    EffectiveStartAt,
    EffectiveEndAt,
    ReferenceId,
}

#[derive(DeriveIden)]
enum ChannelOverrideTemplates {
    Table,
    Id,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
    UserId,
    Name,
    Description,
    OverrideParameters,
    OverrideHeaders,
    HeaderOverrideOperations,
    BodyOverrideOperations,
}

#[derive(DeriveIden)]
enum Projects {
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
