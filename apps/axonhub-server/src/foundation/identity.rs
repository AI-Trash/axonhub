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
    pub profiles: Option<String>,
}

#[derive(Debug)]
pub enum QueryUserError {
    NotFound,
    InvalidPassword,
    Internal,
}

pub(crate) fn require_activated_user(user: StoredUser) -> Result<StoredUser, QueryUserError> {
    if user.status != "activated" {
        Err(QueryUserError::InvalidPassword)
    } else {
        Ok(user)
    }
}

pub(crate) fn parse_json_string_vec(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use rusqlite::{Connection as SqlConnection, Result as SqlResult};

    use super::StoredProject;

    pub(crate) fn query_default_project_for_user(
        connection: &SqlConnection,
        user_id: i64,
    ) -> SqlResult<StoredProject> {
        connection.query_row(
            "SELECT p.id, p.name, p.status
             FROM projects p
             JOIN user_projects up ON up.project_id = p.id
             WHERE up.user_id = ?1 AND p.deleted_at = 0
             ORDER BY p.id ASC
             LIMIT 1",
            [user_id],
            |row| {
                Ok(StoredProject {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    status: row.get(2)?,
                })
            },
        )
    }
}
