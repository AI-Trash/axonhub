use axonhub_http::{AuthApiKeyContext, AuthUserContext};
pub(crate) const SYSTEM_ROLE_PROJECT_ID: i64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthzPrincipalKind {
    User,
    ApiKey,
}

impl AuthzPrincipalKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ApiKey => "api_key",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BypassBoundary {
    Owner,
}

impl BypassBoundary {
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::Owner => "owner_bypass",
        }
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Owner => "permission denied: owner access required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthzFailure {
    MissingScope {
        principal: AuthzPrincipalKind,
        scope: ScopeSlug,
    },
    MissingProjectScope {
        principal: AuthzPrincipalKind,
        project_id: i64,
        scope: ScopeSlug,
    },
    MissingBypass {
        principal: AuthzPrincipalKind,
        bypass: BypassBoundary,
    },
}

impl AuthzFailure {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::MissingScope { .. } | Self::MissingProjectScope { .. } => "permission denied",
            Self::MissingBypass { bypass, .. } => bypass.message(),
        }
    }

    pub(crate) fn audit_reason(self) -> String {
        match self {
            Self::MissingScope { principal, scope } => {
                format!("missing_scope:{}:{}", principal.as_str(), scope.as_str())
            }
            Self::MissingProjectScope {
                principal,
                project_id,
                scope,
            } => format!(
                "missing_project_scope:{}:{}:{}",
                principal.as_str(),
                project_id,
                scope.as_str()
            ),
            Self::MissingBypass { principal, bypass } => {
                format!("missing_bypass:{}:{}", principal.as_str(), bypass.reason())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuditBypassBoundary {
    pub(crate) bypass: BypassBoundary,
    pub(crate) principal: AuthzPrincipalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdminAuthorizationOutcome {
    Allowed,
    Bypass(AuditBypassBoundary),
}

pub(crate) type AuthzResult<T> = Result<T, AuthzFailure>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeLevel {
    System,
    Project,
}

impl ScopeLevel {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Project => "project",
        }
    }

    pub(crate) fn matches(self, value: &str) -> bool {
        value == self.as_str()
    }
}

pub(crate) const ROLE_LEVEL_SYSTEM: ScopeLevel = ScopeLevel::System;
pub(crate) const ROLE_LEVEL_PROJECT: ScopeLevel = ScopeLevel::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeSlug {
    ReadDashboard,
    ReadSettings,
    WriteSettings,
    ReadChannels,
    WriteChannels,
    ReadDataStorages,
    WriteDataStorages,
    ReadUsers,
    WriteUsers,
    ReadRoles,
    WriteRoles,
    ReadProjects,
    WriteProjects,
    ReadApiKeys,
    WriteApiKeys,
    ReadRequests,
    WriteRequests,
    ReadPrompts,
    WritePrompts,
}

impl ScopeSlug {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ReadDashboard => "read_dashboard",
            Self::ReadSettings => "read_settings",
            Self::WriteSettings => "write_settings",
            Self::ReadChannels => "read_channels",
            Self::WriteChannels => "write_channels",
            Self::ReadDataStorages => "read_data_storages",
            Self::WriteDataStorages => "write_data_storages",
            Self::ReadUsers => "read_users",
            Self::WriteUsers => "write_users",
            Self::ReadRoles => "read_roles",
            Self::WriteRoles => "write_roles",
            Self::ReadProjects => "read_projects",
            Self::WriteProjects => "write_projects",
            Self::ReadApiKeys => "read_api_keys",
            Self::WriteApiKeys => "write_api_keys",
            Self::ReadRequests => "read_requests",
            Self::WriteRequests => "write_requests",
            Self::ReadPrompts => "read_prompts",
            Self::WritePrompts => "write_prompts",
        }
    }
}

