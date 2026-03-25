use axonhub_http::{
    AdminAuthError, ApiKeyAuthError, ApiKeyType, AuthApiKeyContext, AuthUserContext,
    ContextResolveError, IdentityPort, ProjectContext, SignInError, SignInRequest, SignInSuccess,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::sync::Arc;

use super::{
    identity::{IdentityStore, QueryUserError, StoredApiKey, StoredProject},
    ports::IdentityRepository,
    seaorm::SeaOrmConnectionFactory,
    shared::{NO_AUTH_API_KEY_VALUE, SYSTEM_KEY_SECRET_KEY},
    system::{verify_password, SystemSettingsStore},
};
#[cfg(test)]
use super::shared::SqliteFoundation;

#[derive(Debug, Clone)]
pub struct IdentityAuthService {
    identities: IdentityStore,
    system_settings: SystemSettingsStore,
    allow_no_auth: bool,
}

impl IdentityAuthService {
    pub fn new(
        identities: IdentityStore,
        system_settings: SystemSettingsStore,
        allow_no_auth: bool,
    ) -> Self {
        Self {
            identities,
            system_settings,
            allow_no_auth,
        }
    }

    pub fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        let user = self
            .identities
            .find_user_by_email(request.email.trim())
            .map_err(map_sign_in_query_error)?;

        if !verify_password(&user.password, &request.password) {
            return Err(SignInError::InvalidCredentials);
        }

        let token = self
            .generate_jwt_token(user.id)
            ?;
        let user = self
            .identities
            .build_user_context(user)
            .map_err(|_| SignInError::Internal)?;
        Ok(SignInSuccess { user, token })
    }

    pub fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        let secret = self
            .system_settings
            .value(SYSTEM_KEY_SECRET_KEY)
            .map_err(|_| AdminAuthError::Internal)?
            .ok_or(AdminAuthError::InvalidToken)?;

        let decoded = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| AdminAuthError::InvalidToken)?;

        let user = self
            .identities
            .find_user_by_id(decoded.claims.user_id)
            .map_err(map_admin_auth_query_error)?;
        self.identities
            .build_user_context(user)
            .map_err(|_| AdminAuthError::Internal)
    }

    pub fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        let lookup_key = match key.map(str::trim).filter(|value| !value.is_empty()) {
            Some(NO_AUTH_API_KEY_VALUE) => return Err(ApiKeyAuthError::Invalid),
            Some(value) => value,
            None if allow_no_auth && self.allow_no_auth => NO_AUTH_API_KEY_VALUE,
            None => return Err(ApiKeyAuthError::Missing),
        };

        let api_key = self.identities.find_api_key_by_value(lookup_key)?;
        self.authorize_api_key(api_key, allow_no_auth)
    }

    pub fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        self.authenticate_api_key(query_key.or(header_key), false)
    }

    pub fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        match self.identities.find_project_by_id(project_id) {
            Ok(project) if project.status == "active" => Ok(Some(project_context(project))),
            Ok(_) => Ok(None),
            Err(ApiKeyAuthError::Invalid) => Ok(None),
            Err(ApiKeyAuthError::Internal) | Err(ApiKeyAuthError::Missing) => {
                Err(ContextResolveError::Internal)
            }
        }
    }

    fn authorize_api_key(
        &self,
        api_key: StoredApiKey,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        if api_key.status != "enabled" {
            return Err(ApiKeyAuthError::Invalid);
        }
        if api_key.key_type == "noauth" && !(allow_no_auth && self.allow_no_auth) {
            return Err(ApiKeyAuthError::Invalid);
        }

        let project = self.identities.find_project_by_id(api_key.project_id)?;
        if project.status != "active" {
            return Err(ApiKeyAuthError::Invalid);
        }

        Ok(AuthApiKeyContext {
            id: api_key.id,
            key: api_key.key,
            name: api_key.name,
            key_type: map_api_key_type(api_key.key_type.as_str()),
            project: project_context(project),
            scopes: api_key.scopes,
        })
    }

    fn generate_jwt_token(&self, user_id: i64) -> Result<String, SignInError> {
        let secret = self
            .system_settings
            .value(SYSTEM_KEY_SECRET_KEY)
            .map_err(|_| SignInError::Internal)?
            .ok_or(SignInError::Internal)?;
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
        .map_err(|_| SignInError::Internal)
    }
}

#[cfg(test)]
pub struct SqliteIdentityService {
    identity_auth: IdentityAuthService,
}

#[cfg(test)]
impl SqliteIdentityService {
    pub fn new(foundation: Arc<SqliteFoundation>, allow_no_auth: bool) -> Self {
        Self {
            identity_auth: foundation.identity_auth(allow_no_auth),
        }
    }
}

