use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        let mut systems_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => systems_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => systems_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => systems_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        systems_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut systems_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => systems_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => systems_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => systems_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        systems_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            systems_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        manager
            .create_table(
                Table::create()
                    .table(Systems::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Systems::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(systems_created_at)
                    .col(systems_updated_at)
                    .col(
                        ColumnDef::new(Systems::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Systems::Key).text().not_null())
                    .col(ColumnDef::new(Systems::Value).text().not_null())
                    .to_owned(),
            )
            .await?;

        let mut systems_key_index = Index::create();
        systems_key_index
            .name("uk_systems_key")
            .table(Systems::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            systems_key_index.col((Systems::Key, 255));
        } else {
            systems_key_index.col(Systems::Key);
        }
        manager.create_index(systems_key_index.to_owned()).await?;

        let mut data_storages_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => data_storages_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => data_storages_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => data_storages_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        data_storages_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut data_storages_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => data_storages_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => data_storages_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => data_storages_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        data_storages_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            data_storages_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut data_storages_status = ColumnDef::new(DataStorages::Status);
        data_storages_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            data_storages_status.default("active");
        }

        manager
            .create_table(
                Table::create()
                    .table(DataStorages::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DataStorages::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(data_storages_created_at)
                    .col(data_storages_updated_at)
                    .col(
                        ColumnDef::new(DataStorages::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
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
                    .col(data_storages_status)
                    .to_owned(),
            )
            .await?;

        let mut data_storages_name_index = Index::create();
        data_storages_name_index
            .name("uk_data_storages_name")
            .table(DataStorages::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            data_storages_name_index.col((DataStorages::Name, 255));
        } else {
            data_storages_name_index.col(DataStorages::Name);
        }
        manager
            .create_index(data_storages_name_index.to_owned())
            .await?;

        let mut users_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => users_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => users_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => users_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        users_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut users_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => users_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => users_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => users_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        users_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            users_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut users_status = ColumnDef::new(Users::Status);
        users_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            users_status.default("activated");
        }

        let mut users_prefer_language = ColumnDef::new(Users::PreferLanguage);
        users_prefer_language.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            users_prefer_language.default("en");
        }

        let mut users_first_name = ColumnDef::new(Users::FirstName);
        users_first_name.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            users_first_name.default("");
        }

        let mut users_last_name = ColumnDef::new(Users::LastName);
        users_last_name.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            users_last_name.default("");
        }

        let mut users_avatar = ColumnDef::new(Users::Avatar);
        if matches!(backend, DatabaseBackend::MySql) {
            users_avatar.custom(Alias::new("MEDIUMTEXT"));
        } else {
            users_avatar.text();
        }
        users_avatar.null();

        let mut users_scopes = ColumnDef::new(Users::Scopes);
        users_scopes.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            users_scopes.default("[]");
        }

        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Users::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(users_created_at)
                    .col(users_updated_at)
                    .col(
                        ColumnDef::new(Users::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Users::Email).text().not_null())
                    .col(users_status)
                    .col(users_prefer_language)
                    .col(ColumnDef::new(Users::Password).text().not_null())
                    .col(users_first_name)
                    .col(users_last_name)
                    .col(users_avatar)
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
                    .col(users_scopes)
                    .to_owned(),
            )
            .await?;

        let mut users_email_index = Index::create();
        users_email_index
            .name("uk_users_email")
            .table(Users::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            users_email_index.col((Users::Email, 255));
        } else {
            users_email_index.col(Users::Email);
        }
        manager.create_index(users_email_index.to_owned()).await?;

        let mut projects_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => projects_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => projects_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => projects_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        projects_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut projects_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => projects_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => projects_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => projects_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        projects_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            projects_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut projects_description = ColumnDef::new(Projects::Description);
        projects_description.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            projects_description.default("");
        }

        let mut projects_status = ColumnDef::new(Projects::Status);
        projects_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            projects_status.default("active");
        }

        manager
            .create_table(
                Table::create()
                    .table(Projects::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Projects::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(projects_created_at)
                    .col(projects_updated_at)
                    .col(
                        ColumnDef::new(Projects::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Projects::Name).text().not_null())
                    .col(projects_description)
                    .col(projects_status)
                    .to_owned(),
            )
            .await?;

        let mut projects_name_index = Index::create();
        projects_name_index
            .name("uk_projects_name")
            .table(Projects::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            projects_name_index.col((Projects::Name, 255));
        } else {
            projects_name_index.col(Projects::Name);
        }
        manager.create_index(projects_name_index.to_owned()).await?;

        let mut user_projects_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => user_projects_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => user_projects_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => user_projects_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        user_projects_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut user_projects_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => user_projects_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => user_projects_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => user_projects_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        user_projects_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            user_projects_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut user_projects_scopes = ColumnDef::new(UserProjects::Scopes);
        user_projects_scopes.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            user_projects_scopes.default("[]");
        }

        manager
            .create_table(
                Table::create()
                    .table(UserProjects::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserProjects::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(user_projects_created_at)
                    .col(user_projects_updated_at)
                    .col(
                        ColumnDef::new(UserProjects::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserProjects::ProjectId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserProjects::IsOwner)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(user_projects_scopes)
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

        let mut roles_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => roles_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => roles_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => roles_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        roles_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut roles_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => roles_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => roles_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => roles_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        roles_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            roles_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut roles_level = ColumnDef::new(Roles::Level);
        roles_level.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            roles_level.default("system");
        }

        let mut roles_scopes = ColumnDef::new(Roles::Scopes);
        roles_scopes.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            roles_scopes.default("[]");
        }

        manager
            .create_table(
                Table::create()
                    .table(Roles::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Roles::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(roles_created_at)
                    .col(roles_updated_at)
                    .col(
                        ColumnDef::new(Roles::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Roles::Name).text().not_null())
                    .col(roles_level)
                    .col(
                        ColumnDef::new(Roles::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(roles_scopes)
                    .to_owned(),
            )
            .await?;

        let mut roles_project_name_index = Index::create();
        roles_project_name_index
            .name("uk_roles_project_name")
            .table(Roles::Table)
            .col(Roles::ProjectId)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            roles_project_name_index.col((Roles::Name, 255));
        } else {
            roles_project_name_index.col(Roles::Name);
        }
        manager
            .create_index(roles_project_name_index.to_owned())
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

        let mut user_roles_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => user_roles_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => user_roles_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => user_roles_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        user_roles_created_at
            .null()
            .default(Expr::current_timestamp());

        let mut user_roles_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => user_roles_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => user_roles_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => user_roles_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        user_roles_updated_at
            .null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            user_roles_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        manager
            .create_table(
                Table::create()
                    .table(UserRoles::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserRoles::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(user_roles_created_at)
                    .col(user_roles_updated_at)
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

        let mut api_keys_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => api_keys_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => api_keys_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => api_keys_created_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        api_keys_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut api_keys_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => api_keys_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => api_keys_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => api_keys_updated_at.timestamp(),
            _ => unreachable!("unsupported database backend: {:?}", backend),
        };
        api_keys_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            api_keys_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut api_keys_type = ColumnDef::new(ApiKeys::Type);
        api_keys_type.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            api_keys_type.default("user");
        }

        let mut api_keys_status = ColumnDef::new(ApiKeys::Status);
        api_keys_status.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            api_keys_status.default("enabled");
        }

        let mut api_keys_scopes = ColumnDef::new(ApiKeys::Scopes);
        api_keys_scopes.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            api_keys_scopes.default("[\"read_channels\",\"write_requests\"]");
        }

        let mut api_keys_profiles = ColumnDef::new(ApiKeys::Profiles);
        api_keys_profiles.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            api_keys_profiles.default("{}");
        }

        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ApiKeys::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(api_keys_created_at)
                    .col(api_keys_updated_at)
                    .col(
                        ColumnDef::new(ApiKeys::DeletedAt)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(ApiKeys::UserId).big_integer().not_null())
                    .col(
                        ColumnDef::new(ApiKeys::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(ApiKeys::Key).text().not_null())
                    .col(ColumnDef::new(ApiKeys::Name).text().not_null())
                    .col(api_keys_type)
                    .col(api_keys_status)
                    .col(api_keys_scopes)
                    .col(api_keys_profiles)
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

        let mut api_keys_key_index = Index::create();
        api_keys_key_index
            .name("uk_api_keys_key")
            .table(ApiKeys::Table)
            .unique()
            .if_not_exists();
        if matches!(backend, DatabaseBackend::MySql) {
            api_keys_key_index.col((ApiKeys::Key, 255));
        } else {
            api_keys_key_index.col(ApiKeys::Key);
        }
        manager.create_index(api_keys_key_index.to_owned()).await?;

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
    #[sea_orm(iden = "key")]
    Key,
    Name,
    #[sea_orm(iden = "type")]
    Type,
    Status,
    Scopes,
    Profiles,
}