pub(crate) const SCOPE_READ_DASHBOARD: ScopeSlug = ScopeSlug::ReadDashboard;
pub(crate) const SCOPE_READ_SETTINGS: ScopeSlug = ScopeSlug::ReadSettings;
pub(crate) const SCOPE_WRITE_SETTINGS: ScopeSlug = ScopeSlug::WriteSettings;
pub(crate) const SCOPE_READ_CHANNELS: ScopeSlug = ScopeSlug::ReadChannels;
pub(crate) const SCOPE_WRITE_CHANNELS: ScopeSlug = ScopeSlug::WriteChannels;
pub(crate) const SCOPE_READ_DATA_STORAGES: ScopeSlug = ScopeSlug::ReadDataStorages;
pub(crate) const SCOPE_WRITE_DATA_STORAGES: ScopeSlug = ScopeSlug::WriteDataStorages;
pub(crate) const SCOPE_READ_USERS: ScopeSlug = ScopeSlug::ReadUsers;
pub(crate) const SCOPE_WRITE_USERS: ScopeSlug = ScopeSlug::WriteUsers;
pub(crate) const SCOPE_READ_ROLES: ScopeSlug = ScopeSlug::ReadRoles;
pub(crate) const SCOPE_WRITE_ROLES: ScopeSlug = ScopeSlug::WriteRoles;
pub(crate) const SCOPE_READ_PROJECTS: ScopeSlug = ScopeSlug::ReadProjects;
pub(crate) const SCOPE_WRITE_PROJECTS: ScopeSlug = ScopeSlug::WriteProjects;
pub(crate) const SCOPE_READ_API_KEYS: ScopeSlug = ScopeSlug::ReadApiKeys;
pub(crate) const SCOPE_WRITE_API_KEYS: ScopeSlug = ScopeSlug::WriteApiKeys;
pub(crate) const SCOPE_READ_REQUESTS: ScopeSlug = ScopeSlug::ReadRequests;
pub(crate) const SCOPE_WRITE_REQUESTS: ScopeSlug = ScopeSlug::WriteRequests;
pub(crate) const SCOPE_READ_PROMPTS: ScopeSlug = ScopeSlug::ReadPrompts;
pub(crate) const SCOPE_WRITE_PROMPTS: ScopeSlug = ScopeSlug::WritePrompts;

const SYSTEM_ONLY_LEVELS: &[ScopeLevel] = &[ROLE_LEVEL_SYSTEM];
const SYSTEM_AND_PROJECT_LEVELS: &[ScopeLevel] = &[ROLE_LEVEL_SYSTEM, ROLE_LEVEL_PROJECT];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scope {
    pub(crate) slug: ScopeSlug,
    pub(crate) description: &'static str,
    pub(crate) levels: &'static [ScopeLevel],
}

pub(crate) const CURRENT_SCOPE_VOCABULARY: &[Scope] = &[
    Scope {
        slug: SCOPE_READ_DASHBOARD,
        description: "View dashboard",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_SETTINGS,
        description: "View system settings",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_SETTINGS,
        description: "Manage system settings",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_CHANNELS,
        description: "View channel information",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_CHANNELS,
        description: "Manage channels/models (create, edit, delete)",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_DATA_STORAGES,
        description: "View data storage information",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_DATA_STORAGES,
        description: "Manage data storages (create, edit, delete)",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_USERS,
        description: "View user information",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_USERS,
        description: "Manage users (create, edit, delete)",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_ROLES,
        description: "View role information",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_ROLES,
        description: "Manage roles (create, edit, delete)",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_PROJECTS,
        description: "View project information",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_PROJECTS,
        description: "Manage projects (create, edit, delete)",
        levels: SYSTEM_ONLY_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_API_KEYS,
        description: "View API keys",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_API_KEYS,
        description: "Manage API keys (create, edit, delete)",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_REQUESTS,
        description: "View request records",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_REQUESTS,
        description: "Manage request records",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_READ_PROMPTS,
        description: "View prompts",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
    Scope {
        slug: SCOPE_WRITE_PROMPTS,
        description: "Manage prompts (create, edit, delete)",
        levels: SYSTEM_AND_PROJECT_LEVELS,
    },
];

pub(crate) const PROJECT_ADMIN_SCOPES: &[ScopeSlug] = &[
    SCOPE_READ_USERS,
    SCOPE_WRITE_USERS,
    SCOPE_READ_ROLES,
    SCOPE_WRITE_ROLES,
    SCOPE_READ_API_KEYS,
    SCOPE_WRITE_API_KEYS,
    SCOPE_READ_REQUESTS,
    SCOPE_WRITE_REQUESTS,
];

pub(crate) const PROJECT_DEVELOPER_SCOPES: &[ScopeSlug] = &[
    SCOPE_READ_USERS,
    SCOPE_READ_API_KEYS,
    SCOPE_WRITE_API_KEYS,
    SCOPE_READ_REQUESTS,
];

pub(crate) const PROJECT_VIEWER_SCOPES: &[ScopeSlug] = &[SCOPE_READ_USERS, SCOPE_READ_REQUESTS];

pub(crate) const DEFAULT_USER_API_KEY_SCOPES: &[ScopeSlug] =
    &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS];
