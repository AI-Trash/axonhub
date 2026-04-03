use axonhub_db_entity::prompt_protection_rules;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder,
};

#[derive(Debug, Clone)]
pub(crate) struct StoredPromptProtectionRuleRecord {
    pub(crate) id: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) pattern: String,
    pub(crate) status: String,
    pub(crate) settings: String,
}

pub(crate) async fn list_prompt_protection_rules_seaorm(
    db: &impl ConnectionTrait,
) -> Result<Vec<StoredPromptProtectionRuleRecord>, String> {
    prompt_protection_rules::Entity::find()
        .filter(prompt_protection_rules::Column::DeletedAt.eq(0_i64))
        .order_by_desc(prompt_protection_rules::Column::CreatedAt)
        .order_by_desc(prompt_protection_rules::Column::Id)
        .all(db)
        .await
        .map_err(|error| error.to_string())
        .map(|models| models.into_iter().map(stored_rule_from_model).collect())
}

pub(crate) async fn list_enabled_prompt_protection_rules_seaorm(
    db: &impl ConnectionTrait,
) -> Result<Vec<StoredPromptProtectionRuleRecord>, String> {
    prompt_protection_rules::Entity::find()
        .filter(prompt_protection_rules::Column::DeletedAt.eq(0_i64))
        .filter(prompt_protection_rules::Column::Status.eq("enabled"))
        .order_by_desc(prompt_protection_rules::Column::CreatedAt)
        .order_by_desc(prompt_protection_rules::Column::Id)
        .all(db)
        .await
        .map_err(|error| error.to_string())
        .map(|models| models.into_iter().map(stored_rule_from_model).collect())
}

pub(crate) async fn load_prompt_protection_rule_seaorm(
    db: &impl ConnectionTrait,
    id: i64,
) -> Result<Option<StoredPromptProtectionRuleRecord>, String> {
    prompt_protection_rules::Entity::find_by_id(id)
        .filter(prompt_protection_rules::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|model| model.map(stored_rule_from_model))
}

pub(crate) async fn create_prompt_protection_rule_seaorm(
    db: &impl ConnectionTrait,
    name: &str,
    description: &str,
    pattern: &str,
    status: &str,
    settings_json: &str,
) -> Result<i64, String> {
    let created = prompt_protection_rules::Entity::insert(prompt_protection_rules::ActiveModel {
        name: Set(name.to_owned()),
        description: Set(description.to_owned()),
        pattern: Set(pattern.to_owned()),
        status: Set(status.to_owned()),
        settings: Set(settings_json.to_owned()),
        deleted_at: Set(0_i64),
        ..Default::default()
    })
    .exec(db)
    .await
    .map_err(|error| error.to_string())?;
    Ok(created.last_insert_id)
}

pub(crate) async fn update_prompt_protection_rule_seaorm(
    db: &impl ConnectionTrait,
    id: i64,
    name: Option<&str>,
    description: Option<&str>,
    pattern: Option<&str>,
    status: Option<&str>,
    settings_json: Option<&str>,
) -> Result<bool, String> {
    let Some(existing) = prompt_protection_rules::Entity::find_by_id(id)
        .filter(prompt_protection_rules::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    let mut active: prompt_protection_rules::ActiveModel = existing.into();
    if let Some(name) = name {
        active.name = Set(name.to_owned());
    }
    if let Some(description) = description {
        active.description = Set(description.to_owned());
    }
    if let Some(pattern) = pattern {
        active.pattern = Set(pattern.to_owned());
    }
    if let Some(status) = status {
        active.status = Set(status.to_owned());
    }
    if let Some(settings_json) = settings_json {
        active.settings = Set(settings_json.to_owned());
    }
    active.deleted_at = Set(0_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    Ok(true)
}

pub(crate) async fn soft_delete_prompt_protection_rule_seaorm(
    db: &impl ConnectionTrait,
    id: i64,
) -> Result<bool, String> {
    let Some(existing) = prompt_protection_rules::Entity::find_by_id(id)
        .filter(prompt_protection_rules::Column::DeletedAt.eq(0_i64))
        .one(db)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };

    let mut active: prompt_protection_rules::ActiveModel = existing.into();
    active.deleted_at = Set(1_i64);
    active.update(db).await.map_err(|error| error.to_string())?;
    Ok(true)
}

pub(crate) async fn set_prompt_protection_rule_status_seaorm(
    db: &impl ConnectionTrait,
    id: i64,
    status: &str,
) -> Result<bool, String> {
    update_prompt_protection_rule_seaorm(db, id, None, None, None, Some(status), None).await
}

pub(crate) async fn bulk_soft_delete_prompt_protection_rules_seaorm(
    db: &impl ConnectionTrait,
    ids: &[i64],
) -> Result<usize, String> {
    let mut changed = 0;
    for id in ids {
        if soft_delete_prompt_protection_rule_seaorm(db, *id).await? {
            changed += 1;
        }
    }
    Ok(changed)
}

pub(crate) async fn bulk_set_prompt_protection_rule_status_seaorm(
    db: &impl ConnectionTrait,
    ids: &[i64],
    status: &str,
) -> Result<usize, String> {
    let mut changed = 0;
    for id in ids {
        if set_prompt_protection_rule_status_seaorm(db, *id, status).await? {
            changed += 1;
        }
    }
    Ok(changed)
}

pub(crate) async fn prompt_protection_rule_name_exists_seaorm(
    db: &impl ConnectionTrait,
    name: &str,
    exclude_id: Option<i64>,
) -> Result<bool, String> {
    let mut query = prompt_protection_rules::Entity::find()
        .filter(prompt_protection_rules::Column::DeletedAt.eq(0_i64))
        .filter(prompt_protection_rules::Column::Name.eq(name));
    if let Some(exclude_id) = exclude_id {
        query = query.filter(prompt_protection_rules::Column::Id.ne(exclude_id));
    }
    query
        .one(db)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.is_some())
}

fn stored_rule_from_model(model: prompt_protection_rules::Model) -> StoredPromptProtectionRuleRecord {
    StoredPromptProtectionRuleRecord {
        id: model.id,
        created_at: model.created_at,
        updated_at: model.updated_at,
        name: model.name,
        description: model.description,
        pattern: model.pattern,
        status: model.status,
        settings: model.settings,
    }
}
