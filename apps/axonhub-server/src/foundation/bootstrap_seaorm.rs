use axonhub_db_entity::{api_keys, data_storages, projects, roles, systems, user_projects, users};
use axonhub_http::InitializeSystemRequest;
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};

use super::{
    authz::{
        serialize_scope_slugs, ScopeLevel, ScopeSlug, DEFAULT_SERVICE_API_KEY_SCOPES,
        DEFAULT_USER_API_KEY_SCOPES, NO_AUTH_API_KEY_SCOPES, PROJECT_ADMIN_SCOPES,
        PROJECT_DEVELOPER_SCOPES, PROJECT_VIEWER_SCOPES, ROLE_LEVEL_PROJECT,
    },
    passwords::{generate_secret_key, hash_password},
    seaorm::SeaOrmConnectionFactory,
    shared::{
        DEFAULT_PROJECT_DESCRIPTION, DEFAULT_PROJECT_NAME, DEFAULT_SERVICE_API_KEY_NAME,
        DEFAULT_SERVICE_API_KEY_VALUE, DEFAULT_USER_API_KEY_NAME, DEFAULT_USER_API_KEY_VALUE,
        NO_AUTH_API_KEY_NAME, NO_AUTH_API_KEY_VALUE, PRIMARY_DATA_STORAGE_DESCRIPTION,
        PRIMARY_DATA_STORAGE_NAME, PRIMARY_DATA_STORAGE_SETTINGS_JSON, SYSTEM_KEY_BRAND_NAME,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE, SYSTEM_KEY_INITIALIZED, SYSTEM_KEY_ONBOARDED,
        SYSTEM_KEY_SECRET_KEY, SYSTEM_KEY_VERSION,
    },
};
use crate::foundation::request_context::{OnboardingRecord, serialize_onboarding_record};

pub(crate) type SeaOrmDbFactory = SeaOrmConnectionFactory;

pub(crate) async fn seaorm_is_initialized(
    dbf: &SeaOrmDbFactory,
) -> Result<bool, sea_orm::DbErr> {
    let db = dbf.connect().await?;
    query_is_initialized_seaorm(&db).await
}

pub(crate) async fn seaorm_initialize(
    dbf: &SeaOrmDbFactory,
    version: &str,
    request: &InitializeSystemRequest,
) -> Result<(), sea_orm::DbErr> {
    let db = dbf.connect_migrated().await?;
    let tx = db.begin().await?;

    if query_is_initialized_seaorm(&tx).await? {
        return Err(sea_orm::DbErr::Custom("system already initialized".to_owned()));
    }

    let primary_data_storage_id = ensure_primary_data_storage_seaorm(&tx).await?;
    let owner_user_id = ensure_owner_user_seaorm(&tx, request).await?;
    let default_project_id = ensure_default_project_seaorm(&tx).await?;
    ensure_default_project_roles_seaorm(&tx, default_project_id).await?;
    ensure_owner_project_membership_seaorm(&tx, owner_user_id, default_project_id).await?;
    ensure_default_api_keys_seaorm(&tx, owner_user_id, default_project_id).await?;

    let secret =
        generate_secret_key().map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
    upsert_system_value_seaorm(&tx, SYSTEM_KEY_SECRET_KEY, &secret).await?;
    upsert_system_value_seaorm(&tx, SYSTEM_KEY_BRAND_NAME, request.brand_name.trim()).await?;
    upsert_system_value_seaorm(&tx, SYSTEM_KEY_VERSION, version).await?;
    upsert_system_value_seaorm(
        &tx,
        SYSTEM_KEY_DEFAULT_DATA_STORAGE,
        &primary_data_storage_id.to_string(),
    )
    .await?;
    let onboarding = default_onboarding_record();
    let onboarding = serialize_onboarding_record(&onboarding)
        .map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
    upsert_system_value_seaorm(&tx, SYSTEM_KEY_ONBOARDED, &onboarding).await?;
    upsert_system_value_seaorm(&tx, SYSTEM_KEY_INITIALIZED, "true").await?;

    tx.commit().await
}

pub(crate) async fn query_is_initialized_seaorm<C>(db: &C) -> Result<bool, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let value = match systems::Entity::find()
        .filter(systems::Column::Key.eq(SYSTEM_KEY_INITIALIZED))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<systems::KeyValue>()
        .one(db)
        .await
    {
        Ok(value) => value.map(|row| row.value),
        Err(error) if is_missing_systems_table_error(&error) => return Ok(false),
        Err(error) => return Err(error),
    };

    Ok(value
        .map(|current| current.eq_ignore_ascii_case("true"))
        .unwrap_or(false))
}

