use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Systems::Table)
                    .if_not_exists()
                    .col(primary_id_column(Systems::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(Systems::DeletedAt))
                    .col(ColumnDef::new(Systems::Key).text().not_null())
                    .col(ColumnDef::new(Systems::Value).text().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_systems_key")
                    .table(Systems::Table)
                    .col(Systems::Key)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(DataStorages::Table)
                    .if_not_exists()
                    .col(primary_id_column(DataStorages::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(DataStorages::DeletedAt))
                    .col(ColumnDef::new(DataStorages::Name).text().not_null())
                    .col(ColumnDef::new(DataStorages::Description).text().not_null())
                    .col(
                        ColumnDef::new(DataStorages::Primary)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(DataStorages::Type).text().not_null())
                    .col(ColumnDef::new(DataStorages::Settings).text().not_null())
                    .col(
                        ColumnDef::new(DataStorages::Status)
                            .text()
                            .not_null()
                            .default("active"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_data_storages_name")
                    .table(DataStorages::Table)
                    .col(DataStorages::Name)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(primary_id_column(Users::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(Users::DeletedAt))
                    .col(ColumnDef::new(Users::Email).text().not_null())
                    .col(
                        ColumnDef::new(Users::Status)
                            .text()
                            .not_null()
                            .default("activated"),
                    )
                    .col(
                        ColumnDef::new(Users::PreferLanguage)
                            .text()
                            .not_null()
                            .default("en"),
                    )
                    .col(ColumnDef::new(Users::Password).text().not_null())
                    .col(
                        ColumnDef::new(Users::FirstName)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(Users::LastName)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Users::Avatar).text().null())
                    .col(
                        ColumnDef::new(Users::IsOwner)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Users::TokenVersion)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Users::Scopes)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_users_email")
                    .table(Users::Table)
                    .col(Users::Email)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(primary_id_column(Projects::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(Projects::DeletedAt))
                    .col(ColumnDef::new(Projects::Name).text().not_null())
                    .col(
                        ColumnDef::new(Projects::Description)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(Projects::Status)
                            .text()
                            .not_null()
                            .default("active"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_projects_name")
                    .table(Projects::Table)
                    .col(Projects::Name)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserProjects::Table)
                    .if_not_exists()
                    .col(primary_id_column(UserProjects::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(ColumnDef::new(UserProjects::UserId).big_integer().not_null())
                    .col(ColumnDef::new(UserProjects::ProjectId).big_integer().not_null())
                    .col(
                        ColumnDef::new(UserProjects::IsOwner)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(UserProjects::Scopes)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_projects_user_id")
                            .from(UserProjects::Table, UserProjects::UserId)
                            .to(Users::Table, Users::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_projects_project_id")
                            .from(UserProjects::Table, UserProjects::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_user_projects_user_project")
                    .table(UserProjects::Table)
                    .col(UserProjects::UserId)
                    .col(UserProjects::ProjectId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Roles::Table)
                    .if_not_exists()
                    .col(primary_id_column(Roles::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(Roles::DeletedAt))
                    .col(ColumnDef::new(Roles::Name).text().not_null())
                    .col(
                        ColumnDef::new(Roles::Level)
                            .text()
                            .not_null()
                            .default("system"),
                    )
                    .col(
                        ColumnDef::new(Roles::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Roles::Scopes)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_roles_project_name")
                    .table(Roles::Table)
                    .col(Roles::ProjectId)
                    .col(Roles::Name)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("roles_by_level")
                    .table(Roles::Table)
                    .col(Roles::Level)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserRoles::Table)
                    .if_not_exists()
                    .col(primary_id_column(UserRoles::Id))
                    .col(nullable_timestamp_column(General::CreatedAt))
                    .col(nullable_timestamp_column(General::UpdatedAt))
                    .col(ColumnDef::new(UserRoles::UserId).big_integer().not_null())
                    .col(ColumnDef::new(UserRoles::RoleId).big_integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_roles_user_id")
                            .from(UserRoles::Table, UserRoles::UserId)
                            .to(Users::Table, Users::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_roles_role_id")
                            .from(UserRoles::Table, UserRoles::RoleId)
                            .to(Roles::Table, Roles::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_user_roles_user_role")
                    .table(UserRoles::Table)
                    .col(UserRoles::UserId)
                    .col(UserRoles::RoleId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("user_roles_by_role_id")
                    .table(UserRoles::Table)
                    .col(UserRoles::RoleId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col(primary_id_column(ApiKeys::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(bigint_default_zero(ApiKeys::DeletedAt))
                    .col(ColumnDef::new(ApiKeys::UserId).big_integer().not_null())
                    .col(
                        ColumnDef::new(ApiKeys::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(ApiKeys::Key).text().not_null())
                    .col(ColumnDef::new(ApiKeys::Name).text().not_null())
                    .col(
                        ColumnDef::new(ApiKeys::Type)
                            .text()
                            .not_null()
                            .default("user"),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::Status)
                            .text()
                            .not_null()
                            .default("enabled"),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::Scopes)
                            .text()
                            .not_null()
                            .default("[\"read_channels\",\"write_requests\"]"),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::Profiles)
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_api_keys_user_id")
                            .from(ApiKeys::Table, ApiKeys::UserId)
                            .to(Users::Table, Users::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_api_keys_project_id")
                            .from(ApiKeys::Table, ApiKeys::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uk_api_keys_key")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::Key)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("api_keys_by_user_id")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("api_keys_by_project_id")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::ProjectId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("user_projects_by_project_id")
                    .table(UserProjects::Table)
                    .col(UserProjects::ProjectId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ApiKeys::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UserRoles::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Roles::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(UserProjects::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Projects::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(DataStorages::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Systems::Table).if_exists().to_owned())
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

fn nullable_timestamp_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column
        .custom(Alias::new("TEXT"))
        .null()
        .default(Expr::cust("CURRENT_TIMESTAMP::text"));
    column
}

#[derive(DeriveIden)]
enum General {
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Systems {
    Table,
    Id,
    DeletedAt,
    #[sea_orm(iden = "key")]
    Key,
    Value,
}

#[derive(DeriveIden)]
enum DataStorages {
    Table,
    Id,
    DeletedAt,
    Name,
    Description,
    #[sea_orm(iden = "primary")]
    Primary,
    #[sea_orm(iden = "type")]
    Type,
    Settings,
    Status,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    DeletedAt,
    Email,
    Status,
    PreferLanguage,
    Password,
    FirstName,
    LastName,
    Avatar,
    IsOwner,
    TokenVersion,
    Scopes,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
    DeletedAt,
    Name,
    Description,
    Status,
}

#[derive(DeriveIden)]
enum UserProjects {
    Table,
    Id,
    UserId,
    ProjectId,
    IsOwner,
    Scopes,
}

#[derive(DeriveIden)]
enum Roles {
    Table,
    Id,
    DeletedAt,
    Name,
    Level,
    ProjectId,
    Scopes,
}

#[derive(DeriveIden)]
enum UserRoles {
    Table,
    Id,
    UserId,
    RoleId,
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
    DeletedAt,
    UserId,
    ProjectId,
    Key,
    Name,
    #[sea_orm(iden = "type")]
    Type,
    Status,
    Scopes,
    Profiles,
}
