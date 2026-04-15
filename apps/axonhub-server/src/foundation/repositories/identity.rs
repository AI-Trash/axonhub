use axonhub_http::{ApiKeyAuthError, AuthUserContext, ProjectContext};
use axonhub_db_entity::{api_keys, projects, roles, systems, user_projects, user_roles, users};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::foundation::{
    authz::{is_project_role_assignment, is_system_role_assignment},
    identity::{
        parse_json_string_vec, require_activated_user, QueryUserError, StoredApiKey,
        StoredProject, StoredRole, StoredUser,
    },
    seaorm::SeaOrmConnectionFactory,
};

pub(crate) trait IdentityAuthRepository: Send + Sync {
    fn query_user_by_email(&self, email: &str) -> Result<StoredUser, QueryUserError>;
    fn query_user_by_id(&self, user_id: i64) -> Result<StoredUser, QueryUserError>;
    fn query_system_value(&self, key: &str) -> Result<Option<String>, sea_orm::DbErr>;
    fn query_api_key(&self, key: &str) -> Result<StoredApiKey, ApiKeyAuthError>;
    fn query_project(&self, project_id: i64) -> Result<StoredProject, ApiKeyAuthError>;
    fn build_user_context(&self, user: StoredUser) -> Result<AuthUserContext, ()>;
    fn project_context(&self, project_id: i64) -> Result<Option<ProjectContext>, ApiKeyAuthError>;
}

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmIdentityAuthRepository {
    db: SeaOrmConnectionFactory,
}

impl SeaOrmIdentityAuthRepository {
    pub(crate) fn new(db: SeaOrmConnectionFactory) -> Self {
        Self { db }
    }
}

impl IdentityAuthRepository for SeaOrmIdentityAuthRepository {
    fn query_user_by_email(&self, email: &str) -> Result<StoredUser, QueryUserError> {
        let db = self.db.clone();
        let email = email.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| QueryUserError::Internal)?;
            query_user_by_email_seaorm(&connection, &email).await
        })
    }

    fn query_user_by_id(&self, user_id: i64) -> Result<StoredUser, QueryUserError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| QueryUserError::Internal)?;
            query_user_by_id_seaorm(&connection, user_id).await
        })
    }

    fn query_system_value(&self, key: &str) -> Result<Option<String>, sea_orm::DbErr> {
        let db = self.db.clone();
        let key = key.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await?;
            query_system_value_seaorm(&connection, &key).await
        })
    }

    fn query_api_key(&self, key: &str) -> Result<StoredApiKey, ApiKeyAuthError> {
        let db = self.db.clone();
        let key = key.to_owned();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ApiKeyAuthError::Internal)?;
            query_api_key_seaorm(&connection, &key).await
        })
    }

    fn query_project(&self, project_id: i64) -> Result<StoredProject, ApiKeyAuthError> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ApiKeyAuthError::Internal)?;
            query_project_seaorm(&connection, project_id).await
        })
    }

    fn build_user_context(&self, user: StoredUser) -> Result<AuthUserContext, ()> {
        let db = self.db.clone();
        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ())?;
            build_user_context_seaorm(&connection, user).await
        })
    }

    fn project_context(&self, project_id: i64) -> Result<Option<ProjectContext>, ApiKeyAuthError> {
        self.query_project(project_id).map(|project| {
            if project.status == "active" {
                Some(ProjectContext {
                    id: project.id,
                    name: project.name,
                    status: project.status,
                })
            } else {
                None
            }
        })
    }
}

async fn query_system_value_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    key: &str,
) -> Result<Option<String>, sea_orm::DbErr> {
    systems::Entity::find()
        .filter(systems::Column::Key.eq(key))
        .filter(systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<systems::KeyValue>()
        .one(db)
        .await
        .map(|row| row.map(|row| row.value))
}

async fn query_user_by_email_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    email: &str,
) -> Result<StoredUser, QueryUserError> {
    users::Entity::find()
        .filter(users::Column::Email.eq(email))
        .filter(users::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<users::AuthLookup>()
        .one(db)
        .await
        .map_err(|_| QueryUserError::Internal)?
        .map(stored_user_from_auth_lookup)
        .ok_or(QueryUserError::NotFound)
        .and_then(require_activated_user)
}

async fn query_user_by_id_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    user_id: i64,
) -> Result<StoredUser, QueryUserError> {
    users::Entity::find_by_id(user_id)
        .filter(users::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<users::AuthLookup>()
        .one(db)
        .await
        .map_err(|_| QueryUserError::Internal)?
        .map(stored_user_from_auth_lookup)
        .ok_or(QueryUserError::NotFound)
        .and_then(require_activated_user)
}

pub(crate) async fn query_project_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    project_id: i64,
) -> Result<StoredProject, ApiKeyAuthError> {
    projects::Entity::find_by_id(project_id)
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<projects::ContextSummary>()
        .one(db)
        .await
        .map_err(|_| ApiKeyAuthError::Internal)?
        .map(stored_project_from_context_summary)
        .ok_or(ApiKeyAuthError::Invalid)
}