fn is_missing_systems_table_error(error: &sea_orm::DbErr) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("relation \"systems\" does not exist")
        || message.contains("table \"systems\" does not exist")
}

pub(crate) async fn upsert_system_value_seaorm<C>(
    db: &C,
    key: &str,
    value: &str,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let existing_id = systems::Entity::find()
        .filter(systems::Column::Key.eq(key))
        .select_only()
        .column(systems::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?;

    if let Some(existing_id) = existing_id {
        systems::Entity::update(systems::ActiveModel {
            id: Set(existing_id),
            value: Set(value.to_owned()),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(db)
        .await?;
        return Ok(());
    }

    systems::Entity::insert(systems::ActiveModel {
        key: Set(key.to_owned()),
        value: Set(value.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|_| ())
}

pub(crate) async fn ensure_primary_data_storage_seaorm<C>(db: &C) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    if let Some(storage_id) = data_storages::Entity::find()
        .filter(data_storages::Column::PrimaryFlag.eq(true))
        .filter(data_storages::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column(data_storages::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?
    {
        return Ok(storage_id);
    }

    data_storages::Entity::insert(data_storages::ActiveModel {
        name: Set(PRIMARY_DATA_STORAGE_NAME.to_owned()),
        description: Set(PRIMARY_DATA_STORAGE_DESCRIPTION.to_owned()),
        primary_flag: Set(true),
        type_field: Set("database".to_owned()),
        settings: Set(PRIMARY_DATA_STORAGE_SETTINGS_JSON.to_owned()),
        status: Set("active".to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await?;

    data_storages::Entity::find()
        .filter(data_storages::Column::PrimaryFlag.eq(true))
        .filter(data_storages::Column::Name.eq(PRIMARY_DATA_STORAGE_NAME))
        .filter(data_storages::Column::DeletedAt.eq(0_i64))
        .order_by_desc(data_storages::Column::Id)
        .select_only()
        .column(data_storages::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::Custom("missing inserted primary data storage".to_owned()))
}

pub(crate) async fn ensure_owner_user_seaorm<C>(
    db: &C,
    request: &InitializeSystemRequest,
) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    if let Some(user_id) = users::Entity::find()
        .filter(users::Column::IsOwner.eq(true))
        .filter(users::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column(users::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?
    {
        return Ok(user_id);
    }

    let password_hash =
        hash_password(request.owner_password.trim()).map_err(seaorm_custom_error)?;
    users::Entity::insert(users::ActiveModel {
        email: Set(request.owner_email.trim().to_owned()),
        status: Set("activated".to_owned()),
        prefer_language: Set("en".to_owned()),
        password: Set(password_hash),
        first_name: Set(request.owner_first_name.trim().to_owned()),
        last_name: Set(request.owner_last_name.trim().to_owned()),
        avatar: Set(Some(String::new())),
        is_owner: Set(true),
        token_version: Set(0),
        scopes: Set("[]".to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await?;

    users::Entity::find()
        .filter(users::Column::Email.eq(request.owner_email.trim()))
        .filter(users::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column(users::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::Custom("missing inserted owner user".to_owned()))
}

pub(crate) async fn ensure_default_project_seaorm<C>(db: &C) -> Result<i64, sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    if let Some(project_id) = projects::Entity::find()
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .order_by_asc(projects::Column::Id)
        .select_only()
        .column(projects::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?
    {
        return Ok(project_id);
    }

    projects::Entity::insert(projects::ActiveModel {
        name: Set(DEFAULT_PROJECT_NAME.to_owned()),
        description: Set(DEFAULT_PROJECT_DESCRIPTION.to_owned()),
        status: Set("active".to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await?;

    projects::Entity::find()
        .filter(projects::Column::Name.eq(DEFAULT_PROJECT_NAME))
        .filter(projects::Column::Description.eq(DEFAULT_PROJECT_DESCRIPTION))
        .filter(projects::Column::Status.eq("active"))
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .order_by_desc(projects::Column::Id)
        .select_only()
        .column(projects::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::Custom("missing inserted default project".to_owned()))
}

pub(crate) async fn ensure_owner_project_membership_seaorm<C>(
    db: &C,
    user_id: i64,
    project_id: i64,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let existing_id = user_projects::Entity::find()
        .filter(user_projects::Column::UserId.eq(user_id))
        .filter(user_projects::Column::ProjectId.eq(project_id))
        .select_only()
        .column(user_projects::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?;

    if let Some(existing_id) = existing_id {
        user_projects::Entity::update(user_projects::ActiveModel {
            id: Set(existing_id),
            is_owner: Set(true),
            ..Default::default()
        })
        .exec(db)
        .await?;
        return Ok(());
    }

    user_projects::Entity::insert(user_projects::ActiveModel {
        user_id: Set(user_id),
        project_id: Set(project_id),
        is_owner: Set(true),
        scopes: Set("[]".to_owned()),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|_| ())
}

pub(crate) async fn ensure_default_project_roles_seaorm<C>(
    db: &C,
    project_id: i64,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    ensure_role_with_scopes_seaorm(db, "Admin", ROLE_LEVEL_PROJECT, project_id, PROJECT_ADMIN_SCOPES)
        .await?;
    ensure_role_with_scopes_seaorm(
        db,
        "Developer",
        ROLE_LEVEL_PROJECT,
        project_id,
        PROJECT_DEVELOPER_SCOPES,
    )
    .await?;
    ensure_role_with_scopes_seaorm(db, "Viewer", ROLE_LEVEL_PROJECT, project_id, PROJECT_VIEWER_SCOPES)
        .await?;
    Ok(())
}

async fn ensure_role_with_scopes_seaorm<C>(
    db: &C,
    name: &str,
    level: ScopeLevel,
    project_id: i64,
    scopes: &[ScopeSlug],
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let scopes_json = serialize_scope_slugs(scopes).map_err(seaorm_custom_error)?;
    let existing_id = roles::Entity::find()
        .filter(roles::Column::ProjectId.eq(project_id))
        .filter(roles::Column::Name.eq(name))
        .select_only()
        .column(roles::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?;

    if let Some(existing_id) = existing_id {
        roles::Entity::update(roles::ActiveModel {
            id: Set(existing_id),
            level: Set(level.as_str().to_owned()),
            scopes: Set(scopes_json),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(db)
        .await?;
        return Ok(());
    }

    roles::Entity::insert(roles::ActiveModel {
        name: Set(name.to_owned()),
        level: Set(level.as_str().to_owned()),
        project_id: Set(project_id),
        scopes: Set(scopes_json),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|_| ())
}

pub(crate) async fn ensure_default_api_keys_seaorm<C>(
    db: &C,
    user_id: i64,
    project_id: i64,
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    ensure_api_key_with_scopes_seaorm(
        db,
        user_id,
        project_id,
        DEFAULT_USER_API_KEY_VALUE,
        DEFAULT_USER_API_KEY_NAME,
        "user",
        DEFAULT_USER_API_KEY_SCOPES,
    )
    .await?;
    ensure_api_key_with_scopes_seaorm(
        db,
        user_id,
        project_id,
        DEFAULT_SERVICE_API_KEY_VALUE,
        DEFAULT_SERVICE_API_KEY_NAME,
        "service_account",
        DEFAULT_SERVICE_API_KEY_SCOPES,
    )
    .await?;
    ensure_api_key_with_scopes_seaorm(
        db,
        user_id,
        project_id,
        NO_AUTH_API_KEY_VALUE,
        NO_AUTH_API_KEY_NAME,
        "noauth",
        NO_AUTH_API_KEY_SCOPES,
    )
    .await?;
    Ok(())
}

async fn ensure_api_key_with_scopes_seaorm<C>(
    db: &C,
    user_id: i64,
    project_id: i64,
    key: &str,
    name: &str,
    key_type: &str,
    scopes: &[ScopeSlug],
) -> Result<(), sea_orm::DbErr>
where
    C: ConnectionTrait,
{
    let scopes_json = serialize_scope_slugs(scopes).map_err(seaorm_custom_error)?;
    let existing_id = api_keys::Entity::find()
        .filter(api_keys::Column::Key.eq(key))
        .select_only()
        .column(api_keys::Column::Id)
        .into_tuple::<i64>()
        .one(db)
        .await?;

    if let Some(existing_id) = existing_id {
        api_keys::Entity::update(api_keys::ActiveModel {
            id: Set(existing_id),
            name: Set(name.to_owned()),
            type_field: Set(key_type.to_owned()),
            status: Set("enabled".to_owned()),
            scopes: Set(scopes_json),
            deleted_at: Set(0_i64),
            ..Default::default()
        })
        .exec(db)
        .await?;
        return Ok(());
    }

    api_keys::Entity::insert(api_keys::ActiveModel {
        user_id: Set(user_id),
        project_id: Set(project_id),
        key: Set(key.to_owned()),
        name: Set(name.to_owned()),
        type_field: Set(key_type.to_owned()),
        status: Set("enabled".to_owned()),
        scopes: Set(scopes_json),
        profiles: Set("{}".to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map(|_| ())
}

fn seaorm_custom_error(error: impl ToString) -> sea_orm::DbErr {
    sea_orm::DbErr::Custom(error.to_string())
}

fn default_onboarding_record() -> OnboardingRecord {
    OnboardingRecord::default()
}