pub(crate) const DEFAULT_SERVICE_API_KEY_SCOPES: &[ScopeSlug] = &[];
pub(crate) const NO_AUTH_API_KEY_SCOPES: &[ScopeSlug] =
    &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS];
pub(crate) const LLM_API_KEY_SCOPES: &[ScopeSlug] = &[SCOPE_READ_CHANNELS, SCOPE_WRITE_REQUESTS];

pub(crate) fn all_scopes(level: Option<ScopeLevel>) -> Vec<Scope> {
    CURRENT_SCOPE_VOCABULARY
        .iter()
        .copied()
        .filter(|scope| {
            level
                .map(|expected| scope.levels.iter().any(|current| *current == expected))
                .unwrap_or(true)
        })
        .collect()
}

pub(crate) fn is_valid_scope(scope: &str) -> bool {
    CURRENT_SCOPE_VOCABULARY
        .iter()
        .any(|current| current.slug.as_str() == scope)
}

pub(crate) fn scope_strings(scopes: &[ScopeSlug]) -> Vec<String> {
    scopes
        .iter()
        .map(|scope| scope.as_str().to_owned())
        .collect()
}

/// Serializes scope slugs to a JSON string for database storage.
///
/// Returns a neutral error type that callers can map to their specific
/// database error types (rusqlite, sea_orm, postgres, etc.).
pub(crate) fn serialize_scope_slugs(scopes: &[ScopeSlug]) -> Result<String, serde_json::Error> {
    serde_json::to_string(&scope_strings(scopes))
}

pub(crate) fn api_key_has_scope(api_key: &AuthApiKeyContext, scope: ScopeSlug) -> bool {
    api_key.has_scope(scope.as_str())
}

pub(crate) fn user_has_system_scope(user: &AuthUserContext, scope: ScopeSlug) -> bool {
    user.has_system_scope(scope.as_str())
}

pub(crate) fn user_has_project_scope(
    user: &AuthUserContext,
    project_id: i64,
    scope: ScopeSlug,
) -> bool {
    user.has_project_scope(project_id, scope.as_str())
}

pub(crate) fn require_user_system_scope(
    user: &AuthUserContext,
    scope: ScopeSlug,
) -> AuthzResult<()> {
    if user_has_system_scope(user, scope) {
        Ok(())
    } else {
        Err(AuthzFailure::MissingScope {
            principal: AuthzPrincipalKind::User,
            scope,
        })
    }
}

pub(crate) fn require_user_project_scope(
    user: &AuthUserContext,
    project_id: i64,
    scope: ScopeSlug,
) -> AuthzResult<()> {
    if user_has_system_scope(user, scope) || user_has_project_scope(user, project_id, scope) {
        Ok(())
    } else {
        Err(AuthzFailure::MissingProjectScope {
            principal: AuthzPrincipalKind::User,
            project_id,
            scope,
        })
    }
}

pub(crate) fn require_api_key_scope(
    api_key: &AuthApiKeyContext,
    scope: ScopeSlug,
) -> AuthzResult<()> {
    if api_key_has_scope(api_key, scope) {
        Ok(())
    } else {
        Err(AuthzFailure::MissingScope {
            principal: AuthzPrincipalKind::ApiKey,
            scope,
        })
    }
}

pub(crate) fn authorize_user_system_scope(
    user: &AuthUserContext,
    scope: ScopeSlug,
) -> AuthzResult<AdminAuthorizationOutcome> {
    if user.is_owner {
        return Ok(AdminAuthorizationOutcome::Bypass(AuditBypassBoundary {
            bypass: BypassBoundary::Owner,
            principal: AuthzPrincipalKind::User,
        }));
    }

    require_user_system_scope(user, scope)?;
    Ok(AdminAuthorizationOutcome::Allowed)
}

