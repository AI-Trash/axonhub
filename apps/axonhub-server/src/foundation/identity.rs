#[cfg(test)]
pub(crate) use super::identity_sqlite_support::query_default_project_for_user;
pub(crate) use super::identity_sqlite_support::{
    build_user_context, query_api_key, query_project, query_user_by_email, query_user_by_id,
    query_user_roles, IdentityStore,
};

#[derive(Debug)]
pub struct StoredUser {
    pub id: i64,
    pub email: String,
    pub status: String,
    pub prefer_language: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    pub avatar: String,
    pub is_owner: bool,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub struct StoredProject {
    pub id: i64,
    pub name: String,
    pub status: String,
}

#[derive(Debug)]
pub struct StoredRole {
    pub name: String,
    pub level: String,
    pub project_id: i64,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub struct StoredApiKey {
    pub id: i64,
    pub user_id: i64,
    pub key: String,
    pub name: String,
    pub key_type: String,
    pub status: String,
    pub project_id: i64,
    pub scopes: Vec<String>,
}

#[derive(Debug)]
pub enum QueryUserError {
    NotFound,
    InvalidPassword,
    Internal,
}

pub(crate) fn parse_json_string_vec(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}