async fn query_api_key_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    key: &str,
) -> Result<StoredApiKey, ApiKeyAuthError> {
    api_keys::Entity::find()
        .filter(api_keys::Column::Key.eq(key))
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<api_keys::AuthLookup>()
        .one(db)
        .await
        .map_err(|_| ApiKeyAuthError::Internal)?
        .map(stored_api_key_from_auth_lookup)
        .ok_or(ApiKeyAuthError::Invalid)
}

async fn query_user_roles_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    user_id: i64,
) -> Result<Vec<StoredRole>, ()> {
    let role_ids = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user_id))
        .select_only()
        .column(user_roles::Column::RoleId)
        .into_tuple::<i64>()
        .all(db)
        .await
        .map_err(|_| ())?;

    if role_ids.is_empty() {
        return Ok(Vec::new());
    }

    roles::Entity::find()
        .filter(roles::Column::Id.is_in(role_ids))
        .filter(roles::Column::DeletedAt.eq(0_i64))
        .order_by_asc(roles::Column::Id)
        .into_partial_model::<roles::Assignment>()
        .all(db)
        .await
        .map_err(|_| ())?
        .into_iter()
        .map(stored_role_from_assignment)
        .collect()
}

async fn build_user_context_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    user: StoredUser,
) -> Result<AuthUserContext, ()> {
    use axonhub_http::{GlobalId, RoleInfo, UserProjectInfo};

    let roles = query_user_roles_seaorm(db, user.id).await?;

    let system_roles = roles
        .iter()
        .filter(|role| is_system_role_assignment(role.project_id, role.level.as_str()))
        .map(|role| RoleInfo {
            name: role.name.clone(),
            scopes: role.scopes.clone(),
        })
        .collect::<Vec<_>>();

    let mut all_scopes = user.scopes.clone();
    for role in &roles {
        if is_system_role_assignment(role.project_id, role.level.as_str()) {
            for scope in &role.scopes {
                if !all_scopes.iter().any(|current| current == scope) {
                    all_scopes.push(scope.clone());
                }
            }
        }
    }

    let memberships = user_projects::Entity::find()
        .filter(user_projects::Column::UserId.eq(user.id))
        .order_by_asc(user_projects::Column::ProjectId)
        .into_partial_model::<user_projects::MembershipLink>()
        .all(db)
        .await
        .map_err(|_| ())?
        .into_iter()
        .map(|link| (link.project_id, link.is_owner, parse_json_string_vec(link.scopes)))
        .collect::<Vec<_>>();

    let mut projects = Vec::with_capacity(memberships.len());
    for (project_id, is_owner, scopes) in memberships {
        let project = query_project_seaorm(db, project_id).await.map_err(|_| ())?;
        let project_roles = roles
            .iter()
            .filter(|role| {
                is_project_role_assignment(role.project_id, role.level.as_str(), project_id)
            })
            .map(|role| RoleInfo {
                name: role.name.clone(),
                scopes: role.scopes.clone(),
            })
            .collect::<Vec<_>>();

        projects.push(UserProjectInfo {
            project_id: GlobalId {
                resource_type: "project".to_owned(),
                id: project.id,
            },
            is_owner,
            scopes,
            roles: project_roles,
        });
    }

    Ok(AuthUserContext {
        id: user.id,
        email: user.email,
        first_name: user.first_name,
        last_name: user.last_name,
        is_owner: user.is_owner,
        prefer_language: user.prefer_language,
        avatar: Some(user.avatar),
        scopes: all_scopes,
        roles: system_roles,
        projects,
    })
}

#[cfg(any())]
pub(crate) mod sqlite_test_support {
    use axonhub_http::ApiKeyAuthError;
    use rusqlite::{Connection as SqlConnection, Error as SqlError, Result as SqlResult};

    use super::super::super::identity::{
        parse_json_string_vec, require_activated_user, QueryUserError, StoredApiKey,
        StoredProject, StoredRole, StoredUser,
    };

    pub(crate) fn query_user_by_email(
        connection: &SqlConnection,
        email: &str,
    ) -> Result<StoredUser, QueryUserError> {
        connection
            .query_row(
                "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
                 , token_version FROM users WHERE email = ?1 AND deleted_at = 0 LIMIT 1",
                [email],
                |row| {
                    Ok(StoredUser {
                        id: row.get(0)?,
                        email: row.get(1)?,
                        status: row.get(2)?,
                        prefer_language: row.get(3)?,
                        password: row.get(4)?,
                        first_name: row.get(5)?,
                        last_name: row.get(6)?,
                        avatar: row.get(7)?,
                        is_owner: row.get::<_, i64>(8)? != 0,
                        scopes: parse_json_string_vec(row.get::<_, String>(9)?),
                        token_version: row.get(10)?,
                    })
                },
            )
            .map_err(|error| match error {
                SqlError::QueryReturnedNoRows => QueryUserError::NotFound,
                _ => QueryUserError::Internal,
            })
            .and_then(require_activated_user)
    }

