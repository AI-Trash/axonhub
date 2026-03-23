use axonhub_http::{
    AdminAuthError, ApiKeyAuthError, ApiKeyType, AuthApiKeyContext, AuthUserContext,
    ContextResolveError, IdentityPort, ProjectContext, SignInError, SignInRequest, SignInSuccess,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{
    identity::{IdentityStore, QueryUserError, StoredApiKey, StoredProject},
    shared::{SqliteFoundation, NO_AUTH_API_KEY_VALUE, SYSTEM_KEY_SECRET_KEY},
    system::{verify_password, SystemSettingsStore},
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