#[cfg(test)]
impl IdentityPort for SqliteIdentityService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        self.identity_auth.admin_signin(request)
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        self.identity_auth.authenticate_admin_jwt(token)
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        self.identity_auth.authenticate_api_key(key, allow_no_auth)
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        self.identity_auth
            .authenticate_gemini_key(query_key, header_key)
    }
}

#[cfg(test)]
impl IdentityRepository for SqliteIdentityService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        <Self as IdentityPort>::admin_signin(self, request)
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        <Self as IdentityPort>::authenticate_admin_jwt(self, token)
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <Self as IdentityPort>::authenticate_api_key(self, key, allow_no_auth)
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <Self as IdentityPort>::authenticate_gemini_key(self, query_key, header_key)
    }
}

pub struct SeaOrmIdentityService {
    db: SeaOrmConnectionFactory,
    allow_no_auth: bool,
}

impl SeaOrmIdentityService {
    pub fn new(db: SeaOrmConnectionFactory, allow_no_auth: bool) -> Self {
        Self { db, allow_no_auth }
    }
}

impl IdentityPort for SeaOrmIdentityService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        let db = self.db.clone();
        let email = request.email.trim().to_owned();
        let password = request.password.clone();

        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| SignInError::Internal)?;
            let backend = db.backend();
            let user = query_user_by_email_seaorm(&connection, backend, &email)
                .await
                .map_err(map_sign_in_query_error)?;

            if !verify_password(&user.password, &password) {
                return Err(SignInError::InvalidCredentials);
            }

            let token = generate_jwt_token_seaorm(&connection, backend, user.id).await?;
            let user = build_user_context_seaorm(&connection, backend, user)
                .await
                .map_err(|_| SignInError::Internal)?;
            Ok(SignInSuccess { user, token })
        })
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        let db = self.db.clone();
        let token = token.to_owned();

        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| AdminAuthError::Internal)?;
            let backend = db.backend();
            let secret = query_system_value_seaorm(&connection, backend, SYSTEM_KEY_SECRET_KEY)
                .await
                .map_err(|_| AdminAuthError::Internal)?
                .ok_or(AdminAuthError::InvalidToken)?;

            let decoded = decode::<JwtClaims>(
                &token,
                &DecodingKey::from_secret(secret.as_bytes()),
                &Validation::new(Algorithm::HS256),
            )
            .map_err(|_| AdminAuthError::InvalidToken)?;

            let user = query_user_by_id_seaorm(&connection, backend, decoded.claims.user_id)
                .await
                .map_err(map_admin_auth_query_error)?;
            build_user_context_seaorm(&connection, backend, user)
                .await
                .map_err(|_| AdminAuthError::Internal)
        })
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        let lookup_key = match key.map(str::trim).filter(|value| !value.is_empty()) {
            Some(NO_AUTH_API_KEY_VALUE) => return Err(ApiKeyAuthError::Invalid),
            Some(value) => value.to_owned(),
            None if allow_no_auth && self.allow_no_auth => NO_AUTH_API_KEY_VALUE.to_owned(),
            None => return Err(ApiKeyAuthError::Missing),
        };
        let service_allow_no_auth = self.allow_no_auth;
        let db = self.db.clone();

        db.run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|_| ApiKeyAuthError::Internal)?;
            let backend = db.backend();
            let api_key = query_api_key_seaorm(&connection, backend, &lookup_key).await?;

            if api_key.status != "enabled" {
                return Err(ApiKeyAuthError::Invalid);
            }
            if api_key.key_type == "noauth" && !(allow_no_auth && service_allow_no_auth) {
                return Err(ApiKeyAuthError::Invalid);
            }

            let project = query_project_seaorm(&connection, backend, api_key.project_id).await?;
            if project.status != "active" {
                return Err(ApiKeyAuthError::Invalid);
            }

            Ok(AuthApiKeyContext {
                id: api_key.id,
                key: api_key.key,
                name: api_key.name,
                key_type: map_api_key_type(api_key.key_type.as_str()),
                project: project_context(project),
                scopes: api_key.scopes,
            })
        })
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <Self as IdentityPort>::authenticate_api_key(self, query_key.or(header_key), false)
    }
}

impl IdentityRepository for SeaOrmIdentityService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        <Self as IdentityPort>::admin_signin(self, request)
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        <Self as IdentityPort>::authenticate_admin_jwt(self, token)
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <Self as IdentityPort>::authenticate_api_key(self, key, allow_no_auth)
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <Self as IdentityPort>::authenticate_gemini_key(self, query_key, header_key)
    }
}

