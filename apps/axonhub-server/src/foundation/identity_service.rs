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
use super::sqlite_support::SqliteFoundation;

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