    pub(crate) fn query_user_by_id(
        connection: &SqlConnection,
        user_id: i64,
    ) -> Result<StoredUser, QueryUserError> {
        connection
            .query_row(
                "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
                 , token_version FROM users WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
                [user_id],
                |row| {
                    Ok(StoredUser {
                        id: row.get(0)?,
                        email: row.get(1)?,
                        status: row.get(2)?,
                        prefer_language: row.get(3)?,
                        password: row.get(4)?,
                        first_name: row.get(5)?,
                        last_name: row.get(6)?,
                        avatar: row.get(7)?,
                        is_owner: row.get::<_, i64>(8)? != 0,
                        scopes: parse_json_string_vec(row.get::<_, String>(9)?),
                        token_version: row.get(10)?,
                    })
                },
            )
            .map_err(|error| match error {
                SqlError::QueryReturnedNoRows => QueryUserError::NotFound,
                _ => QueryUserError::Internal,
            })
            .and_then(require_activated_user)
    }

    pub(crate) fn query_project(
        connection: &SqlConnection,
        project_id: i64,
    ) -> Result<StoredProject, ApiKeyAuthError> {
        connection
            .query_row(
                "SELECT id, name, status FROM projects WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
                [project_id],
                |row| {
                    Ok(StoredProject {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        status: row.get(2)?,
                    })
                },
            )
            .map_err(|error| match error {
                SqlError::QueryReturnedNoRows => ApiKeyAuthError::Invalid,
                _ => ApiKeyAuthError::Internal,
            })
    }

    pub(crate) fn query_api_key(
        connection: &SqlConnection,
        key: &str,
    ) -> Result<StoredApiKey, ApiKeyAuthError> {
        connection
            .query_row(
                "SELECT id, user_id, key, name, type, status, project_id, scopes, profiles
                 FROM api_keys WHERE key = ?1 AND deleted_at = 0 LIMIT 1",
                [key],
                |row| {
                    Ok(StoredApiKey {
                        id: row.get(0)?,
                        user_id: row.get(1)?,
                        key: row.get(2)?,
                        name: row.get(3)?,
                        key_type: row.get(4)?,
                        status: row.get(5)?,
                        project_id: row.get(6)?,
                        scopes: parse_json_string_vec(row.get::<_, String>(7)?),
                        profiles: Some(row.get(8)?),
                    })
                },
            )
            .map_err(|error| match error {
                SqlError::QueryReturnedNoRows => ApiKeyAuthError::Invalid,
                _ => ApiKeyAuthError::Internal,
            })
    }

    pub(crate) fn query_user_roles(
        connection: &SqlConnection,
        user_id: i64,
    ) -> SqlResult<Vec<StoredRole>> {
        let mut statement = connection.prepare(
            "SELECT r.name, r.level, r.project_id, r.scopes
             FROM roles r
             JOIN user_roles ur ON ur.role_id = r.id
             WHERE ur.user_id = ?1 AND r.deleted_at = 0
             ORDER BY r.id ASC",
        )?;
        let rows = statement.query_map([user_id], |row| {
            Ok(StoredRole {
                name: row.get(0)?,
                level: row.get(1)?,
                project_id: row.get(2)?,
                scopes: parse_json_string_vec(row.get::<_, String>(3)?),
            })
        })?;
        rows.collect()
    }
}

fn stored_user_from_auth_lookup(user: users::AuthLookup) -> StoredUser {
    StoredUser {
        id: user.id,
        email: user.email,
        status: user.status,
        prefer_language: user.prefer_language,
        password: user.password,
        first_name: user.first_name,
        last_name: user.last_name,
        avatar: user.avatar.unwrap_or_default(),
        is_owner: user.is_owner,
        token_version: user.token_version,
        scopes: parse_json_string_vec(user.scopes),
    }
}

fn stored_project_from_context_summary(project: projects::ContextSummary) -> StoredProject {
    StoredProject {
        id: project.id,
        name: project.name,
        status: project.status,
    }
}

fn stored_api_key_from_auth_lookup(api_key: api_keys::AuthLookup) -> StoredApiKey {
    StoredApiKey {
        id: api_key.id,
        user_id: api_key.user_id,
        key: api_key.key,
        name: api_key.name,
        key_type: api_key.key_type,
        status: api_key.status,
        project_id: api_key.project_id,
        scopes: parse_json_string_vec(api_key.scopes),
        profiles: Some(api_key.profiles),
    }
}

fn stored_role_from_assignment(role: roles::Assignment) -> Result<StoredRole, ()> {
    Ok(StoredRole {
        name: role.name,
        level: role.level,
        project_id: role.project_id,
        scopes: parse_json_string_vec(role.scopes),
    })
}
