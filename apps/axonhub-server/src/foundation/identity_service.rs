use axonhub_http::{
    AdminAuthError, ApiKeyAuthError, ApiKeyType, AuthApiKeyContext, AuthUserContext,
    ContextResolveError, IdentityPort, ProjectContext, SignInError, SignInRequest, SignInSuccess,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
#[cfg(test)]
use std::sync::Arc;

use super::{
    identity::{QueryUserError, StoredApiKey, StoredProject, StoredUser},
    passwords::verify_password,
    ports::IdentityRepository,
    repositories::identity::{IdentityAuthRepository, SeaOrmIdentityAuthRepository},
    seaorm::SeaOrmConnectionFactory,
    shared::{NO_AUTH_API_KEY_VALUE, SYSTEM_KEY_SECRET_KEY},
};

#[cfg(test)]
use super::system::sqlite_test_support::SqliteFoundation;

#[cfg(test)]
pub struct SqliteIdentityService {
    identity_auth: SeaOrmIdentityService,
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
        <SeaOrmIdentityService as IdentityPort>::admin_signin(&self.identity_auth, request)
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        <SeaOrmIdentityService as IdentityPort>::authenticate_admin_jwt(&self.identity_auth, token)
    }

    fn authenticate_api_key(
        &self,
        key: Option<&str>,
        allow_no_auth: bool,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <SeaOrmIdentityService as IdentityPort>::authenticate_api_key(
            &self.identity_auth,
            key,
            allow_no_auth,
        )
    }

    fn authenticate_gemini_key(
        &self,
        query_key: Option<&str>,
        header_key: Option<&str>,
    ) -> Result<AuthApiKeyContext, ApiKeyAuthError> {
        <SeaOrmIdentityService as IdentityPort>::authenticate_gemini_key(
            &self.identity_auth,
            query_key,
            header_key,
        )
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

#[derive(Debug, Clone)]
pub struct SeaOrmIdentityService {
    repository: SeaOrmIdentityAuthRepository,
    allow_no_auth: bool,
}

impl SeaOrmIdentityService {
    pub fn new(db: SeaOrmConnectionFactory, allow_no_auth: bool) -> Self {
        Self {
            repository: SeaOrmIdentityAuthRepository::new(db),
            allow_no_auth,
        }
    }

    fn jwt_secret_for_signin(&self) -> Result<String, SignInError> {
        self.repository
            .query_system_value(SYSTEM_KEY_SECRET_KEY)
            .map_err(|_| SignInError::Internal)?
            .ok_or(SignInError::Internal)
    }

    fn jwt_secret_for_admin_auth(&self) -> Result<String, AdminAuthError> {
        self.repository
            .query_system_value(SYSTEM_KEY_SECRET_KEY)
            .map_err(|_| AdminAuthError::Internal)?
            .ok_or(AdminAuthError::InvalidToken)
    }

    fn build_signin_success(
        &self,
        request: &SignInRequest,
        secret: &str,
    ) -> Result<SignInSuccess, SignInError> {
        let user = self
            .repository
            .query_user_by_email(request.email.trim())
            .map_err(map_sign_in_query_error)?;

        self.verify_password_and_sign_in(user, &request.password, secret)
    }

    fn verify_password_and_sign_in(
        &self,
        user: StoredUser,
        password: &str,
        secret: &str,
    ) -> Result<SignInSuccess, SignInError> {
        if !verify_password(&user.password, password) {
            return Err(SignInError::InvalidCredentials);
        }

        let token = encode_jwt_token(user.id, secret)?;
        let user = self
            .repository
            .build_user_context(user)
            .map_err(|_| SignInError::Internal)?;
        Ok(SignInSuccess { user, token })
    }

    fn authenticate_admin_jwt_with_secret(
        &self,
        token: &str,
        secret: &str,
    ) -> Result<AuthUserContext, AdminAuthError> {
        let decoded = decode_jwt_claims(token, secret).map_err(|_| AdminAuthError::InvalidToken)?;

        let user = self
            .repository
            .query_user_by_id(decoded.user_id)
            .map_err(map_admin_auth_query_error)?;
        self.repository
            .build_user_context(user)
            .map_err(|_| AdminAuthError::Internal)
    }

    #[cfg(test)]
    pub(crate) fn resolve_project(
        &self,
        project_id: i64,
    ) -> Result<Option<ProjectContext>, ContextResolveError> {
        match self.repository.project_context(project_id) {
            Ok(project) => Ok(project),
            Err(ApiKeyAuthError::Invalid) => Ok(None),
            Err(ApiKeyAuthError::Internal) | Err(ApiKeyAuthError::Missing) => {
                Err(ContextResolveError::Internal)
            }
        }
    }
}

impl IdentityPort for SeaOrmIdentityService {
    fn admin_signin(&self, request: &SignInRequest) -> Result<SignInSuccess, SignInError> {
        let secret = self.jwt_secret_for_signin()?;
        self.build_signin_success(request, &secret)
    }

    fn authenticate_admin_jwt(&self, token: &str) -> Result<AuthUserContext, AdminAuthError> {
        let secret = self.jwt_secret_for_admin_auth()?;
        self.authenticate_admin_jwt_with_secret(token, &secret)
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
        let api_key = self.repository.query_api_key(&lookup_key)?;

        if api_key.status != "enabled" {
            return Err(ApiKeyAuthError::Invalid);
        }
        if api_key.key_type == "noauth" && !(allow_no_auth && self.allow_no_auth) {
            return Err(ApiKeyAuthError::Invalid);
        }

        let project = self.repository.query_project(api_key.project_id)?;
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
            profiles_json: api_key.profiles,
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

#[derive(Debug, serde::Serialize, serde::Deserialize)]
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

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use axonhub_http::{ApiKeyAuthError, AuthUserContext, GlobalId, RoleInfo, UserProjectInfo};
    use rusqlite::{Connection as SqlConnection, Error as SqlError, Result as SqlResult};

    use super::super::{
        authz::{is_project_role_assignment, is_system_role_assignment},
        identity::{
            sqlite_test_support::query_default_project_for_user, QueryUserError, StoredApiKey,
            StoredProject, StoredUser,
        },
        repositories::identity::sqlite_test_support::{
            query_api_key, query_project, query_user_by_email, query_user_by_id, query_user_roles,
        },
        system::sqlite_test_support::{ensure_identity_tables, SqliteConnectionFactory},
    };

    #[derive(Debug, Clone)]
    pub(crate) struct IdentityStore {
        connection_factory: SqliteConnectionFactory,
    }

    impl IdentityStore {
        pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
            Self { connection_factory }
        }

        pub(crate) fn ensure_schema(&self) -> SqlResult<()> {
            let connection = self.connection_factory.open(true)?;
            ensure_identity_tables(&connection)
        }

        pub(crate) fn find_user_by_email(&self, email: &str) -> Result<StoredUser, QueryUserError> {
            let connection = self
                .connection_factory
                .open(true)
                .map_err(|_| QueryUserError::Internal)?;
            ensure_identity_tables(&connection).map_err(|_| QueryUserError::Internal)?;
            query_user_by_email(&connection, email)
        }

        pub(crate) fn find_user_by_id(&self, user_id: i64) -> Result<StoredUser, QueryUserError> {
            let connection = self
                .connection_factory
                .open(true)
                .map_err(|_| QueryUserError::Internal)?;
            ensure_identity_tables(&connection).map_err(|_| QueryUserError::Internal)?;
            query_user_by_id(&connection, user_id)
        }

        pub(crate) fn find_default_project_for_user(
            &self,
            user_id: i64,
        ) -> SqlResult<StoredProject> {
            let connection = self.connection_factory.open(true)?;
            ensure_identity_tables(&connection)?;
            query_default_project_for_user(&connection, user_id)
        }

        pub(crate) fn find_project_by_id(
            &self,
            project_id: i64,
        ) -> Result<StoredProject, ApiKeyAuthError> {
            let connection = self
                .connection_factory
                .open(true)
                .map_err(|_| ApiKeyAuthError::Internal)?;
            ensure_identity_tables(&connection).map_err(|_| ApiKeyAuthError::Internal)?;
            query_project(&connection, project_id)
        }

        pub(crate) fn find_api_key_by_value(
            &self,
            key: &str,
        ) -> Result<StoredApiKey, ApiKeyAuthError> {
            let connection = self
                .connection_factory
                .open(true)
                .map_err(|_| ApiKeyAuthError::Internal)?;
            ensure_identity_tables(&connection).map_err(|_| ApiKeyAuthError::Internal)?;
            query_api_key(&connection, key)
        }

        pub(crate) fn build_user_context(&self, user: StoredUser) -> SqlResult<AuthUserContext> {
            let connection = self.connection_factory.open(true)?;
            ensure_identity_tables(&connection)?;
            build_user_context(&connection, user)
        }
    }

    pub(crate) fn build_user_context(
        connection: &SqlConnection,
        user: StoredUser,
    ) -> SqlResult<AuthUserContext> {
        let roles = query_user_roles(connection, user.id)?;

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

        let mut statement = connection.prepare(
            "SELECT project_id, is_owner, scopes FROM user_projects WHERE user_id = ?1 ORDER BY project_id ASC",
        )?;
        let rows = statement.query_map([user.id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)? != 0,
                super::super::identity::parse_json_string_vec(row.get::<_, String>(2)?),
            ))
        })?;
        let memberships = rows.collect::<SqlResult<Vec<_>>>()?;

        let projects = memberships
            .into_iter()
            .map(|(project_id, is_owner, scopes)| {
                let project =
                    query_project(connection, project_id).map_err(|error| match error {
                        ApiKeyAuthError::Internal => SqlError::InvalidQuery,
                        _ => SqlError::QueryReturnedNoRows,
                    })?;
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
            .collect::<SqlResult<Vec<_>>>()?;

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
}

fn encode_jwt_token(user_id: i64, secret: &str) -> Result<String, SignInError> {
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

fn decode_jwt_claims(token: &str, secret: &str) -> Result<JwtClaims, jsonwebtoken::errors::Error> {
    decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|decoded| decoded.claims)
}