fn sql_for_backend<'a>(
    backend: DatabaseBackend,
    sqlite: &'a str,
    postgres: &'a str,
    mysql: &'a str,
) -> &'a str {
    match backend {
        DatabaseBackend::Sqlite => sqlite,
        DatabaseBackend::Postgres => postgres,
        DatabaseBackend::MySql => mysql,
    }
}

async fn query_one_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Option<sea_orm::QueryResult>, sea_orm::DbErr> {
    db.query_one(Statement::from_sql_and_values(
        backend,
        sql_for_backend(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
}

async fn query_all_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>, sea_orm::DbErr> {
    db.query_all(Statement::from_sql_and_values(
        backend,
        sql_for_backend(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
}

async fn query_system_value_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    key: &str,
) -> Result<Option<String>, sea_orm::DbErr> {
    let row = query_one_seaorm(
        db,
        backend,
        "SELECT value FROM systems WHERE key = ? AND deleted_at = 0 LIMIT 1",
        "SELECT value FROM systems WHERE key = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT value FROM systems WHERE `key` = ? AND deleted_at = 0 LIMIT 1",
        vec![key.into()],
    )
    .await?;
    row.map(|row| row.try_get_by_index(0)).transpose()
}

async fn generate_jwt_token_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    user_id: i64,
) -> Result<String, SignInError> {
    let secret = query_system_value_seaorm(db, backend, SYSTEM_KEY_SECRET_KEY)
        .await
        .map_err(|_| SignInError::Internal)?
        .ok_or(SignInError::Internal)?;
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
    .map_err(|_| SignInError::Internal)
}

async fn query_user_by_email_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    email: &str,
) -> Result<super::identity::StoredUser, QueryUserError> {
    query_one_seaorm(
        db,
        backend,
        "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes FROM users WHERE email = ? AND deleted_at = 0 LIMIT 1",
        "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes FROM users WHERE email = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes FROM users WHERE email = ? AND deleted_at = 0 LIMIT 1",
        vec![email.into()],
    )
    .await
    .map_err(|_| QueryUserError::Internal)?
    .map(stored_user_from_seaorm_row)
    .transpose()
    .map_err(|_| QueryUserError::Internal)?
    .ok_or(QueryUserError::NotFound)
    .and_then(|user| if user.status != "activated" { Err(QueryUserError::InvalidPassword) } else { Ok(user) })
}

async fn query_user_by_id_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    user_id: i64,
) -> Result<super::identity::StoredUser, QueryUserError> {
    query_one_seaorm(
        db,
        backend,
        "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes FROM users WHERE id = ? AND deleted_at = 0 LIMIT 1",
        "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes FROM users WHERE id = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes FROM users WHERE id = ? AND deleted_at = 0 LIMIT 1",
        vec![user_id.into()],
    )
    .await
    .map_err(|_| QueryUserError::Internal)?
    .map(stored_user_from_seaorm_row)
    .transpose()
    .map_err(|_| QueryUserError::Internal)?
    .ok_or(QueryUserError::NotFound)
    .and_then(|user| if user.status != "activated" { Err(QueryUserError::InvalidPassword) } else { Ok(user) })
}

pub(crate) async fn query_project_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    project_id: i64,
) -> Result<StoredProject, ApiKeyAuthError> {
    query_one_seaorm(
        db,
        backend,
        "SELECT id, name, status FROM projects WHERE id = ? AND deleted_at = 0 LIMIT 1",
        "SELECT id, name, status FROM projects WHERE id = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT id, name, status FROM projects WHERE id = ? AND deleted_at = 0 LIMIT 1",
        vec![project_id.into()],
    )
    .await
    .map_err(|_| ApiKeyAuthError::Internal)?
    .map(stored_project_from_seaorm_row)
    .transpose()
    .map_err(|_| ApiKeyAuthError::Internal)?
    .ok_or(ApiKeyAuthError::Invalid)
}

async fn query_api_key_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    key: &str,
) -> Result<StoredApiKey, ApiKeyAuthError> {
    query_one_seaorm(
        db,
        backend,
        "SELECT id, user_id, key, name, type, status, project_id, scopes FROM api_keys WHERE key = ? AND deleted_at = 0 LIMIT 1",
        "SELECT id, user_id, key, name, type, status, project_id, scopes FROM api_keys WHERE key = $1 AND deleted_at = 0 LIMIT 1",
        "SELECT id, user_id, `key`, name, type, status, project_id, scopes FROM api_keys WHERE `key` = ? AND deleted_at = 0 LIMIT 1",
        vec![key.into()],
    )
    .await
    .map_err(|_| ApiKeyAuthError::Internal)?
    .map(stored_api_key_from_seaorm_row)
    .transpose()
    .map_err(|_| ApiKeyAuthError::Internal)?
    .ok_or(ApiKeyAuthError::Invalid)
}

