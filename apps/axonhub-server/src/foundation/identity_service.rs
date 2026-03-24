use axonhub_http::{
    AdminAuthError, ApiKeyAuthError, ApiKeyType, AuthApiKeyContext, AuthUserContext,
    ContextResolveError, IdentityPort, ProjectContext, SignInError, SignInRequest, SignInSuccess,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use postgres::{Client as PostgresClient, NoTls, Row};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{
    identity::{IdentityStore, QueryUserError, StoredApiKey, StoredProject},
    shared::{SqliteFoundation, NO_AUTH_API_KEY_VALUE, SYSTEM_KEY_SECRET_KEY},
    system::{
        ensure_identity_tables_postgres, ensure_systems_table_postgres,
        query_system_value_postgres, verify_password, SystemSettingsStore,
    },
};

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
            .map_err(|_| SignInError::Internal)?;
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

    fn generate_jwt_token(&self, user_id: i64) -> rusqlite::Result<String> {
        let secret = self
            .system_settings
            .value(SYSTEM_KEY_SECRET_KEY)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
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
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
    }
}

pub struct SqliteIdentityService {
    identity_auth: IdentityAuthService,
}

impl SqliteIdentityService {
    pub fn new(foundation: Arc<SqliteFoundation>, allow_no_auth: bool) -> Self {
        Self {
            identity_auth: foundation.identity_auth(allow_no_auth),
        }
    }
}

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

pub struct PostgresIdentityService {
    dsn: String,
    allow_no_auth: bool,
}

impl PostgresIdentityService {
    pub fn new(dsn: impl Into<String>, allow_no_auth: bool) -> Self {
        Self {
            dsn: dsn.into(),
            allow_no_auth,
        }
    }

