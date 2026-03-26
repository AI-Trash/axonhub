use axonhub_http::{ApiKeyAuthError, AuthUserContext, ProjectContext};
use axonhub_db_entity::{api_keys, projects, roles, systems, user_projects, users};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use sea_orm::{ColumnTrait, EntityTrait, ModelTrait, QueryFilter};

use crate::foundation::{
    authz::{is_project_role_assignment, is_system_role_assignment},
    identity::{parse_json_string_vec, QueryUserError, StoredApiKey, StoredProject, StoredRole, StoredUser},
    seaorm::SeaOrmConnectionFactory,
    shared::SYSTEM_KEY_SECRET_KEY,
};

use super::common::query_all;

pub(crate) trait IdentityAuthRepository: Send + Sync {
    fn query_user_by_email(&self, email: &str) -> Result<StoredUser, QueryUserError>;
    fn query_user_by_id(&self, user_id: i64) -> Result<StoredUser, QueryUserError>;
    fn query_system_value(&self, key: &str) -> Result<Option<String>, sea_orm::DbErr>;
    fn generate_jwt_token(&self, user_id: i64) -> Result<String, axonhub_http::SignInError>;
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

    fn generate_jwt_token(&self, user_id: i64) -> Result<String, axonhub_http::SignInError> {
        let secret = self
            .query_system_value(SYSTEM_KEY_SECRET_KEY)
            .map_err(|_| axonhub_http::SignInError::Internal)?
            .ok_or(axonhub_http::SignInError::Internal)?;
        let claims = JwtClaims {
            user_id,
            exp: (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 60 * 60 * 24 * 7) as usize,
        };

        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .map_err(|_| axonhub_http::SignInError::Internal)
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
            build_user_context_seaorm(&connection, db.backend(), user).await
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
    .and_then(|user| {
        if user.status != "activated" {
            Err(QueryUserError::InvalidPassword)
        } else {
            Ok(user)
        }
    })
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
    .and_then(|user| {
        if user.status != "activated" {
            Err(QueryUserError::InvalidPassword)
        } else {
            Ok(user)
        }
    })
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
    backend: sea_orm::DatabaseBackend,
    user_id: i64,
) -> Result<Vec<StoredRole>, ()> {
    query_all(
        db,
        backend,
        "SELECT r.name, r.level, r.project_id, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ? AND r.deleted_at = 0 ORDER BY r.id ASC",
        "SELECT r.name, r.level, r.project_id, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = $1 AND r.deleted_at = 0 ORDER BY r.id ASC",
        "SELECT r.name, r.level, r.project_id, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ? AND r.deleted_at = 0 ORDER BY r.id ASC",
        vec![user_id.into()],
    )
    .await
    .map_err(|_| ())?
    .into_iter()
    .map(stored_role_from_seaorm_row)
    .collect()
}

async fn build_user_context_seaorm(
    db: &impl sea_orm::ConnectionTrait,
    backend: sea_orm::DatabaseBackend,
    user: StoredUser,
) -> Result<AuthUserContext, ()> {
    use axonhub_http::{GlobalId, RoleInfo, UserProjectInfo};

    let roles = query_user_roles_seaorm(db, backend, user.id).await?;

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

    let memberships = query_all(
        db,
        backend,
        "SELECT project_id, is_owner, scopes FROM user_projects WHERE user_id = ? ORDER BY project_id ASC",
        "SELECT project_id, is_owner, scopes FROM user_projects WHERE user_id = $1 ORDER BY project_id ASC",
        "SELECT project_id, is_owner, scopes FROM user_projects WHERE user_id = ? ORDER BY project_id ASC",
        vec![user.id.into()],
    )
    .await
    .map_err(|_| ())?
    .into_iter()
    .map(|row| {
        Ok((
            row.try_get_by_index(0).map_err(|_| ())?,
            row.try_get_by_index::<bool>(1).map_err(|_| ())?,
            parse_json_string_vec(row.try_get_by_index(2).map_err(|_| ())?),
        ))
    })
    .collect::<Result<Vec<_>, ()>>()?;

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
    }
}

fn stored_role_from_seaorm_row(row: sea_orm::QueryResult) -> Result<StoredRole, ()> {
    Ok(StoredRole {
        name: row.try_get_by_index(0).map_err(|_| ())?,
        level: row.try_get_by_index(1).map_err(|_| ())?,
        project_id: row.try_get_by_index(2).map_err(|_| ())?,
        scopes: parse_json_string_vec(row.try_get_by_index(3).map_err(|_| ())?),
    })
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JwtClaims {
    user_id: i64,
    exp: usize,
}