pub(crate) fn require_owner_bypass(user: &AuthUserContext) -> AuthzResult<AuditBypassBoundary> {
    if user.is_owner {
        Ok(AuditBypassBoundary {
            bypass: BypassBoundary::Owner,
            principal: AuthzPrincipalKind::User,
        })
    } else {
        Err(AuthzFailure::MissingBypass {
            principal: AuthzPrincipalKind::User,
            bypass: BypassBoundary::Owner,
        })
    }
}

pub(crate) fn require_service_api_key_write_access(api_key: &AuthApiKeyContext) -> AuthzResult<()> {
    require_api_key_scope(api_key, SCOPE_WRITE_API_KEYS)?;
    if api_key.is_service_account() {
        Ok(())
    } else {
        Err(AuthzFailure::MissingScope {
            principal: AuthzPrincipalKind::ApiKey,
            scope: SCOPE_WRITE_API_KEYS,
        })
    }
}

pub(crate) fn is_system_role_assignment(project_id: i64, level: &str) -> bool {
    project_id == SYSTEM_ROLE_PROJECT_ID || ROLE_LEVEL_SYSTEM.matches(level)
}

pub(crate) fn is_project_role_assignment(
    project_id: i64,
    level: &str,
    expected_project_id: i64,
) -> bool {
    project_id == expected_project_id && ROLE_LEVEL_PROJECT.matches(level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhub_http::{ApiKeyType, GlobalId, ProjectContext, RoleInfo, UserProjectInfo};

    fn user_with_context() -> AuthUserContext {
        AuthUserContext {
            id: 1,
            email: "owner@example.com".to_owned(),
            first_name: "System".to_owned(),
            last_name: "Owner".to_owned(),
            is_owner: false,
            prefer_language: "en".to_owned(),
            avatar: Some(String::new()),
            scopes: Vec::new(),
            roles: Vec::new(),
            projects: Vec::new(),
        }
    }

    #[test]
    fn scope_catalog_keeps_the_current_rust_subset() {
        assert!(is_valid_scope(SCOPE_READ_SETTINGS.as_str()));
        assert!(is_valid_scope(SCOPE_WRITE_API_KEYS.as_str()));
        assert!(is_valid_scope(SCOPE_READ_PROJECTS.as_str()));
        assert!(is_valid_scope(SCOPE_READ_PROMPTS.as_str()));
        assert_eq!(all_scopes(None).len(), CURRENT_SCOPE_VOCABULARY.len());
        assert!(all_scopes(Some(ROLE_LEVEL_PROJECT))
            .iter()
            .all(|scope| scope
                .levels
                .iter()
                .any(|level| *level == ROLE_LEVEL_PROJECT)));
    }

    #[test]
    fn system_scope_permission_includes_owner_and_system_role_scope() {
        let mut user = user_with_context();
        user.roles.push(RoleInfo {
            name: "System Reader".to_owned(),
            scopes: scope_strings(&[SCOPE_READ_CHANNELS]),
        });

        assert!(user_has_system_scope(&user, SCOPE_READ_CHANNELS));
        assert!(!user_has_system_scope(&user, SCOPE_READ_SETTINGS));

        user.is_owner = true;
        assert!(user_has_system_scope(&user, SCOPE_READ_SETTINGS));
    }

    #[test]
    fn project_permission_includes_membership_role_scope_and_project_owner() {
        let mut user = user_with_context();
        user.projects.push(UserProjectInfo {
            project_id: GlobalId {
                resource_type: "project".to_owned(),
                id: 7,
            },
            is_owner: false,
            scopes: Vec::new(),
            roles: vec![RoleInfo {
                name: "Request Reader".to_owned(),
                scopes: scope_strings(&[SCOPE_READ_REQUESTS]),
            }],
        });

        assert!(user_has_project_scope(&user, 7, SCOPE_READ_REQUESTS));
        assert!(!user_has_project_scope(&user, 8, SCOPE_READ_REQUESTS));

        user.projects[0].is_owner = true;
        assert!(user_has_project_scope(&user, 7, SCOPE_WRITE_REQUESTS));
    }

    #[test]
    fn api_key_permission_checks_match_scope_strings() {
        let api_key = AuthApiKeyContext {
            id: 1,
            key: "service-key".to_owned(),
            name: "Service".to_owned(),
            key_type: ApiKeyType::ServiceAccount,
            project: ProjectContext {
                id: 7,
                name: "Default Project".to_owned(),
                status: "active".to_owned(),
            },
            scopes: scope_strings(LLM_API_KEY_SCOPES),
            profiles_json: None,
        };

        assert!(api_key_has_scope(&api_key, SCOPE_WRITE_REQUESTS));
        assert!(!api_key_has_scope(&api_key, SCOPE_WRITE_API_KEYS));
    }

    #[test]
    fn default_service_api_key_scopes_match_go_service_account_creation_semantics() {
        assert!(scope_strings(DEFAULT_SERVICE_API_KEY_SCOPES).is_empty());
        assert_eq!(
            scope_strings(DEFAULT_USER_API_KEY_SCOPES),
            vec!["read_channels".to_owned(), "write_requests".to_owned()]
        );
    }

    #[test]
    fn role_level_permission_helpers_preserve_system_and_project_semantics() {
        assert!(is_system_role_assignment(
            SYSTEM_ROLE_PROJECT_ID,
            ROLE_LEVEL_PROJECT.as_str()
        ));
        assert!(is_system_role_assignment(42, ROLE_LEVEL_SYSTEM.as_str()));
        assert!(is_project_role_assignment(
            7,
            ROLE_LEVEL_PROJECT.as_str(),
            7
        ));
        assert!(!is_project_role_assignment(
            7,
            ROLE_LEVEL_SYSTEM.as_str(),
            7
        ));
    }

    #[test]
    fn admin_authorization_outcome_tracks_owner_bypass_boundary() {
        let mut user = user_with_context();
        user.is_owner = true;

        let outcome = authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).unwrap();
        assert_eq!(
            outcome,
            AdminAuthorizationOutcome::Bypass(AuditBypassBoundary {
                bypass: BypassBoundary::Owner,
                principal: AuthzPrincipalKind::User,
            })
        );
    }

    #[test]
    fn user_project_scope_requirement_rejects_missing_project_membership_scope() {
        let user = user_with_context();
        let error = require_user_project_scope(&user, 7, SCOPE_READ_REQUESTS).unwrap_err();
        assert_eq!(
            error,
            AuthzFailure::MissingProjectScope {
                principal: AuthzPrincipalKind::User,
                project_id: 7,
                scope: SCOPE_READ_REQUESTS,
            }
        );
        assert_eq!(error.message(), "permission denied");
    }

    #[test]
    fn service_api_key_write_access_requires_service_account_and_scope() {
        let user_key = AuthApiKeyContext {
            id: 1,
            key: "user-key".to_owned(),
            name: "User Key".to_owned(),
            key_type: ApiKeyType::User,
            project: ProjectContext {
                id: 7,
                name: "Default Project".to_owned(),
                status: "active".to_owned(),
            },
            scopes: scope_strings(&[SCOPE_WRITE_API_KEYS]),
            profiles_json: None,
        };
        assert_eq!(
            require_service_api_key_write_access(&user_key).unwrap_err(),
            AuthzFailure::MissingScope {
                principal: AuthzPrincipalKind::ApiKey,
                scope: SCOPE_WRITE_API_KEYS,
            }
        );

        let service_key = AuthApiKeyContext {
            key_type: ApiKeyType::ServiceAccount,
            ..user_key
        };
        assert!(require_service_api_key_write_access(&service_key).is_ok());
    }

    #[test]
    fn authz_failure_audit_reasons_are_stable_and_auditable() {
        let missing_owner = AuthzFailure::MissingBypass {
            principal: AuthzPrincipalKind::User,
            bypass: BypassBoundary::Owner,
        };
        assert_eq!(
            missing_owner.message(),
            "permission denied: owner access required"
        );
        assert_eq!(
            missing_owner.audit_reason(),
            "missing_bypass:user:owner_bypass"
        );

        let missing_scope = AuthzFailure::MissingScope {
            principal: AuthzPrincipalKind::ApiKey,
            scope: SCOPE_WRITE_API_KEYS,
        };
        assert_eq!(
            missing_scope.audit_reason(),
            "missing_scope:api_key:write_api_keys"
        );
    }
}
