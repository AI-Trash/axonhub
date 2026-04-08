use axonhub_http::{ApiKeyAuthError, AuthUserContext, GlobalId, RoleInfo, UserProjectInfo};
use rusqlite::{
    Connection as SqlConnection, Error as SqlError, OptionalExtension, Result as SqlResult,
};

use super::{
    authz::{is_project_role_assignment, is_system_role_assignment},
    identity::{
        parse_json_string_vec, QueryUserError, StoredApiKey, StoredProject, StoredRole, StoredUser,
    },
    sqlite_support::{ensure_identity_tables, SqliteConnectionFactory},
};

#[derive(Debug, Clone)]
pub struct IdentityStore {
    connection_factory: SqliteConnectionFactory,
}

impl IdentityStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> SqlResult<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_identity_tables(&connection)
    }

    pub fn find_user_by_email(&self, email: &str) -> Result<StoredUser, QueryUserError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| QueryUserError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| QueryUserError::Internal)?;
        query_user_by_email(&connection, email)
    }

    pub fn find_user_by_id(&self, user_id: i64) -> Result<StoredUser, QueryUserError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| QueryUserError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| QueryUserError::Internal)?;
        query_user_by_id(&connection, user_id)
    }

    #[cfg(test)]
    pub fn find_default_project_for_user(&self, user_id: i64) -> SqlResult<StoredProject> {
        let connection = self.connection_factory.open(true)?;
        ensure_identity_tables(&connection)?;
        query_default_project_for_user(&connection, user_id)
    }

    pub fn find_project_by_id(&self, project_id: i64) -> Result<StoredProject, ApiKeyAuthError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| ApiKeyAuthError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| ApiKeyAuthError::Internal)?;
        query_project(&connection, project_id)
    }

    pub fn find_api_key_by_value(&self, key: &str) -> Result<StoredApiKey, ApiKeyAuthError> {
        let connection = self
            .connection_factory
            .open(true)
            .map_err(|_| ApiKeyAuthError::Internal)?;
        ensure_identity_tables(&connection).map_err(|_| ApiKeyAuthError::Internal)?;
        query_api_key(&connection, key)
    }

    pub fn build_user_context(&self, user: StoredUser) -> SqlResult<AuthUserContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_identity_tables(&connection)?;
        build_user_context(&connection, user)
    }
}

pub(crate) fn query_user_by_email(
    connection: &SqlConnection,
    email: &str,
) -> Result<StoredUser, QueryUserError> {
    connection
        .query_row(
            "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
             FROM users WHERE email = ?1 AND deleted_at = 0 LIMIT 1",
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
                })
            },
        )
        .map_err(|error| match error {
            SqlError::QueryReturnedNoRows => QueryUserError::NotFound,
            _ => QueryUserError::Internal,
        })
        .and_then(|user| if user.status != "activated" { Err(QueryUserError::InvalidPassword) } else { Ok(user) })
}

pub(crate) fn query_user_by_id(
    connection: &SqlConnection,
    user_id: i64,
) -> Result<StoredUser, QueryUserError> {
    connection
        .query_row(
            "SELECT id, email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes
             FROM users WHERE id = ?1 AND deleted_at = 0 LIMIT 1",
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
                })
            },
        )
        .map_err(|error| match error {
            SqlError::QueryReturnedNoRows => QueryUserError::NotFound,
            _ => QueryUserError::Internal,
        })
        .and_then(|user| if user.status != "activated" { Err(QueryUserError::InvalidPassword) } else { Ok(user) })
}

#[cfg(test)]
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
            parse_json_string_vec(row.get::<_, String>(2)?),
        ))
    })?;
    let memberships = rows.collect::<SqlResult<Vec<_>>>()?;

    let projects = memberships
        .into_iter()
        .map(|(project_id, is_owner, scopes)| {
            let project = query_project(connection, project_id).map_err(|error| match error {
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