async fn query_user_roles_seaorm(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    user_id: i64,
) -> Result<Vec<super::identity::StoredRole>, ()> {
    query_all_seaorm(
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
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    user: super::identity::StoredUser,
) -> Result<AuthUserContext, ()> {
    use super::authz::{is_project_role_assignment, is_system_role_assignment};
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

    let memberships = query_all_seaorm(
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
            super::identity::parse_json_string_vec(row.try_get_by_index(2).map_err(|_| ())?),
        ))
    })
    .collect::<Result<Vec<_>, ()>>()?;

    let mut projects = Vec::with_capacity(memberships.len());
    for (project_id, is_owner, scopes) in memberships {
        let project = query_project_seaorm(db, backend, project_id).await.map_err(|_| ())?;
        let project_roles = roles
            .iter()
            .filter(|role| is_project_role_assignment(role.project_id, role.level.as_str(), project_id))
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

fn stored_user_from_seaorm_row(row: sea_orm::QueryResult) -> Result<super::identity::StoredUser, ()> {
    Ok(super::identity::StoredUser {
        id: row.try_get_by_index(0).map_err(|_| ())?,
        email: row.try_get_by_index(1).map_err(|_| ())?,
        status: row.try_get_by_index(2).map_err(|_| ())?,
        prefer_language: row.try_get_by_index(3).map_err(|_| ())?,
        password: row.try_get_by_index(4).map_err(|_| ())?,
        first_name: row.try_get_by_index(5).map_err(|_| ())?,
        last_name: row.try_get_by_index(6).map_err(|_| ())?,
        avatar: row.try_get_by_index(7).map_err(|_| ())?,
        is_owner: row.try_get_by_index::<bool>(8).map_err(|_| ())?,
        scopes: super::identity::parse_json_string_vec(row.try_get_by_index(9).map_err(|_| ())?),
    })
}

fn stored_project_from_seaorm_row(row: sea_orm::QueryResult) -> Result<StoredProject, ()> {
    Ok(StoredProject {
        id: row.try_get_by_index(0).map_err(|_| ())?,
        name: row.try_get_by_index(1).map_err(|_| ())?,
        status: row.try_get_by_index(2).map_err(|_| ())?,
    })
}

fn stored_api_key_from_seaorm_row(row: sea_orm::QueryResult) -> Result<StoredApiKey, ()> {
    Ok(StoredApiKey {
        id: row.try_get_by_index(0).map_err(|_| ())?,
        user_id: row.try_get_by_index(1).map_err(|_| ())?,
        key: row.try_get_by_index(2).map_err(|_| ())?,
        name: row.try_get_by_index(3).map_err(|_| ())?,
        key_type: row.try_get_by_index(4).map_err(|_| ())?,
        status: row.try_get_by_index(5).map_err(|_| ())?,
        project_id: row.try_get_by_index(6).map_err(|_| ())?,
        scopes: super::identity::parse_json_string_vec(row.try_get_by_index(7).map_err(|_| ())?),
    })
}

fn stored_role_from_seaorm_row(row: sea_orm::QueryResult) -> Result<super::identity::StoredRole, ()> {
    Ok(super::identity::StoredRole {
        name: row.try_get_by_index(0).map_err(|_| ())?,
        level: row.try_get_by_index(1).map_err(|_| ())?,
        project_id: row.try_get_by_index(2).map_err(|_| ())?,
        scopes: super::identity::parse_json_string_vec(row.try_get_by_index(3).map_err(|_| ())?),
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    user_id: i64,
    exp: usize,
}

pub(crate) fn map_sign_in_query_error(error: QueryUserError) -> SignInError {
    match error {
        QueryUserError::NotFound | QueryUserError::InvalidPassword => {
            SignInError::InvalidCredentials
        }
        QueryUserError::Internal => SignInError::Internal,
    }
}

pub(crate) fn map_admin_auth_query_error(error: QueryUserError) -> AdminAuthError {
    match error {
        QueryUserError::NotFound | QueryUserError::InvalidPassword => AdminAuthError::InvalidToken,
        QueryUserError::Internal => AdminAuthError::Internal,
    }
}

pub(crate) fn map_api_key_type(value: &str) -> ApiKeyType {
    match value {
        "service_account" => ApiKeyType::ServiceAccount,
        "noauth" => ApiKeyType::NoAuth,
        _ => ApiKeyType::User,
    }
}

pub(crate) fn project_context(project: StoredProject) -> ProjectContext {
    ProjectContext {
        id: project.id,
        name: project.name,
        status: project.status,
    }
}