    fn run_blocking<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(String, bool) -> Result<T, E> + Send + 'static,
    {
        let dsn = self.dsn.clone();
        let allow_no_auth = self.allow_no_auth;

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(move || operation(dsn, allow_no_auth))
                .join()
                .unwrap_or_else(|_| panic!("postgres identity worker thread panicked"))
        } else {
            operation(dsn, allow_no_auth)
        }
    }

    fn connect(dsn: &str) -> Result<PostgresClient, ()> {
        let mut client = PostgresClient::connect(dsn, NoTls).map_err(|_| ())?;
        ensure_systems_table_postgres(&mut client).map_err(|_| ())?;
        ensure_identity_tables_postgres(&mut client).map_err(|_| ())?;
        Ok(client)
    }

    fn generate_jwt_token(
        client: &mut PostgresClient,
        user_id: i64,
    ) -> Result<String, SignInError> {
        let secret = query_system_value_postgres(client, SYSTEM_KEY_SECRET_KEY)
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

impl IdentityPort for PostgresIdentityService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        let email = request.email.trim().to_owned();
        let password = request.password.clone();

        self.run_blocking(move |dsn, _| {
            let mut client = Self::connect(&dsn).map_err(|_| SignInError::Internal)?;
            let user = query_user_by_email_postgres(&mut client, &email)
                .map_err(map_sign_in_query_error)?;

            if !verify_password(&user.password, &password) {
                return Err(SignInError::InvalidCredentials);
            }

            let token = Self::generate_jwt_token(&mut client, user.id)?;
            let user = build_user_context_postgres(&mut client, user)
                .map_err(|_| SignInError::Internal)?;

            Ok(SignInSuccess { user, token })
        })
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        let token = token.to_owned();

        self.run_blocking(move |dsn, _| {
            let mut client = Self::connect(&dsn).map_err(|_| AdminAuthError::Internal)?;
            let secret = query_system_value_postgres(&mut client, SYSTEM_KEY_SECRET_KEY)
                .map_err(|_| AdminAuthError::Internal)?
                .ok_or(AdminAuthError::InvalidToken)?;

            let decoded = decode::<JwtClaims>(
                &token,
                &DecodingKey::from_secret(secret.as_bytes()),
                &Validation::new(Algorithm::HS256),
            )
            .map_err(|_| AdminAuthError::InvalidToken)?;

            let user = query_user_by_id_postgres(&mut client, decoded.claims.user_id)
                .map_err(map_admin_auth_query_error)?;
            build_user_context_postgres(&mut client, user).map_err(|_| AdminAuthError::Internal)
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

        self.run_blocking(move |dsn, service_allow_no_auth| {
            let mut client = Self::connect(&dsn).map_err(|_| ApiKeyAuthError::Internal)?;
            let api_key = query_api_key_postgres(&mut client, &lookup_key)?;

            if api_key.status != "enabled" {
                return Err(ApiKeyAuthError::Invalid);
            }
            if api_key.key_type == "noauth" && !(allow_no_auth && service_allow_no_auth) {
                return Err(ApiKeyAuthError::Invalid);
            }

            let project = query_project_postgres(&mut client, api_key.project_id)?;
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
        self.authenticate_api_key(query_key.or(header_key), false)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    user_id: i64,
    exp: usize,
}

fn map_sign_in_query_error(error: QueryUserError) -> SignInError {
    match error {
        QueryUserError::NotFound | QueryUserError::InvalidPassword => {
            SignInError::InvalidCredentials
        }
        QueryUserError::Internal => SignInError::Internal,
    }
}

fn map_admin_auth_query_error(error: QueryUserError) -> AdminAuthError {
    match error {
        QueryUserError::NotFound | QueryUserError::InvalidPassword => AdminAuthError::InvalidToken,
        QueryUserError::Internal => AdminAuthError::Internal,
    }
}

fn map_api_key_type(value: &str) -> ApiKeyType {
    match value {
        "service_account" => ApiKeyType::ServiceAccount,
        "noauth" => ApiKeyType::NoAuth,
        _ => ApiKeyType::User,
    }
}

fn project_context(project: StoredProject) -> ProjectContext {
    ProjectContext {
        id: project.id,
        name: project.name,
        status: project.status,
    }
}

fn query_user_by_email_postgres(
    client: &mut PostgresClient,
    email: &str,
) -> Result<super::identity::StoredUser, QueryUserError> {
    client
        .query_opt(
            "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
             FROM users WHERE email = $1 AND deleted_at = 0 LIMIT 1",
            &[&email],
        )
        .map_err(|_| QueryUserError::Internal)?
        .map(stored_user_from_postgres_row)
        .transpose()
        .map_err(|_| QueryUserError::Internal)?
        .ok_or(QueryUserError::NotFound)
        .and_then(|user| {
            if user.status != "activated" {
                Err(QueryUserError::InvalidPassword)
            } else {
                Ok(user)
            }
        })
}

fn query_user_by_id_postgres(
    client: &mut PostgresClient,
    user_id: i64,
) -> Result<super::identity::StoredUser, QueryUserError> {
    client
        .query_opt(
            "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
             FROM users WHERE id = $1 AND deleted_at = 0 LIMIT 1",
            &[&user_id],
        )
        .map_err(|_| QueryUserError::Internal)?
        .map(stored_user_from_postgres_row)
        .transpose()
        .map_err(|_| QueryUserError::Internal)?
        .ok_or(QueryUserError::NotFound)
        .and_then(|user| {
            if user.status != "activated" {
                Err(QueryUserError::InvalidPassword)
            } else {
                Ok(user)
            }
        })
}

pub(crate) fn query_project_postgres(
    client: &mut PostgresClient,
    project_id: i64,
) -> Result<StoredProject, ApiKeyAuthError> {
    client
        .query_opt(
            "SELECT id, name, status FROM projects WHERE id = $1 AND deleted_at = 0 LIMIT 1",
            &[&project_id],
        )
        .map_err(|_| ApiKeyAuthError::Internal)?
        .map(stored_project_from_postgres_row)
        .transpose()
        .map_err(|_| ApiKeyAuthError::Internal)?
        .ok_or(ApiKeyAuthError::Invalid)
}

pub(crate) fn query_api_key_postgres(
    client: &mut PostgresClient,
    key: &str,
) -> Result<StoredApiKey, ApiKeyAuthError> {
    client
        .query_opt(
            "SELECT id, user_id, key, name, type, status, project_id, scopes
             FROM api_keys WHERE key = $1 AND deleted_at = 0 LIMIT 1",
            &[&key],
        )
        .map_err(|_| ApiKeyAuthError::Internal)?
        .map(stored_api_key_from_postgres_row)
        .transpose()
        .map_err(|_| ApiKeyAuthError::Internal)?
        .ok_or(ApiKeyAuthError::Invalid)
}

fn query_user_roles_postgres(
    client: &mut PostgresClient,
    user_id: i64,
) -> Result<Vec<super::identity::StoredRole>, ()> {
    client
        .query(
            "SELECT r.name, r.level, r.project_id, r.scopes
             FROM roles r
             JOIN user_roles ur ON ur.role_id = r.id
             WHERE ur.user_id = $1 AND r.deleted_at = 0
             ORDER BY r.id ASC",
            &[&user_id],
        )
        .map_err(|_| ())?
        .into_iter()
        .map(stored_role_from_postgres_row)
        .collect()
}

fn build_user_context_postgres(
    client: &mut PostgresClient,
    user: super::identity::StoredUser,
) -> Result<AuthUserContext, ()> {
    use super::authz::{is_project_role_assignment, is_system_role_assignment};
    use axonhub_http::{GlobalId, RoleInfo, UserProjectInfo};

    let roles = query_user_roles_postgres(client, user.id)?;

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

    let memberships = client
        .query(
            "SELECT project_id, is_owner, scopes FROM user_projects WHERE user_id = $1 ORDER BY project_id ASC",
            &[&user.id],
        )
        .map_err(|_| ())?
        .into_iter()
        .map(|row| {
            Ok((
                row.get::<_, i64>(0),
                row.get::<_, bool>(1),
                super::identity::parse_json_string_vec(row.get::<_, String>(2)),
            ))
        })
        .collect::<Result<Vec<_>, ()>>()?;

    let projects = memberships
        .into_iter()
        .map(|(project_id, is_owner, scopes)| {
            let project = query_project_postgres(client, project_id).map_err(|_| ())?;
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

            Ok(UserProjectInfo {
                project_id: GlobalId {
                    resource_type: "project".to_owned(),
                    id: project.id,
                },
                is_owner,
                scopes,
                roles: project_roles,
            })
        })
        .collect::<Result<Vec<_>, ()>>()?;

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

fn stored_user_from_postgres_row(row: Row) -> Result<super::identity::StoredUser, ()> {
    Ok(super::identity::StoredUser {
        id: row.try_get(0).map_err(|_| ())?,
        email: row.try_get(1).map_err(|_| ())?,
        status: row.try_get(2).map_err(|_| ())?,
        prefer_language: row.try_get(3).map_err(|_| ())?,
        password: row.try_get(4).map_err(|_| ())?,
        first_name: row.try_get(5).map_err(|_| ())?,
        last_name: row.try_get(6).map_err(|_| ())?,
        avatar: row.try_get(7).map_err(|_| ())?,
        is_owner: row.try_get(8).map_err(|_| ())?,
        scopes: super::identity::parse_json_string_vec(row.try_get(9).map_err(|_| ())?),
    })
}

fn stored_project_from_postgres_row(row: Row) -> Result<StoredProject, ()> {
    Ok(StoredProject {
        id: row.try_get(0).map_err(|_| ())?,
        name: row.try_get(1).map_err(|_| ())?,
        status: row.try_get(2).map_err(|_| ())?,
    })
}

fn stored_api_key_from_postgres_row(row: Row) -> Result<StoredApiKey, ()> {
    Ok(StoredApiKey {
        id: row.try_get(0).map_err(|_| ())?,
        user_id: row.try_get(1).map_err(|_| ())?,
        key: row.try_get(2).map_err(|_| ())?,
        name: row.try_get(3).map_err(|_| ())?,
        key_type: row.try_get(4).map_err(|_| ())?,
        status: row.try_get(5).map_err(|_| ())?,
        project_id: row.try_get(6).map_err(|_| ())?,
        scopes: super::identity::parse_json_string_vec(row.try_get(7).map_err(|_| ())?),
    })
}

fn stored_role_from_postgres_row(row: Row) -> Result<super::identity::StoredRole, ()> {
    Ok(super::identity::StoredRole {
        name: row.try_get(0).map_err(|_| ())?,
        level: row.try_get(1).map_err(|_| ())?,
        project_id: row.try_get(2).map_err(|_| ())?,
        scopes: super::identity::parse_json_string_vec(row.try_get(3).map_err(|_| ())?),
    })
}
