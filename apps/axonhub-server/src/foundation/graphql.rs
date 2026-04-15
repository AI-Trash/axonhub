use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_graphql::{Enum, InputObject, SimpleObject};
use chrono::{Datelike, TimeZone};
use chrono_tz::Tz;
use axonhub_db_entity::{api_keys, channel_probes, channels, projects, request_executions, requests, usage_logs};
use axonhub_http::{
    AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
    GraphqlRequestPayload, OpenApiGraphqlPort, ProjectContext, TraceContext,
};
use getrandom::fill as getrandom;
use hex::encode as hex_encode;
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use tracing::{field, Instrument, Span};

    use super::{
        admin::{
            default_auto_backup_settings, default_retry_policy, default_storage_policy, default_system_channel_settings,
            default_system_model_settings,
            default_system_general_settings,
            normalize_retry_policy_load_balancer_strategy,
        default_video_storage_settings, parse_graphql_resource_id, AutoSyncFrequencySetting, BackupFrequencySetting,
            ProbeFrequencySetting, StoredAutoBackupSettings, StoredChannelProbeData,
        StoredSystemGeneralSettings, StoredSystemModelSettings,
        StoredCircuitBreakerStatus,
        StoredProviderQuotaStatus, StoredProxyPreset, StoredRetryPolicy, StoredStoragePolicy,
        StoredSystemChannelSettings, StoredVideoStorageSettings,
    },
    admin_operational::SeaOrmOperationalService,
    authz::{
        authorize_user_system_scope, is_valid_scope, require_owner_bypass,
        require_service_api_key_write_access, require_user_project_scope, scope_strings,
        serialize_scope_slugs, LLM_API_KEY_SCOPES, SCOPE_READ_API_KEYS, SCOPE_READ_CHANNELS,
        SCOPE_READ_PROJECTS, SCOPE_READ_PROMPTS, SCOPE_READ_REQUESTS, SCOPE_READ_ROLES, SCOPE_READ_SETTINGS,
        SCOPE_WRITE_API_KEYS, SCOPE_WRITE_CHANNELS,
        SCOPE_WRITE_PROJECTS, SCOPE_WRITE_PROMPTS, SCOPE_WRITE_ROLES, SCOPE_WRITE_SETTINGS,
    },
    circuit_breaker::SharedCircuitBreaker,
    openai_v1::{parse_model_card, StoredChannelSummary, StoredModelRecord, StoredRequestSummary},
    ports::{AdminGraphqlRepository, OpenApiGraphqlRepository},
    prompt_protection::{
        default_prompt_protection_connection_json, normalize_prompt_protection_status,
        prompt_protection_rule_from_record, prompt_protection_rule_json,
        validate_prompt_protection_rule, PromptProtectionAction, PromptProtectionScope,
        StoredPromptProtectionSettings,
    },
    repositories::graphql::{
        AdminGraphqlSubsetRepository, GraphqlApiKeyRecord, GraphqlAutoBackupSettingsRecord,
        GraphqlChannelRecord, GraphqlDefaultDataStorageRecord, GraphqlModelRecord,
        GraphqlProjectRecord, GraphqlPromptRecord, GraphqlRetryPolicyRecord, GraphqlRoleRecord, GraphqlRoleSummaryRecord,
        GraphqlStoragePolicyRecord, GraphqlSystemChannelSettingsRecord, GraphqlSystemGeneralSettingsRecord,
        GraphqlSystemModelSettingsRecord,
        GraphqlVideoStorageSettingsRecord,
        OpenApiGraphqlMutationRepository,
        SeaOrmAdminGraphqlSubsetRepository, SeaOrmOpenApiGraphqlMutationRepository,
    },
    shared::{
        format_unix_timestamp, graphql_gid, i64_to_i32,
    },
    passwords::hash_password,
};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredPromptAction {
    #[serde(rename = "type")]
    pub(crate) type_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredPromptActivationCondition {
    #[serde(rename = "type")]
    pub(crate) type_field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model_pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) api_key_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredPromptActivationConditionComposite {
    #[serde(default)]
    pub(crate) conditions: Vec<StoredPromptActivationCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredPromptSettings {
    pub(crate) action: StoredPromptAction,
    #[serde(default)]
    pub(crate) conditions: Vec<StoredPromptActivationConditionComposite>,
}

#[cfg(test)]
use super::circuit_breaker::CircuitBreakerPolicy;
use crate::app::build_info::BuildInfo;

pub struct SeaOrmAdminGraphqlService {
    repository: SeaOrmAdminGraphqlSubsetRepository,
    circuit_breaker: SharedCircuitBreaker,
    update_checker: AdminGraphqlUpdateChecker,
}

pub struct SeaOrmOpenApiGraphqlService {
    repository: SeaOrmOpenApiGraphqlMutationRepository,
}



#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlSystemStatus {
    pub(crate) is_initialized: bool,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "SystemVersion", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlSystemVersion {
    pub(crate) version: String,
    pub(crate) commit: String,
    #[graphql(name = "buildTime")]
    pub(crate) build_time: String,
    #[graphql(name = "goVersion")]
    pub(crate) go_version: String,
    pub(crate) platform: String,
    pub(crate) uptime: String,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "VersionCheck", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlVersionCheck {
    pub(crate) current_version: String,
    pub(crate) latest_version: String,
    pub(crate) has_update: bool,
    pub(crate) release_url: String,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "RequestStats", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRequestStats {
    pub(crate) requests_today: i32,
    pub(crate) requests_this_week: i32,
    pub(crate) requests_last_week: i32,
    pub(crate) requests_this_month: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "RequestStatsByChannel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRequestStatsByChannel {
    pub(crate) channel_name: String,
    pub(crate) count: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "RequestStatsByModel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRequestStatsByModel {
    pub(crate) model_id: String,
    pub(crate) count: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "RequestStatsByAPIKey", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRequestStatsByAPIKey {
    pub(crate) api_key_id: String,
    pub(crate) api_key_name: String,
    pub(crate) count: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "TokenStats", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTokenStats {
    pub(crate) total_input_tokens_today: i32,
    pub(crate) total_output_tokens_today: i32,
    pub(crate) total_cached_tokens_today: i32,
    pub(crate) total_input_tokens_this_week: i32,
    pub(crate) total_output_tokens_this_week: i32,
    pub(crate) total_cached_tokens_this_week: i32,
    pub(crate) total_input_tokens_this_month: i32,
    pub(crate) total_output_tokens_this_month: i32,
    pub(crate) total_cached_tokens_this_month: i32,
    pub(crate) total_input_tokens_all_time: i32,
    pub(crate) total_output_tokens_all_time: i32,
    pub(crate) total_cached_tokens_all_time: i32,
    pub(crate) last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CostStatsByModel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCostStatsByModel {
    pub(crate) model_id: String,
    pub(crate) cost: f64,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CostStatsByChannel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCostStatsByChannel {
    pub(crate) channel_name: String,
    pub(crate) cost: f64,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CostStatsByAPIKey", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCostStatsByAPIKey {
    pub(crate) api_key_id: String,
    pub(crate) api_key_name: String,
    pub(crate) cost: f64,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "TokenStatsByAPIKey", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTokenStatsByAPIKey {
    pub(crate) api_key_id: String,
    pub(crate) api_key_name: String,
    pub(crate) input_tokens: i32,
    pub(crate) output_tokens: i32,
    pub(crate) cached_tokens: i32,
    pub(crate) reasoning_tokens: i32,
    pub(crate) total_tokens: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "ModelTokenUsageStats", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModelTokenUsageStats {
    pub(crate) model_id: String,
    pub(crate) input_tokens: i32,
    pub(crate) output_tokens: i32,
    pub(crate) cached_tokens: i32,
    pub(crate) reasoning_tokens: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "APIKeyTokenUsageStats", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlApiKeyTokenUsageStats {
    pub(crate) api_key_id: String,
    pub(crate) input_tokens: i32,
    pub(crate) output_tokens: i32,
    pub(crate) cached_tokens: i32,
    pub(crate) reasoning_tokens: i32,
    pub(crate) top_models: Vec<AdminGraphqlModelTokenUsageStats>,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "TokenStatsByChannel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTokenStatsByChannel {
    pub(crate) channel_name: String,
    pub(crate) input_tokens: i32,
    pub(crate) output_tokens: i32,
    pub(crate) cached_tokens: i32,
    pub(crate) reasoning_tokens: i32,
    pub(crate) total_tokens: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "TokenStatsByModel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTokenStatsByModel {
    pub(crate) model_id: String,
    pub(crate) input_tokens: i32,
    pub(crate) output_tokens: i32,
    pub(crate) cached_tokens: i32,
    pub(crate) reasoning_tokens: i32,
    pub(crate) total_tokens: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "DailyRequestStats", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlDailyRequestStats {
    pub(crate) date: String,
    pub(crate) count: i32,
    pub(crate) tokens: i32,
    pub(crate) cost: f64,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "TopRequestsProjects", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTopRequestsProject {
    pub(crate) project_id: String,
    pub(crate) project_name: String,
    pub(crate) project_description: String,
    pub(crate) request_count: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "DashboardOverview", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlDashboardOverview {
    pub(crate) total_requests: i32,
    pub(crate) request_stats: AdminGraphqlRequestStats,
    pub(crate) failed_requests: i32,
    pub(crate) average_response_time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "ChannelSuccessRate", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelSuccessRate {
    pub(crate) channel_id: String,
    pub(crate) channel_name: String,
    pub(crate) channel_type: String,
    pub(crate) success_count: i32,
    pub(crate) failed_count: i32,
    pub(crate) total_count: i32,
    pub(crate) success_rate: f64,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "FastestChannel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlFastestChannel {
    pub(crate) channel_id: String,
    pub(crate) channel_name: String,
    pub(crate) channel_type: String,
    pub(crate) throughput: f64,
    pub(crate) tokens_count: i32,
    pub(crate) latency_ms: i32,
    pub(crate) request_count: i32,
    pub(crate) confidence_level: String,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "FastestModel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlFastestModel {
    pub(crate) model_id: String,
    pub(crate) model_name: String,
    pub(crate) throughput: f64,
    pub(crate) tokens_count: i32,
    pub(crate) latency_ms: i32,
    pub(crate) request_count: i32,
    pub(crate) confidence_level: String,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "ModelPerformanceStat", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModelPerformanceStat {
    pub(crate) date: String,
    pub(crate) model_id: String,
    pub(crate) throughput: Option<f64>,
    pub(crate) ttft_ms: Option<f64>,
    pub(crate) request_count: i32,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "ChannelPerformanceStat", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelPerformanceStat {
    pub(crate) date: String,
    pub(crate) channel_id: String,
    pub(crate) channel_name: String,
    pub(crate) throughput: Option<f64>,
    pub(crate) ttft_ms: Option<f64>,
    pub(crate) request_count: i32,
}

#[derive(Debug, Clone)]
struct AdminGraphqlUpdateChecker {
    releases_api_url: String,
    release_url_prefix: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct AdminGraphqlGeneralSettings {
    #[serde(alias = "currencyCode")]
    currency_code: String,
    timezone: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct LegacySystemChannelSettings {
    fallback_to_channels_on_model_not_found: Option<bool>,
    query_all_channel_models: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateSystemGeneralSettingsInput")]
pub(crate) struct AdminGraphqlUpdateSystemGeneralSettingsInput {
    pub(crate) currency_code: Option<String>,
    pub(crate) timezone: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateVideoStorageSettingsInput")]
pub(crate) struct AdminGraphqlUpdateVideoStorageSettingsInput {
    pub(crate) enabled: Option<bool>,
    #[serde(rename = "dataStorageID")]
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: Option<i64>,
    pub(crate) scan_interval_minutes: Option<i32>,
    pub(crate) scan_limit: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "FastestChannelsInput")]
pub(crate) struct AdminGraphqlFastestChannelsInput {
    pub(crate) time_window: String,
    pub(crate) limit: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "APIKeyTokenUsageStatsInput")]
pub(crate) struct AdminGraphqlApiKeyTokenUsageStatsInput {
    #[serde(rename = "apiKeyIds")]
    #[graphql(name = "apiKeyIds")]
    pub(crate) api_key_ids: Option<Vec<String>>,
    #[serde(rename = "createdAtGTE")]
    #[graphql(name = "createdAtGTE")]
    pub(crate) created_at_gte: Option<String>,
    #[serde(rename = "createdAtLTE")]
    #[graphql(name = "createdAtLTE")]
    pub(crate) created_at_lte: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AdminGraphqlGitHubRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    published_at: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<SemanticVersionIdentifier>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum SemanticVersionIdentifier {
    Numeric(u64),
    AlphaNumeric(String),
}

const ADMIN_GRAPHQL_RELEASES_API_URL: &str =
    "https://api.github.com/repos/looplj/axonhub/releases";
const ADMIN_GRAPHQL_RELEASE_URL_PREFIX: &str =
    "https://github.com/looplj/axonhub/releases/tag/";
const ADMIN_GRAPHQL_VERSION_CHECK_USER_AGENT: &str = "AxonHub-Version-Checker";
const ADMIN_GRAPHQL_RELEASE_COOLDOWN: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "OnboardingModule", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlOnboardingModule {
    pub(crate) onboarded: bool,
    #[graphql(name = "completedAt")]
    pub(crate) completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, SimpleObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "OnboardingInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlOnboardingInfo {
    pub(crate) onboarded: bool,
    #[graphql(name = "completedAt")]
    pub(crate) completed_at: Option<String>,
    pub(crate) system_model_setting: Option<AdminGraphqlOnboardingModule>,
    pub(crate) auto_disable_channel: Option<AdminGraphqlOnboardingModule>,
}

impl From<crate::foundation::request_context::OnboardingModule> for AdminGraphqlOnboardingModule {
    fn from(value: crate::foundation::request_context::OnboardingModule) -> Self {
        Self {
            onboarded: value.onboarded,
            completed_at: value.completed_at,
        }
    }
}

impl From<crate::foundation::request_context::OnboardingRecord> for AdminGraphqlOnboardingInfo {
    fn from(value: crate::foundation::request_context::OnboardingRecord) -> Self {
        let onboarded = value.onboarded;
        let completed_at = value.completed_at;
        let system_completed_at = completed_at.clone();
        let system_model_setting = Some(match value.system_model_setting {
            Some(module) => module.into(),
            None => AdminGraphqlOnboardingModule {
                onboarded,
                completed_at: system_completed_at,
            },
        });
        let auto_disable_channel = Some(match value.auto_disable_channel {
            Some(module) => module.into(),
            None => AdminGraphqlOnboardingModule {
                onboarded,
                completed_at: completed_at.clone(),
            },
        });

        Self {
            onboarded,
            completed_at,
            system_model_setting,
            auto_disable_channel,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "BrandSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlBrandSettings {
    pub(crate) brand_name: Option<String>,
    pub(crate) brand_logo: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "CleanupOption", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCleanupOption {
    pub(crate) resource_type: String,
    pub(crate) enabled: bool,
    pub(crate) cleanup_days: i32,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "StoragePolicy", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlStoragePolicy {
    pub(crate) store_chunks: bool,
    pub(crate) store_request_body: bool,
    pub(crate) store_response_body: bool,
    pub(crate) cleanup_options: Vec<AdminGraphqlCleanupOption>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RetryPolicy", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRetryPolicy {
    pub(crate) enabled: bool,
    pub(crate) max_channel_retries: i32,
    pub(crate) max_single_channel_retries: i32,
    pub(crate) retry_delay_ms: i32,
    pub(crate) load_balancer_strategy: String,
    pub(crate) auto_disable_channel: AdminGraphqlAutoDisableChannel,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AutoDisableChannel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlAutoDisableChannel {
    pub(crate) enabled: bool,
    pub(crate) statuses: Vec<AdminGraphqlAutoDisableChannelStatus>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AutoDisableChannelStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlAutoDisableChannelStatus {
    pub(crate) status: i32,
    pub(crate) times: i32,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CleanupOptionInput")]
pub(crate) struct AdminGraphqlCleanupOptionInput {
    pub(crate) resource_type: String,
    pub(crate) enabled: bool,
    pub(crate) cleanup_days: i32,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateStoragePolicyInput")]
pub(crate) struct AdminGraphqlUpdateStoragePolicyInput {
    pub(crate) store_chunks: Option<bool>,
    pub(crate) store_request_body: Option<bool>,
    pub(crate) store_response_body: Option<bool>,
    pub(crate) cleanup_options: Option<Vec<AdminGraphqlCleanupOptionInput>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "AutoDisableChannelStatusInput")]
pub(crate) struct AdminGraphqlAutoDisableChannelStatusInput {
    pub(crate) status: i32,
    pub(crate) times: i32,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "AutoDisableChannelInput")]
pub(crate) struct AdminGraphqlAutoDisableChannelInput {
    pub(crate) enabled: Option<bool>,
    pub(crate) statuses: Option<Vec<AdminGraphqlAutoDisableChannelStatusInput>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateRetryPolicyInput")]
pub(crate) struct AdminGraphqlUpdateRetryPolicyInput {
    pub(crate) enabled: Option<bool>,
    pub(crate) max_channel_retries: Option<i32>,
    pub(crate) max_single_channel_retries: Option<i32>,
    pub(crate) retry_delay_ms: Option<i32>,
    pub(crate) load_balancer_strategy: Option<String>,
    pub(crate) auto_disable_channel: Option<AdminGraphqlAutoDisableChannelInput>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateBrandSettingsInput")]
pub(crate) struct AdminGraphqlUpdateBrandSettingsInput {
    pub(crate) brand_name: Option<String>,
    pub(crate) brand_logo: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AutoBackupSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlAutoBackupSettings {
    pub(crate) enabled: bool,
    pub(crate) frequency: BackupFrequencySetting,
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: i32,
    pub(crate) include_channels: bool,
    pub(crate) include_models: bool,
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: bool,
    pub(crate) include_model_prices: bool,
    pub(crate) retention_days: i32,
    pub(crate) last_backup_at: Option<String>,
    pub(crate) last_backup_error: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "VideoStorageSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlVideoStorageSettings {
    pub(crate) enabled: bool,
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: i32,
    pub(crate) scan_interval_minutes: i32,
    pub(crate) scan_limit: i32,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateAutoBackupSettingsInput")]
pub(crate) struct AdminGraphqlUpdateAutoBackupSettingsInput {
    pub(crate) enabled: Option<bool>,
    pub(crate) frequency: Option<BackupFrequencySetting>,
    #[serde(rename = "dataStorageID")]
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: Option<i32>,
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
    #[serde(rename = "includeAPIKeys")]
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: Option<bool>,
    pub(crate) include_model_prices: Option<bool>,
    pub(crate) retention_days: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "TriggerBackupPayload", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTriggerBackupPayload {
    pub(crate) success: bool,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbeSetting", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelProbeSetting {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemChannelSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlSystemChannelSettings {
    pub(crate) probe: AdminGraphqlChannelProbeSetting,
    pub(crate) auto_sync: AdminGraphqlChannelModelAutoSyncSetting,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "SystemModelSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlSystemModelSettings {
    pub(crate) fallback_to_channels_on_model_not_found: bool,
    pub(crate) query_all_channel_models: bool,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PromptAction", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPromptAction {
    #[graphql(name = "type")]
    pub(crate) type_field: String,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PromptActivationCondition", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPromptActivationCondition {
    #[graphql(name = "type")]
    pub(crate) type_field: String,
    pub(crate) model_id: Option<String>,
    pub(crate) model_pattern: Option<String>,
    pub(crate) api_key_id: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PromptActivationConditionComposite", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPromptActivationConditionComposite {
    pub(crate) conditions: Vec<AdminGraphqlPromptActivationCondition>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PromptSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPromptSettings {
    pub(crate) action: AdminGraphqlPromptAction,
    pub(crate) conditions: Vec<AdminGraphqlPromptActivationConditionComposite>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Prompt", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPrompt {
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    #[graphql(name = "projectID")]
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) status: String,
    pub(crate) order: i32,
    pub(crate) settings: AdminGraphqlPromptSettings,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PromptEdge", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPromptEdge {
    pub(crate) cursor: Option<String>,
    pub(crate) node: Option<AdminGraphqlPrompt>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PromptConnection", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPromptConnection {
    pub(crate) edges: Vec<AdminGraphqlPromptEdge>,
    pub(crate) page_info: AdminGraphqlPageInfo,
    pub(crate) total_count: i32,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelModelAutoSyncSetting", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelModelAutoSyncSetting {
    pub(crate) frequency: AutoSyncFrequencySetting,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateChannelProbeSettingInput")]
pub(crate) struct AdminGraphqlUpdateChannelProbeSettingInput {
    pub(crate) enabled: bool,
    pub(crate) frequency: ProbeFrequencySetting,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateSystemChannelSettingsInput")]
pub(crate) struct AdminGraphqlUpdateSystemChannelSettingsInput {
    pub(crate) probe: Option<AdminGraphqlUpdateChannelProbeSettingInput>,
    pub(crate) auto_sync: Option<AdminGraphqlUpdateChannelModelAutoSyncSettingInput>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateChannelModelAutoSyncSettingInput")]
pub(crate) struct AdminGraphqlUpdateChannelModelAutoSyncSettingInput {
    pub(crate) frequency: AutoSyncFrequencySetting,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateSystemModelSettingsInput")]
pub(crate) struct AdminGraphqlUpdateSystemModelSettingsInput {
    pub(crate) fallback_to_channels_on_model_not_found: Option<bool>,
    pub(crate) query_all_channel_models: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CompleteOnboardingInput")]
pub(crate) struct AdminGraphqlCompleteOnboardingInput {
    pub(crate) dummy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CompleteSystemModelSettingOnboardingInput")]
pub(crate) struct AdminGraphqlCompleteSystemModelSettingOnboardingInput {
    pub(crate) dummy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CompleteAutoDisableChannelOnboardingInput")]
pub(crate) struct AdminGraphqlCompleteAutoDisableChannelOnboardingInput {
    pub(crate) dummy: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProxyPreset", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProxyPreset {
    pub(crate) name: Option<String>,
    pub(crate) url: String,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "SaveProxyPresetInput")]
pub(crate) struct AdminGraphqlSaveProxyPresetInput {
    pub(crate) name: Option<String>,
    pub(crate) url: String,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserAgentPassThroughSettings", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserAgentPassThroughSettings {
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateDefaultDataStorageInput")]
pub(crate) struct AdminGraphqlUpdateDefaultDataStorageInput {
    #[serde(rename = "dataStorageID")]
    #[graphql(name = "dataStorageID")]
    pub(crate) data_storage_id: String,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbePoint", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelProbePoint {
    pub(crate) timestamp: i64,
    pub(crate) total_request_count: i32,
    pub(crate) success_request_count: i32,
    pub(crate) avg_tokens_per_second: Option<f64>,
    pub(crate) avg_time_to_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ChannelProbeData", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannelProbeData {
    pub(crate) channel_id: String,
    pub(crate) points: Vec<AdminGraphqlChannelProbePoint>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "GetChannelProbeDataInput")]
pub(crate) struct AdminGraphqlGetChannelProbeDataInput {
    #[serde(rename = "channelIDs")]
    #[graphql(name = "channelIDs")]
    pub(crate) channel_ids: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProviderQuotaStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProviderQuotaStatus {
    pub(crate) id: String,
    pub(crate) channel_id: String,
    pub(crate) provider_type: String,
    pub(crate) status: String,
    pub(crate) ready: bool,
    pub(crate) next_reset_at: Option<String>,
    pub(crate) next_check_at: String,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "CircuitBreakerStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlCircuitBreakerStatus {
    pub(crate) channel_id: String,
    pub(crate) model_id: String,
    pub(crate) state: String,
    pub(crate) consecutive_failures: i32,
    pub(crate) next_probe_at_seconds: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum PromptProtectionRuleStatus {
    Enabled,
    Disabled,
    Archived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum AdminGraphqlPromptProtectionAction {
    Mask,
    Reject,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum AdminGraphqlPromptProtectionScope {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "PromptProtectionSettingsInput")]
pub(crate) struct AdminGraphqlPromptProtectionSettingsInput {
    pub(crate) action: AdminGraphqlPromptProtectionAction,
    pub(crate) replacement: Option<String>,
    pub(crate) scopes: Option<Vec<AdminGraphqlPromptProtectionScope>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreatePromptProtectionRuleInput")]
pub(crate) struct AdminGraphqlCreatePromptProtectionRuleInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) pattern: String,
    pub(crate) settings: AdminGraphqlPromptProtectionSettingsInput,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdatePromptProtectionRuleInput")]
pub(crate) struct AdminGraphqlUpdatePromptProtectionRuleInput {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) pattern: Option<String>,
    pub(crate) status: Option<PromptProtectionRuleStatus>,
    pub(crate) settings: Option<AdminGraphqlPromptProtectionSettingsInput>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "PromptActionInput")]
pub(crate) struct AdminGraphqlPromptActionInput {
    #[serde(rename = "type")]
    #[graphql(name = "type")]
    pub(crate) type_field: String,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "PromptActivationConditionInput")]
pub(crate) struct AdminGraphqlPromptActivationConditionInput {
    #[serde(rename = "type")]
    #[graphql(name = "type")]
    pub(crate) type_field: String,
    pub(crate) model_id: Option<String>,
    pub(crate) model_pattern: Option<String>,
    pub(crate) api_key_id: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "PromptActivationConditionCompositeInput")]
pub(crate) struct AdminGraphqlPromptActivationConditionCompositeInput {
    pub(crate) conditions: Vec<AdminGraphqlPromptActivationConditionInput>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "PromptSettingsInput")]
pub(crate) struct AdminGraphqlPromptSettingsInput {
    pub(crate) action: AdminGraphqlPromptActionInput,
    pub(crate) conditions: Option<Vec<AdminGraphqlPromptActivationConditionCompositeInput>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreatePromptInput")]
pub(crate) struct AdminGraphqlCreatePromptInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) role: String,
    pub(crate) content: String,
    pub(crate) status: Option<String>,
    pub(crate) order: Option<i32>,
    pub(crate) settings: AdminGraphqlPromptSettingsInput,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdatePromptInput")]
pub(crate) struct AdminGraphqlUpdatePromptInput {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) role: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) order: Option<i32>,
    pub(crate) settings: Option<AdminGraphqlPromptSettingsInput>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Channel", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlChannel {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) channel_type: String,
    pub(crate) base_url: String,
    pub(crate) status: String,
    pub(crate) supported_models: Vec<String>,
    pub(crate) ordering_weight: i32,
    pub(crate) provider_quota_status: Option<AdminGraphqlProviderQuotaStatus>,
    pub(crate) circuit_breaker_status: Option<AdminGraphqlCircuitBreakerStatus>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Model", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModel {
    pub(crate) id: String,
    pub(crate) developer: String,
    pub(crate) model_id: String,
    pub(crate) model_type: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) remark: String,
    pub(crate) context_length: Option<i32>,
    pub(crate) max_output_tokens: Option<i32>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Request", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRequestSummaryObject {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) trace_id: Option<String>,
    pub(crate) channel_id: Option<String>,
    pub(crate) model_id: String,
    pub(crate) format: String,
    pub(crate) status: String,
    pub(crate) source: String,
    pub(crate) external_id: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Trace", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlTrace {
    pub(crate) id: String,
    pub(crate) trace_id: String,
    pub(crate) project_id: String,
    pub(crate) thread_id: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "APIKey", rename_fields = "camelCase")]
pub(crate) struct OpenApiGraphqlApiKey {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AdminApiKey", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlApiKey {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) key_type: String,
    pub(crate) status: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "BackupPayload", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlBackupPayload {
    pub(crate) success: bool,
    pub(crate) data: Option<String>,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RestorePayload", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRestorePayload {
    pub(crate) success: bool,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum CreateLlmApiKeyError {
    InvalidName,
    PermissionDenied,
    Internal(String),
}

impl SeaOrmAdminGraphqlService {
    pub fn new(db: super::seaorm::SeaOrmConnectionFactory) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory(&db);
        Self::with_dependencies(db, circuit_breaker, AdminGraphqlUpdateChecker::github())
    }

    fn with_dependencies(
        db: super::seaorm::SeaOrmConnectionFactory,
        circuit_breaker: SharedCircuitBreaker,
        update_checker: AdminGraphqlUpdateChecker,
    ) -> Self {
        Self {
            repository: SeaOrmAdminGraphqlSubsetRepository::new(db),
            circuit_breaker,
            update_checker,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_circuit_breaker_policy(
        db: super::seaorm::SeaOrmConnectionFactory,
        policy: CircuitBreakerPolicy,
    ) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory_and_policy(&db, policy);
        Self::with_dependencies(db, circuit_breaker, AdminGraphqlUpdateChecker::github())
    }

    #[cfg(test)]
    pub(crate) fn new_with_update_checker_urls(
        db: super::seaorm::SeaOrmConnectionFactory,
        releases_api_url: impl Into<String>,
        release_url_prefix: impl Into<String>,
    ) -> Self {
        let circuit_breaker = SharedCircuitBreaker::with_factory(&db);
        let update_checker = AdminGraphqlUpdateChecker::new(releases_api_url, release_url_prefix);
        Self::with_dependencies(db, circuit_breaker, update_checker)
    }
}

impl SeaOrmOpenApiGraphqlService {
    pub fn new(db: super::seaorm::SeaOrmConnectionFactory) -> Self {
        Self {
            repository: SeaOrmOpenApiGraphqlMutationRepository::new(db),
        }
    }
}

impl AdminGraphqlPort for SeaOrmAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let repository = self.repository.clone();
        let circuit_breaker = self.circuit_breaker.clone();
        let update_checker = self.update_checker.clone();
        let span = graphql_execution_span(
            "admin",
            graphql_request_kind(request.query.as_str()),
            graphql_variables_present(&request.variables),
            "jwt",
            "admin",
            project_id.is_some(),
        );
        Box::pin(async move {
            let payload = request;
            match execute_admin_graphql_seaorm_request(
                repository,
                circuit_breaker,
                update_checker,
                payload,
                project_id,
                user,
            )
            .await
            {
                Ok(result) => {
                    record_graphql_execution_outcome(&Span::current(), &result);
                    result
                }
                Err(message) => {
                    let result = GraphqlExecutionResult {
                        status: 200,
                        body: serde_json::json!({
                            "data": null,
                            "errors": [{"message": format!("Failed to execute GraphQL request: {message}")}],
                        }),
                    };
                    record_graphql_internal_failure(&Span::current(), result.status);
                    result
                }
            }
        }
        .instrument(span))
    }
}

impl AdminGraphqlRepository for SeaOrmAdminGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        project_id: Option<i64>,
        user: AuthUserContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        <Self as AdminGraphqlPort>::execute_graphql(self, request, project_id, user)
    }
}

impl OpenApiGraphqlPort for SeaOrmOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        let repository = self.repository.clone();
        let span = graphql_execution_span(
            "openapi",
            graphql_request_kind(request.query.as_str()),
            graphql_variables_present(&request.variables),
            graphql_api_key_auth_mode(&owner_api_key),
            graphql_api_key_auth_subject(&owner_api_key),
            true,
        );
        Box::pin(async move {
            let payload = request;
            match execute_openapi_graphql_seaorm_request(repository, payload, owner_api_key).await {
                Ok(result) => {
                    record_graphql_execution_outcome(&Span::current(), &result);
                    result
                }
                Err(message) => {
                    let result = GraphqlExecutionResult {
                        status: 200,
                        body: serde_json::json!({
                            "data": null,
                            "errors": [{"message": format!("Failed to execute GraphQL request: {message}")}],
                        }),
                    };
                    record_graphql_internal_failure(&Span::current(), result.status);
                    result
                }
            }
        }
        .instrument(span))
    }
}

impl OpenApiGraphqlRepository for SeaOrmOpenApiGraphqlService {
    fn execute_graphql(
        &self,
        request: GraphqlRequestPayload,
        owner_api_key: AuthApiKeyContext,
    ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
        <Self as OpenApiGraphqlPort>::execute_graphql(self, request, owner_api_key)
    }
}

fn graphql_execution_span(
    surface: &'static str,
    kind: &'static str,
    variables_bound: bool,
    auth_mode: &'static str,
    auth_subject: &'static str,
    project_bound: bool,
) -> Span {
    tracing::span!(
        tracing::Level::INFO,
        "graphql.execution",
        operation.name = "graphql.execute",
        graphql.surface = surface,
        graphql.kind = kind,
        graphql.variables.bound = variables_bound,
        auth.mode = auth_mode,
        auth.subject = auth_subject,
        project.bound = project_bound,
        request.outcome = field::Empty,
        http.status_code = field::Empty,
    )
}

fn graphql_request_kind(query: &str) -> &'static str {
    let trimmed = query.trim_start();
    if trimmed.starts_with("mutation") {
        "mutation"
    } else if trimmed.starts_with("query") || trimmed.starts_with('{') {
        "query"
    } else {
        "unknown"
    }
}

fn graphql_variables_present(variables: &Value) -> bool {
    match variables {
        Value::Null => false,
        Value::Object(values) => !values.is_empty(),
        _ => true,
    }
}

fn graphql_api_key_auth_mode(api_key: &AuthApiKeyContext) -> &'static str {
    match api_key.key_type {
        axonhub_http::ApiKeyType::NoAuth => "noauth",
        _ => "api_key",
    }
}

fn graphql_api_key_auth_subject(api_key: &AuthApiKeyContext) -> &'static str {
    match api_key.key_type {
        axonhub_http::ApiKeyType::User => "user_api_key",
        axonhub_http::ApiKeyType::ServiceAccount => "service_api_key",
        axonhub_http::ApiKeyType::NoAuth => "system_noauth",
    }
}

fn record_graphql_execution_outcome(span: &Span, result: &GraphqlExecutionResult) {
    span.record("http.status_code", i64::from(result.status));
    let outcome = if result.status >= 500 {
        "internal_error"
    } else if graphql_response_has_error(&result.body) {
        "graphql_error"
    } else {
        "success"
    };
    span.record("request.outcome", outcome);
}

fn record_graphql_internal_failure(span: &Span, status_code: u16) {
    span.record("http.status_code", i64::from(status_code));
    span.record("request.outcome", "internal_error");
}

fn graphql_response_has_error(body: &Value) -> bool {
    body.get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
        || body.get("error").is_some()
}

async fn execute_admin_graphql_seaorm_request(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    circuit_breaker: SharedCircuitBreaker,
    update_checker: AdminGraphqlUpdateChecker,
    payload: GraphqlRequestPayload,
    _project_id: Option<i64>,
    user: AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let query = payload.query.trim();

    if query.contains("storagePolicy") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("storagePolicy");
        }

        return query_storage_policy_seaorm(&repository);
    }

    if query.contains("autoBackupSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("autoBackupSettings");
        }

        return query_auto_backup_settings_seaorm(&repository);
    }

    if query.contains("videoStorageSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("videoStorageSettings");
        }

        return query_video_storage_settings_seaorm(&repository);
    }

    if query.contains("allScopes") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("allScopes");
        }

        let level = graphql_string_argument(query, "level");
        let scopes = filter_admin_graphql_scope_info(level.as_deref())?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "allScopes": scopes.into_iter().map(|scope| json!({
                        "scope": scope.scope,
                        "description": scope.description,
                        "levels": scope.levels,
                    })).collect::<Vec<_>>()
                }
            }),
        });
    }

    if query.contains("defaultDataStorageID") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("defaultDataStorageID");
        }

        return query_default_data_storage_id_seaorm(&repository);
    }

    if query.contains("systemChannelSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("systemChannelSettings");
        }

        return query_system_channel_settings_seaorm(&repository);
    }

    if query.contains("systemGeneralSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("systemGeneralSettings");
        }

        return query_system_general_settings_seaorm(&repository);
    }

    if query.contains("systemModelSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("systemModelSettings");
        }

        return query_system_model_settings_seaorm(&repository);
    }

    if query.contains("brandSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("brandSettings");
        }

        return query_brand_settings_seaorm(&repository);
    }

    if query.contains("onboardingInfo") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("onboardingInfo");
        }

        return query_onboarding_info_seaorm(&repository);
    }

    if query.contains("proxyPresets") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("proxyPresets");
        }

        return query_proxy_presets_seaorm(&repository);
    }

    if query.contains("userAgentPassThroughSettings") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("userAgentPassThroughSettings");
        }

        return query_user_agent_pass_through_settings_seaorm(&repository);
    }

    if query.contains("channels") {
        if authorize_user_system_scope(&user, SCOPE_READ_CHANNELS).is_err() {
            return graphql_permission_denied("channels");
        }

        return query_channels_seaorm(&repository, &circuit_breaker);
    }

    if first_graphql_field_name(query).as_deref() == Some("projects") {
        if authorize_user_system_scope(&user, SCOPE_READ_PROJECTS).is_err() {
            return graphql_permission_denied("projects");
        }

        return query_projects_seaorm(&repository);
    }

    if first_graphql_field_name(query).as_deref() == Some("myProjects") {
        return query_my_projects_seaorm(&repository, &user);
    }

    if first_graphql_field_name(query).as_deref() == Some("roles") {
        if authorize_user_system_scope(&user, SCOPE_READ_ROLES).is_err() {
            return graphql_permission_denied("roles");
        }

        return query_roles_seaorm(&repository);
    }

    if first_graphql_field_name(query).as_deref() == Some("apiKeys") {
        if authorize_user_system_scope(&user, SCOPE_READ_API_KEYS).is_err() {
            return graphql_permission_denied("apiKeys");
        }

        return query_api_keys_seaorm(&repository);
    }

    if first_graphql_field_name(query).as_deref() == Some("requests") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this query".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_READ_REQUESTS).is_err() {
            return graphql_permission_denied("requests");
        }

        let requests = repository.query_requests(project_id)?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "requests": requests.iter().map(request_summary_json).collect::<Vec<_>>()
                }
            }),
        });
    }

    if query.contains("updateStoragePolicy") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateStoragePolicy");
        }

        return update_storage_policy_seaorm(&repository, payload.variables);
    }

    if query.contains("updateRetryPolicy") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateRetryPolicy");
        }

        return update_retry_policy_seaorm(&repository, payload.variables);
    }

    if query.contains("updateBrandSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateBrandSettings");
        }

        return update_brand_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("retryPolicy") {
        if authorize_user_system_scope(&user, SCOPE_READ_SETTINGS).is_err() {
            return graphql_permission_denied("retryPolicy");
        }

        return query_retry_policy_seaorm(&repository);
    }

    if query.contains("updateAutoBackupSettings") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("updateAutoBackupSettings");
        }

        return update_auto_backup_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("updateDefaultDataStorage") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateDefaultDataStorage");
        }

        return update_default_data_storage_seaorm(&repository, payload.variables);
    }

    if query.contains("updateSystemChannelSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateSystemChannelSettings");
        }

        return update_system_channel_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("updateSystemGeneralSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateSystemGeneralSettings");
        }

        return update_system_general_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("updateSystemModelSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateSystemModelSettings");
        }

        return update_system_model_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("updateVideoStorageSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateVideoStorageSettings");
        }

        return update_video_storage_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("saveProxyPreset") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("saveProxyPreset");
        }

        return save_proxy_preset_seaorm(&repository, payload.variables);
    }

    if query.contains("deleteProxyPreset") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("deleteProxyPreset");
        }

        return delete_proxy_preset_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUserAgentPassThroughSettings") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("updateUserAgentPassThroughSettings");
        }

        return update_user_agent_pass_through_settings_seaorm(&repository, payload.variables);
    }

    if query.contains("completeSystemModelSettingOnboarding") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("completeSystemModelSettingOnboarding");
        }

        return complete_system_model_setting_onboarding_seaorm(&repository, payload.variables);
    }

    if query.contains("completeAutoDisableChannelOnboarding") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("completeAutoDisableChannelOnboarding");
        }

        return complete_auto_disable_channel_onboarding_seaorm(&repository, payload.variables);
    }

    if query.contains("completeOnboarding") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("completeOnboarding");
        }

        return complete_onboarding_seaorm(&repository, payload.variables);
    }

    if query.contains("triggerAutoBackup") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("triggerAutoBackup");
        }

        return trigger_auto_backup_seaorm(&repository, user.id);
    }

    if query.contains("backup") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("backup");
        }

        return backup_seaorm(&repository, payload.variables);
    }

    if query.contains("restore") {
        if require_owner_bypass(&user).is_err() {
            return graphql_owner_denied("restore");
        }

        return restore_seaorm(&repository, payload.variables, user.id);
    }

    if query.contains("checkProviderQuotas") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("checkProviderQuotas");
        }

        return check_provider_quotas_seaorm(&repository, user.id);
    }

    if query.contains("resetProviderQuota") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return graphql_permission_denied("resetProviderQuota");
        }

        return reset_provider_quota_seaorm(&repository, payload.variables, user.id);
    }

    if query.contains("triggerGcCleanup") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_SETTINGS).is_err() {
            return Ok(GraphqlExecutionResult {
                status: 200,
                body: json!({
                    "data": {"triggerGcCleanup": Value::Null},
                    "errors": [{"message": "permission denied: requires write:settings scope"}],
                }),
            });
        }

        return trigger_gc_cleanup_seaorm(&repository, user.id);
    }

    if query.contains("createUser") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_WRITE_USERS).is_err() {
            return graphql_permission_denied("createUser");
        }

        return create_user_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUserStatus") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_WRITE_USERS).is_err() {
            return graphql_permission_denied("updateUserStatus");
        }

        return update_user_status_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updateUser") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_WRITE_USERS).is_err() {
            return graphql_permission_denied("updateUser");
        }

        return update_user_seaorm(&repository, payload.variables);
    }

    if first_graphql_field_name(query).as_deref() == Some("createPrompt") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("createPrompt");
        }

        return create_prompt_graphql_seaorm(&repository, payload.variables, project_id);
    }

    if first_graphql_field_name(query).as_deref() == Some("updatePromptStatus") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("updatePromptStatus");
        }

        return update_prompt_status_graphql_seaorm(&repository, payload.variables, project_id);
    }

    if first_graphql_field_name(query).as_deref() == Some("bulkDeletePrompts") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkDeletePrompts");
        }

        return bulk_delete_prompts_graphql_seaorm(&repository, payload.variables, project_id);
    }

    if first_graphql_field_name(query).as_deref() == Some("bulkEnablePrompts") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkEnablePrompts");
        }

        return bulk_update_prompts_status_graphql_seaorm(&repository, payload.variables, project_id, "enabled", "bulkEnablePrompts");
    }

    if first_graphql_field_name(query).as_deref() == Some("bulkDisablePrompts") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkDisablePrompts");
        }

        return bulk_update_prompts_status_graphql_seaorm(&repository, payload.variables, project_id, "disabled", "bulkDisablePrompts");
    }

    if first_graphql_field_name(query).as_deref() == Some("deletePrompt") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("deletePrompt");
        }

        return delete_prompt_graphql_seaorm(&repository, payload.variables, project_id);
    }

    if first_graphql_field_name(query).as_deref() == Some("updatePrompt") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this mutation".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("updatePrompt");
        }

        return update_prompt_graphql_seaorm(&repository, payload.variables, project_id);
    }

    if query.contains("createPromptProtectionRule") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("createPromptProtectionRule");
        }

        return create_prompt_protection_rule_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updatePromptProtectionRuleStatus") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("updatePromptProtectionRuleStatus");
        }

        return update_prompt_protection_rule_status_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("bulkDeletePromptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkDeletePromptProtectionRules");
        }

        return bulk_delete_prompt_protection_rules_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("bulkEnablePromptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkEnablePromptProtectionRules");
        }

        return bulk_set_prompt_protection_rules_status_graphql_seaorm(
            &repository,
            payload.variables,
            "enabled",
            "bulkEnablePromptProtectionRules",
        );
    }

    if query.contains("bulkDisablePromptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("bulkDisablePromptProtectionRules");
        }

        return bulk_set_prompt_protection_rules_status_graphql_seaorm(
            &repository,
            payload.variables,
            "disabled",
            "bulkDisablePromptProtectionRules",
        );
    }

    if query.contains("deletePromptProtectionRule") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("deletePromptProtectionRule");
        }

        return delete_prompt_protection_rule_graphql_seaorm(&repository, payload.variables);
    }

    if query.contains("updatePromptProtectionRule") {
        if authorize_user_system_scope(&user, SCOPE_WRITE_PROMPTS).is_err() {
            return graphql_permission_denied("updatePromptProtectionRule");
        }

        return update_prompt_protection_rule_graphql_seaorm(&repository, payload.variables);
    }

    if first_graphql_field_name(query).as_deref() == Some("me") {
        return query_me_graphql_seaorm(&repository, &user);
    }

    if query.contains("updateMe") {
        return update_me_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createProject") {
        return create_project_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateProject") {
        return update_project_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createRole") {
        return create_role_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateRole") {
        return update_role_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createAPIKey") {
        return create_api_key_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateAPIKey") {
        return update_api_key_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createChannel") {
        return create_channel_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateChannel") {
        return update_channel_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("createModel") {
        return create_model_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("updateModel") {
        return update_model_seaorm(&repository, payload.variables, &user);
    }

    if query.contains("queryModels") {
        if authorize_user_system_scope(&user, SCOPE_READ_CHANNELS).is_err() {
            return graphql_permission_denied("queryModels");
        }

        let models = repository
            .query_model_statuses()?
            .into_iter()
            .map(|row| {
                serde_json::json!({
                    "id": graphql_gid("model", row.id),
                    "status": row.status,
                })
            })
            .collect::<Vec<_>>();

        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({"data": {"queryModels": models}}),
        });
    }

    if query.contains("promptProtectionRules") {
        if authorize_user_system_scope(&user, SCOPE_READ_PROMPTS).is_err() {
            return graphql_permission_denied("promptProtectionRules");
        }

        return query_prompt_protection_rules_graphql_seaorm(&repository);
    }

    if query.contains("systemStatus") {
        let is_initialized = repository.query_is_initialized()?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({"data": {"systemStatus": {"isInitialized": is_initialized}}}),
        });
    }

    if query.contains("systemVersion") {
        return query_system_version_seaorm(&repository);
    }

    if query.contains("dashboardOverview") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("dashboardOverview");
        }

        return query_dashboard_overview_seaorm(repository).await;
    }

    if first_graphql_field_name(query).as_deref() == Some("requestStats") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("requestStats");
        }

        let request_stats = query_request_stats_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "requestStats": request_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("requestStatsByChannel") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("requestStatsByChannel");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let request_stats = query_request_stats_by_channel_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "requestStatsByChannel": request_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("requestStatsByModel") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("requestStatsByModel");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let request_stats = query_request_stats_by_model_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "requestStatsByModel": request_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("requestStatsByAPIKey") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("requestStatsByAPIKey");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let request_stats = query_request_stats_by_api_key_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "requestStatsByAPIKey": request_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("tokenStats") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("tokenStats");
        }

        let token_stats = query_token_stats_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "tokenStats": token_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("costStatsByModel") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("costStatsByModel");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let cost_stats = query_cost_stats_by_model_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "costStatsByModel": cost_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("costStatsByChannel") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("costStatsByChannel");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let cost_stats = query_cost_stats_by_channel_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "costStatsByChannel": cost_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("costStatsByAPIKey") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("costStatsByAPIKey");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let cost_stats = query_cost_stats_by_api_key_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "costStatsByAPIKey": cost_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("tokenStatsByAPIKey") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("tokenStatsByAPIKey");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let token_stats = query_token_stats_by_api_key_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "tokenStatsByAPIKey": token_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("tokenStatsByChannel") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("tokenStatsByChannel");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let token_stats = query_token_stats_by_channel_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "tokenStatsByChannel": token_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("tokenStatsByModel") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("tokenStatsByModel");
        }

        let time_window = graphql_input_string(&payload.variables, query, "timeWindow");
        let token_stats = query_token_stats_by_model_seaorm(repository, time_window).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "tokenStatsByModel": token_stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("dailyRequestStats") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("dailyRequestStats");
        }

        let stats = query_daily_request_stats_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "dailyRequestStats": stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("prompts") {
        let project_id = _project_id.ok_or_else(|| "project context is required for this query".to_owned())?;
        if require_user_project_scope(&user, project_id, SCOPE_READ_PROMPTS).is_err() {
            return graphql_permission_denied("prompts");
        }

        return query_prompts_graphql_seaorm(&repository, project_id);
    }

    if first_graphql_field_name(query).as_deref() == Some("topRequestsProjects") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("topRequestsProjects");
        }

        let projects = query_top_requests_projects_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "topRequestsProjects": projects,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("fastestChannels") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("fastestChannels");
        }

        let input = parse_graphql_variable_input::<AdminGraphqlFastestChannelsInput>(
            payload.variables.clone(),
            "input",
            "input is required",
        )?;
        let channels = query_fastest_channels_seaorm(repository, input).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "fastestChannels": channels,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("fastestModels") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("fastestModels");
        }

        let input = parse_graphql_variable_input::<AdminGraphqlFastestChannelsInput>(
            payload.variables.clone(),
            "input",
            "input is required",
        )?;
        let models = query_fastest_models_seaorm(repository, input).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "fastestModels": models,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("modelPerformanceStats") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("modelPerformanceStats");
        }

        let stats = query_model_performance_stats_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "modelPerformanceStats": stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("channelPerformanceStats") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("channelPerformanceStats");
        }

        let stats = query_channel_performance_stats_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "channelPerformanceStats": stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("channelProbeData") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_CHANNELS).is_err() {
            return graphql_permission_denied("channelProbeData");
        }

        let input = parse_graphql_variable_input::<AdminGraphqlGetChannelProbeDataInput>(
            payload.variables.clone(),
            "input",
            "input is required",
        )?;
        let probe_data = query_channel_probe_data_seaorm(repository, input)?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "channelProbeData": probe_data,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("apiKeyTokenUsageStats") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_API_KEYS).is_err() {
            return graphql_permission_denied("apiKeyTokenUsageStats");
        }

        let input = parse_graphql_variable_input::<AdminGraphqlApiKeyTokenUsageStatsInput>(
            payload.variables.clone(),
            "input",
            "input is required",
        )?;
        let stats = query_api_key_token_usage_stats_seaorm(repository, input).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "apiKeyTokenUsageStats": stats,
                }
            }),
        });
    }

    if first_graphql_field_name(query).as_deref() == Some("channelSuccessRates") {
        if authorize_user_system_scope(&user, super::authz::SCOPE_READ_DASHBOARD).is_err() {
            return graphql_permission_denied("channelSuccessRates");
        }

        let success_rates = query_channel_success_rates_seaorm(repository).await?;
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {
                    "channelSuccessRates": success_rates,
                }
            }),
        });
    }

    if query.contains("checkForUpdate") {
        return query_check_for_update_seaorm(update_checker).await;
    }

    if let Some(field) = first_graphql_field_name(query) {
        return graphql_not_implemented_for_route("/admin/graphql", field.as_str());
    }

    Err("unsupported graphql subset query".to_owned())
}

fn graphql_permission_denied(field: &str) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {field: Value::Null},
            "errors": [{"message": "permission denied"}],
        }),
    })
}

fn graphql_owner_denied(field: &str) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {field: Value::Null},
            "errors": [{"message": "permission denied: owner access required"}],
        }),
    })
}

fn graphql_field_error(field: &str, message: impl Into<String>) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {field: Value::Null},
            "errors": [{"message": message.into()}],
        }),
    })
}

fn graphql_not_implemented_for_route(
    route_family: &str,
    field: &str,
) -> Result<GraphqlExecutionResult, String> {
    Ok(GraphqlExecutionResult {
        status: 501,
        body: json!({
            "error": "not_implemented",
            "status": 501,
            "route_family": route_family,
            "method": "POST",
            "path": route_family,
            "message": format!("GraphQL field `{field}` is not supported"),
        }),
    })
}

fn query_storage_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let policy = load_storage_policy_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "storagePolicy": storage_policy_json(&policy),
            }
        }),
    })
}

fn query_retry_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let policy = load_retry_policy_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "retryPolicy": retry_policy_json(&policy),
            }
        }),
    })
}

fn query_me_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let profile = load_graphql_user_profile(repository, user.id)?
        .ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"me": admin_user_profile_json(&profile)}}),
    })
}

fn query_auto_backup_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_auto_backup_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "autoBackupSettings": auto_backup_settings_json(&settings),
            }
        }),
    })
}

fn query_video_storage_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_video_storage_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "videoStorageSettings": video_storage_settings_json(&settings),
            }
        }),
    })
}

fn query_default_data_storage_id_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let current = load_default_data_storage_id_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "defaultDataStorageID": current.map(|id| graphql_gid("DataStorage", id)),
            }
        }),
    })
}

fn query_system_channel_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_system_channel_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "systemChannelSettings": system_channel_settings_json(&settings),
            }
        }),
    })
}

fn query_system_general_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_system_general_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "systemGeneralSettings": system_general_settings_json(&settings),
            }
        }),
    })
}

fn query_system_model_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let settings = load_system_model_settings_seaorm(repository)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "systemModelSettings": system_model_settings_json(&settings),
            }
        }),
    })
}

fn query_brand_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let brand_name = repository.query_brand_name()?;
    let brand_logo = repository.query_brand_logo()?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "brandSettings": {
                    "brandName": brand_name,
                    "brandLogo": brand_logo,
                }
            }
        }),
    })
}

fn update_brand_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = variables.get("input").ok_or_else(|| "input is required".to_owned())?;
    if let Some(brand_name) = input.get("brandName").and_then(Value::as_str) {
        repository.upsert_brand_name(brand_name)?;
    }
    if let Some(brand_logo) = input.get("brandLogo").and_then(Value::as_str) {
        repository.upsert_brand_logo(brand_logo)?;
    }

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateBrandSettings": true}}),
    })
}

fn query_onboarding_info_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let onboarding = repository
        .query_onboarding_record()?
        .unwrap_or_default();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "onboardingInfo": AdminGraphqlOnboardingInfo::from(onboarding),
            }
        }),
    })
}

fn query_prompts_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    project_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let prompts = repository
        .query_prompts(project_id)?
        .into_iter()
        .map(admin_graphql_prompt_from_record)
        .collect::<Result<Vec<_>, _>>()?;
    let edges = prompts
        .iter()
        .map(|prompt| {
            json!({
                "cursor": prompt.id,
                "node": prompt_json(prompt),
            })
        })
        .collect::<Vec<_>>();
    let start_cursor = prompts.first().map(|prompt| prompt.id.clone());
    let end_cursor = prompts.last().map(|prompt| prompt.id.clone());

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "prompts": {
                    "edges": edges,
                    "pageInfo": {
                        "hasNextPage": false,
                        "hasPreviousPage": false,
                        "startCursor": start_cursor,
                        "endCursor": end_cursor,
                    },
                    "totalCount": prompts.len(),
                }
            }
        }),
    })
}

fn query_system_version_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let current = BuildInfo::current();
    let stored_version = repository.query_version()?.unwrap_or_else(|| current.version().to_owned());
    let commit = current.commit().unwrap_or_default().to_owned();
    let build_time = current.build_time().unwrap_or_default().to_owned();
    let go_version = current
        .go_version()
        .unwrap_or("n/a (Rust build)")
        .to_owned();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "systemVersion": {
                    "version": stored_version,
                    "commit": commit,
                    "buildTime": build_time,
                    "goVersion": go_version,
                    "platform": current.platform(),
                    "uptime": current.uptime(),
                }
            }
        }),
    })
}

async fn query_dashboard_overview_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

    let db = repository.db();
    let overview = db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;

            let total_requests = requests::Entity::find()
                .count(&connection)
                .await
                .map_err(|error| error.to_string())?;
            let total_requests = total_requests as i32;

            let failed_requests = requests::Entity::find()
                .filter(requests::Column::Status.eq("failed"))
                .count(&connection)
                .await
                .map_err(|error| error.to_string())?;
            let failed_requests = failed_requests as i32;

            let request_stats = query_request_stats_connection(connection).await?;

            Ok::<AdminGraphqlDashboardOverview, String>(AdminGraphqlDashboardOverview {
                total_requests,
                request_stats,
                failed_requests,
                average_response_time: None,
            })
        })
        .map_err(|error| error.to_string())?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "dashboardOverview": overview,
            }
        }),
    })
}

async fn query_request_stats_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<AdminGraphqlRequestStats, String> {
    let db = repository.db();
    Ok(db.run_sync(move |db| async move {
        let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
        query_request_stats_connection(connection).await
    })
    .map_err(|error| error.to_string())?)
}

async fn query_request_stats_by_channel_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlRequestStatsByChannel>, String> {
    let db = repository.db();
    Ok(db.run_sync(move |db| async move {
        let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
        query_request_stats_by_channel_connection(connection, time_window).await
    })
    .map_err(|error| error.to_string())?)
}

async fn query_request_stats_by_model_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlRequestStatsByModel>, String> {
    let db = repository.db();
    Ok(db.run_sync(move |db| async move {
        let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
        query_request_stats_by_model_connection(connection, time_window).await
    })
    .map_err(|error| error.to_string())?)
}

async fn query_request_stats_by_api_key_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlRequestStatsByAPIKey>, String> {
    let db = repository.db();
    Ok(db.run_sync(move |db| async move {
        let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
        query_request_stats_by_api_key_connection(connection, time_window).await
    })
    .map_err(|error| error.to_string())?)
}

async fn query_token_stats_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<AdminGraphqlTokenStats, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_token_stats_connection(connection).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_cost_stats_by_model_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlCostStatsByModel>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_cost_stats_by_model_connection(connection, time_window).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_cost_stats_by_channel_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlCostStatsByChannel>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_cost_stats_by_channel_connection(connection, time_window).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_cost_stats_by_api_key_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlCostStatsByAPIKey>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_cost_stats_by_api_key_connection(connection, time_window).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_token_stats_by_api_key_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlTokenStatsByAPIKey>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_token_stats_by_api_key_connection(connection, time_window).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_api_key_token_usage_stats_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    input: AdminGraphqlApiKeyTokenUsageStatsInput,
) -> Result<Vec<AdminGraphqlApiKeyTokenUsageStats>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_api_key_token_usage_stats_connection(connection, input).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_token_stats_by_channel_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlTokenStatsByChannel>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_token_stats_by_channel_connection(connection, time_window).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_token_stats_by_model_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlTokenStatsByModel>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_token_stats_by_model_connection(connection, time_window).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_daily_request_stats_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Vec<AdminGraphqlDailyRequestStats>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_daily_request_stats_connection(connection).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_top_requests_projects_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Vec<AdminGraphqlTopRequestsProject>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_top_requests_projects_connection(connection).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_fastest_channels_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    input: AdminGraphqlFastestChannelsInput,
) -> Result<Vec<AdminGraphqlFastestChannel>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_fastest_channels_connection(connection, input).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_fastest_models_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    input: AdminGraphqlFastestChannelsInput,
) -> Result<Vec<AdminGraphqlFastestModel>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_fastest_models_connection(connection, input).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_model_performance_stats_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Vec<AdminGraphqlModelPerformanceStat>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_model_performance_stats_connection(connection).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_channel_performance_stats_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Vec<AdminGraphqlChannelPerformanceStat>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_channel_performance_stats_connection(connection).await
        })
        .map_err(|error| error.to_string())?)
}

fn query_channel_probe_data_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
    input: AdminGraphqlGetChannelProbeDataInput,
) -> Result<Vec<Value>, String> {
    let channel_ids = parse_graphql_id_list(Some(input.channel_ids), "channel")?;
    let probe_data = SeaOrmOperationalService::new(repository.db()).channel_probe_data(&channel_ids)?;
    Ok(probe_data
        .into_iter()
        .map(|item| {
            json!({
                "channelID": graphql_gid("channel", item.channel_id),
                "points": item.points.into_iter().map(|point| json!({
                    "timestamp": point.timestamp,
                    "totalRequestCount": point.total_request_count,
                    "successRequestCount": point.success_request_count,
                    "avgTokensPerSecond": point.avg_tokens_per_second,
                    "avgTimeToFirstTokenMs": point.avg_time_to_first_token_ms,
                })).collect::<Vec<_>>()
            })
        })
        .collect())
}

async fn query_channel_success_rates_seaorm(
    repository: SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Vec<AdminGraphqlChannelSuccessRate>, String> {
    let db = repository.db();
    Ok(db
        .run_sync(move |db| async move {
            let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
            query_channel_success_rates_connection(connection).await
        })
        .map_err(|error| error.to_string())?)
}

async fn query_request_stats_by_channel_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlRequestStatsByChannel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    let query = usage_logs::Entity::find()
        .join(sea_orm::JoinType::InnerJoin, usage_logs::Relation::Channels.def())
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column(channels::Column::Name)
        .column_as(sea_orm::sea_query::Expr::cust("COUNT(*)"), "request_count")
        .group_by(channels::Column::Name);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_tuple::<(String, i64)>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|(channel_name, count)| AdminGraphqlRequestStatsByChannel {
            channel_name,
            count: count as i32,
        })
        .collect())
}

async fn query_request_stats_by_model_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlRequestStatsByModel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ModelId)
        .column_as(sea_orm::sea_query::Expr::cust("COUNT(*)"), "request_count")
        .group_by(usage_logs::Column::ModelId);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_tuple::<(String, i64)>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|(model_id, count)| AdminGraphqlRequestStatsByModel {
            model_id,
            count: count as i32,
        })
        .collect())
}

async fn query_request_stats_by_api_key_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlRequestStatsByAPIKey>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct ApiKeyStatsRow {
        api_key_id: i64,
        request_count: i64,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ApiKeyId)
        .column_as(sea_orm::sea_query::Expr::cust("COUNT(*)"), "request_count")
        .filter(usage_logs::Column::ApiKeyId.is_not_null())
        .group_by(usage_logs::Column::ApiKeyId);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<ApiKeyStatsRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| right.request_count.cmp(&left.request_count).then_with(|| left.api_key_id.cmp(&right.api_key_id)));
    rows.truncate(10);

    let api_key_ids: Vec<i64> = rows.iter().map(|row| row.api_key_id).collect();
    let api_key_map = api_keys::Entity::find()
        .select_only()
        .column(api_keys::Column::Id)
        .column(api_keys::Column::Name)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .filter(api_keys::Column::Id.is_in(api_key_ids))
        .into_tuple::<(i64, String)>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<i64, String>>();

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            api_key_map.get(&row.api_key_id).map(|api_key_name| AdminGraphqlRequestStatsByAPIKey {
                api_key_id: graphql_gid("api_key", row.api_key_id),
                api_key_name: api_key_name.clone(),
                count: row.request_count as i32,
            })
        })
        .collect())
}

async fn query_top_requests_projects_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<Vec<AdminGraphqlTopRequestsProject>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait};

    #[derive(sea_orm::FromQueryResult)]
    struct TopRequestsProjectsRow {
        project_id: i64,
        project_name: String,
        project_description: String,
        request_count: i64,
    }

    let mut rows = requests::Entity::find()
        .join(sea_orm::JoinType::InnerJoin, requests::Relation::Projects.def())
        .filter(projects::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column(requests::Column::ProjectId)
        .column_as(projects::Column::Name, "project_name")
        .column_as(projects::Column::Description, "project_description")
        .column_as(sea_orm::sea_query::Expr::cust("COUNT(*)"), "request_count")
        .group_by(requests::Column::ProjectId)
        .group_by(projects::Column::Name)
        .group_by(projects::Column::Description)
        .into_model::<TopRequestsProjectsRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        right
            .request_count
            .cmp(&left.request_count)
            .then_with(|| left.project_id.cmp(&right.project_id))
    });
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|row| AdminGraphqlTopRequestsProject {
            project_id: graphql_gid("project", row.project_id),
            project_name: row.project_name,
            project_description: row.project_description,
            request_count: i64_to_i32(row.request_count),
        })
        .collect())
}

#[derive(sea_orm::FromQueryResult)]
struct AdminGraphqlTokenStatsAggregateRow {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    last_updated: Option<String>,
}

async fn query_token_stats_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<AdminGraphqlTokenStats, String> {
    let now = chrono::Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let this_week_start =
        today_start - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
    let this_month_start = now
        .date_naive()
        .with_day(1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();

    let today_start = today_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let this_week_start = this_week_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let this_month_start = this_month_start.format("%Y-%m-%d %H:%M:%S").to_string();

    let today = query_token_stats_aggregate(&connection, Some(today_start.as_str())).await?;
    let this_week = query_token_stats_aggregate(&connection, Some(this_week_start.as_str())).await?;
    let this_month = query_token_stats_aggregate(&connection, Some(this_month_start.as_str())).await?;
    let all_time = query_token_stats_aggregate(&connection, None).await?;

    Ok(AdminGraphqlTokenStats {
        total_input_tokens_today: i64_to_i32(today.input_tokens.unwrap_or(0)),
        total_output_tokens_today: i64_to_i32(today.output_tokens.unwrap_or(0)),
        total_cached_tokens_today: i64_to_i32(today.cached_tokens.unwrap_or(0)),
        total_input_tokens_this_week: i64_to_i32(this_week.input_tokens.unwrap_or(0)),
        total_output_tokens_this_week: i64_to_i32(this_week.output_tokens.unwrap_or(0)),
        total_cached_tokens_this_week: i64_to_i32(this_week.cached_tokens.unwrap_or(0)),
        total_input_tokens_this_month: i64_to_i32(this_month.input_tokens.unwrap_or(0)),
        total_output_tokens_this_month: i64_to_i32(this_month.output_tokens.unwrap_or(0)),
        total_cached_tokens_this_month: i64_to_i32(this_month.cached_tokens.unwrap_or(0)),
        total_input_tokens_all_time: i64_to_i32(all_time.input_tokens.unwrap_or(0)),
        total_output_tokens_all_time: i64_to_i32(all_time.output_tokens.unwrap_or(0)),
        total_cached_tokens_all_time: i64_to_i32(all_time.cached_tokens.unwrap_or(0)),
        last_updated: all_time
            .last_updated
            .as_deref()
            .map(normalize_usage_log_last_updated),
    })
}

async fn query_cost_stats_by_model_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlCostStatsByModel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct CostStatsByModelRow {
        model_id: String,
        total_cost: Option<f64>,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ModelId)
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(total_cost), 0)"),
            "total_cost",
        )
        .group_by(usage_logs::Column::ModelId);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<CostStatsByModelRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        right
            .total_cost
            .unwrap_or(0.0)
            .total_cmp(&left.total_cost.unwrap_or(0.0))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|row| AdminGraphqlCostStatsByModel {
            model_id: row.model_id,
            cost: row.total_cost.unwrap_or(0.0),
        })
        .collect())
}

async fn query_cost_stats_by_channel_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlCostStatsByChannel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct CostStatsByChannelRow {
        channel_name: String,
        total_cost: Option<f64>,
    }

    let query = usage_logs::Entity::find()
        .join(sea_orm::JoinType::InnerJoin, usage_logs::Relation::Channels.def())
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column_as(channels::Column::Name, "channel_name")
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(total_cost), 0)"),
            "total_cost",
        )
        .group_by(channels::Column::Name);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<CostStatsByChannelRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        right
            .total_cost
            .unwrap_or(0.0)
            .total_cmp(&left.total_cost.unwrap_or(0.0))
            .then_with(|| left.channel_name.cmp(&right.channel_name))
    });
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|row| AdminGraphqlCostStatsByChannel {
            channel_name: row.channel_name,
            cost: row.total_cost.unwrap_or(0.0),
        })
        .collect())
}

async fn query_cost_stats_by_api_key_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlCostStatsByAPIKey>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct ApiKeyCostRow {
        api_key_id: i64,
        total_cost: Option<f64>,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ApiKeyId)
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(total_cost), 0)"),
            "total_cost",
        )
        .filter(usage_logs::Column::ApiKeyId.is_not_null())
        .group_by(usage_logs::Column::ApiKeyId);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<ApiKeyCostRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        right
            .total_cost
            .unwrap_or(0.0)
            .total_cmp(&left.total_cost.unwrap_or(0.0))
            .then_with(|| left.api_key_id.cmp(&right.api_key_id))
    });
    rows.truncate(10);

    let api_key_ids: Vec<i64> = rows.iter().map(|row| row.api_key_id).collect();
    let api_key_map = api_keys::Entity::find()
        .select_only()
        .column(api_keys::Column::Id)
        .column(api_keys::Column::Name)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .filter(api_keys::Column::Id.is_in(api_key_ids))
        .into_tuple::<(i64, String)>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<i64, String>>();

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            api_key_map.get(&row.api_key_id).map(|api_key_name| AdminGraphqlCostStatsByAPIKey {
                api_key_id: graphql_gid("api_key", row.api_key_id),
                api_key_name: api_key_name.clone(),
                cost: row.total_cost.unwrap_or(0.0),
            })
        })
        .collect())
}

async fn query_token_stats_by_api_key_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlTokenStatsByAPIKey>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct ApiKeyTokenRow {
        api_key_id: i64,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        reasoning_tokens: Option<i64>,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ApiKeyId)
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_tokens), 0)"),
            "input_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_tokens), 0)"),
            "output_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_cached_tokens), 0)"),
            "cached_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_reasoning_tokens), 0)"),
            "reasoning_tokens",
        )
        .filter(usage_logs::Column::ApiKeyId.is_not_null())
        .group_by(usage_logs::Column::ApiKeyId);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<ApiKeyTokenRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        let left_total = left.input_tokens.unwrap_or(0)
            + left.output_tokens.unwrap_or(0)
            + left.cached_tokens.unwrap_or(0)
            + left.reasoning_tokens.unwrap_or(0);
        let right_total = right.input_tokens.unwrap_or(0)
            + right.output_tokens.unwrap_or(0)
            + right.cached_tokens.unwrap_or(0)
            + right.reasoning_tokens.unwrap_or(0);
        right_total.cmp(&left_total).then_with(|| left.api_key_id.cmp(&right.api_key_id))
    });
    rows.truncate(3);

    let api_key_ids: Vec<i64> = rows.iter().map(|row| row.api_key_id).collect();
    let api_key_map = api_keys::Entity::find()
        .select_only()
        .column(api_keys::Column::Id)
        .column(api_keys::Column::Name)
        .filter(api_keys::Column::DeletedAt.eq(0_i64))
        .filter(api_keys::Column::Id.is_in(api_key_ids))
        .into_tuple::<(i64, String)>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<i64, String>>();

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            api_key_map.get(&row.api_key_id).map(|api_key_name| {
                let input_tokens = i64_to_i32(row.input_tokens.unwrap_or(0));
                let output_tokens = i64_to_i32(row.output_tokens.unwrap_or(0));
                let cached_tokens = i64_to_i32(row.cached_tokens.unwrap_or(0));
                let reasoning_tokens = i64_to_i32(row.reasoning_tokens.unwrap_or(0));

                AdminGraphqlTokenStatsByAPIKey {
                    api_key_id: graphql_gid("api_key", row.api_key_id),
                    api_key_name: api_key_name.clone(),
                    input_tokens,
                    output_tokens,
                    cached_tokens,
                    reasoning_tokens,
                    total_tokens: input_tokens + output_tokens + cached_tokens + reasoning_tokens,
                }
            })
        })
        .collect())
}

async fn query_api_key_token_usage_stats_connection(
    connection: sea_orm::DatabaseConnection,
    input: AdminGraphqlApiKeyTokenUsageStatsInput,
) -> Result<Vec<AdminGraphqlApiKeyTokenUsageStats>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let api_key_ids = input
        .api_key_ids
        .ok_or_else(|| "apiKeyIds is required and must contain at least one API key".to_owned())?;
    if api_key_ids.is_empty() {
        return Err("apiKeyIds is required and must contain at least one API key".to_owned());
    }
    if api_key_ids.len() > 100 {
        return Err("apiKeyIds cannot exceed 100 items".to_owned());
    }

    let mut parsed_api_key_ids = Vec::with_capacity(api_key_ids.len());
    for api_key_id in api_key_ids {
        let parsed = parse_graphql_resource_id(api_key_id.as_str(), "api_key")
            .or_else(|_| parse_graphql_resource_id(api_key_id.as_str(), "apiKey"))
            .map_err(|_| "invalid api key id".to_owned())?;
        parsed_api_key_ids.push(parsed);
    }

    #[derive(sea_orm::FromQueryResult)]
    struct ApiKeyTokenUsageRow {
        api_key_id: i64,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        reasoning_tokens: Option<i64>,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ApiKeyId)
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_tokens), 0)"),
            "input_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_tokens), 0)"),
            "output_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_cached_tokens), 0)"),
            "cached_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_reasoning_tokens), 0)"),
            "reasoning_tokens",
        )
        .filter(usage_logs::Column::ApiKeyId.is_in(parsed_api_key_ids.clone()))
        .group_by(usage_logs::Column::ApiKeyId);

    let query = if let Some(created_at_gte) = input.created_at_gte.as_deref() {
        query.filter(usage_logs::Column::CreatedAt.gte(created_at_gte))
    } else {
        query
    };
    let query = if let Some(created_at_lte) = input.created_at_lte.as_deref() {
        query.filter(usage_logs::Column::CreatedAt.lte(created_at_lte))
    } else {
        query
    };

    let rows = query
        .into_model::<ApiKeyTokenUsageRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let top_models = query_top_models_for_api_keys_connection(
        connection.clone(),
        &parsed_api_key_ids,
        input.created_at_gte.as_deref(),
        input.created_at_lte.as_deref(),
    )
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AdminGraphqlApiKeyTokenUsageStats {
            api_key_id: graphql_gid("api_key", row.api_key_id),
            input_tokens: i64_to_i32(row.input_tokens.unwrap_or(0)),
            output_tokens: i64_to_i32(row.output_tokens.unwrap_or(0)),
            cached_tokens: i64_to_i32(row.cached_tokens.unwrap_or(0)),
            reasoning_tokens: i64_to_i32(row.reasoning_tokens.unwrap_or(0)),
            top_models: top_models.get(&row.api_key_id).cloned().unwrap_or_default(),
        })
        .collect())
}

async fn query_top_models_for_api_keys_connection(
    connection: sea_orm::DatabaseConnection,
    api_key_ids: &[i64],
    created_at_gte: Option<&str>,
    created_at_lte: Option<&str>,
) -> Result<HashMap<i64, Vec<AdminGraphqlModelTokenUsageStats>>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    #[derive(sea_orm::FromQueryResult)]
    struct ApiKeyModelTokenUsageRow {
        api_key_id: i64,
        model_id: String,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        reasoning_tokens: Option<i64>,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ApiKeyId)
        .column(usage_logs::Column::ModelId)
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_tokens), 0)"),
            "input_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_tokens), 0)"),
            "output_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_cached_tokens), 0)"),
            "cached_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_reasoning_tokens), 0)"),
            "reasoning_tokens",
        )
        .filter(usage_logs::Column::ApiKeyId.is_in(api_key_ids.to_vec()))
        .group_by(usage_logs::Column::ApiKeyId)
        .group_by(usage_logs::Column::ModelId);

    let query = if let Some(created_at_gte) = created_at_gte {
        query.filter(usage_logs::Column::CreatedAt.gte(created_at_gte))
    } else {
        query
    };
    let query = if let Some(created_at_lte) = created_at_lte {
        query.filter(usage_logs::Column::CreatedAt.lte(created_at_lte))
    } else {
        query
    };

    let rows = query
        .into_model::<ApiKeyModelTokenUsageRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let mut grouped = HashMap::<i64, Vec<AdminGraphqlModelTokenUsageStats>>::new();
    for row in rows {
        grouped
            .entry(row.api_key_id)
            .or_default()
            .push(AdminGraphqlModelTokenUsageStats {
                model_id: row.model_id,
                input_tokens: i64_to_i32(row.input_tokens.unwrap_or(0)),
                output_tokens: i64_to_i32(row.output_tokens.unwrap_or(0)),
                cached_tokens: i64_to_i32(row.cached_tokens.unwrap_or(0)),
                reasoning_tokens: i64_to_i32(row.reasoning_tokens.unwrap_or(0)),
            });
    }

    for models in grouped.values_mut() {
        models.sort_by(|left, right| {
            let left_total = left.input_tokens + left.output_tokens + left.cached_tokens + left.reasoning_tokens;
            let right_total = right.input_tokens + right.output_tokens + right.cached_tokens + right.reasoning_tokens;
            right_total.cmp(&left_total).then_with(|| left.model_id.cmp(&right.model_id))
        });
        models.truncate(3);
    }

    Ok(grouped)
}

async fn query_token_stats_by_channel_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlTokenStatsByChannel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, RelationTrait};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct ChannelTokenRow {
        channel_name: String,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        reasoning_tokens: Option<i64>,
    }

    let query = usage_logs::Entity::find()
        .join(sea_orm::JoinType::InnerJoin, usage_logs::Relation::Channels.def())
        .filter(channels::Column::DeletedAt.eq(0_i64))
        .select_only()
        .column_as(channels::Column::Name, "channel_name")
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_tokens), 0)"),
            "input_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_tokens), 0)"),
            "output_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_cached_tokens), 0)"),
            "cached_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_reasoning_tokens), 0)"),
            "reasoning_tokens",
        )
        .group_by(channels::Column::Name);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<ChannelTokenRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        let left_total = left.input_tokens.unwrap_or(0)
            + left.output_tokens.unwrap_or(0)
            + left.cached_tokens.unwrap_or(0)
            + left.reasoning_tokens.unwrap_or(0);
        let right_total = right.input_tokens.unwrap_or(0)
            + right.output_tokens.unwrap_or(0)
            + right.cached_tokens.unwrap_or(0)
            + right.reasoning_tokens.unwrap_or(0);
        right_total.cmp(&left_total).then_with(|| left.channel_name.cmp(&right.channel_name))
    });
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|row| {
            let input_tokens = i64_to_i32(row.input_tokens.unwrap_or(0));
            let output_tokens = i64_to_i32(row.output_tokens.unwrap_or(0));
            let cached_tokens = i64_to_i32(row.cached_tokens.unwrap_or(0));
            let reasoning_tokens = i64_to_i32(row.reasoning_tokens.unwrap_or(0));
            AdminGraphqlTokenStatsByChannel {
                channel_name: row.channel_name,
                input_tokens,
                output_tokens,
                cached_tokens,
                reasoning_tokens,
                total_tokens: input_tokens + output_tokens + cached_tokens + reasoning_tokens,
            }
        })
        .collect())
}

async fn query_token_stats_by_model_connection(
    connection: sea_orm::DatabaseConnection,
    time_window: Option<String>,
) -> Result<Vec<AdminGraphqlTokenStatsByModel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let since = parse_admin_graphql_time_window(time_window.as_deref())?;

    #[derive(sea_orm::FromQueryResult)]
    struct ModelTokenRow {
        model_id: String,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        reasoning_tokens: Option<i64>,
    }

    let query = usage_logs::Entity::find()
        .select_only()
        .column(usage_logs::Column::ModelId)
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_tokens), 0)"),
            "input_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_tokens), 0)"),
            "output_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_cached_tokens), 0)"),
            "cached_tokens",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_reasoning_tokens), 0)"),
            "reasoning_tokens",
        )
        .group_by(usage_logs::Column::ModelId);

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since.as_str()))
    } else {
        query
    };

    let mut rows = query
        .into_model::<ModelTokenRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        let left_total = left.input_tokens.unwrap_or(0)
            + left.output_tokens.unwrap_or(0)
            + left.cached_tokens.unwrap_or(0)
            + left.reasoning_tokens.unwrap_or(0);
        let right_total = right.input_tokens.unwrap_or(0)
            + right.output_tokens.unwrap_or(0)
            + right.cached_tokens.unwrap_or(0)
            + right.reasoning_tokens.unwrap_or(0);
        right_total.cmp(&left_total).then_with(|| left.model_id.cmp(&right.model_id))
    });
    rows.truncate(10);

    Ok(rows
        .into_iter()
        .map(|row| {
            let input_tokens = i64_to_i32(row.input_tokens.unwrap_or(0));
            let output_tokens = i64_to_i32(row.output_tokens.unwrap_or(0));
            let cached_tokens = i64_to_i32(row.cached_tokens.unwrap_or(0));
            let reasoning_tokens = i64_to_i32(row.reasoning_tokens.unwrap_or(0));
            AdminGraphqlTokenStatsByModel {
                model_id: row.model_id,
                input_tokens,
                output_tokens,
                cached_tokens,
                reasoning_tokens,
                total_tokens: input_tokens + output_tokens + cached_tokens + reasoning_tokens,
            }
        })
        .collect())
}

async fn query_channel_success_rates_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<Vec<AdminGraphqlChannelSuccessRate>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    #[derive(sea_orm::FromQueryResult)]
    struct ChannelExecutionStatsRow {
        channel_id: i64,
        success_count: Option<i64>,
        failed_count: Option<i64>,
    }

    let mut rows = request_executions::Entity::find()
        .select_only()
        .column(request_executions::Column::ChannelId)
        .column_as(
            sea_orm::sea_query::Expr::cust("SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END)"),
            "success_count",
        )
        .column_as(
            sea_orm::sea_query::Expr::cust("SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END)"),
            "failed_count",
        )
        .filter(request_executions::Column::ChannelId.is_not_null())
        .group_by(request_executions::Column::ChannelId)
        .into_model::<ChannelExecutionStatsRow>()
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    rows.sort_by(|left, right| {
        let right_total = right.success_count.unwrap_or(0) + right.failed_count.unwrap_or(0);
        let left_total = left.success_count.unwrap_or(0) + left.failed_count.unwrap_or(0);
        right_total.cmp(&left_total).then_with(|| left.channel_id.cmp(&right.channel_id))
    });
    rows.truncate(5);

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let channel_ids: Vec<i64> = rows.iter().map(|row| row.channel_id).collect();
    let channel_map = channels::Entity::find()
        .filter(channels::Column::Id.is_in(channel_ids.clone()))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|channel| (channel.id, (channel.name, channel.type_field)))
        .collect::<HashMap<i64, (String, String)>>();

    Ok(rows
        .into_iter()
        .map(|row| {
            let success_count = i64_to_i32(row.success_count.unwrap_or(0));
            let failed_count = i64_to_i32(row.failed_count.unwrap_or(0));
            let total_count = success_count + failed_count;
            let success_rate = if total_count > 0 {
                (success_count as f64 / total_count as f64) * 100.0
            } else {
                0.0
            };
            let (channel_name, channel_type) = channel_map
                .get(&row.channel_id)
                .cloned()
                .unwrap_or_else(|| (String::new(), String::new()));

            AdminGraphqlChannelSuccessRate {
                channel_id: graphql_gid("channel", row.channel_id),
                channel_name,
                channel_type,
                success_count,
                failed_count,
                total_count,
                success_rate,
            }
        })
        .collect())
}

async fn query_fastest_channels_connection(
    connection: sea_orm::DatabaseConnection,
    input: AdminGraphqlFastestChannelsInput,
) -> Result<Vec<AdminGraphqlFastestChannel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let limit = input.limit.unwrap_or(5).clamp(1, 100) as usize;
    let since = match input.time_window.as_str() {
        "day" => Some(chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap()),
        "week" => {
            let now = chrono::Utc::now();
            Some(
                (now.date_naive()
                    - chrono::Duration::days(now.weekday().num_days_from_monday() as i64))
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            )
        }
        "month" => Some(
            chrono::Utc::now()
                .date_naive()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        ),
        _ => Some(chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap()),
    }
    .unwrap()
    .format("%Y-%m-%d %H:%M:%S")
    .to_string();

    let executions = request_executions::Entity::find()
        .filter(request_executions::Column::CreatedAt.gte(since.as_str()))
        .filter(request_executions::Column::MetricsLatencyMs.is_not_null())
        .filter(request_executions::Column::MetricsLatencyMs.gt(0_i64))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let mut latest_by_request = HashMap::<i64, axonhub_db_entity::request_executions::Model>::new();
    for execution in executions {
        if execution.status != "completed" {
            continue;
        }
        match latest_by_request.get(&execution.request_id) {
            Some(current)
                if current.created_at > execution.created_at
                    || (current.created_at == execution.created_at && current.id >= execution.id) => {}
            _ => {
                latest_by_request.insert(execution.request_id, execution);
            }
        }
    }

    let latest_completed: Vec<_> = latest_by_request
        .into_values()
        .filter(|execution| execution.channel_id.is_some())
        .collect();
    if latest_completed.is_empty() {
        return Ok(Vec::new());
    }

    let request_ids: Vec<i64> = latest_completed.iter().map(|execution| execution.request_id).collect();
    let usage_rows = usage_logs::Entity::find()
        .filter(usage_logs::Column::RequestId.is_in(request_ids.clone()))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let usage_by_request = usage_rows
        .into_iter()
        .map(|row| (row.request_id, row))
        .collect::<HashMap<i64, axonhub_db_entity::usage_logs::Model>>();

    let channel_ids: Vec<i64> = latest_completed.iter().filter_map(|execution| execution.channel_id).collect();
    let channel_map = channels::Entity::find()
        .filter(channels::Column::Id.is_in(channel_ids))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|channel| (channel.id, (channel.name, channel.type_field)))
        .collect::<HashMap<i64, (String, String)>>();

    #[derive(Clone)]
    struct FastestChannelAggregate {
        channel_id: i64,
        channel_name: String,
        channel_type: String,
        tokens_count: i64,
        latency_ms: i64,
        request_count: i64,
        throughput: f64,
        confidence_level: String,
        confidence_score: i32,
    }

    #[derive(Default)]
    struct ChannelAccumulator {
        channel_name: String,
        channel_type: String,
        tokens_count: i64,
        latency_ms: i64,
        request_count: i64,
    }

    let mut by_channel = HashMap::<i64, ChannelAccumulator>::new();
    for execution in latest_completed {
        let Some(channel_id) = execution.channel_id else { continue; };
        let Some(usage) = usage_by_request.get(&execution.request_id) else { continue; };
        let Some((channel_name, channel_type)) = channel_map.get(&channel_id) else { continue; };
        let Some(latency_ms) = execution.metrics_latency_ms else { continue; };

        let completion_latency = if execution.stream {
            match execution.metrics_first_token_latency_ms {
                Some(first_token) if first_token < latency_ms => latency_ms - first_token,
                Some(_) => 0,
                None => latency_ms,
            }
        } else {
            latency_ms
        };

        let tokens = usage.completion_tokens
            + usage.completion_reasoning_tokens
            + usage.completion_audio_tokens;

        let entry = by_channel.entry(channel_id).or_default();
        entry.channel_name = channel_name.clone();
        entry.channel_type = channel_type.clone();
        entry.tokens_count += tokens;
        entry.latency_ms += completion_latency;
        entry.request_count += 1;
    }

    let mut aggregates = by_channel
        .into_iter()
        .map(|(channel_id, acc)| FastestChannelAggregate {
            channel_id,
            channel_name: acc.channel_name,
            channel_type: acc.channel_type,
            tokens_count: acc.tokens_count,
            latency_ms: acc.latency_ms,
            request_count: acc.request_count,
            throughput: if acc.latency_ms > 0 {
                acc.tokens_count as f64 * 1000.0 / acc.latency_ms as f64
            } else {
                0.0
            },
            confidence_level: String::new(),
            confidence_score: 0,
        })
        .collect::<Vec<_>>();

    if aggregates.is_empty() {
        return Ok(Vec::new());
    }

    let mut request_counts = aggregates.iter().map(|item| item.request_count).collect::<Vec<_>>();
    request_counts.sort_unstable();
    let mid = request_counts.len() / 2;
    let median = if request_counts.len() % 2 == 0 {
        (request_counts[mid - 1] + request_counts[mid]) as f64 / 2.0
    } else {
        request_counts[mid] as f64
    };

    for item in &mut aggregates {
        let confidence_level = graphql_calculate_confidence_level(item.request_count, median);
        let confidence_score = match confidence_level.as_str() {
            "high" => 3,
            "medium" => 2,
            _ => 1,
        };
        item.confidence_level = confidence_level;
        item.confidence_score = confidence_score;
    }

    let high_medium_count = aggregates
        .iter()
        .filter(|item| item.confidence_level == "high" || item.confidence_level == "medium")
        .count();
    let mut results_to_show = if high_medium_count >= limit {
        aggregates
            .into_iter()
            .filter(|item| item.confidence_level == "high" || item.confidence_level == "medium")
            .collect::<Vec<_>>()
    } else {
        aggregates
    };

    results_to_show.sort_by(|left, right| {
        right
            .confidence_score
            .cmp(&left.confidence_score)
            .then_with(|| right.throughput.total_cmp(&left.throughput))
            .then_with(|| left.channel_id.cmp(&right.channel_id))
    });
    results_to_show.truncate(limit);

    Ok(results_to_show
        .into_iter()
        .map(|item| AdminGraphqlFastestChannel {
            channel_id: graphql_gid("channel", item.channel_id),
            channel_name: item.channel_name,
            channel_type: item.channel_type,
            throughput: item.throughput,
            tokens_count: i64_to_i32(item.tokens_count),
            latency_ms: i64_to_i32(item.latency_ms),
            request_count: i64_to_i32(item.request_count),
            confidence_level: item.confidence_level,
        })
        .collect())
}

fn graphql_calculate_confidence_level(request_count: i64, median: f64) -> String {
    if median == 0.0 {
        return "low".to_owned();
    }
    if request_count < 100 {
        return "low".to_owned();
    }
    let ratio = request_count as f64 / median;
    if ratio >= 1.5 && request_count >= 500 {
        return "high".to_owned();
    }
    if ratio >= 0.5 {
        return "medium".to_owned();
    }
    "low".to_owned()
}

async fn query_fastest_models_connection(
    connection: sea_orm::DatabaseConnection,
    input: AdminGraphqlFastestChannelsInput,
) -> Result<Vec<AdminGraphqlFastestModel>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let limit = input.limit.unwrap_or(5).clamp(1, 100) as usize;
    let since = match input.time_window.as_str() {
        "day" => Some(chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap()),
        "week" => {
            let now = chrono::Utc::now();
            Some(
                (now.date_naive()
                    - chrono::Duration::days(now.weekday().num_days_from_monday() as i64))
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            )
        }
        "month" => Some(
            chrono::Utc::now()
                .date_naive()
                .with_day(1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        ),
        _ => Some(chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap()),
    }
    .unwrap()
    .format("%Y-%m-%d %H:%M:%S")
    .to_string();

    let executions = request_executions::Entity::find()
        .filter(request_executions::Column::CreatedAt.gte(since.as_str()))
        .filter(request_executions::Column::MetricsLatencyMs.is_not_null())
        .filter(request_executions::Column::MetricsLatencyMs.gt(0_i64))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let mut latest_by_request = HashMap::<i64, axonhub_db_entity::request_executions::Model>::new();
    for execution in executions {
        if execution.status != "completed" {
            continue;
        }
        match latest_by_request.get(&execution.request_id) {
            Some(current)
                if current.created_at > execution.created_at
                    || (current.created_at == execution.created_at && current.id >= execution.id) => {}
            _ => {
                latest_by_request.insert(execution.request_id, execution);
            }
        }
    }

    let latest_completed: Vec<_> = latest_by_request.into_values().collect();
    if latest_completed.is_empty() {
        return Ok(Vec::new());
    }

    let request_ids: Vec<i64> = latest_completed.iter().map(|execution| execution.request_id).collect();
    let usage_rows = usage_logs::Entity::find()
        .filter(usage_logs::Column::RequestId.is_in(request_ids.clone()))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let usage_by_request = usage_rows
        .into_iter()
        .map(|row| (row.request_id, row))
        .collect::<HashMap<i64, axonhub_db_entity::usage_logs::Model>>();

    let request_rows = requests::Entity::find()
        .filter(requests::Column::Id.is_in(request_ids))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let request_by_id = request_rows
        .into_iter()
        .map(|row| (row.id, row))
        .collect::<HashMap<i64, axonhub_db_entity::requests::Model>>();

    let model_ids: Vec<String> = request_by_id.values().map(|request| request.model_id.clone()).collect();
    let model_rows = axonhub_db_entity::models::Entity::find()
        .filter(axonhub_db_entity::models::Column::ModelId.is_in(model_ids))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let model_name_by_id = model_rows
        .into_iter()
        .map(|model| (model.model_id, model.name))
        .collect::<HashMap<String, String>>();

    #[derive(Clone)]
    struct FastestModelAggregate {
        model_id: String,
        model_name: String,
        tokens_count: i64,
        latency_ms: i64,
        request_count: i64,
        throughput: f64,
        confidence_level: String,
        confidence_score: i32,
    }

    #[derive(Default)]
    struct ModelAccumulator {
        model_name: String,
        tokens_count: i64,
        latency_ms: i64,
        request_count: i64,
    }

    let mut by_model = HashMap::<String, ModelAccumulator>::new();
    for execution in latest_completed {
        let Some(usage) = usage_by_request.get(&execution.request_id) else { continue; };
        let Some(request) = request_by_id.get(&execution.request_id) else { continue; };
        let model_id = request.model_id.clone();
        let model_name = model_name_by_id
            .get(&model_id)
            .cloned()
            .unwrap_or_else(|| model_id.clone());
        let Some(latency_ms) = execution.metrics_latency_ms else { continue; };

        let completion_latency = if execution.stream {
            match execution.metrics_first_token_latency_ms {
                Some(first_token) if first_token < latency_ms => latency_ms - first_token,
                Some(_) => 0,
                None => latency_ms,
            }
        } else {
            latency_ms
        };

        let tokens = usage.completion_tokens
            + usage.completion_reasoning_tokens
            + usage.completion_audio_tokens;

        let entry = by_model.entry(model_id).or_default();
        entry.model_name = model_name;
        entry.tokens_count += tokens;
        entry.latency_ms += completion_latency;
        entry.request_count += 1;
    }

    let mut aggregates = by_model
        .into_iter()
        .map(|(model_id, acc)| FastestModelAggregate {
            model_id,
            model_name: acc.model_name,
            tokens_count: acc.tokens_count,
            latency_ms: acc.latency_ms,
            request_count: acc.request_count,
            throughput: if acc.latency_ms > 0 {
                acc.tokens_count as f64 * 1000.0 / acc.latency_ms as f64
            } else {
                0.0
            },
            confidence_level: String::new(),
            confidence_score: 0,
        })
        .collect::<Vec<_>>();

    if aggregates.is_empty() {
        return Ok(Vec::new());
    }

    let mut request_counts = aggregates.iter().map(|item| item.request_count).collect::<Vec<_>>();
    request_counts.sort_unstable();
    let mid = request_counts.len() / 2;
    let median = if request_counts.len() % 2 == 0 {
        (request_counts[mid - 1] + request_counts[mid]) as f64 / 2.0
    } else {
        request_counts[mid] as f64
    };

    for item in &mut aggregates {
        let confidence_level = graphql_calculate_confidence_level(item.request_count, median);
        let confidence_score = match confidence_level.as_str() {
            "high" => 3,
            "medium" => 2,
            _ => 1,
        };
        item.confidence_level = confidence_level;
        item.confidence_score = confidence_score;
    }

    let high_medium_count = aggregates
        .iter()
        .filter(|item| item.confidence_level == "high" || item.confidence_level == "medium")
        .count();
    let mut results_to_show = if high_medium_count >= limit {
        aggregates
            .into_iter()
            .filter(|item| item.confidence_level == "high" || item.confidence_level == "medium")
            .collect::<Vec<_>>()
    } else {
        aggregates
    };

    results_to_show.sort_by(|left, right| {
        right
            .confidence_score
            .cmp(&left.confidence_score)
            .then_with(|| right.throughput.total_cmp(&left.throughput))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    results_to_show.truncate(limit);

    Ok(results_to_show
        .into_iter()
        .map(|item| AdminGraphqlFastestModel {
            model_id: item.model_id,
            model_name: item.model_name,
            throughput: item.throughput,
            tokens_count: i64_to_i32(item.tokens_count),
            latency_ms: i64_to_i32(item.latency_ms),
            request_count: i64_to_i32(item.request_count),
            confidence_level: item.confidence_level,
        })
        .collect())
}

async fn query_model_performance_stats_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<Vec<AdminGraphqlModelPerformanceStat>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    const TOP_PERFORMERS_LIMIT: usize = 6;

    let timezone = query_graphql_general_settings_timezone(&connection)
        .await?
        .unwrap_or_else(|| "UTC".to_owned());
    let tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);

    let days_count = 30_i64;
    let now_utc = chrono::Utc::now();
    let now_local = now_utc.with_timezone(&tz);
    let start_date_local = now_local.date_naive() - chrono::Duration::days(days_count - 1);
    let start_of_window_local = resolve_tz_local_datetime(
        tz,
        start_date_local.and_hms_opt(0, 0, 0).unwrap(),
        timezone.as_str(),
    )?;
    let start_utc_string = start_of_window_local
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let executions = request_executions::Entity::find()
        .filter(request_executions::Column::CreatedAt.gte(start_utc_string.as_str()))
        .filter(request_executions::Column::MetricsLatencyMs.is_not_null())
        .filter(request_executions::Column::MetricsLatencyMs.gt(0_i64))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let mut latest_by_request = HashMap::<i64, axonhub_db_entity::request_executions::Model>::new();
    for execution in executions {
        if execution.status != "completed" {
            continue;
        }
        match latest_by_request.get(&execution.request_id) {
            Some(current)
                if current.created_at > execution.created_at
                    || (current.created_at == execution.created_at && current.id >= execution.id) => {}
            _ => {
                latest_by_request.insert(execution.request_id, execution);
            }
        }
    }

    let latest_completed = latest_by_request.into_values().collect::<Vec<_>>();
    if latest_completed.is_empty() {
        return Ok(Vec::new());
    }

    let request_ids = latest_completed.iter().map(|execution| execution.request_id).collect::<Vec<_>>();
    let usage_by_request = usage_logs::Entity::find()
        .filter(usage_logs::Column::RequestId.is_in(request_ids.clone()))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| (row.request_id, row))
        .collect::<HashMap<i64, axonhub_db_entity::usage_logs::Model>>();
    let request_by_id = requests::Entity::find()
        .filter(requests::Column::Id.is_in(request_ids.clone()))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| (row.id, row))
        .collect::<HashMap<i64, axonhub_db_entity::requests::Model>>();

    let model_ids = request_by_id
        .values()
        .map(|request| request.model_id.clone())
        .collect::<Vec<_>>();
    let known_models = axonhub_db_entity::models::Entity::find()
        .filter(axonhub_db_entity::models::Column::ModelId.is_in(model_ids))
        .filter(axonhub_db_entity::models::Column::DeletedAt.eq(0_i64))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|model| model.model_id)
        .collect::<std::collections::HashSet<_>>();

    #[derive(Default)]
    struct DailyPerfAccumulator {
        tokens_count: i64,
        latency_ms: i64,
        first_token_sum: i64,
        first_token_count: i64,
        request_count: i64,
    }

    let mut by_day_model = HashMap::<(String, String), DailyPerfAccumulator>::new();
    let mut total_requests_by_model = HashMap::<String, i64>::new();

    for execution in latest_completed {
        let Some(request) = request_by_id.get(&execution.request_id) else { continue; };
        if !known_models.contains(&request.model_id) {
            continue;
        }
        let Some(usage) = usage_by_request.get(&execution.request_id) else { continue; };
        let Some(latency_ms) = execution.metrics_latency_ms else { continue; };

        let exec_naive = chrono::NaiveDateTime::parse_from_str(&execution.created_at, "%Y-%m-%d %H:%M:%S")
            .map_err(|error| format!("failed to parse request execution timestamp {:?}: {error}", execution.created_at))?;
        let local_date = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(exec_naive, chrono::Utc)
            .with_timezone(&tz)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();

        let effective_latency = if execution.stream {
            match execution.metrics_first_token_latency_ms {
                Some(first_token) if first_token < latency_ms => latency_ms - first_token,
                Some(_) => 0,
                None => latency_ms,
            }
        } else {
            latency_ms
        };

        let tokens = usage.completion_tokens
            + usage.completion_reasoning_tokens
            + usage.completion_audio_tokens;

        let entry = by_day_model.entry((local_date, request.model_id.clone())).or_default();
        entry.tokens_count += tokens;
        entry.latency_ms += effective_latency;
        entry.request_count += 1;
        if let Some(first_token) = execution.metrics_first_token_latency_ms.filter(|value| *value > 0) {
            entry.first_token_sum += first_token;
            entry.first_token_count += 1;
        }

        *total_requests_by_model.entry(request.model_id.clone()).or_insert(0) += 1;
    }

    if by_day_model.is_empty() {
        return Ok(Vec::new());
    }

    let mut model_infos = total_requests_by_model
        .into_iter()
        .map(|(model_id, request_count)| (model_id, request_count))
        .collect::<Vec<_>>();
    let request_counts = model_infos.iter().map(|(_, request_count)| *request_count).collect::<Vec<_>>();
    let mut sorted_counts = request_counts;
    sorted_counts.sort_unstable();
    let mid = sorted_counts.len() / 2;
    let median = if sorted_counts.len() % 2 == 0 {
        (sorted_counts[mid - 1] + sorted_counts[mid]) as f64 / 2.0
    } else {
        sorted_counts[mid] as f64
    };

    #[derive(Clone)]
    struct RankedModel {
        model_id: String,
        request_count: i64,
        confidence_score: i32,
    }

    let mut ranked_models = model_infos
        .drain(..)
        .map(|(model_id, request_count)| {
            let confidence_level = graphql_calculate_confidence_level(request_count, median);
            let confidence_score = match confidence_level.as_str() {
                "high" => 3,
                "medium" => 2,
                _ => 1,
            };
            RankedModel {
                model_id,
                request_count,
                confidence_score,
            }
        })
        .collect::<Vec<_>>();

    let high_medium_count = ranked_models
        .iter()
        .filter(|item| item.confidence_score >= 2)
        .count();
    if high_medium_count >= TOP_PERFORMERS_LIMIT {
        ranked_models.retain(|item| item.confidence_score >= 2);
    }
    ranked_models.sort_by(|left, right| {
        right
            .confidence_score
            .cmp(&left.confidence_score)
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    ranked_models.truncate(TOP_PERFORMERS_LIMIT);

    let top_model_ids = ranked_models
        .into_iter()
        .map(|item| item.model_id)
        .collect::<std::collections::HashSet<_>>();

    let mut results = by_day_model
        .into_iter()
        .filter(|((_, model_id), _)| top_model_ids.contains(model_id))
        .filter_map(|((date, model_id), acc)| {
            let throughput = if acc.latency_ms > 0 {
                Some(acc.tokens_count as f64 * 1000.0 / acc.latency_ms as f64)
            } else {
                None
            };
            if throughput.is_none_or(|value| value <= 0.0) {
                return None;
            }
            let ttft_ms = if acc.first_token_count > 0 {
                Some(acc.first_token_sum as f64 / acc.first_token_count as f64)
            } else {
                None
            };

            Some(AdminGraphqlModelPerformanceStat {
                date,
                model_id,
                throughput,
                ttft_ms,
                request_count: i64_to_i32(acc.request_count),
            })
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| right.throughput.unwrap_or(0.0).total_cmp(&left.throughput.unwrap_or(0.0)))
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    Ok(results)
}

async fn query_channel_performance_stats_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<Vec<AdminGraphqlChannelPerformanceStat>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    const TOP_PERFORMERS_LIMIT: usize = 6;

    let timezone = query_graphql_general_settings_timezone(&connection)
        .await?
        .unwrap_or_else(|| "UTC".to_owned());
    let tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);

    let days_count = 30_i64;
    let now_utc = chrono::Utc::now();
    let now_local = now_utc.with_timezone(&tz);
    let start_date_local = now_local.date_naive() - chrono::Duration::days(days_count - 1);
    let start_of_window_local = resolve_tz_local_datetime(
        tz,
        start_date_local.and_hms_opt(0, 0, 0).unwrap(),
        timezone.as_str(),
    )?;
    let start_utc = start_of_window_local.with_timezone(&chrono::Utc);
    let start_utc_string = start_utc.format("%Y-%m-%d %H:%M:%S").to_string();

    let probe_rows = channel_probes::Entity::find()
        .filter(channel_probes::Column::Timestamp.gte(start_utc.timestamp()))
        .filter(channel_probes::Column::AvgTokensPerSecond.lte(2000.0))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    if !probe_rows.is_empty() {
        return query_channel_performance_stats_from_probes(connection, probe_rows, tz, TOP_PERFORMERS_LIMIT).await;
    }

    query_channel_performance_stats_from_executions(
        connection,
        tz,
        start_utc_string.as_str(),
        TOP_PERFORMERS_LIMIT,
    )
    .await
}

async fn query_channel_performance_stats_from_probes(
    connection: sea_orm::DatabaseConnection,
    probe_rows: Vec<axonhub_db_entity::channel_probes::Model>,
    tz: Tz,
    limit: usize,
) -> Result<Vec<AdminGraphqlChannelPerformanceStat>, String> {
    #[derive(Default)]
    struct ProbeDailyAccumulator {
        request_count: i64,
        throughput_weighted_sum: f64,
        throughput_weight: i64,
        ttft_weighted_sum: f64,
        ttft_weight: i64,
    }

    let mut by_day_channel = HashMap::<(String, i64), ProbeDailyAccumulator>::new();
    let mut total_requests_by_channel = HashMap::<i64, i64>::new();

    for probe in probe_rows {
        let date = chrono::DateTime::<chrono::Utc>::from_timestamp(probe.timestamp, 0)
            .ok_or_else(|| format!("invalid probe timestamp: {}", probe.timestamp))?
            .with_timezone(&tz)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let weight = i64::from(probe.total_request_count);
        let entry = by_day_channel.entry((date, probe.channel_id)).or_default();
        entry.request_count += weight;
        if let Some(throughput) = probe.avg_tokens_per_second {
            entry.throughput_weighted_sum += throughput * weight as f64;
            entry.throughput_weight += weight;
        }
        if let Some(ttft) = probe.avg_time_to_first_token_ms {
            entry.ttft_weighted_sum += ttft * weight as f64;
            entry.ttft_weight += weight;
        }
        *total_requests_by_channel.entry(probe.channel_id).or_insert(0) += weight;
    }

    if by_day_channel.is_empty() {
        return Ok(Vec::new());
    }

    let top_channel_ids = graphql_select_top_channel_ids(total_requests_by_channel, limit);
    let channel_names = query_channel_name_map(&connection, &top_channel_ids.iter().copied().collect::<Vec<_>>()).await?;

    let mut results = by_day_channel
        .into_iter()
        .filter(|((_, channel_id), _)| top_channel_ids.contains(channel_id))
        .map(|((date, channel_id), acc)| {
            let throughput = if acc.throughput_weight > 0 {
                let value = acc.throughput_weighted_sum / acc.throughput_weight as f64;
                (value > 0.0).then_some(value)
            } else {
                None
            };
            let ttft_ms = if acc.ttft_weight > 0 {
                let value = acc.ttft_weighted_sum / acc.ttft_weight as f64;
                (value > 0.0).then_some(value)
            } else {
                None
            };

            AdminGraphqlChannelPerformanceStat {
                date,
                channel_id: channel_id.to_string(),
                channel_name: channel_names
                    .get(&channel_id)
                    .cloned()
                    .unwrap_or_else(|| format!("channel-{channel_id}")),
                throughput,
                ttft_ms,
                request_count: i64_to_i32(acc.request_count),
            }
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| left.date.cmp(&right.date).then_with(|| left.channel_id.cmp(&right.channel_id)));
    Ok(results)
}

async fn query_channel_performance_stats_from_executions(
    connection: sea_orm::DatabaseConnection,
    tz: Tz,
    start_utc_string: &str,
    limit: usize,
) -> Result<Vec<AdminGraphqlChannelPerformanceStat>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let executions = request_executions::Entity::find()
        .filter(request_executions::Column::CreatedAt.gte(start_utc_string))
        .filter(request_executions::Column::MetricsLatencyMs.is_not_null())
        .filter(request_executions::Column::MetricsLatencyMs.gt(0_i64))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let mut latest_by_request = HashMap::<i64, axonhub_db_entity::request_executions::Model>::new();
    for execution in executions {
        if execution.status != "completed" {
            continue;
        }
        match latest_by_request.get(&execution.request_id) {
            Some(current)
                if current.created_at > execution.created_at
                    || (current.created_at == execution.created_at && current.id >= execution.id) => {}
            _ => {
                latest_by_request.insert(execution.request_id, execution);
            }
        }
    }

    let latest_completed = latest_by_request
        .into_values()
        .filter(|execution| execution.channel_id.is_some())
        .collect::<Vec<_>>();
    if latest_completed.is_empty() {
        return Ok(Vec::new());
    }

    let request_ids = latest_completed.iter().map(|execution| execution.request_id).collect::<Vec<_>>();
    let usage_by_request = usage_logs::Entity::find()
        .filter(usage_logs::Column::RequestId.is_in(request_ids))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|row| (row.request_id, row))
        .collect::<HashMap<i64, axonhub_db_entity::usage_logs::Model>>();

    #[derive(Default)]
    struct ChannelPerfAccumulator {
        tokens_count: i64,
        effective_latency_ms: i64,
        first_token_sum: i64,
        first_token_count: i64,
        request_count: i64,
    }

    let mut by_day_channel = HashMap::<(String, i64), ChannelPerfAccumulator>::new();
    let mut total_requests_by_channel = HashMap::<i64, i64>::new();

    for execution in latest_completed {
        let Some(channel_id) = execution.channel_id else { continue; };
        let Some(usage) = usage_by_request.get(&execution.request_id) else { continue; };
        let Some(latency_ms) = execution.metrics_latency_ms else { continue; };

        let exec_naive = chrono::NaiveDateTime::parse_from_str(&execution.created_at, "%Y-%m-%d %H:%M:%S")
            .map_err(|error| format!("failed to parse request execution timestamp {:?}: {error}", execution.created_at))?;
        let local_date = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(exec_naive, chrono::Utc)
            .with_timezone(&tz)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();

        let effective_latency = if execution.stream {
            match execution.metrics_first_token_latency_ms {
                Some(first_token) if first_token < latency_ms => latency_ms - first_token,
                Some(_) => 0,
                None => latency_ms,
            }
        } else {
            latency_ms
        };

        let tokens = usage.completion_tokens
            + usage.completion_reasoning_tokens
            + usage.completion_audio_tokens;

        let entry = by_day_channel.entry((local_date, channel_id)).or_default();
        entry.tokens_count += tokens;
        entry.effective_latency_ms += effective_latency;
        entry.request_count += 1;
        if let Some(first_token) = execution.metrics_first_token_latency_ms.filter(|value| *value > 0) {
            entry.first_token_sum += first_token;
            entry.first_token_count += 1;
        }

        *total_requests_by_channel.entry(channel_id).or_insert(0) += 1;
    }

    if by_day_channel.is_empty() {
        return Ok(Vec::new());
    }

    let top_channel_ids = graphql_select_top_channel_ids(total_requests_by_channel, limit);
    let channel_names = query_channel_name_map(&connection, &top_channel_ids.iter().copied().collect::<Vec<_>>()).await?;

    let mut results = by_day_channel
        .into_iter()
        .filter(|((_, channel_id), _)| top_channel_ids.contains(channel_id))
        .map(|((date, channel_id), acc)| {
            let throughput = if acc.effective_latency_ms > 0 {
                let value = acc.tokens_count as f64 * 1000.0 / acc.effective_latency_ms as f64;
                (value > 0.0).then_some(value)
            } else {
                None
            };
            let ttft_ms = if acc.first_token_count > 0 {
                let value = acc.first_token_sum as f64 / acc.first_token_count as f64;
                (value > 0.0).then_some(value)
            } else {
                None
            };

            AdminGraphqlChannelPerformanceStat {
                date,
                channel_id: channel_id.to_string(),
                channel_name: channel_names
                    .get(&channel_id)
                    .cloned()
                    .unwrap_or_else(|| format!("channel-{channel_id}")),
                throughput,
                ttft_ms,
                request_count: i64_to_i32(acc.request_count),
            }
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| left.date.cmp(&right.date).then_with(|| left.channel_id.cmp(&right.channel_id)));
    Ok(results)
}

fn graphql_select_top_channel_ids(
    total_requests_by_channel: HashMap<i64, i64>,
    limit: usize,
) -> std::collections::HashSet<i64> {
    #[derive(Clone)]
    struct RankedChannel {
        channel_id: i64,
        request_count: i64,
        confidence_score: i32,
    }

    let request_counts = total_requests_by_channel.values().copied().collect::<Vec<_>>();
    let mut sorted_counts = request_counts;
    sorted_counts.sort_unstable();
    let mid = sorted_counts.len() / 2;
    let median = if sorted_counts.len() % 2 == 0 {
        (sorted_counts[mid - 1] + sorted_counts[mid]) as f64 / 2.0
    } else {
        sorted_counts[mid] as f64
    };

    let mut ranked_channels = total_requests_by_channel
        .into_iter()
        .map(|(channel_id, request_count)| {
            let confidence_level = graphql_calculate_confidence_level(request_count, median);
            let confidence_score = match confidence_level.as_str() {
                "high" => 3,
                "medium" => 2,
                _ => 1,
            };
            RankedChannel {
                channel_id,
                request_count,
                confidence_score,
            }
        })
        .collect::<Vec<_>>();

    let high_medium_count = ranked_channels
        .iter()
        .filter(|item| item.confidence_score >= 2)
        .count();
    if high_medium_count >= limit {
        ranked_channels.retain(|item| item.confidence_score >= 2);
    }
    ranked_channels.sort_by(|left, right| {
        right
            .confidence_score
            .cmp(&left.confidence_score)
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.channel_id.cmp(&right.channel_id))
    });
    ranked_channels.truncate(limit);

    ranked_channels
        .into_iter()
        .map(|item| item.channel_id)
        .collect::<std::collections::HashSet<_>>()
}

async fn query_channel_name_map(
    connection: &sea_orm::DatabaseConnection,
    channel_ids: &[i64],
) -> Result<HashMap<i64, String>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    if channel_ids.is_empty() {
        return Ok(HashMap::new());
    }

    Ok(channels::Entity::find()
        .filter(channels::Column::Id.is_in(channel_ids.to_vec()))
        .all(connection)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|channel| (channel.id, channel.name))
        .collect())
}

async fn query_daily_request_stats_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<Vec<AdminGraphqlDailyRequestStats>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let timezone = query_graphql_general_settings_timezone(&connection)
        .await?
        .unwrap_or_else(|| "UTC".to_owned());
    let tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);

    let days_count = 30_i64;
    let now_utc = chrono::Utc::now();
    let now_local = now_utc.with_timezone(&tz);
    let start_date_local = now_local.date_naive() - chrono::Duration::days(days_count - 1);
    let start_of_window_local = resolve_tz_local_datetime(
        tz,
        start_date_local.and_hms_opt(0, 0, 0).unwrap(),
        timezone.as_str(),
    )?;

    let start_utc = start_of_window_local.with_timezone(&chrono::Utc);
    let start_utc_string = start_utc.format("%Y-%m-%d %H:%M:%S").to_string();
    let now_utc_string = now_utc.format("%Y-%m-%d %H:%M:%S").to_string();
    let rows = usage_logs::Entity::find()
        .filter(usage_logs::Column::CreatedAt.gte(start_utc_string.as_str()))
        .filter(usage_logs::Column::CreatedAt.lt(now_utc_string.as_str()))
        .all(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let mut daily_map = HashMap::<String, (i64, i64, f64)>::new();
    for row in rows {
        let naive = chrono::NaiveDateTime::parse_from_str(row.created_at.as_str(), "%Y-%m-%d %H:%M:%S")
            .map_err(|error| format!("failed to parse usage log timestamp {:?}: {error}", row.created_at))?;
        let local_date = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
            .with_timezone(&tz)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let entry = daily_map.entry(local_date).or_insert((0, 0, 0.0));
        entry.0 += 1;
        entry.1 += row.total_tokens;
        entry.2 += row.total_cost.unwrap_or(0.0);
    }

    Ok((0..days_count)
        .map(|offset| {
            let date = start_date_local + chrono::Duration::days(offset);
            let date_string = date.format("%Y-%m-%d").to_string();
            let (count, tokens, cost) = daily_map
                .get(date_string.as_str())
                .copied()
                .unwrap_or((0, 0, 0.0));

            AdminGraphqlDailyRequestStats {
                date: date_string,
                count: i64_to_i32(count),
                tokens: i64_to_i32(tokens),
                cost,
            }
        })
        .collect())
}

async fn query_graphql_general_settings_timezone(
    connection: &sea_orm::DatabaseConnection,
) -> Result<Option<String>, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let value = axonhub_db_entity::systems::Entity::find()
        .filter(axonhub_db_entity::systems::Column::Key.eq("system_general_settings"))
        .filter(axonhub_db_entity::systems::Column::DeletedAt.eq(0_i64))
        .into_partial_model::<axonhub_db_entity::systems::KeyValue>()
        .one(connection)
        .await
        .map_err(|error| error.to_string())?
        .map(|row| row.value);

    Ok(value.and_then(|raw| {
        serde_json::from_str::<AdminGraphqlGeneralSettings>(&raw)
            .ok()
            .map(|settings| settings.timezone.trim().to_owned())
            .filter(|timezone| !timezone.is_empty())
    }))
}

fn resolve_tz_local_datetime(
    tz: Tz,
    naive: chrono::NaiveDateTime,
    timezone_name: &str,
) -> Result<chrono::DateTime<Tz>, String> {
    tz.from_local_datetime(&naive)
        .single()
        .or_else(|| tz.from_local_datetime(&naive).earliest())
        .or_else(|| tz.from_local_datetime(&naive).latest())
        .ok_or_else(|| format!("failed to resolve local datetime for timezone {timezone_name:?}"))
}

async fn query_token_stats_aggregate(
    connection: &sea_orm::DatabaseConnection,
    since: Option<&str>,
) -> Result<AdminGraphqlTokenStatsAggregateRow, String> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

    let query = usage_logs::Entity::find().select_only().column_as(
        sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_tokens), 0)"),
        "input_tokens",
    )
    .column_as(
        sea_orm::sea_query::Expr::cust("COALESCE(SUM(completion_tokens), 0)"),
        "output_tokens",
    )
    .column_as(
        sea_orm::sea_query::Expr::cust("COALESCE(SUM(prompt_cached_tokens), 0)"),
        "cached_tokens",
    )
    .column_as(
        sea_orm::sea_query::Expr::cust("MAX(created_at)"),
        "last_updated",
    );

    let query = if let Some(since) = since {
        query.filter(usage_logs::Column::CreatedAt.gte(since))
    } else {
        query
    };

    query
        .into_model::<AdminGraphqlTokenStatsAggregateRow>()
        .one(connection)
        .await
        .map_err(|error| error.to_string())
        .map(|row| row.unwrap_or(AdminGraphqlTokenStatsAggregateRow {
            input_tokens: Some(0),
            output_tokens: Some(0),
            cached_tokens: Some(0),
            last_updated: None,
        }))
}

fn normalize_usage_log_last_updated(value: &str) -> String {
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .map(|naive| {
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
                .to_rfc3339()
        })
        .unwrap_or_else(|_| value.to_owned())
}

fn parse_admin_graphql_time_window(time_window: Option<&str>) -> Result<Option<String>, String> {
    let time_window = time_window.map(str::trim).filter(|value| !value.is_empty());
    let Some(time_window) = time_window else {
        return Ok(None);
    };

    if time_window == "allTime" {
        return Ok(None);
    }

    let now = chrono::Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let since = match time_window {
        "day" => today_start,
        "week" => today_start - chrono::Duration::days(now.weekday().num_days_from_monday() as i64),
        "month" => now
            .date_naive()
            .with_day(1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        other => {
            return Err(format!(
                "unsupported timeWindow value: {other:?} (expected day, week, month, allTime)"
            ));
        }
    };

    Ok(Some(since.format("%Y-%m-%d %H:%M:%S").to_string()))
}

async fn query_request_stats_connection(
    connection: sea_orm::DatabaseConnection,
) -> Result<AdminGraphqlRequestStats, String> {
    use chrono::Datelike;
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

    let now = chrono::Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let this_week_start = today_start
        - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
    let last_week_start = this_week_start - chrono::Duration::days(7);
    let this_month_start = now
        .date_naive()
        .with_day(1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();

    let today_start = today_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let this_week_start = this_week_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let last_week_start = last_week_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let this_month_start = this_month_start.format("%Y-%m-%d %H:%M:%S").to_string();

    let requests_today = usage_logs::Entity::find()
        .filter(usage_logs::Column::CreatedAt.gte(today_start.as_str()))
        .count(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let requests_this_week = usage_logs::Entity::find()
        .filter(usage_logs::Column::CreatedAt.gte(this_week_start.as_str()))
        .count(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let requests_last_week = usage_logs::Entity::find()
        .filter(usage_logs::Column::CreatedAt.gte(last_week_start.as_str()))
        .filter(usage_logs::Column::CreatedAt.lt(this_week_start.as_str()))
        .count(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let requests_this_month = usage_logs::Entity::find()
        .filter(usage_logs::Column::CreatedAt.gte(this_month_start.as_str()))
        .count(&connection)
        .await
        .map_err(|error| error.to_string())?;

    Ok(AdminGraphqlRequestStats {
        requests_today: requests_today as i32,
        requests_this_week: requests_this_week as i32,
        requests_last_week: requests_last_week as i32,
        requests_this_month: requests_this_month as i32,
    })
}

async fn query_check_for_update_seaorm(
    update_checker: AdminGraphqlUpdateChecker,
) -> Result<GraphqlExecutionResult, String> {
    let current_version = BuildInfo::current().version().to_owned();
    let version_check = tokio::task::spawn_blocking(move || {
        update_checker.check_for_update(current_version.as_str())
    })
    .await
    .map_err(|error| format!("failed to join update check task: {error}"))??;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "checkForUpdate": version_check,
            }
        }),
    })
}

impl AdminGraphqlUpdateChecker {
    fn github() -> Self {
        Self {
            releases_api_url: ADMIN_GRAPHQL_RELEASES_API_URL.to_owned(),
            release_url_prefix: ADMIN_GRAPHQL_RELEASE_URL_PREFIX.to_owned(),
        }
    }

    #[cfg(test)]
    fn new(
        releases_api_url: impl Into<String>,
        release_url_prefix: impl Into<String>,
    ) -> Self {
        Self {
            releases_api_url: releases_api_url.into(),
            release_url_prefix: release_url_prefix.into(),
        }
    }

    fn check_for_update(&self, current_version: &str) -> Result<AdminGraphqlVersionCheck, String> {
        let latest_version = self.fetch_latest_release()?;
        Ok(AdminGraphqlVersionCheck {
            current_version: current_version.to_owned(),
            release_url: format!("{}{}", self.release_url_prefix, latest_version),
            has_update: is_newer_version(current_version, latest_version.as_str()),
            latest_version,
        })
    }

    fn fetch_latest_release(&self) -> Result<String, String> {
        let mut api_url = reqwest::Url::parse(self.releases_api_url.as_str())
            .map_err(|error| format!("failed to parse URL: {error}"))?;
        {
            let mut query = api_url.query_pairs_mut();
            query.append_pair("per_page", "10");
            query.append_pair("page", "1");
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| format!("failed to build HTTP client: {error}"))?;
        let response = client
            .get(api_url)
            .header(ACCEPT, "application/vnd.github.v3+json")
            .header(USER_AGENT, ADMIN_GRAPHQL_VERSION_CHECK_USER_AGENT)
            .send()
            .map_err(|error| format!("failed to fetch releases: {error}"))?;

        if response.status() != reqwest::StatusCode::OK {
            return Err(format!("GitHub API returned status {}", response.status().as_u16()));
        }

        let releases = response
            .json::<Vec<AdminGraphqlGitHubRelease>>()
            .map_err(|error| format!("failed to decode releases: {error}"))?;
        let now = SystemTime::now();

        for release in releases {
            if release.is_stable(now)? {
                return Ok(release.tag_name);
            }
        }

        Err("no stable release found".to_owned())
    }
}

impl AdminGraphqlGitHubRelease {
    fn is_stable(&self, now: SystemTime) -> Result<bool, String> {
        if self.draft || self.prerelease {
            return Ok(false);
        }

        if !is_axonhub_tag(self.tag_name.as_str()) || is_pre_release_tag(self.tag_name.as_str()) {
            return Ok(false);
        }

        let published_at = self.published_at()?;
        let elapsed = now.duration_since(published_at).unwrap_or_default();
        Ok(elapsed >= ADMIN_GRAPHQL_RELEASE_COOLDOWN)
    }

    fn published_at(&self) -> Result<SystemTime, String> {
        match self.published_at.as_deref().map(str::trim) {
            Some("") | None => Ok(UNIX_EPOCH),
            Some(value) => humantime::parse_rfc3339_weak(value).map_err(|error| {
                format!(
                    "failed to parse release published_at `{value}` for tag `{}`: {error}",
                    self.tag_name
                )
            }),
        }
    }
}

pub(crate) fn is_axonhub_tag(tag: &str) -> bool {
    tag.starts_with('v')
}

pub(crate) fn is_pre_release_tag(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    ["-beta", "-rc", "-alpha", "-dev", "-preview", "-snapshot"]
        .into_iter()
        .any(|pattern| lower.contains(pattern))
}

pub(crate) fn is_newer_version(current: &str, latest: &str) -> bool {
    let Ok(current) = parse_semantic_version(current) else {
        return false;
    };
    let Ok(latest) = parse_semantic_version(latest) else {
        return false;
    };

    latest > current
}

fn parse_semantic_version(raw: &str) -> Result<SemanticVersion, String> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let without_build = trimmed.split_once('+').map_or(trimmed, |(value, _)| value);
    let (core, prerelease_raw) = without_build
        .split_once('-')
        .map_or((without_build, None), |(value, prerelease)| {
            (value, Some(prerelease))
        });
    let mut core_parts = core.split('.');
    let major = parse_semantic_numeric_identifier(core_parts.next(), raw, "major")?;
    let minor = parse_semantic_numeric_identifier(core_parts.next(), raw, "minor")?;
    let patch = parse_semantic_numeric_identifier(core_parts.next(), raw, "patch")?;
    if core_parts.next().is_some() {
        return Err(format!("invalid semantic version `{raw}`"));
    }

    let prerelease = prerelease_raw
        .map(|value| {
            value
                .split('.')
                .map(|identifier| {
                    if identifier.is_empty() {
                        return Err(format!("invalid semantic version `{raw}`"));
                    }
                    Ok(identifier
                        .parse::<u64>()
                        .map(SemanticVersionIdentifier::Numeric)
                        .unwrap_or_else(|_| {
                            SemanticVersionIdentifier::AlphaNumeric(identifier.to_owned())
                        }))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(SemanticVersion {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn parse_semantic_numeric_identifier(
    value: Option<&str>,
    raw: &str,
    part_name: &str,
) -> Result<u64, String> {
    value
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| format!("invalid semantic version `{raw}`"))?
        .parse::<u64>()
        .map_err(|_| format!("invalid {part_name} version segment in `{raw}`"))
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| compare_semantic_prerelease(self.prerelease.as_slice(), other.prerelease.as_slice()))
    }
}

fn compare_semantic_prerelease(
    left: &[SemanticVersionIdentifier],
    right: &[SemanticVersionIdentifier],
) -> std::cmp::Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => {
            for (left_identifier, right_identifier) in left.iter().zip(right.iter()) {
                let ordering = match (left_identifier, right_identifier) {
                    (SemanticVersionIdentifier::Numeric(left), SemanticVersionIdentifier::Numeric(right)) => left.cmp(right),
                    (SemanticVersionIdentifier::Numeric(_), SemanticVersionIdentifier::AlphaNumeric(_)) => {
                        std::cmp::Ordering::Less
                    }
                    (SemanticVersionIdentifier::AlphaNumeric(_), SemanticVersionIdentifier::Numeric(_)) => {
                        std::cmp::Ordering::Greater
                    }
                    (
                        SemanticVersionIdentifier::AlphaNumeric(left),
                        SemanticVersionIdentifier::AlphaNumeric(right),
                    ) => left.cmp(right),
                };

                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }

            left.len().cmp(&right.len())
        }
    }
}

fn query_proxy_presets_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let presets = SeaOrmOperationalService::new(repository.db()).proxy_presets()?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "proxyPresets": presets.into_iter().map(|preset| proxy_preset_json(&preset)).collect::<Vec<_>>(),
            }
        }),
    })
}

fn query_user_agent_pass_through_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let enabled = SeaOrmOperationalService::new(repository.db()).user_agent_pass_through()?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "userAgentPassThroughSettings": {
                    "enabled": enabled,
                },
            }
        }),
    })
}

fn query_channels_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    circuit_breaker: &SharedCircuitBreaker,
) -> Result<GraphqlExecutionResult, String> {
    let channels = repository.query_channels()?;

    let quota_by_channel = SeaOrmOperationalService::new(repository.db())
        .provider_quota_statuses()
        .unwrap_or_default()
        .into_iter()
        .map(|status| (status.channel_id, AdminGraphqlProviderQuotaStatus::from(status)))
        .collect::<std::collections::HashMap<_, _>>();

    let items = channels
        .into_iter()
        .map(|channel| {
            let mut gql = AdminGraphqlChannel {
                id: graphql_gid("channel", channel.id),
                name: channel.name,
                channel_type: channel.channel_type,
                base_url: channel.base_url,
                status: channel.status,
                supported_models: serde_json::from_str(&channel.supported_models).unwrap_or_default(),
                ordering_weight: channel.ordering_weight,
                provider_quota_status: quota_by_channel.get(&channel.id).cloned(),
                circuit_breaker_status: stored_circuit_breaker_status(channel.id, circuit_breaker)
                    .map(AdminGraphqlCircuitBreakerStatus::from),
            };
            if gql.provider_quota_status.is_none() {
                gql.provider_quota_status = None;
            }
            channel_json(&gql)
        })
        .collect::<Vec<_>>();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"channels": items}}),
    })
}

fn update_storage_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateStoragePolicyInput>(
        variables,
        "input",
        "storage policy input is required",
    )?;
    let mut policy = load_storage_policy_seaorm(repository)?;
    if let Some(store_chunks) = input.store_chunks {
        policy.store_chunks = store_chunks;
    }
    if let Some(store_request_body) = input.store_request_body {
        policy.store_request_body = store_request_body;
    }
    if let Some(store_response_body) = input.store_response_body {
        policy.store_response_body = store_response_body;
    }
    if let Some(cleanup_options) = input.cleanup_options {
        policy.cleanup_options = cleanup_options
            .into_iter()
            .map(|option| super::admin::StoredCleanupOption {
                resource_type: option.resource_type,
                enabled: option.enabled,
                cleanup_days: option.cleanup_days,
            })
            .collect();
    }
    SeaOrmOperationalService::new(repository.db()).update_storage_policy(policy)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateStoragePolicy": true}}),
    })
}

fn update_retry_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateRetryPolicyInput>(
        variables,
        "input",
        "retry policy input is required",
    )?;
    let mut policy = load_retry_policy_seaorm(repository)?;
    if let Some(enabled) = input.enabled {
        policy.enabled = enabled;
    }
    if let Some(max_channel_retries) = input.max_channel_retries {
        policy.max_channel_retries = max_channel_retries;
    }
    if let Some(max_single_channel_retries) = input.max_single_channel_retries {
        policy.max_single_channel_retries = max_single_channel_retries;
    }
    if let Some(retry_delay_ms) = input.retry_delay_ms {
        policy.retry_delay_ms = retry_delay_ms;
    }
    if let Some(load_balancer_strategy) = input.load_balancer_strategy {
        policy.load_balancer_strategy = normalize_retry_policy_load_balancer_strategy(&load_balancer_strategy);
    }
    if let Some(auto_disable_channel) = input.auto_disable_channel {
        if let Some(enabled) = auto_disable_channel.enabled {
            policy.auto_disable_channel.enabled = enabled;
        }
        if let Some(statuses) = auto_disable_channel.statuses {
            policy.auto_disable_channel.statuses = statuses
                .into_iter()
                .map(|status| super::admin::StoredAutoDisableChannelStatus {
                    status: status.status,
                    times: status.times,
                })
                .collect();
        }
    }

    SeaOrmOperationalService::new(repository.db()).update_retry_policy(policy)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateRetryPolicy": true}}),
    })
}

fn update_auto_backup_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateAutoBackupSettingsInput>(
        variables,
        "input",
        "auto backup input is required",
    )?;
    let mut settings = load_auto_backup_settings_seaorm(repository)?;
    if let Some(enabled) = input.enabled {
        settings.enabled = enabled;
    }
    if let Some(frequency) = input.frequency {
        settings.frequency = frequency;
    }
    if let Some(data_storage_id) = input.data_storage_id {
        settings.data_storage_id = i64::from(data_storage_id);
    }
    if let Some(include_channels) = input.include_channels {
        settings.include_channels = include_channels;
    }
    if let Some(include_models) = input.include_models {
        settings.include_models = include_models;
    }
    if let Some(include_api_keys) = input.include_api_keys {
        settings.include_api_keys = include_api_keys;
    }
    if let Some(include_model_prices) = input.include_model_prices {
        settings.include_model_prices = include_model_prices;
    }
    if let Some(retention_days) = input.retention_days {
        settings.retention_days = retention_days.max(0);
    }
    if settings.enabled && settings.data_storage_id <= 0 {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateAutoBackupSettings": Value::Null},
                "errors": [{"message": "dataStorageID is required when auto backup is enabled"}],
            }),
        });
    }
    SeaOrmOperationalService::new(repository.db()).update_auto_backup_settings(settings)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateAutoBackupSettings": true}}),
    })
}

fn update_default_data_storage_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateDefaultDataStorageInput>(
        variables,
        "input",
        "default data storage input is required",
    )?;
    let data_storage_id = match parse_graphql_resource_id(input.data_storage_id.as_str(), "DataStorage") {
        Ok(id) => id,
        Err(_) => {
            return Ok(GraphqlExecutionResult {
                status: 200,
                body: json!({
                    "data": {"updateDefaultDataStorage": Value::Null},
                    "errors": [{"message": "invalid dataStorageID"}],
                }),
            })
        }
    };
    let current = repository.query_data_storage_status(data_storage_id)?;
    let Some(current) = current else {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateDefaultDataStorage": Value::Null},
                "errors": [{"message": "data storage not found"}],
            }),
        });
    };
    if current.id <= 0 {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateDefaultDataStorage": Value::Null},
                "errors": [{"message": "data storage not found"}],
            }),
        });
    }
    if !current.status.eq_ignore_ascii_case("active") {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateDefaultDataStorage": Value::Null},
                "errors": [{"message": "data storage is not active"}],
            }),
        });
    }

    repository.upsert_default_data_storage(data_storage_id.to_string().as_str())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateDefaultDataStorage": true}}),
    })
}

fn update_system_channel_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let mut settings = load_system_channel_settings_seaorm(repository)?;
    if let Some(probe) = variables.get("input").and_then(|input| input.get("probe")) {
        let enabled = probe
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(|| "invalid probe.enabled: expected boolean".to_owned())?;
        let frequency = parse_probe_frequency_graphql_value(
            probe
                .get("frequency")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid probe.frequency: expected string".to_owned())?,
        )?;
        settings.probe = super::admin::StoredChannelProbeSettings { enabled, frequency };
    }
    if let Some(auto_sync) = variables.get("input").and_then(|input| input.get("autoSync")) {
        let frequency = parse_auto_sync_frequency_graphql_value(
            auto_sync
                .get("frequency")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid autoSync.frequency: expected string".to_owned())?,
        )?;
        settings.auto_sync = super::admin::StoredChannelModelAutoSyncSettings { frequency };
    }
    SeaOrmOperationalService::new(repository.db()).update_system_channel_settings(settings)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateSystemChannelSettings": true}}),
    })
}

fn update_system_general_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateSystemGeneralSettingsInput>(
        variables,
        "input",
        "input is required",
    )?;
    let mut settings = load_system_general_settings_seaorm(repository)?;
    let defaults = default_system_general_settings();

    if let Some(currency_code) = input.currency_code {
        let trimmed = currency_code.trim();
        settings.currency_code = if trimmed.is_empty() {
            defaults.currency_code.clone()
        } else {
            trimmed.to_owned()
        };
    }
    if let Some(timezone) = input.timezone {
        let trimmed = timezone.trim();
        settings.timezone = if trimmed.is_empty() {
            defaults.timezone.clone()
        } else {
            trimmed.to_owned()
        };
    }

    repository.upsert_system_general_settings(
        serde_json::to_string(&serde_json::json!({
            "currencyCode": settings.currency_code,
            "timezone": settings.timezone,
        }))
        .map_err(|error| error.to_string())?
        .as_str(),
    )?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateSystemGeneralSettings": true}}),
    })
}

fn update_system_model_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateSystemModelSettingsInput>(
        variables,
        "input",
        "input is required",
    )?;
    let mut settings = load_system_model_settings_seaorm(repository)?;
    if let Some(fallback) = input.fallback_to_channels_on_model_not_found {
        settings.fallback_to_channels_on_model_not_found = fallback;
    }
    if let Some(query_all_channel_models) = input.query_all_channel_models {
        settings.query_all_channel_models = query_all_channel_models;
    }
    repository.upsert_system_model_settings(
        serde_json::to_string(&settings)
            .map_err(|error| format!("failed to serialize system model settings: {error}"))?
            .as_str(),
    )?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateSystemModelSettings": true}}),
    })
}

fn update_video_storage_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateVideoStorageSettingsInput>(
        variables,
        "input",
        "input is required",
    )?;
    SeaOrmOperationalService::new(repository.db()).update_video_storage_settings(input)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateVideoStorageSettings": true}}),
    })
}

fn save_proxy_preset_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = variables
        .get("input")
        .ok_or_else(|| "input is required".to_owned())?;
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "invalid url: expected non-empty string".to_owned())?;
    let preset = StoredProxyPreset {
        name: input.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_owned(),
        url: url.to_owned(),
        username: input.get("username").and_then(Value::as_str).unwrap_or_default().trim().to_owned(),
        password: input.get("password").and_then(Value::as_str).unwrap_or_default().to_owned(),
    };
    SeaOrmOperationalService::new(repository.db()).save_proxy_preset(preset)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"saveProxyPreset": true}}),
    })
}

fn delete_proxy_preset_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let url = variables
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "invalid url: expected non-empty string".to_owned())?;
    SeaOrmOperationalService::new(repository.db()).delete_proxy_preset(url)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"deleteProxyPreset": true}}),
    })
}

fn update_user_agent_pass_through_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let enabled = variables
        .get("input")
        .and_then(|input| input.get("enabled"))
        .and_then(Value::as_bool)
        .ok_or_else(|| "invalid enabled: expected boolean".to_owned())?;
    SeaOrmOperationalService::new(repository.db()).set_user_agent_pass_through(enabled)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateUserAgentPassThroughSettings": true}}),
    })
}

fn trigger_auto_backup_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let message = SeaOrmOperationalService::new(repository.db()).trigger_backup_now(Some(user_id))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "triggerAutoBackup": {
                    "success": true,
                    "message": message,
                }
            }
        }),
    })
}

fn query_projects_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let edges = repository
        .query_projects()?
        .into_iter()
        .map(|project| {
            json!({
                "cursor": Value::Null,
                "node": project_json(&project),
            })
        })
        .collect::<Vec<_>>();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "projects": {
                    "edges": edges,
                    "pageInfo": {
                        "hasNextPage": false,
                        "hasPreviousPage": false,
                        "startCursor": Value::Null,
                        "endCursor": Value::Null,
                    }
                }
            }
        }),
    })
}

fn query_my_projects_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let active_projects_by_id = repository
        .query_projects()?
        .into_iter()
        .filter(|project| project.status.eq_ignore_ascii_case("active"))
        .map(|project| (project.id, project))
        .collect::<HashMap<_, _>>();

    let projects = repository
        .query_user_projects(user.id)?
        .into_iter()
        .filter_map(|membership| active_projects_by_id.get(&membership.project_id))
        .map(project_json)
        .collect::<Vec<_>>();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "myProjects": projects,
            }
        }),
    })
}

fn query_roles_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let edges = repository
        .query_roles()?
        .into_iter()
        .map(|role| {
            json!({
                "cursor": Value::Null,
                "node": role_json(&role),
            })
        })
        .collect::<Vec<_>>();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "roles": {
                    "edges": edges,
                    "pageInfo": {
                        "hasNextPage": false,
                        "hasPreviousPage": false,
                        "startCursor": Value::Null,
                        "endCursor": Value::Null,
                    }
                }
            }
        }),
    })
}

fn query_api_keys_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let edges = repository
        .query_api_keys()?
        .into_iter()
        .map(|api_key| {
            json!({
                "cursor": Value::Null,
                "node": api_key_json(&api_key),
            })
        })
        .collect::<Vec<_>>();

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "apiKeys": {
                    "edges": edges,
                    "pageInfo": {
                        "hasNextPage": false,
                        "hasPreviousPage": false,
                        "startCursor": Value::Null,
                        "endCursor": Value::Null,
                    }
                }
            }
        }),
    })
}

fn request_summary_json(request: &StoredRequestSummary) -> Value {
    json!({
        "id": graphql_gid("request", request.id),
        "projectID": graphql_gid("project", request.project_id),
        "traceID": request.trace_id.map(|id| graphql_gid("trace", id)),
        "channelID": request.channel_id.map(|id| graphql_gid("channel", id)),
        "modelID": request.model_id,
        "format": request.format,
        "status": request.status,
        "source": request.source,
        "externalID": request.external_id,
    })
}

fn check_provider_quotas_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    SeaOrmOperationalService::new(repository.db())
        .run_provider_quota_check_tick(true, std::time::Duration::from_secs(20 * 60), Some(user_id))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"checkProviderQuotas": true}}),
    })
}

fn reset_provider_quota_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let channel_id = variables
        .get("channelID")
        .and_then(Value::as_str)
        .ok_or_else(|| "channel id is required".to_owned())
        .and_then(|value| parse_graphql_resource_id(value, "channel"))?;
    let updated = SeaOrmOperationalService::new(repository.db())
        .reset_provider_quota_status(channel_id, Some(user_id))?;
    if !updated {
        return graphql_field_error("resetProviderQuota", "provider quota channel not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"resetProviderQuota": true}}),
    })
}

fn trigger_gc_cleanup_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    SeaOrmOperationalService::new(repository.db()).run_gc_cleanup_now(false, Some(user_id))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"triggerGcCleanup": true}}),
    })
}

fn backup_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlBackupInput>(
        variables,
        "input",
        "backup input is required",
    )?;

    let mut settings = default_auto_backup_settings();
    if let Some(include_channels) = input.include_channels {
        settings.include_channels = include_channels;
    }
    if let Some(include_models) = input.include_models {
        settings.include_models = include_models;
    }
    if let Some(include_api_keys) = input.include_api_keys {
        settings.include_api_keys = include_api_keys;
    }
    if let Some(include_model_prices) = input.include_model_prices {
        settings.include_model_prices = include_model_prices;
    }

    let payload = SeaOrmOperationalService::new(repository.db()).build_backup_payload(&settings)?;
    let data = serde_json::to_string(&payload)
        .map_err(|error| format!("failed to serialize backup payload: {error}"))?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "backup": {
                    "success": true,
                    "data": data,
                    "message": "Backup completed successfully",
                }
            }
        }),
    })
}

fn restore_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlRestoreInput>(
        variables,
        "input",
        "restore input is required",
    )?;
    let message = SeaOrmOperationalService::new(repository.db()).restore_backup(
        input.payload.as_bytes(),
        super::admin_operational::RestoreOptions {
            include_channels: input.include_channels.unwrap_or(true),
            include_models: input.include_models.unwrap_or(true),
            include_api_keys: input.include_api_keys.unwrap_or(true),
            include_model_prices: input.include_model_prices.unwrap_or(true),
            overwrite_existing: input.overwrite_existing.unwrap_or(false),
        },
        Some(user_id),
    )?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "restore": {
                    "success": true,
                    "message": message,
                }
            }
        }),
    })
}

fn create_project_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_PROJECTS).is_err() {
        return graphql_permission_denied("createProject");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlCreateProjectInput>(
        variables,
        "input",
        "project input is required",
    )?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createProject", "project name is required");
    }
    let description = input.description.unwrap_or_default();
    let status = normalize_project_status(input.status.as_deref())?.to_owned();

    let record = repository.create_project(name.as_str(), description.as_str(), status.as_str())?;

    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"createProject": project_json(&record)}}),
    })
}

fn update_project_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_PROJECTS).is_err() {
        return graphql_permission_denied("updateProject");
    }
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "project id is required".to_owned())?;
    let project_id = parse_graphql_resource_id(id, "project")
        .map_err(|_| "invalid project id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateProjectInput>(
        variables,
        "input",
        "project input is required",
    )?;
    if input.name.is_none() && input.description.is_none() && input.status.is_none() {
        return graphql_field_error("updateProject", "no fields to update");
    }

    let next_name = input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty());
    let next_status = input
        .status
        .as_deref()
        .map(|status| normalize_project_status(Some(status)).map(str::to_owned))
        .transpose()?;
    let record = repository.update_project(
        project_id,
        next_name,
        input.description.as_deref(),
        next_status.as_deref(),
    );

    match record {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateProject": project_json(&record)}}),
        }),
        Err(message) if message == "project not found" => graphql_field_error("updateProject", message),
        Err(message) if message == "project already exists" => graphql_field_error("updateProject", message),
        Err(message) => Err(message),
    }
}

fn create_role_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreateRoleInput>(
        variables,
        "input",
        "role input is required",
    )?;
    let level = normalize_role_level(input.level.as_deref())?;
    let project_id = parse_role_project_id(input.project_id.as_deref(), level)?;
    authorize_role_write(user, level, project_id, "createRole")?;
    validate_scope_list(input.scopes.as_deref().unwrap_or(&[]), "createRole")?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createRole", "role name is required");
    }
    let level = level.to_owned();
    let scopes_json = serde_json::to_string(&input.scopes.unwrap_or_default())
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;

    let record = repository.create_role(name.as_str(), level.as_str(), project_id, scopes_json.as_str());

    match record {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createRole": role_json(&record)}}),
        }),
        Err(message) if message == "role already exists" => graphql_field_error("createRole", message),
        Err(message) => Err(message),
    }
}

fn update_role_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "role id is required".to_owned())?;
    let role_id = parse_graphql_resource_id(id, "role").map_err(|_| "invalid role id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateRoleInput>(
        variables,
        "input",
        "role input is required",
    )?;
    if input.name.is_none() && input.level.is_none() && input.project_id.is_none() && input.scopes.is_none() {
        return graphql_field_error("updateRole", "no fields to update");
    }

    let user_owned = user.clone();
    let existing = repository.query_role(role_id)?.ok_or_else(|| "role not found".to_owned())?;
    let level = normalize_role_level(input.level.as_deref().or(Some(existing.level.as_str())))?;
    let project_id = parse_role_project_id(input.project_id.as_deref(), level).unwrap_or(existing.project_id);
    authorize_role_write(&user_owned, level, project_id, "updateRole")?;
    if let Some(scopes) = input.scopes.as_deref() {
        validate_scope_list(scopes, "updateRole")?;
    }
    let scopes_json = input
        .scopes
        .as_ref()
        .map(|scopes| serde_json::to_string(scopes))
        .transpose()
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let result = repository.update_role(
        role_id,
        input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()),
        level,
        project_id,
        scopes_json.as_deref(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateRole": role_json(&record)}}),
        }),
        Err(message) if message == "permission denied" => graphql_permission_denied("updateRole"),
        Err(message) if message == "role not found" || message == "role already exists" => {
            graphql_field_error("updateRole", message)
        }
        Err(message) => Err(message),
    }
}

fn create_api_key_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreateApiKeyInput>(
        variables,
        "input",
        "api key input is required",
    )?;
    let project_id = parse_graphql_resource_id(input.project_id.as_str(), "project")
        .map_err(|_| "invalid project id".to_owned())?;
    if require_user_project_scope(user, project_id, SCOPE_WRITE_API_KEYS).is_err() {
        return graphql_permission_denied("createAPIKey");
    }
    validate_scope_list(input.scopes.as_deref().unwrap_or(&[]), "createAPIKey")?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createAPIKey", "api key name is required");
    }
    let key = input.key.unwrap_or(generate_llm_api_key()?);
    let key_type = normalize_api_key_type(input.key_type.as_deref())?.to_owned();
    let status = normalize_api_key_status(input.status.as_deref())?.to_owned();
    let scopes_json = serde_json::to_string(&input.scopes.unwrap_or_default())
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let owner_user_id = user.id;

    let profiles_json = input.profiles_json.unwrap_or_else(|| "{}".to_owned());
    let result = repository.create_api_key(
        owner_user_id,
        project_id,
        key.as_str(),
        name.as_str(),
        key_type.as_str(),
        status.as_str(),
        scopes_json.as_str(),
        profiles_json.as_str(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createAPIKey": api_key_json(&record)}}),
        }),
        Err(message)
            if message == "project not found" || message == "api key already exists" =>
        {
            graphql_field_error("createAPIKey", message)
        }
        Err(message) => Err(message),
    }
}

fn update_api_key_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "api key id is required".to_owned())?;
    let api_key_id = parse_graphql_resource_id(id, "api_key")
        .or_else(|_| parse_graphql_resource_id(id, "apiKey"))
        .map_err(|_| "invalid api key id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateApiKeyInput>(
        variables,
        "input",
        "api key input is required",
    )?;
    if input.name.is_none() && input.status.is_none() && input.scopes.is_none() {
        return graphql_field_error("updateAPIKey", "no fields to update");
    }
    if let Some(scopes) = input.scopes.as_deref() {
        validate_scope_list(scopes, "updateAPIKey")?;
    }

    let existing = repository.query_api_key(api_key_id)?.ok_or_else(|| "api key not found".to_owned())?;
    if require_user_project_scope(user, existing.project_id, SCOPE_WRITE_API_KEYS).is_err() {
        return graphql_permission_denied("updateAPIKey");
    }
    let scopes_json = input
        .scopes
        .as_ref()
        .map(|scopes| serde_json::to_string(scopes))
        .transpose()
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let normalized_status = input
        .status
        .as_deref()
        .map(|status| normalize_api_key_status(Some(status)).map(str::to_owned))
        .transpose()?;
    let result = repository.update_api_key(
        api_key_id,
        input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()),
        normalized_status.as_deref(),
        scopes_json.as_deref(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateAPIKey": api_key_json(&record)}}),
        }),
        Err(message) if message == "permission denied" => graphql_permission_denied("updateAPIKey"),
        Err(message) if message == "api key not found" => graphql_field_error("updateAPIKey", message),
        Err(message) => Err(message),
    }
}

fn create_channel_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("createChannel");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlCreateChannelInput>(
        variables,
        "input",
        "channel input is required",
    )?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createChannel", "channel name is required");
    }
    let channel_type = input.channel_type.trim().to_owned();
    if channel_type.is_empty() {
        return graphql_field_error("createChannel", "channel type is required");
    }
    let status = normalize_enable_status(input.status.as_deref(), "channel")?.to_owned();
    let supported_models = serde_json::to_string(&input.supported_models.unwrap_or_default())
        .map_err(|error| format!("failed to serialize supported models: {error}"))?;
    let tags = serde_json::to_string(&input.tags.unwrap_or_default())
        .map_err(|error| format!("failed to serialize tags: {error}"))?;
    let credentials_json = normalize_json_blob(input.credentials_json.as_deref())?;
    let settings_json = normalize_json_blob(input.settings_json.as_deref())?;

    let result = repository.create_channel(
        channel_type.as_str(),
        input.base_url.unwrap_or_default().as_str(),
        name.as_str(),
        status.as_str(),
        credentials_json.as_str(),
        supported_models.as_str(),
        input.auto_sync_supported_models.unwrap_or(false),
        input.default_test_model.unwrap_or_default().as_str(),
        settings_json.as_str(),
        tags.as_str(),
        input.ordering_weight.unwrap_or(100),
        input.error_message.unwrap_or_default().as_str(),
        input.remark.unwrap_or_default().as_str(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createChannel": channel_json(&admin_graphql_channel_from_record(&record))}}),
        }),
        Err(message) if message == "channel already exists" => graphql_field_error("createChannel", message),
        Err(message) => Err(message),
    }
}

fn update_channel_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("updateChannel");
    }
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "channel id is required".to_owned())?;
    let channel_id = parse_graphql_resource_id(id, "channel")
        .map_err(|_| "invalid channel id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateChannelInput>(
        variables,
        "input",
        "channel input is required",
    )?;
    if input.name.is_none()
        && input.base_url.is_none()
        && input.status.is_none()
        && input.supported_models.is_none()
        && input.auto_sync_supported_models.is_none()
        && input.default_test_model.is_none()
        && input.credentials_json.is_none()
        && input.settings_json.is_none()
        && input.tags.is_none()
        && input.ordering_weight.is_none()
        && input.error_message.is_none()
        && input.remark.is_none()
    {
        return graphql_field_error("updateChannel", "no fields to update");
    }

    let normalized_status = input
        .status
        .as_deref()
        .map(|value| normalize_enable_status(Some(value), "channel").map(str::to_owned))
        .transpose()?;
    let supported_models = input
        .supported_models
        .map(|value| serde_json::to_string(&value).map_err(|error| format!("failed to serialize supported models: {error}")))
        .transpose()?;
    let credentials_json = input
        .credentials_json
        .as_deref()
        .map(|value| normalize_json_blob(Some(value)))
        .transpose()?;
    let settings_json = input
        .settings_json
        .as_deref()
        .map(|value| normalize_json_blob(Some(value)))
        .transpose()?;
    let tags = input
        .tags
        .map(|value| serde_json::to_string(&value).map_err(|error| format!("failed to serialize tags: {error}")))
        .transpose()?;
    let result = repository.update_channel(
        channel_id,
        input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()),
        input.base_url.as_deref(),
        normalized_status.as_deref(),
        supported_models.as_deref(),
        input.auto_sync_supported_models,
        input.default_test_model.as_deref(),
        credentials_json.as_deref(),
        settings_json.as_deref(),
        tags.as_deref(),
        input.ordering_weight,
        input.error_message.as_deref(),
        input.remark.as_deref(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateChannel": channel_json(&admin_graphql_channel_from_record(&record))}}),
        }),
        Err(message) if message == "channel not found" || message == "channel already exists" => {
            graphql_field_error("updateChannel", message)
        }
        Err(message) => Err(message),
    }
}

fn create_model_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("createModel");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlCreateModelInput>(
        variables,
        "input",
        "model input is required",
    )?;
    let developer = input.developer.trim().to_owned();
    let model_id = input.model_id.trim().to_owned();
    let model_type = input.model_type.trim().to_owned();
    let name = input.name.trim().to_owned();
    if developer.is_empty() || model_id.is_empty() || model_type.is_empty() || name.is_empty() {
        return graphql_field_error("createModel", "developer, modelID, type, and name are required");
    }
    let status = normalize_enable_status(input.status.as_deref(), "model")?.to_owned();
    let model_card_json = normalize_json_blob(input.model_card_json.as_deref())?;
    let settings_json = normalize_json_blob(input.settings_json.as_deref())?;

    let result = repository.create_model(
        developer.as_str(),
        model_id.as_str(),
        model_type.as_str(),
        name.as_str(),
        input.icon.unwrap_or_default().as_str(),
        input.group.unwrap_or_default().as_str(),
        model_card_json.as_str(),
        settings_json.as_str(),
        status.as_str(),
        input.remark.as_deref(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createModel": model_json(&admin_graphql_model_from_record(&record))}}),
        }),
        Err(message) if message == "model already exists" => graphql_field_error("createModel", message),
        Err(message) => Err(message),
    }
}

fn update_model_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    if authorize_user_system_scope(user, SCOPE_WRITE_CHANNELS).is_err() {
        return graphql_permission_denied("updateModel");
    }
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "model id is required".to_owned())?;
    let model_id = parse_graphql_resource_id(id, "model").map_err(|_| "invalid model id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateModelInput>(
        variables,
        "input",
        "model input is required",
    )?;
    if input.name.is_none()
        && input.icon.is_none()
        && input.group.is_none()
        && input.model_card_json.is_none()
        && input.settings_json.is_none()
        && input.status.is_none()
        && input.remark.is_none()
    {
        return graphql_field_error("updateModel", "no fields to update");
    }

    let normalized_model_card_json = input
        .model_card_json
        .as_deref()
        .map(|value| normalize_json_blob(Some(value)))
        .transpose()?;
    let normalized_settings_json = input
        .settings_json
        .as_deref()
        .map(|value| normalize_json_blob(Some(value)))
        .transpose()?;
    let normalized_status = input
        .status
        .as_deref()
        .map(|status| normalize_enable_status(Some(status), "model").map(str::to_owned))
        .transpose()?;
    let result = repository.update_model(
        model_id,
        input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()),
        input.icon.as_deref(),
        input.group.as_deref(),
        normalized_model_card_json.as_deref(),
        normalized_settings_json.as_deref(),
        normalized_status.as_deref(),
        input.remark.as_deref().map(Some),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updateModel": model_json(&admin_graphql_model_from_record(&record))}}),
        }),
        Err(message) if message == "model not found" => graphql_field_error("updateModel", message),
        Err(message) => Err(message),
    }
}

fn create_user_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreateUserInput>(
        variables,
        "input",
        "user input is required",
    )?;

    let password_hash = hash_password(input.password.as_str())
        .map_err(|error| format!("failed to hash password: {error}"))?;
    let status = match input.status {
        Some(UserStatus::Activated) => "activated",
        Some(UserStatus::Deactivated) => "deactivated",
        None => "activated",
    };
    let scopes_json = serde_json::to_string(&input.scopes.unwrap_or_default())
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let project_ids = parse_graphql_id_list(input.project_ids, "project")?;
    let role_ids = parse_graphql_id_list(input.role_ids, "role")?;

    let user_id = repository.create_user(
        input.email.as_str(),
        status,
        input.prefer_language.unwrap_or_else(|| "en".to_owned()).as_str(),
        password_hash.as_str(),
        input.first_name.unwrap_or_default().as_str(),
        input.last_name.unwrap_or_default().as_str(),
        input.avatar.as_deref(),
        input.is_owner.unwrap_or(false),
        scopes_json.as_str(),
        &project_ids,
        &role_ids,
    )?;

    let user = load_graphql_user(repository, user_id)?.ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"createUser": admin_user_json(&user)}}),
    })
}

fn update_user_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "user id is required".to_owned())?;
    let user_id = parse_graphql_resource_id(id, "user")
        .map_err(|_| "invalid user id".to_owned())?;
    let status_value = variables
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "status is required".to_owned())?;
    let status = match status_value {
        "activated" => "activated",
        "deactivated" => "deactivated",
        _ => return Err(format!("invalid status: {status_value}")),
    };

    if !repository.update_user_status(user_id, status)? {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateUserStatus": Value::Null},
                "errors": [{"message": "user not found"}],
            }),
        });
    }

    let user = load_graphql_user(repository, user_id)?.ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateUserStatus": admin_user_json(&user)}}),
    })
}

fn update_user_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "user id is required".to_owned())?;
    let user_id = parse_graphql_resource_id(id, "user")
        .map_err(|_| "invalid user id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateUserInput>(
        variables,
        "input",
        "user input is required",
    )?;

    if input.first_name.is_none()
        && input.last_name.is_none()
        && input.prefer_language.is_none()
        && input.avatar.is_none()
        && input.password.is_none()
        && input.scopes.is_none()
        && input.role_ids.is_none()
    {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateUser": Value::Null},
                "errors": [{"message": "no fields to update"}],
            }),
        });
    }

    let scopes_json = input
        .scopes
        .as_ref()
        .map(|scopes| serde_json::to_string(scopes))
        .transpose()
        .map_err(|error| format!("failed to serialize scopes: {error}"))?;
    let role_ids = input
        .role_ids
        .map(Some)
        .map(|ids| parse_graphql_id_list(ids, "role"))
        .transpose()?;
    let password_hash = input
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(hash_password)
        .transpose()
        .map_err(|error| format!("failed to hash password: {error}"))?;

    if !repository.update_user(
        user_id,
        input.first_name.as_deref(),
        input.last_name.as_deref(),
        input.prefer_language.as_deref(),
        input.avatar.as_deref(),
        password_hash.as_deref(),
        scopes_json.as_deref(),
        role_ids.as_deref(),
    )? {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateUser": Value::Null},
                "errors": [{"message": "user not found"}],
            }),
        });
    }

    let user = load_graphql_user(repository, user_id)?.ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateUser": admin_user_json(&user)}}),
    })
}

fn update_me_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    user: &AuthUserContext,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlUpdateMeInput>(
        variables,
        "input",
        "user input is required",
    )?;

    if input.first_name.is_none()
        && input.last_name.is_none()
        && input.prefer_language.is_none()
        && input.avatar.is_none()
    {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateMe": Value::Null},
                "errors": [{"message": "no fields to update"}],
            }),
        });
    }

    if !repository.update_user_profile(
        user.id,
        input.first_name.as_deref(),
        input.last_name.as_deref(),
        input.prefer_language.as_deref(),
        input.avatar.as_deref(),
    )? {
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({
                "data": {"updateMe": Value::Null},
                "errors": [{"message": "user not found"}],
            }),
        });
    }

    let profile = load_graphql_user_profile(repository, user.id)?
        .ok_or_else(|| "user not found".to_owned())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updateMe": admin_user_profile_json(&profile)}}),
    })
}

fn create_prompt_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    project_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreatePromptInput>(
        variables,
        "input",
        "prompt input is required",
    )?;
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return graphql_field_error("createPrompt", "prompt name is required");
    }
    let role = normalize_prompt_role(Some(input.role.as_str()))?.to_owned();
    let status = normalize_enable_status(input.status.as_deref(), "prompt")?.to_owned();
    let settings_json = serde_json::to_string(&stored_prompt_settings_from_input(input.settings)?)
        .map_err(|error| format!("failed to serialize prompt settings: {error}"))?;
    let result = repository.create_prompt(
        project_id,
        name.as_str(),
        input.description.unwrap_or_default().as_str(),
        role.as_str(),
        input.content.as_str(),
        status.as_str(),
        input.order.unwrap_or_default(),
        settings_json.as_str(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createPrompt": prompt_json(&admin_graphql_prompt_from_record(record)?)}}),
        }),
        Err(message) if message == "project not found" || message == "prompt already exists" => {
            graphql_field_error("createPrompt", message)
        }
        Err(message) => Err(message),
    }
}

fn update_prompt_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    project_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let prompt_id = parse_prompt_id_from_variables(&variables)?;
    let existing = repository.query_prompt(prompt_id)?.ok_or_else(|| "prompt not found".to_owned())?;
    if existing.project_id != project_id {
        return graphql_field_error("updatePrompt", "prompt not found");
    }
    let input = parse_graphql_variable_input::<AdminGraphqlUpdatePromptInput>(
        variables,
        "input",
        "prompt input is required",
    )?;
    if input.name.is_none()
        && input.description.is_none()
        && input.role.is_none()
        && input.content.is_none()
        && input.status.is_none()
        && input.order.is_none()
        && input.settings.is_none()
    {
        return graphql_field_error("updatePrompt", "no fields to update");
    }

    let role = input
        .role
        .as_deref()
        .map(|value| normalize_prompt_role(Some(value)).map(str::to_owned))
        .transpose()?;
    let status = input
        .status
        .as_deref()
        .map(|value| normalize_enable_status(Some(value), "prompt").map(str::to_owned))
        .transpose()?;
    let settings_json = input
        .settings
        .map(stored_prompt_settings_from_input)
        .transpose()?
        .map(|settings| serde_json::to_string(&settings))
        .transpose()
        .map_err(|error| format!("failed to serialize prompt settings: {error}"))?;
    let result = repository.update_prompt(
        prompt_id,
        input.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()),
        input.description.as_deref(),
        role.as_deref(),
        input.content.as_deref(),
        status.as_deref(),
        input.order,
        settings_json.as_deref(),
    );

    match result {
        Ok(record) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updatePrompt": prompt_json(&admin_graphql_prompt_from_record(record)?)}}),
        }),
        Err(message) if message == "prompt not found" || message == "prompt already exists" => {
            graphql_field_error("updatePrompt", message)
        }
        Err(message) => Err(message),
    }
}

fn delete_prompt_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    project_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let prompt_id = parse_prompt_id_from_variables(&variables)?;
    let existing = repository.query_prompt(prompt_id)?.ok_or_else(|| "prompt not found".to_owned())?;
    if existing.project_id != project_id {
        return graphql_field_error("deletePrompt", "prompt not found");
    }
    if !repository.delete_prompt(prompt_id)? {
        return graphql_field_error("deletePrompt", "prompt not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"deletePrompt": true}}),
    })
}

fn update_prompt_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    project_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let prompt_id = parse_prompt_id_from_variables(&variables)?;
    let existing = repository.query_prompt(prompt_id)?.ok_or_else(|| "prompt not found".to_owned())?;
    if existing.project_id != project_id {
        return graphql_field_error("updatePromptStatus", "prompt not found");
    }
    let status = variables
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "status is required".to_owned())?;
    let status = normalize_enable_status(Some(status), "prompt")?.to_owned();
    if !repository.update_prompt_status(prompt_id, status.as_str())? {
        return graphql_field_error("updatePromptStatus", "prompt not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updatePromptStatus": true}}),
    })
}

fn bulk_delete_prompts_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    project_id: i64,
) -> Result<GraphqlExecutionResult, String> {
    let ids = parse_prompt_ids(&variables)?;
    ensure_prompts_in_project(repository, &ids, project_id, "bulkDeletePrompts")?;
    repository.bulk_delete_prompts(&ids)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"bulkDeletePrompts": true}}),
    })
}

fn bulk_update_prompts_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    project_id: i64,
    status: &str,
    field: &str,
) -> Result<GraphqlExecutionResult, String> {
    let ids = parse_prompt_ids(&variables)?;
    ensure_prompts_in_project(repository, &ids, project_id, field)?;
    repository.bulk_update_prompts_status(&ids, status)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {field: true}}),
    })
}

fn complete_onboarding_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let _ = parse_graphql_variable_input::<AdminGraphqlCompleteOnboardingInput>(
        variables,
        "input",
        "input is required",
    )?;
    let mut record = repository.query_onboarding_record()?.unwrap_or_default();
    let completed_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    record.onboarded = true;
    record.completed_at = Some(completed_at.clone());
    if record.auto_disable_channel.is_none() {
        record.auto_disable_channel = Some(crate::foundation::request_context::OnboardingModule {
            onboarded: true,
            completed_at: Some(completed_at),
        });
    }
    persist_onboarding_record(repository, &record)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"completeOnboarding": true}}),
    })
}

fn complete_system_model_setting_onboarding_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let _ = parse_graphql_variable_input::<AdminGraphqlCompleteSystemModelSettingOnboardingInput>(
        variables,
        "input",
        "input is required",
    )?;
    let mut record = repository.query_onboarding_record()?.unwrap_or_default();
    record.system_model_setting = Some(crate::foundation::request_context::OnboardingModule {
        onboarded: true,
        completed_at: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    });
    persist_onboarding_record(repository, &record)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"completeSystemModelSettingOnboarding": true}}),
    })
}

fn complete_auto_disable_channel_onboarding_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let _ = parse_graphql_variable_input::<AdminGraphqlCompleteAutoDisableChannelOnboardingInput>(
        variables,
        "input",
        "input is required",
    )?;
    let mut record = repository.query_onboarding_record()?.unwrap_or_default();
    record.auto_disable_channel = Some(crate::foundation::request_context::OnboardingModule {
        onboarded: true,
        completed_at: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    });
    persist_onboarding_record(repository, &record)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"completeAutoDisableChannelOnboarding": true}}),
    })
}

fn query_prompt_protection_rules_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<GraphqlExecutionResult, String> {
    let rules = repository
        .query_prompt_protection_rules()?
        .into_iter()
        .map(prompt_protection_rule_from_record)
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_prompt_protection_openai_error)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({
            "data": {
                "promptProtectionRules": default_prompt_protection_connection_json(
                    rules.iter().map(prompt_protection_rule_json).collect(),
                )
            }
        }),
    })
}

fn create_prompt_protection_rule_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let input = parse_graphql_variable_input::<AdminGraphqlCreatePromptProtectionRuleInput>(
        variables,
        "input",
        "prompt protection rule input is required",
    )?;
    let settings = stored_prompt_protection_settings_from_input(input.settings);
    if let Err(message) = validate_prompt_protection_rule(
        input.name.as_str(),
        input.pattern.as_str(),
        &settings,
        "prompt protection rule",
    ) {
        return graphql_field_error("createPromptProtectionRule", message);
    }
    let name = input.name.trim().to_owned();
    let description = input.description.unwrap_or_default();
    let pattern = input.pattern.trim().to_owned();
    let settings_json = serde_json::to_string(&settings)
        .map_err(|error| format!("failed to serialize prompt protection settings: {error}"))?;

    let result = repository
        .create_prompt_protection_rule(
            name.as_str(),
            description.as_str(),
            pattern.as_str(),
            "disabled",
            settings_json.as_str(),
        )
        .and_then(|record| {
            record
                .ok_or_else(|| "prompt protection rule not found".to_owned())
                .and_then(|record| prompt_protection_rule_from_record(record).map_err(map_prompt_protection_openai_error))
        });

    match result {
        Ok(rule) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"createPromptProtectionRule": prompt_protection_rule_json(&rule)}}),
        }),
        Err(message)
            if message == "prompt protection rule already exists"
                || message == "prompt protection rule not found"
                || message.starts_with("invalid ")
                || message.contains("requires replacement")
                || message.contains("must target at least one scope") =>
        {
            graphql_field_error("createPromptProtectionRule", message)
        }
        Err(message) => Err(message),
    }
}

fn update_prompt_protection_rule_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt protection rule id is required".to_owned())?;
    let rule_id = parse_graphql_resource_id(id, "promptProtectionRule")
        .or_else(|_| parse_graphql_resource_id(id, "prompt_protection_rule"))
        .map_err(|_| "invalid prompt protection rule id".to_owned())?;
    let input = parse_graphql_variable_input::<AdminGraphqlUpdatePromptProtectionRuleInput>(
        variables,
        "input",
        "prompt protection rule input is required",
    )?;
    if input.name.is_none()
        && input.description.is_none()
        && input.pattern.is_none()
        && input.status.is_none()
        && input.settings.is_none()
    {
        return graphql_field_error("updatePromptProtectionRule", "no fields to update");
    }

    let existing = repository
        .query_prompt_protection_rules()?
        .into_iter()
        .find(|record| record.id == rule_id)
        .ok_or_else(|| "prompt protection rule not found".to_owned())?;
    let existing_rule = prompt_protection_rule_from_record(existing).map_err(map_prompt_protection_openai_error)?;
    let next_name = input.name.clone().unwrap_or(existing_rule.name).trim().to_owned();
    let next_description = input.description.clone().unwrap_or(existing_rule.description);
    let next_pattern = input.pattern.clone().unwrap_or(existing_rule.pattern).trim().to_owned();
    let next_status = normalize_prompt_protection_status(input.status.map(prompt_protection_rule_status_name))?
        .to_owned();
    let next_settings = input
        .settings
        .map(stored_prompt_protection_settings_from_input)
        .unwrap_or(existing_rule.settings);
    validate_prompt_protection_rule(
        next_name.as_str(),
        next_pattern.as_str(),
        &next_settings,
        "prompt protection rule",
    )?;
    let settings_json = serde_json::to_string(&next_settings)
        .map_err(|error| format!("failed to serialize prompt protection settings: {error}"))?;
    let result = repository
        .update_prompt_protection_rule(
            rule_id,
            Some(next_name.as_str()),
            Some(next_description.as_str()),
            Some(next_pattern.as_str()),
            Some(next_status.as_str()),
            Some(settings_json.as_str()),
        )
        .and_then(|record| {
            record
                .ok_or_else(|| "prompt protection rule not found".to_owned())
                .and_then(|record| prompt_protection_rule_from_record(record).map_err(map_prompt_protection_openai_error))
        });

    match result {
        Ok(rule) => Ok(GraphqlExecutionResult {
            status: 200,
            body: json!({"data": {"updatePromptProtectionRule": prompt_protection_rule_json(&rule)}}),
        }),
        Err(message)
            if message == "prompt protection rule not found"
                || message == "prompt protection rule already exists"
                || message.starts_with("invalid ")
                || message.contains("requires replacement")
                || message.contains("must target at least one scope") =>
        {
            graphql_field_error("updatePromptProtectionRule", message)
        }
        Err(message) => Err(message),
    }
}

fn update_prompt_protection_rule_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt protection rule id is required".to_owned())?;
    let rule_id = parse_graphql_resource_id(id, "promptProtectionRule")
        .or_else(|_| parse_graphql_resource_id(id, "prompt_protection_rule"))
        .map_err(|_| "invalid prompt protection rule id".to_owned())?;
    let status = variables
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "status is required".to_owned())?;
    let status = normalize_prompt_protection_status(Some(status))?.to_owned();
    let updated = repository.set_prompt_protection_rule_status(rule_id, status.as_str())?;
    if !updated {
        return graphql_field_error("updatePromptProtectionRuleStatus", "prompt protection rule not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"updatePromptProtectionRuleStatus": true}}),
    })
}

fn delete_prompt_protection_rule_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt protection rule id is required".to_owned())?;
    let rule_id = parse_graphql_resource_id(id, "promptProtectionRule")
        .or_else(|_| parse_graphql_resource_id(id, "prompt_protection_rule"))
        .map_err(|_| "invalid prompt protection rule id".to_owned())?;
    let deleted = repository.delete_prompt_protection_rule(rule_id)?;
    if !deleted {
        return graphql_field_error("deletePromptProtectionRule", "prompt protection rule not found");
    }
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"deletePromptProtectionRule": true}}),
    })
}

fn bulk_delete_prompt_protection_rules_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
) -> Result<GraphqlExecutionResult, String> {
    let ids = parse_graphql_prompt_protection_rule_ids(&variables)?;
    repository.bulk_delete_prompt_protection_rules(&ids)?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {"bulkDeletePromptProtectionRules": true}}),
    })
}

fn bulk_set_prompt_protection_rules_status_graphql_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    variables: Value,
    status: &str,
    field: &str,
) -> Result<GraphqlExecutionResult, String> {
    let ids = parse_graphql_prompt_protection_rule_ids(&variables)?;
    let status = status.to_owned();
    repository.bulk_set_prompt_protection_rules_status(&ids, status.as_str())?;
    Ok(GraphqlExecutionResult {
        status: 200,
        body: json!({"data": {field: true}}),
    })
}

fn load_storage_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredStoragePolicy, String> {
    deserialize_setting_or_default(repository.query_storage_policy()?, default_storage_policy)
}

fn load_retry_policy_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredRetryPolicy, String> {
    deserialize_setting_or_default(repository.query_retry_policy()?, default_retry_policy)
}

fn load_auto_backup_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredAutoBackupSettings, String> {
    deserialize_setting_or_default(repository.query_auto_backup_settings()?, default_auto_backup_settings)
}

fn load_video_storage_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredVideoStorageSettings, String> {
    deserialize_setting_or_default(
        repository.query_video_storage_settings()?,
        default_video_storage_settings,
    )
}

fn load_system_channel_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredSystemChannelSettings, String> {
    deserialize_setting_or_default(
        repository.query_system_channel_settings()?,
        default_system_channel_settings,
    )
}

fn load_system_model_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredSystemModelSettings, String> {
    let mut settings = deserialize_setting_or_default(
        repository.query_system_model_settings()?,
        default_system_model_settings,
    )?;

    if let Some(legacy_channel_settings) = repository.query_system_channel_settings()? {
        let legacy: LegacySystemChannelSettings = serde_json::from_str(legacy_channel_settings.value.as_str())
            .map_err(|error| format!("failed to decode stored admin setting: {error}"))?;
        if let Some(value) = legacy.fallback_to_channels_on_model_not_found {
            settings.fallback_to_channels_on_model_not_found = value;
        }
        if let Some(value) = legacy.query_all_channel_models {
            settings.query_all_channel_models = value;
        }
    }

    Ok(settings)
}

fn load_system_general_settings_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<StoredSystemGeneralSettings, String> {
    let defaults = default_system_general_settings();
    let mut settings = deserialize_setting_or_default(
        repository.query_system_general_settings()?,
        default_system_general_settings,
    )?;
    if settings.currency_code.trim().is_empty() {
        settings.currency_code = defaults.currency_code;
    }
    if settings.timezone.trim().is_empty() {
        settings.timezone = defaults.timezone;
    }
    Ok(settings)
}

fn load_default_data_storage_id_seaorm(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
) -> Result<Option<i64>, String> {
    let Some(record) = repository.query_default_data_storage()? else {
        return Ok(None);
    };

    let Ok(id) = record.value.trim().parse::<i64>() else {
        return Ok(None);
    };
    if id <= 0 {
        return Ok(None);
    }

    let Some(storage) = repository.query_data_storage_status(id)? else {
        return Ok(None);
    };
    if !storage.status.eq_ignore_ascii_case("active") {
        return Ok(None);
    }

    Ok(Some(id))
}

fn load_graphql_user_profile(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<Option<AdminGraphqlUserInfo>, String> {
    let Some(profile) = repository.query_user_profile(user_id)? else {
        return Ok(None);
    };
    let roles = repository
        .query_user_roles(user_id)?
        .into_iter()
        .map(role_record_to_graphql_role)
        .collect::<Result<Vec<_>, _>>()?;
    let projects = repository
        .query_user_projects(user_id)?
        .into_iter()
        .map(|membership| {
            let roles = repository
                .query_project_roles(user_id, membership.project_id)?
                .into_iter()
                .map(role_record_to_graphql_role)
                .collect::<Result<Vec<_>, _>>()?;
            let scopes = parse_scope_json(membership.scopes.as_str());
            Ok::<AdminGraphqlUserProjectInfo, String>(AdminGraphqlUserProjectInfo {
                project_id: graphql_gid("project", membership.project_id),
                is_owner: membership.is_owner,
                scopes,
                roles,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(AdminGraphqlUserInfo {
        id: graphql_gid("user", profile.id),
        email: profile.email,
        first_name: profile.first_name,
        last_name: profile.last_name,
        is_owner: profile.is_owner,
        prefer_language: profile.prefer_language,
        avatar: profile.avatar.filter(|value| !value.is_empty()),
        scopes: parse_scope_json(profile.scopes.as_str()),
        roles,
        projects,
    }))
}

fn load_graphql_user(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    user_id: i64,
) -> Result<Option<AdminGraphqlUser>, String> {
    let Some(user) = repository.query_user(user_id)? else {
        return Ok(None);
    };
    let roles = repository
        .query_user_roles(user_id)?
        .into_iter()
        .map(role_record_to_graphql_role)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(AdminGraphqlUser {
        id: graphql_gid("user", user.id),
        created_at: user.created_at,
        updated_at: user.updated_at,
        email: user.email,
        status: user.status,
        first_name: user.first_name,
        last_name: user.last_name,
        is_owner: user.is_owner,
        prefer_language: user.prefer_language,
        scopes: parse_scope_json(user.scopes.as_str()),
        roles: AdminGraphqlRoleConnection {
            edges: roles
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect(),
            page_info: empty_page_info(),
        },
    }))
}

fn deserialize_setting_or_default<T, Record, F>(
    record: Option<Record>,
    default_factory: F,
) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
    Record: IntoGraphqlSettingValue,
    F: FnOnce() -> T,
{
    let Some(record) = record else {
        return Ok(default_factory());
    };
    serde_json::from_str(record.setting_value())
        .map_err(|error| format!("failed to decode stored admin setting: {error}"))
}

trait IntoGraphqlSettingValue {
    fn setting_value(&self) -> &str;
}

impl IntoGraphqlSettingValue for GraphqlStoragePolicyRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlRetryPolicyRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlAutoBackupSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlVideoStorageSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlDefaultDataStorageRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlSystemChannelSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlSystemGeneralSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

impl IntoGraphqlSettingValue for GraphqlSystemModelSettingsRecord {
    fn setting_value(&self) -> &str {
        self.value.as_str()
    }
}

fn parse_graphql_variable_input<T>(
    variables: Value,
    key: &str,
    missing_message: &str,
) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let input = variables
        .get(key)
        .cloned()
        .ok_or_else(|| missing_message.to_owned())?;
    serde_json::from_value(input).map_err(|error| format!("invalid {key}: {error}"))
}

fn storage_policy_json(policy: &StoredStoragePolicy) -> Value {
    json!({
        "storeChunks": policy.store_chunks,
        "storeRequestBody": policy.store_request_body,
        "storeResponseBody": policy.store_response_body,
        "cleanupOptions": policy.cleanup_options.iter().map(|option| {
            json!({
                "resourceType": option.resource_type,
                "enabled": option.enabled,
                "cleanupDays": option.cleanup_days,
            })
        }).collect::<Vec<_>>(),
    })
}

fn retry_policy_json(policy: &StoredRetryPolicy) -> Value {
    json!({
        "enabled": policy.enabled,
        "maxChannelRetries": policy.max_channel_retries,
        "maxSingleChannelRetries": policy.max_single_channel_retries,
        "retryDelayMs": policy.retry_delay_ms,
        "loadBalancerStrategy": policy.load_balancer_strategy,
        "autoDisableChannel": {
            "enabled": policy.auto_disable_channel.enabled,
            "statuses": policy.auto_disable_channel.statuses.iter().map(|status| {
                json!({
                    "status": status.status,
                    "times": status.times,
                })
            }).collect::<Vec<_>>(),
        },
    })
}

fn video_storage_settings_json(settings: &StoredVideoStorageSettings) -> Value {
    json!({
        "enabled": settings.enabled,
        "dataStorageID": settings.data_storage_id,
        "scanIntervalMinutes": settings.scan_interval_minutes,
        "scanLimit": settings.scan_limit,
    })
}

fn auto_backup_settings_json(settings: &StoredAutoBackupSettings) -> Value {
    json!({
        "enabled": settings.enabled,
        "frequency": settings.frequency,
        "dataStorageID": settings.data_storage_id,
        "includeChannels": settings.include_channels,
        "includeModels": settings.include_models,
        "includeAPIKeys": settings.include_api_keys,
        "includeModelPrices": settings.include_model_prices,
        "retentionDays": settings.retention_days,
        "lastBackupAt": settings.last_backup_at.map(format_unix_timestamp),
        "lastBackupError": if settings.last_backup_error.trim().is_empty() {
            Value::Null
        } else {
            Value::String(settings.last_backup_error.clone())
        },
    })
}

fn system_channel_settings_json(settings: &StoredSystemChannelSettings) -> Value {
    json!({
        "probe": {
            "enabled": settings.probe.enabled,
            "frequency": probe_frequency_graphql_name(settings.probe.frequency),
        },
        "autoSync": {
            "frequency": auto_sync_frequency_graphql_name(settings.auto_sync.frequency),
        },
    })
}

fn system_general_settings_json(settings: &StoredSystemGeneralSettings) -> Value {
    json!({
        "currencyCode": settings.currency_code,
        "timezone": settings.timezone,
    })
}

fn system_model_settings_json(settings: &StoredSystemModelSettings) -> Value {
    json!({
        "fallbackToChannelsOnModelNotFound": settings.fallback_to_channels_on_model_not_found,
        "queryAllChannelModels": settings.query_all_channel_models,
    })
}

fn parse_probe_frequency_graphql_value(value: &str) -> Result<ProbeFrequencySetting, String> {
    match value {
        "ONE_MINUTE" => Ok(ProbeFrequencySetting::OneMinute),
        "FIVE_MINUTES" => Ok(ProbeFrequencySetting::FiveMinutes),
        "THIRTY_MINUTES" => Ok(ProbeFrequencySetting::ThirtyMinutes),
        "ONE_HOUR" => Ok(ProbeFrequencySetting::OneHour),
        other => Err(format!("invalid probe.frequency: {other}")),
    }
}

fn parse_auto_sync_frequency_graphql_value(value: &str) -> Result<AutoSyncFrequencySetting, String> {
    match value {
        "ONE_HOUR" => Ok(AutoSyncFrequencySetting::OneHour),
        "SIX_HOURS" => Ok(AutoSyncFrequencySetting::SixHours),
        "ONE_DAY" => Ok(AutoSyncFrequencySetting::OneDay),
        other => Err(format!("invalid autoSync.frequency: {other}")),
    }
}

fn probe_frequency_graphql_name(value: ProbeFrequencySetting) -> &'static str {
    match value {
        ProbeFrequencySetting::OneMinute => "ONE_MINUTE",
        ProbeFrequencySetting::FiveMinutes => "FIVE_MINUTES",
        ProbeFrequencySetting::ThirtyMinutes => "THIRTY_MINUTES",
        ProbeFrequencySetting::OneHour => "ONE_HOUR",
    }
}

fn auto_sync_frequency_graphql_name(value: AutoSyncFrequencySetting) -> &'static str {
    match value {
        AutoSyncFrequencySetting::OneHour => "ONE_HOUR",
        AutoSyncFrequencySetting::SixHours => "SIX_HOURS",
        AutoSyncFrequencySetting::OneDay => "ONE_DAY",
    }
}

fn proxy_preset_json(preset: &StoredProxyPreset) -> Value {
    json!({
        "name": if preset.name.trim().is_empty() { Value::Null } else { Value::String(preset.name.clone()) },
        "url": preset.url,
        "username": if preset.username.trim().is_empty() { Value::Null } else { Value::String(preset.username.clone()) },
        "password": if preset.password.trim().is_empty() { Value::Null } else { Value::String(preset.password.clone()) },
    })
}

fn empty_page_info() -> AdminGraphqlPageInfo {
    AdminGraphqlPageInfo {
        has_next_page: false,
        has_previous_page: false,
        start_cursor: None,
        end_cursor: None,
    }
}

fn parse_scope_json(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn role_record_to_graphql_role(record: GraphqlRoleSummaryRecord) -> Result<AdminGraphqlRoleInfo, String> {
    Ok(AdminGraphqlRoleInfo {
        id: graphql_gid("role", record.id),
        name: record.name,
        scopes: parse_scope_json(record.scopes.as_str()),
    })
}

fn admin_user_json(user: &AdminGraphqlUser) -> Value {
    json!({
        "id": user.id,
        "createdAt": user.created_at,
        "updatedAt": user.updated_at,
        "email": user.email,
        "status": user.status,
        "firstName": user.first_name,
        "lastName": user.last_name,
        "isOwner": user.is_owner,
        "preferLanguage": user.prefer_language,
        "scopes": user.scopes,
        "roles": {
            "edges": user.roles.edges.iter().map(|edge| {
                json!({
                    "cursor": edge.cursor,
                    "node": edge.node.as_ref().map(|node| {
                        json!({
                            "id": node.id,
                            "name": node.name,
                            "scopes": node.scopes,
                        })
                    })
                })
            }).collect::<Vec<_>>(),
            "pageInfo": {
                "hasNextPage": user.roles.page_info.has_next_page,
                "hasPreviousPage": user.roles.page_info.has_previous_page,
                "startCursor": user.roles.page_info.start_cursor,
                "endCursor": user.roles.page_info.end_cursor,
            }
        }
    })
}

fn admin_user_profile_json(user: &AdminGraphqlUserInfo) -> Value {
    json!({
        "id": user.id,
        "email": user.email,
        "firstName": user.first_name,
        "lastName": user.last_name,
        "isOwner": user.is_owner,
        "preferLanguage": user.prefer_language,
        "avatar": user.avatar,
        "scopes": user.scopes,
        "roles": user.roles.iter().map(|role| {
            json!({
                "id": role.id,
                "name": role.name,
                "scopes": role.scopes,
            })
        }).collect::<Vec<_>>(),
        "projects": user.projects.iter().map(|project| {
            json!({
                "projectID": project.project_id,
                "isOwner": project.is_owner,
                "scopes": project.scopes,
                "roles": project.roles.iter().map(|role| {
                    json!({
                        "id": role.id,
                        "name": role.name,
                        "scopes": role.scopes,
                    })
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>(),
    })
}

fn parse_graphql_id_list(ids: Option<Vec<String>>, expected_type: &str) -> Result<Vec<i64>, String> {
    ids.unwrap_or_default()
        .into_iter()
        .map(|value| parse_graphql_resource_id(value.as_str(), expected_type))
        .collect()
}

fn generate_llm_api_key() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom(&mut bytes).map_err(|error| error.to_string())?;
    Ok(format!("ah-{}", hex_encode(bytes)))
}

async fn create_llm_api_key_seaorm(
    repository: &SeaOrmOpenApiGraphqlMutationRepository,
    owner_api_key: &AuthApiKeyContext,
    trimmed_name: &str,
) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
    if trimmed_name.is_empty() {
        return Err(CreateLlmApiKeyError::InvalidName);
    }

    require_service_api_key_write_access(owner_api_key)
        .map_err(|_| CreateLlmApiKeyError::PermissionDenied)?;

    let owner_record = repository
        .query_owner_api_key(owner_api_key.key.as_str())
        .map_err(CreateLlmApiKeyError::Internal)?
    .ok_or_else(|| CreateLlmApiKeyError::Internal("failed to load owner api key".to_owned()))?;

    let owner_key_type = owner_record.key_type;
    let owner_user_id = owner_record.user_id;
    let owner_project_id = owner_record.project_id;

    if owner_key_type != "service_account" || owner_project_id != owner_api_key.project.id {
        return Err(CreateLlmApiKeyError::PermissionDenied);
    }

    let generated_key = generate_llm_api_key().map_err(CreateLlmApiKeyError::Internal)?;
    let scopes = scope_strings(LLM_API_KEY_SCOPES);
    let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES)
        .map_err(|error| CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}")))?;

    repository
        .insert_llm_api_key(
            owner_user_id,
            owner_api_key,
            generated_key.as_str(),
            trimmed_name,
            scopes_json.as_str(),
        )
        .map_err(CreateLlmApiKeyError::Internal)?;

    Ok(OpenApiGraphqlApiKey {
        key: generated_key,
        name: trimmed_name.to_owned(),
        scopes,
    })
}

async fn execute_openapi_graphql_seaorm_request(
    repository: SeaOrmOpenApiGraphqlMutationRepository,
    payload: GraphqlRequestPayload,
    owner_api_key: AuthApiKeyContext,
) -> Result<GraphqlExecutionResult, String> {
    let query = payload.query.trim();

    if query.contains("serviceAccountProject") {
        let project = GraphqlProject::from(owner_api_key.project);
        return Ok(GraphqlExecutionResult {
            status: 200,
            body: serde_json::json!({
                "data": {
                    "serviceAccountProject": {
                        "id": project.id,
                        "name": project.name,
                        "status": project.status,
                    }
                }
            }),
        });
    }

    if query.contains("createLLMAPIKey") {
        let name = extract_create_llm_api_key_name(&payload.query, &payload.variables)
            .ok_or_else(|| "api key name is required".to_owned())?;
        return create_llm_api_key_seaorm(&repository, &owner_api_key, &name)
            .await
            .map(|api_key| GraphqlExecutionResult {
                status: 200,
                body: serde_json::json!({
                    "data": {
                        "createLLMAPIKey": {
                            "key": api_key.key,
                            "name": api_key.name,
                            "scopes": api_key.scopes,
                        }
                    }
                }),
            })
            .or_else(|error| {
                let message = match error {
                    CreateLlmApiKeyError::InvalidName => "api key name is required".to_owned(),
                    CreateLlmApiKeyError::PermissionDenied => "permission denied".to_owned(),
                    CreateLlmApiKeyError::Internal(message) => message,
                };
                Ok(GraphqlExecutionResult {
                    status: 200,
                    body: serde_json::json!({
                        "data": {"createLLMAPIKey": Value::Null},
                        "errors": [{"message": message}],
                    }),
                })
            });
    }

    if let Some(field) = first_graphql_field_name(query) {
        return graphql_not_implemented_for_route("/openapi/v1/graphql", field.as_str());
    }

    Err("unsupported openapi graphql query".to_owned())
}

pub(crate) fn extract_create_llm_api_key_name(query: &str, variables: &Value) -> Option<String> {
    if let Some(value) = variables
        .get("name")
        .or_else(|| variables.get("input").and_then(|input| input.get("name")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_owned());
    }

    let marker = "createLLMAPIKey(name:";
    let start = query.find(marker)? + marker.len();
    let remainder = query.get(start..)?.trim_start();
    let first_quote = remainder.find('"')? + 1;
    let after_first = remainder.get(first_quote..)?;
    let end_quote = after_first.find('"')?;
    let name = after_first[..end_quote].trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn first_graphql_field_name(query: &str) -> Option<String> {
    let trimmed = query.trim();
    let body = if let Some(start) = trimmed.find('{') {
        trimmed.get(start + 1..)?
    } else {
        trimmed
    };

    let token = body
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .find(|token| !token.is_empty())?;

    Some(token.to_owned())
}

fn graphql_string_argument(query: &str, argument_name: &str) -> Option<String> {
    let marker = format!("{argument_name}:");
    let start = query.find(&marker)? + marker.len();
    let remainder = query.get(start..)?.trim_start();
    let first_quote = remainder.find('"')? + 1;
    let after_first = remainder.get(first_quote..)?;
    let end_quote = after_first.find('"')?;
    let value = after_first[..end_quote].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn graphql_input_string(variables: &Value, query: &str, argument_name: &str) -> Option<String> {
    variables
        .get(argument_name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| graphql_string_argument(query, argument_name))
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Project", rename_fields = "camelCase")]
pub(crate) struct GraphqlProject {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "AdminProject", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProject {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) status: String,
}

impl From<ProjectContext> for GraphqlProject {
    fn from(value: ProjectContext) -> Self {
        Self {
            id: graphql_gid("project", value.id),
            name: value.name,
            status: value.status,
        }
    }
}

impl From<StoredChannelSummary> for AdminGraphqlChannel {
    fn from(value: StoredChannelSummary) -> Self {
        Self {
            id: graphql_gid("channel", value.id),
            name: value.name,
            channel_type: value.channel_type,
            base_url: value.base_url,
            status: value.status,
            supported_models: value.supported_models,
            ordering_weight: i64_to_i32(value.ordering_weight),
            provider_quota_status: None,
            circuit_breaker_status: None,
        }
    }
}

impl From<StoredModelRecord> for AdminGraphqlModel {
    fn from(value: StoredModelRecord) -> Self {
        let parsed = parse_model_card(value.model_card_json.as_str());
        Self {
            id: graphql_gid("model", value.id),
            developer: value.developer,
            model_id: value.model_id,
            model_type: value.model_type,
            name: value.name,
            icon: value.icon,
            remark: value.remark,
            context_length: parsed.context_length.map(i64_to_i32),
            max_output_tokens: parsed.max_output_tokens.map(i64_to_i32),
        }
    }
}

impl From<StoredRequestSummary> for AdminGraphqlRequestSummaryObject {
    fn from(value: StoredRequestSummary) -> Self {
        Self {
            id: graphql_gid("request", value.id),
            project_id: graphql_gid("project", value.project_id),
            trace_id: value.trace_id.map(|id| graphql_gid("trace", id)),
            channel_id: value.channel_id.map(|id| graphql_gid("channel", id)),
            model_id: value.model_id,
            format: value.format,
            status: value.status,
            source: value.source,
            external_id: value.external_id,
        }
    }
}

impl From<TraceContext> for AdminGraphqlTrace {
    fn from(value: TraceContext) -> Self {
        Self {
            id: graphql_gid("trace", value.id),
            trace_id: value.trace_id,
            project_id: graphql_gid("project", value.project_id),
            thread_id: value.thread_id.map(|id| graphql_gid("thread", id)),
        }
    }
}

impl From<StoredStoragePolicy> for AdminGraphqlStoragePolicy {
    fn from(value: StoredStoragePolicy) -> Self {
        Self {
            store_chunks: value.store_chunks,
            store_request_body: value.store_request_body,
            store_response_body: value.store_response_body,
            cleanup_options: value
                .cleanup_options
                .into_iter()
                .map(|option| AdminGraphqlCleanupOption {
                    resource_type: option.resource_type,
                    enabled: option.enabled,
                    cleanup_days: option.cleanup_days,
                })
                .collect(),
        }
    }
}

impl From<StoredRetryPolicy> for AdminGraphqlRetryPolicy {
    fn from(value: StoredRetryPolicy) -> Self {
        Self {
            enabled: value.enabled,
            max_channel_retries: value.max_channel_retries,
            max_single_channel_retries: value.max_single_channel_retries,
            retry_delay_ms: value.retry_delay_ms,
            load_balancer_strategy: value.load_balancer_strategy,
            auto_disable_channel: AdminGraphqlAutoDisableChannel {
                enabled: value.auto_disable_channel.enabled,
                statuses: value
                    .auto_disable_channel
                    .statuses
                    .into_iter()
                    .map(|status| AdminGraphqlAutoDisableChannelStatus {
                        status: status.status,
                        times: status.times,
                    })
                    .collect(),
            },
        }
    }
}

impl From<StoredAutoBackupSettings> for AdminGraphqlAutoBackupSettings {
    fn from(value: StoredAutoBackupSettings) -> Self {
        Self {
            enabled: value.enabled,
            frequency: value.frequency,
            data_storage_id: i64_to_i32(value.data_storage_id),
            include_channels: value.include_channels,
            include_models: value.include_models,
            include_api_keys: value.include_api_keys,
            include_model_prices: value.include_model_prices,
            retention_days: value.retention_days,
            last_backup_at: value.last_backup_at.map(format_unix_timestamp),
            last_backup_error: if value.last_backup_error.trim().is_empty() {
                None
            } else {
                Some(value.last_backup_error)
            },
        }
    }
}

impl From<StoredVideoStorageSettings> for AdminGraphqlVideoStorageSettings {
    fn from(value: StoredVideoStorageSettings) -> Self {
        Self {
            enabled: value.enabled,
            data_storage_id: i64_to_i32(value.data_storage_id),
            scan_interval_minutes: value.scan_interval_minutes,
            scan_limit: value.scan_limit,
        }
    }
}

impl From<StoredSystemChannelSettings> for AdminGraphqlSystemChannelSettings {
    fn from(value: StoredSystemChannelSettings) -> Self {
        Self {
            probe: AdminGraphqlChannelProbeSetting {
                enabled: value.probe.enabled,
                frequency: value.probe.frequency,
            },
            auto_sync: AdminGraphqlChannelModelAutoSyncSetting {
                frequency: value.auto_sync.frequency,
            },
        }
    }
}

impl From<StoredSystemModelSettings> for AdminGraphqlSystemModelSettings {
    fn from(value: StoredSystemModelSettings) -> Self {
        Self {
            fallback_to_channels_on_model_not_found: value.fallback_to_channels_on_model_not_found,
            query_all_channel_models: value.query_all_channel_models,
        }
    }
}

impl From<StoredChannelProbeData> for AdminGraphqlChannelProbeData {
    fn from(value: StoredChannelProbeData) -> Self {
        Self {
            channel_id: graphql_gid("channel", value.channel_id),
            points: value
                .points
                .into_iter()
                .map(|point| AdminGraphqlChannelProbePoint {
                    timestamp: point.timestamp,
                    total_request_count: point.total_request_count,
                    success_request_count: point.success_request_count,
                    avg_tokens_per_second: point.avg_tokens_per_second,
                    avg_time_to_first_token_ms: point.avg_time_to_first_token_ms,
                })
                .collect(),
        }
    }
}

impl From<StoredProviderQuotaStatus> for AdminGraphqlProviderQuotaStatus {
    fn from(value: StoredProviderQuotaStatus) -> Self {
        let quota_data = serde_json::from_str::<Value>(value.quota_data_json.as_str()).unwrap_or(Value::Null);
        let message = quota_data
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| quota_data.get("error").and_then(Value::as_str))
            .map(ToOwned::to_owned);
        Self {
            id: graphql_gid("provider_quota_status", value.id),
            channel_id: graphql_gid("channel", value.channel_id),
            provider_type: value.provider_type,
            status: value.status,
            ready: value.ready,
            next_reset_at: value.next_reset_at.map(format_unix_timestamp),
            next_check_at: format_unix_timestamp(value.next_check_at),
            message,
        }
    }
}

impl From<StoredCircuitBreakerStatus> for AdminGraphqlCircuitBreakerStatus {
    fn from(value: StoredCircuitBreakerStatus) -> Self {
        Self {
            channel_id: graphql_gid("channel", value.channel_id),
            model_id: value.model_id,
            state: value.state,
            consecutive_failures: value.consecutive_failures,
            next_probe_at_seconds: value.next_probe_at_seconds,
        }
    }
}

pub(crate) fn stored_circuit_breaker_status(
    channel_id: i64,
    circuit_breaker: &SharedCircuitBreaker,
) -> Option<StoredCircuitBreakerStatus> {
    let snapshot = circuit_breaker.channel_status(channel_id).active?;
    Some(StoredCircuitBreakerStatus {
        channel_id: snapshot.channel_id,
        model_id: snapshot.model_id,
        state: snapshot.state.as_str().to_owned(),
        consecutive_failures: i32::try_from(snapshot.consecutive_failures).unwrap_or(i32::MAX),
        next_probe_at_seconds: snapshot.next_probe_in_seconds,
    })
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateMeInput")]
pub(crate) struct AdminGraphqlUpdateMeInput {
    pub(crate) first_name: Option<String>,
    pub(crate) last_name: Option<String>,
    pub(crate) prefer_language: Option<String>,
    pub(crate) avatar: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "PageInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlPageInfo {
    pub(crate) has_next_page: bool,
    pub(crate) has_previous_page: bool,
    pub(crate) start_cursor: Option<String>,
    pub(crate) end_cursor: Option<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RoleInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRoleInfo {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RoleEdge", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRoleEdge {
    pub(crate) cursor: Option<String>,
    pub(crate) node: Option<AdminGraphqlRoleInfo>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "RoleConnection", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlRoleConnection {
    pub(crate) edges: Vec<AdminGraphqlRoleEdge>,
    pub(crate) page_info: AdminGraphqlPageInfo,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserProjectInfo")]
pub(crate) struct AdminGraphqlUserProjectInfo {
    #[graphql(name = "projectID")]
    pub(crate) project_id: String,
    #[graphql(name = "isOwner")]
    pub(crate) is_owner: bool,
    pub(crate) scopes: Vec<String>,
    pub(crate) roles: Vec<AdminGraphqlRoleInfo>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserInfo {
    pub(crate) id: String,
    pub(crate) email: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) is_owner: bool,
    pub(crate) prefer_language: String,
    pub(crate) avatar: Option<String>,
    pub(crate) scopes: Vec<String>,
    pub(crate) roles: Vec<AdminGraphqlRoleInfo>,
    pub(crate) projects: Vec<AdminGraphqlUserProjectInfo>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "User", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUser {
    pub(crate) id: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) email: String,
    pub(crate) status: String,
    pub(crate) first_name: String,
    pub(crate) last_name: String,
    pub(crate) is_owner: bool,
    pub(crate) prefer_language: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) roles: AdminGraphqlRoleConnection,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserEdge", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserEdge {
    pub(crate) cursor: Option<String>,
    pub(crate) node: Option<AdminGraphqlUser>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "UserConnection", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlUserConnection {
    pub(crate) edges: Vec<AdminGraphqlUserEdge>,
    pub(crate) page_info: AdminGraphqlPageInfo,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProjectEdge", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProjectEdge {
    pub(crate) cursor: Option<String>,
    pub(crate) node: Option<AdminGraphqlProject>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ProjectConnection", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlProjectConnection {
    pub(crate) edges: Vec<AdminGraphqlProjectEdge>,
    pub(crate) page_info: AdminGraphqlPageInfo,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ScopeInfo", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlScopeInfo {
    pub(crate) scope: String,
    pub(crate) description: String,
    pub(crate) levels: Vec<String>,
}

fn admin_graphql_scope_info_list() -> Vec<AdminGraphqlScopeInfo> {
    vec![
        AdminGraphqlScopeInfo {
            scope: "read_settings".to_string(),
            description: "Read system settings".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "write_settings".to_string(),
            description: "Write system settings".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "read_channels".to_string(),
            description: "Read channel configurations".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "write_channels".to_string(),
            description: "Write channel configurations".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "read_requests".to_string(),
            description: "Read request data".to_string(),
            levels: vec!["system".to_string(), "project".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "write_requests".to_string(),
            description: "Write request data".to_string(),
            levels: vec!["system".to_string(), "project".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "read_users".to_string(),
            description: "Read user data".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "write_users".to_string(),
            description: "Write user data".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "read_api_keys".to_string(),
            description: "Read API keys".to_string(),
            levels: vec!["system".to_string()],
        },
        AdminGraphqlScopeInfo {
            scope: "write_api_keys".to_string(),
            description: "Write API keys".to_string(),
            levels: vec!["system".to_string()],
        },
    ]
}

fn filter_admin_graphql_scope_info(level: Option<&str>) -> Result<Vec<AdminGraphqlScopeInfo>, String> {
    let all_scopes = admin_graphql_scope_info_list();

    match level {
        Some("system") => Ok(all_scopes
            .into_iter()
            .filter(|scope| scope.levels.iter().any(|value| value == "system"))
            .collect()),
        Some("project") => Ok(all_scopes
            .into_iter()
            .filter(|scope| scope.levels.iter().any(|value| value == "project"))
            .collect()),
        Some(invalid) => Err(format!("invalid level: {}", invalid)),
        None => Ok(all_scopes),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Enum)]
#[serde(rename_all = "lowercase")]
#[graphql(rename_items = "lowercase")]
pub(crate) enum UserStatus {
    Activated,
    Deactivated,
}

#[derive(Debug, Clone, InputObject)]
#[graphql(name = "QueryModelsInput")]
pub(crate) struct AdminGraphqlQueryModelsInput {
    pub(crate) first: Option<i32>,
    pub(crate) last: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateUserInput")]
pub(crate) struct AdminGraphqlCreateUserInput {
    pub(crate) email: String,
    pub(crate) status: Option<UserStatus>,
    pub(crate) prefer_language: Option<String>,
    pub(crate) password: String,
    pub(crate) first_name: Option<String>,
    pub(crate) last_name: Option<String>,
    pub(crate) avatar: Option<String>,
    pub(crate) is_owner: Option<bool>,
    pub(crate) scopes: Option<Vec<String>>,
    #[serde(rename = "projectIDs")]
    #[graphql(name = "projectIDs")]
    pub(crate) project_ids: Option<Vec<String>>,
    #[serde(rename = "roleIDs")]
    #[graphql(name = "roleIDs")]
    pub(crate) role_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateUserInput")]
pub(crate) struct AdminGraphqlUpdateUserInput {
    #[graphql(name = "firstName")]
    pub(crate) first_name: Option<String>,
    #[graphql(name = "lastName")]
    pub(crate) last_name: Option<String>,
    #[graphql(name = "preferLanguage")]
    pub(crate) prefer_language: Option<String>,
    #[graphql(name = "avatar")]
    pub(crate) avatar: Option<String>,
    #[graphql(name = "password")]
    pub(crate) password: Option<String>,
    #[graphql(name = "scopes")]
    pub(crate) scopes: Option<Vec<String>>,
    #[serde(rename = "roleIDs")]
    #[graphql(name = "roleIDs")]
    pub(crate) role_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateProjectInput")]
pub(crate) struct AdminGraphqlCreateProjectInput {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateProjectInput")]
pub(crate) struct AdminGraphqlUpdateProjectInput {
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateRoleInput")]
pub(crate) struct AdminGraphqlCreateRoleInput {
    pub(crate) name: String,
    pub(crate) level: Option<String>,
    #[serde(rename = "projectID")]
    #[graphql(name = "projectID")]
    pub(crate) project_id: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateRoleInput")]
pub(crate) struct AdminGraphqlUpdateRoleInput {
    pub(crate) name: Option<String>,
    pub(crate) level: Option<String>,
    #[serde(rename = "projectID")]
    #[graphql(name = "projectID")]
    pub(crate) project_id: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateAPIKeyInput")]
pub(crate) struct AdminGraphqlCreateApiKeyInput {
    #[serde(rename = "projectID")]
    #[graphql(name = "projectID")]
    pub(crate) project_id: String,
    pub(crate) key: Option<String>,
    pub(crate) name: String,
    #[serde(rename = "keyType")]
    #[graphql(name = "keyType")]
    pub(crate) key_type: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
    #[serde(rename = "profilesJSON")]
    #[graphql(name = "profilesJSON")]
    pub(crate) profiles_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateAPIKeyInput")]
pub(crate) struct AdminGraphqlUpdateApiKeyInput {
    pub(crate) name: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateChannelInput")]
pub(crate) struct AdminGraphqlCreateChannelInput {
    pub(crate) name: String,
    #[serde(rename = "channelType")]
    #[graphql(name = "channelType")]
    pub(crate) channel_type: String,
    #[serde(rename = "baseURL")]
    #[graphql(name = "baseURL")]
    pub(crate) base_url: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(rename = "credentialsJSON")]
    #[graphql(name = "credentialsJSON")]
    pub(crate) credentials_json: Option<String>,
    #[serde(rename = "supportedModels")]
    #[graphql(name = "supportedModels")]
    pub(crate) supported_models: Option<Vec<String>>,
    #[serde(rename = "autoSyncSupportedModels")]
    #[graphql(name = "autoSyncSupportedModels")]
    pub(crate) auto_sync_supported_models: Option<bool>,
    #[serde(rename = "defaultTestModel")]
    #[graphql(name = "defaultTestModel")]
    pub(crate) default_test_model: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    #[serde(rename = "orderingWeight")]
    #[graphql(name = "orderingWeight")]
    pub(crate) ordering_weight: Option<i32>,
    #[serde(rename = "errorMessage")]
    #[graphql(name = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateChannelInput")]
pub(crate) struct AdminGraphqlUpdateChannelInput {
    pub(crate) name: Option<String>,
    #[serde(rename = "baseURL")]
    #[graphql(name = "baseURL")]
    pub(crate) base_url: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(rename = "credentialsJSON")]
    #[graphql(name = "credentialsJSON")]
    pub(crate) credentials_json: Option<String>,
    #[serde(rename = "supportedModels")]
    #[graphql(name = "supportedModels")]
    pub(crate) supported_models: Option<Vec<String>>,
    #[serde(rename = "autoSyncSupportedModels")]
    #[graphql(name = "autoSyncSupportedModels")]
    pub(crate) auto_sync_supported_models: Option<bool>,
    #[serde(rename = "defaultTestModel")]
    #[graphql(name = "defaultTestModel")]
    pub(crate) default_test_model: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    #[serde(rename = "orderingWeight")]
    #[graphql(name = "orderingWeight")]
    pub(crate) ordering_weight: Option<i32>,
    #[serde(rename = "errorMessage")]
    #[graphql(name = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "CreateModelInput")]
pub(crate) struct AdminGraphqlCreateModelInput {
    pub(crate) developer: String,
    #[serde(rename = "modelID")]
    #[graphql(name = "modelID")]
    pub(crate) model_id: String,
    #[serde(rename = "modelType")]
    #[graphql(name = "modelType")]
    pub(crate) model_type: String,
    pub(crate) name: String,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(rename = "modelCardJSON")]
    #[graphql(name = "modelCardJSON")]
    pub(crate) model_card_json: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "UpdateModelInput")]
pub(crate) struct AdminGraphqlUpdateModelInput {
    pub(crate) name: Option<String>,
    pub(crate) icon: Option<String>,
    pub(crate) group: Option<String>,
    #[serde(rename = "modelCardJSON")]
    #[graphql(name = "modelCardJSON")]
    pub(crate) model_card_json: Option<String>,
    #[serde(rename = "settingsJSON")]
    #[graphql(name = "settingsJSON")]
    pub(crate) settings_json: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "BackupInput")]
pub(crate) struct AdminGraphqlBackupInput {
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
    #[serde(rename = "includeAPIKeys")]
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: Option<bool>,
    pub(crate) include_model_prices: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, InputObject)]
#[serde(rename_all = "camelCase")]
#[graphql(name = "RestoreInput")]
pub(crate) struct AdminGraphqlRestoreInput {
    pub(crate) payload: String,
    pub(crate) include_channels: Option<bool>,
    pub(crate) include_models: Option<bool>,
    #[serde(rename = "includeAPIKeys")]
    #[graphql(name = "includeAPIKeys")]
    pub(crate) include_api_keys: Option<bool>,
    pub(crate) include_model_prices: Option<bool>,
    pub(crate) overwrite_existing: Option<bool>,
}

fn stored_prompt_protection_settings_from_input(
    input: AdminGraphqlPromptProtectionSettingsInput,
) -> StoredPromptProtectionSettings {
    StoredPromptProtectionSettings {
        action: match input.action {
            AdminGraphqlPromptProtectionAction::Mask => PromptProtectionAction::Mask,
            AdminGraphqlPromptProtectionAction::Reject => PromptProtectionAction::Reject,
        },
        replacement: input.replacement,
        scopes: input
            .scopes
            .unwrap_or_default()
            .into_iter()
            .map(|scope| match scope {
                AdminGraphqlPromptProtectionScope::System => PromptProtectionScope::System,
                AdminGraphqlPromptProtectionScope::Developer => PromptProtectionScope::Developer,
                AdminGraphqlPromptProtectionScope::User => PromptProtectionScope::User,
                AdminGraphqlPromptProtectionScope::Assistant => PromptProtectionScope::Assistant,
                AdminGraphqlPromptProtectionScope::Tool => PromptProtectionScope::Tool,
            })
            .collect(),
    }
}

fn stored_prompt_settings_from_input(input: AdminGraphqlPromptSettingsInput) -> Result<StoredPromptSettings, String> {
    Ok(StoredPromptSettings {
        action: StoredPromptAction {
            type_field: normalize_prompt_action_type(input.action.type_field.as_str())?.to_owned(),
        },
        conditions: input
            .conditions
            .unwrap_or_default()
            .into_iter()
            .map(|composite| {
                composite
                    .conditions
                    .into_iter()
                    .map(|condition| {
                        Ok(StoredPromptActivationCondition {
                            type_field: normalize_prompt_condition_type(condition.type_field.as_str())?.to_owned(),
                            model_id: condition.model_id,
                            model_pattern: condition.model_pattern,
                            api_key_id: condition.api_key_id,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()
                    .map(|conditions| StoredPromptActivationConditionComposite { conditions })
            })
            .collect::<Result<Vec<_>, String>>()?,
    })
}

fn prompt_protection_rule_status_name(status: PromptProtectionRuleStatus) -> &'static str {
    match status {
        PromptProtectionRuleStatus::Enabled => "enabled",
        PromptProtectionRuleStatus::Disabled => "disabled",
        PromptProtectionRuleStatus::Archived => "archived",
    }
}

fn map_prompt_protection_openai_error(error: axonhub_http::OpenAiV1Error) -> String {
    match error {
        axonhub_http::OpenAiV1Error::Internal { message }
        | axonhub_http::OpenAiV1Error::InvalidRequest { message } => message,
        axonhub_http::OpenAiV1Error::Upstream { .. } => "unexpected upstream error".to_owned(),
    }
}

fn parse_graphql_prompt_protection_rule_ids(variables: &Value) -> Result<Vec<i64>, String> {
    let ids = variables
        .get("ids")
        .cloned()
        .ok_or_else(|| "ids are required".to_owned())
        .and_then(|value| {
            serde_json::from_value::<Vec<String>>(value)
                .map_err(|error| format!("invalid ids: {error}"))
        })?;
    parse_graphql_id_list(Some(ids.clone()), "promptProtectionRule")
        .or_else(|_| parse_graphql_id_list(Some(ids), "prompt_protection_rule"))
}

fn parse_prompt_id_from_variables(variables: &Value) -> Result<i64, String> {
    let id = variables
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "prompt id is required".to_owned())?;
    parse_graphql_resource_id(id, "prompt").map_err(|_| "invalid prompt id".to_owned())
}

fn parse_prompt_ids(variables: &Value) -> Result<Vec<i64>, String> {
    let ids = variables
        .get("ids")
        .cloned()
        .ok_or_else(|| "ids are required".to_owned())
        .and_then(|value| serde_json::from_value::<Vec<String>>(value).map_err(|error| format!("invalid ids: {error}")))?;
    parse_graphql_id_list(Some(ids), "prompt")
}

fn ensure_prompts_in_project(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    ids: &[i64],
    project_id: i64,
    field: &str,
) -> Result<(), String> {
    for id in ids {
        let prompt = repository.query_prompt(*id)?.ok_or_else(|| "prompt not found".to_owned())?;
        if prompt.project_id != project_id {
            return graphql_field_error(field, "prompt not found").map(|_| ()).and(Err("prompt not found".to_owned()));
        }
    }
    Ok(())
}

fn normalize_prompt_role(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("system").trim().to_ascii_lowercase().as_str() {
        "system" => Ok("system"),
        "developer" => Ok("developer"),
        "user" => Ok("user"),
        "assistant" => Ok("assistant"),
        "tool" => Ok("tool"),
        _ => Err("invalid prompt role".to_owned()),
    }
}

fn normalize_prompt_action_type(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "prepend" => Ok("prepend"),
        "append" => Ok("append"),
        _ => Err("invalid prompt action type".to_owned()),
    }
}

fn normalize_prompt_condition_type(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "model_id" => Ok("model_id"),
        "model_pattern" => Ok("model_pattern"),
        "api_key" => Ok("api_key"),
        _ => Err("invalid prompt condition type".to_owned()),
    }
}

fn persist_onboarding_record(
    repository: &SeaOrmAdminGraphqlSubsetRepository,
    record: &crate::foundation::request_context::OnboardingRecord,
) -> Result<(), String> {
    let encoded = crate::foundation::request_context::serialize_onboarding_record(record)
        .map_err(|error| format!("failed to serialize onboarding record: {error}"))?;
    repository.upsert_onboarding_record(encoded.as_str())
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "ModelIdentityWithStatus", rename_fields = "camelCase")]
pub(crate) struct AdminGraphqlModelIdentityWithStatus {
    pub(crate) id: String,
    pub(crate) status: String,
}

fn project_json(project: &GraphqlProjectRecord) -> Value {
    json!({
        "id": graphql_gid("project", project.id),
        "createdAt": project.created_at,
        "updatedAt": project.updated_at,
        "name": project.name,
        "description": project.description,
        "status": project.status,
    })
}

fn role_json(role: &GraphqlRoleRecord) -> Value {
    json!({
        "id": graphql_gid("role", role.id),
        "name": role.name,
        "level": role.level,
        "projectID": graphql_gid("project", role.project_id.max(1)),
        "scopes": parse_scope_json(role.scopes.as_str()),
    })
}

fn api_key_json(api_key: &GraphqlApiKeyRecord) -> Value {
    json!({
        "id": graphql_gid("api_key", api_key.id),
        "key": api_key.key,
        "name": api_key.name,
        "type": api_key.key_type,
        "status": api_key.status,
        "scopes": parse_scope_json(api_key.scopes.as_str()),
    })
}

fn admin_graphql_channel_from_record(channel: &GraphqlChannelRecord) -> AdminGraphqlChannel {
    AdminGraphqlChannel {
        id: graphql_gid("channel", channel.id),
        name: channel.name.clone(),
        channel_type: channel.channel_type.clone(),
        base_url: channel.base_url.clone(),
        status: channel.status.clone(),
        supported_models: serde_json::from_str(&channel.supported_models).unwrap_or_default(),
        ordering_weight: channel.ordering_weight,
        provider_quota_status: None,
        circuit_breaker_status: None,
    }
}

fn admin_graphql_model_from_record(model: &GraphqlModelRecord) -> AdminGraphqlModel {
    let parsed = parse_model_card(model.model_card_json.as_str());
    AdminGraphqlModel {
        id: graphql_gid("model", model.id),
        developer: model.developer.clone(),
        model_id: model.model_id.clone(),
        model_type: model.model_type.clone(),
        name: model.name.clone(),
        icon: model.icon.clone(),
        remark: model.remark.clone(),
        context_length: parsed.context_length.map(i64_to_i32),
        max_output_tokens: parsed.max_output_tokens.map(i64_to_i32),
    }
}


fn channel_json(channel: &AdminGraphqlChannel) -> Value {
    json!({
        "id": channel.id,
        "name": channel.name,
        "channelType": channel.channel_type,
        "baseURL": channel.base_url,
        "status": channel.status,
        "supportedModels": channel.supported_models,
        "orderingWeight": channel.ordering_weight,
        "providerQuotaStatus": channel.provider_quota_status.as_ref().map(provider_quota_status_json),
        "circuitBreakerStatus": channel.circuit_breaker_status.as_ref().map(circuit_breaker_status_json),
    })
}

fn provider_quota_status_json(status: &AdminGraphqlProviderQuotaStatus) -> Value {
    json!({
        "id": status.id,
        "channelID": status.channel_id,
        "providerType": status.provider_type,
        "status": status.status,
        "ready": status.ready,
        "nextResetAt": status.next_reset_at,
        "nextCheckAt": status.next_check_at,
        "message": status.message,
    })
}

fn circuit_breaker_status_json(status: &AdminGraphqlCircuitBreakerStatus) -> Value {
    json!({
        "channelID": status.channel_id,
        "modelID": status.model_id,
        "state": status.state,
        "consecutiveFailures": status.consecutive_failures,
        "nextProbeAtSeconds": status.next_probe_at_seconds,
    })
}

fn model_json(model: &AdminGraphqlModel) -> Value {
    json!({
        "id": model.id,
        "developer": model.developer,
        "modelID": model.model_id,
        "modelType": model.model_type,
        "name": model.name,
        "icon": model.icon,
        "remark": model.remark,
        "contextLength": model.context_length,
        "maxOutputTokens": model.max_output_tokens,
    })
}

fn prompt_json(prompt: &AdminGraphqlPrompt) -> Value {
    json!({
        "id": prompt.id,
        "createdAt": prompt.created_at,
        "updatedAt": prompt.updated_at,
        "projectID": prompt.project_id,
        "name": prompt.name,
        "description": prompt.description,
        "role": prompt.role,
        "content": prompt.content,
        "status": prompt.status,
        "order": prompt.order,
        "settings": {
            "action": {
                "type": prompt.settings.action.type_field,
            },
            "conditions": prompt.settings.conditions.iter().map(|composite| {
                json!({
                    "conditions": composite.conditions.iter().map(|condition| {
                        json!({
                            "type": condition.type_field,
                            "modelId": condition.model_id,
                            "modelPattern": condition.model_pattern,
                            "apiKeyId": condition.api_key_id,
                        })
                    }).collect::<Vec<_>>()
                })
            }).collect::<Vec<_>>()
        }
    })
}

fn admin_graphql_prompt_from_record(record: GraphqlPromptRecord) -> Result<AdminGraphqlPrompt, String> {
    let settings = serde_json::from_str::<StoredPromptSettings>(record.settings.as_str())
        .map_err(|error| format!("failed to decode prompt settings: {error}"))?;
    Ok(AdminGraphqlPrompt {
        id: graphql_gid("prompt", record.id),
        created_at: record.created_at,
        updated_at: record.updated_at,
        project_id: graphql_gid("project", record.project_id),
        name: record.name,
        description: record.description,
        role: record.role,
        content: record.content,
        status: record.status,
        order: record.order,
        settings: AdminGraphqlPromptSettings {
            action: AdminGraphqlPromptAction {
                type_field: settings.action.type_field,
            },
            conditions: settings
                .conditions
                .into_iter()
                .map(|composite| AdminGraphqlPromptActivationConditionComposite {
                    conditions: composite
                        .conditions
                        .into_iter()
                        .map(|condition| AdminGraphqlPromptActivationCondition {
                            type_field: condition.type_field,
                            model_id: condition.model_id,
                            model_pattern: condition.model_pattern,
                            api_key_id: condition.api_key_id,
                        })
                        .collect(),
                })
                .collect(),
        },
    })
}

fn normalize_project_status(status: Option<&str>) -> Result<&'static str, String> {
    match status.unwrap_or("active").trim().to_ascii_lowercase().as_str() {
        "active" => Ok("active"),
        "disabled" => Ok("disabled"),
        "archived" => Ok("archived"),
        _ => Err("invalid project status".to_owned()),
    }
}

fn normalize_enable_status(status: Option<&str>, resource: &str) -> Result<&'static str, String> {
    match status.unwrap_or("enabled").trim().to_ascii_lowercase().as_str() {
        "enabled" => Ok("enabled"),
        "disabled" => Ok("disabled"),
        "archived" => Ok("archived"),
        _ => Err(format!("invalid {resource} status")),
    }
}

fn normalize_role_level(level: Option<&str>) -> Result<&'static str, String> {
    match level.unwrap_or("system").trim().to_ascii_lowercase().as_str() {
        "system" => Ok("system"),
        "project" => Ok("project"),
        _ => Err("invalid role level".to_owned()),
    }
}

fn normalize_api_key_type(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("user").trim().to_ascii_lowercase().as_str() {
        "user" => Ok("user"),
        "service_account" => Ok("service_account"),
        "noauth" => Ok("noauth"),
        _ => Err("invalid api key type".to_owned()),
    }
}

fn normalize_api_key_status(value: Option<&str>) -> Result<&'static str, String> {
    match value.unwrap_or("enabled").trim().to_ascii_lowercase().as_str() {
        "enabled" => Ok("enabled"),
        "disabled" => Ok("disabled"),
        "archived" => Ok("archived"),
        _ => Err("invalid api key status".to_owned()),
    }
}

fn normalize_json_blob(value: Option<&str>) -> Result<String, String> {
    let raw = value.unwrap_or("{}").trim();
    if raw.is_empty() {
        return Ok("{}".to_owned());
    }
    let parsed = serde_json::from_str::<Value>(raw).map_err(|error| format!("invalid json payload: {error}"))?;
    serde_json::to_string(&parsed).map_err(|error| format!("failed to normalize json payload: {error}"))
}

fn parse_role_project_id(value: Option<&str>, level: &str) -> Result<i64, String> {
    if level == "system" {
        return Ok(0);
    }
    let value = value.ok_or_else(|| "projectID is required for project roles".to_owned())?;
    parse_graphql_resource_id(value, "project").map_err(|_| "invalid project id".to_owned())
}

fn authorize_role_write(
    user: &AuthUserContext,
    level: &str,
    project_id: i64,
    field: &str,
) -> Result<(), String> {
    if level == "system" {
        if authorize_user_system_scope(user, SCOPE_WRITE_ROLES).is_err() {
            graphql_permission_denied(field)?;
            return Err("permission denied".to_owned());
        }
        return Ok(());
    }

    if require_user_project_scope(user, project_id, SCOPE_WRITE_ROLES).is_err() {
        graphql_permission_denied(field)?;
        return Err("permission denied".to_owned());
    }
    Ok(())
}

fn validate_scope_list(scopes: &[String], field: &str) -> Result<(), String> {
    for scope in scopes {
        if !is_valid_scope(scope.as_str()) {
            graphql_field_error(field, format!("invalid scope: {scope}"))?;
            return Err(format!("invalid scope: {scope}"));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod sqlite_test_support {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use async_graphql::{
        Context, EmptySubscription, InputObject, Object, Request as AsyncGraphqlRequest, Schema,
        Variables,
    };
    use axonhub_db_entity::models;
    use axonhub_db_entity::traces;
    use axonhub_db_entity::{projects, roles, user_projects, user_roles, users};
    use axonhub_http::{
        AdminGraphqlPort, AuthApiKeyContext, AuthUserContext, GraphqlExecutionResult,
        GraphqlRequestPayload, OpenApiGraphqlPort, TraceContext,
    };
    use sea_orm::{
        ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait,
        QueryFilter, QueryOrder,
    };
    use sea_orm::{QuerySelect, RelationTrait};

    use super::*;
    use super::super::{
        admin::{parse_graphql_resource_id, StoredProxyPreset},
        admin_operational::sqlite_test_support::SqliteOperationalService,
        authz::{
            authorize_user_system_scope, require_owner_bypass,
            require_service_api_key_write_access, require_user_project_scope, scope_strings,
            serialize_scope_slugs, ScopeSlug, LLM_API_KEY_SCOPES, SCOPE_READ_CHANNELS,
            SCOPE_READ_REQUESTS, SCOPE_READ_ROLES, SCOPE_READ_SETTINGS, SCOPE_READ_USERS,
            SCOPE_WRITE_SETTINGS, SCOPE_WRITE_USERS,
        },
        circuit_breaker::{CircuitBreakerPolicy, SharedCircuitBreaker},
        shared::graphql_gid,
        system::{
            hash_password,
            sqlite_test_support::{ensure_identity_tables, SqliteFoundation},
        },
    };

    pub(crate) struct SqliteAdminGraphqlService {
        pub(crate) schema: Arc<AdminGraphqlSchema>,
    }

    pub(crate) struct SqliteOpenApiGraphqlService {
        pub(crate) schema: Arc<OpenApiGraphqlSchema>,
    }

    pub(crate) type AdminGraphqlSchema =
        Schema<AdminGraphqlQueryRoot, AdminGraphqlMutationRoot, EmptySubscription>;
    pub(crate) type OpenApiGraphqlSchema =
        Schema<OpenApiGraphqlQueryRoot, OpenApiGraphqlMutationRoot, EmptySubscription>;

    #[derive(Clone)]
    pub(crate) struct AdminGraphqlRequestContext {
        pub(crate) project_id: Option<i64>,
        pub(crate) user: AuthUserContext,
    }

    #[derive(Clone)]
    pub(crate) struct OpenApiGraphqlRequestContext {
        pub(crate) owner_api_key: AuthApiKeyContext,
    }

    #[derive(Clone)]
    pub(crate) struct AdminGraphqlQueryRoot {
        pub(crate) foundation: Arc<SqliteFoundation>,
        pub(crate) operational: Arc<SqliteOperationalService>,
        pub(crate) circuit_breaker: SharedCircuitBreaker,
    }

    #[derive(Clone)]
    pub(crate) struct AdminGraphqlMutationRoot {
        pub(crate) operational: Arc<SqliteOperationalService>,
    }

    #[derive(Clone)]
    pub(crate) struct OpenApiGraphqlQueryRoot;

    #[derive(Clone)]
    pub(crate) struct OpenApiGraphqlMutationRoot {
        pub(crate) foundation: Arc<SqliteFoundation>,
    }

    #[derive(Debug, Clone, InputObject)]
    #[graphql(name = "UpdateUserAgentPassThroughSettingsInput")]
    pub(crate) struct AdminGraphqlUpdateUserAgentPassThroughSettingsInput {
        pub(crate) enabled: bool,
    }

    impl SqliteAdminGraphqlService {
        pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
            let operational = Arc::new(SqliteOperationalService::new(foundation.clone()));
            let circuit_breaker = SharedCircuitBreaker::new(CircuitBreakerPolicy::default());
            let schema = Schema::build(
                AdminGraphqlQueryRoot {
                    foundation,
                    operational: operational.clone(),
                    circuit_breaker,
                },
                AdminGraphqlMutationRoot { operational },
                EmptySubscription,
            )
            .finish();

            Self {
                schema: Arc::new(schema),
            }
        }
    }

    impl SqliteOpenApiGraphqlService {
        pub fn new(foundation: Arc<SqliteFoundation>) -> Self {
            let schema = Schema::build(
                OpenApiGraphqlQueryRoot,
                OpenApiGraphqlMutationRoot { foundation },
                EmptySubscription,
            )
            .finish();

            Self {
                schema: Arc::new(schema),
            }
        }
    }

    impl AdminGraphqlPort for SqliteAdminGraphqlService {
        fn execute_graphql(
            &self,
            request: GraphqlRequestPayload,
            project_id: Option<i64>,
            user: AuthUserContext,
        ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
            let schema = Arc::clone(&self.schema);
            Box::pin(async move {
                execute_graphql_schema(
                    schema,
                    request,
                    AdminGraphqlRequestContext { project_id, user },
                )
                .await
            })
        }
    }

    impl OpenApiGraphqlPort for SqliteOpenApiGraphqlService {
        fn execute_graphql(
            &self,
            request: GraphqlRequestPayload,
            owner_api_key: AuthApiKeyContext,
        ) -> Pin<Box<dyn Future<Output = GraphqlExecutionResult> + Send>> {
            let schema = Arc::clone(&self.schema);
            Box::pin(async move {
                execute_graphql_schema(schema, request, OpenApiGraphqlRequestContext { owner_api_key })
                    .await
            })
        }
    }

    async fn execute_graphql_schema<Query, Mutation, Data>(
        schema: Arc<Schema<Query, Mutation, EmptySubscription>>,
        payload: GraphqlRequestPayload,
        context_data: Data,
    ) -> GraphqlExecutionResult
    where
        Query: async_graphql::ObjectType + Send + Sync + 'static,
        Mutation: async_graphql::ObjectType + Send + Sync + 'static,
        Data: Send + Sync + Clone + 'static,
    {
        let mut request = AsyncGraphqlRequest::new(payload.query)
            .variables(Variables::from_json(payload.variables))
            .data(context_data);
        if let Some(operation_name) = payload
            .operation_name
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        {
            request = request.operation_name(operation_name);
        }

        let response = schema.execute(request).await;
        let body = serde_json::to_value(response).unwrap_or_else(|error| {
            serde_json::json!({
                "data": null,
                "errors": [{"message": format!("Failed to serialize GraphQL response: {error}")}] 
            })
        });

        GraphqlExecutionResult { status: 200, body }
    }

    async fn list_traces_by_project_graphql(
        db: &impl ConnectionTrait,
        project_id: i64,
    ) -> Result<Vec<TraceContext>, String> {
        traces::Entity::find()
            .filter(traces::Column::ProjectId.eq(project_id))
            .order_by_desc(traces::Column::Id)
            .into_partial_model::<traces::ResolveContext>()
            .all(db)
            .await
            .map_err(|error| error.to_string())
            .map(|rows| {
                rows.into_iter()
                    .map(|row| TraceContext {
                        id: row.id,
                        trace_id: row.trace_id,
                        project_id: row.project_id,
                        thread_id: row.thread_id,
                    })
                    .collect()
            })
    }

    #[Object]
    impl AdminGraphqlQueryRoot {
        async fn system_status(
            &self,
            _ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlSystemStatus> {
            let is_initialized = self
                .foundation
                .system_settings()
                .is_initialized()
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to check system status: {error}"))
                })?;

            Ok(AdminGraphqlSystemStatus { is_initialized })
        }

        async fn system_version(
            &self,
            _ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlSystemVersion> {
            let build = BuildInfo::current();
            let version = self
                .foundation
                .system_settings()
                .value(crate::foundation::shared::SYSTEM_KEY_VERSION)
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to read system version: {error}"))
                })?
                .unwrap_or_else(|| build.version().to_owned());

            Ok(AdminGraphqlSystemVersion {
                version,
                commit: build.commit().unwrap_or_default().to_owned(),
                build_time: build.build_time().unwrap_or_default().to_owned(),
                go_version: build
                    .go_version()
                    .unwrap_or("n/a (Rust build)")
                    .to_owned(),
                platform: build.platform().to_owned(),
                uptime: build.uptime().to_owned(),
            })
        }

        async fn onboarding_info(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlOnboardingInfo> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            let onboarding = self
                .foundation
                .system_settings()
                .value(crate::foundation::shared::SYSTEM_KEY_ONBOARDED)
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to read onboarding info: {error}"))
                })?
                .map(|raw| {
                    crate::foundation::request_context::parse_onboarding_record(&raw)
                        .map_err(|error| {
                            async_graphql::Error::new(format!("failed to parse onboarding info: {error}"))
                        })
                })
                .transpose()?
                .unwrap_or_default();

            Ok(AdminGraphqlOnboardingInfo::from(onboarding))
        }

        async fn brand_settings(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlBrandSettings> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            let brand_name = self
                .foundation
                .system_settings()
                .value(crate::foundation::shared::SYSTEM_KEY_BRAND_NAME)
            .map_err(|error| {
                async_graphql::Error::new(format!("failed to read brand settings: {error}"))
            })?;
            let brand_logo = self
                .foundation
                .system_settings()
                .value(crate::foundation::shared::SYSTEM_KEY_BRAND_LOGO)
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to read brand settings: {error}"))
                })?;

            Ok(AdminGraphqlBrandSettings {
                brand_name,
                brand_logo,
            })
        }

        async fn channels(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<Vec<AdminGraphqlChannel>> {
            require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
            let quota_by_channel = self
                .operational
                .provider_quota_statuses()
                .unwrap_or_default()
                .into_iter()
                .map(|status| (status.channel_id, AdminGraphqlProviderQuotaStatus::from(status)))
                .collect::<HashMap<_, _>>();
            let channels = self
                .foundation
                .channel_models()
                .list_channels()
                .map_err(|error| async_graphql::Error::new(format!("failed to list channels: {error}")))?;

            Ok(channels
                .into_iter()
                .map(|channel| {
                    let mut gql = AdminGraphqlChannel::from(channel.clone());
                    gql.provider_quota_status = quota_by_channel.get(&channel.id).cloned();
                    gql.circuit_breaker_status = stored_circuit_breaker_status(channel.id, &self.circuit_breaker)
                        .map(AdminGraphqlCircuitBreakerStatus::from);
                    gql
                })
                .collect())
        }

        async fn storage_policy(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlStoragePolicy> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .storage_policy()
                .map(AdminGraphqlStoragePolicy::from)
                .map_err(async_graphql::Error::new)
        }

        async fn retry_policy(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlRetryPolicy> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            let retry_policy = self
                .foundation
                .system_settings()
                .value("retry_policy")
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to read retry policy: {error}"))
                })?
                .map(|raw| {
                    serde_json::from_str::<StoredRetryPolicy>(raw.as_str()).map_err(|error| {
                        async_graphql::Error::new(format!("failed to parse retry policy: {error}"))
                    })
                })
                .transpose()?
                .unwrap_or_else(default_retry_policy);

            Ok(AdminGraphqlRetryPolicy::from(retry_policy))
        }

        async fn auto_backup_settings(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlAutoBackupSettings> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .auto_backup_settings()
                .map(AdminGraphqlAutoBackupSettings::from)
                .map_err(async_graphql::Error::new)
        }

        async fn system_channel_settings(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlSystemChannelSettings> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .system_channel_settings()
                .map(AdminGraphqlSystemChannelSettings::from)
                .map_err(async_graphql::Error::new)
        }

        async fn video_storage_settings(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlVideoStorageSettings> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .video_storage_settings()
                .map(AdminGraphqlVideoStorageSettings::from)
                .map_err(async_graphql::Error::new)
        }

        async fn system_model_settings(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlSystemModelSettings> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .system_model_settings()
                .map(AdminGraphqlSystemModelSettings::from)
                .map_err(async_graphql::Error::new)
        }

        async fn proxy_presets(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<Vec<AdminGraphqlProxyPreset>> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .proxy_presets()
                .map(|presets| {
                    presets
                        .into_iter()
                        .map(|preset| AdminGraphqlProxyPreset {
                            name: if preset.name.trim().is_empty() {
                                None
                            } else {
                                Some(preset.name)
                            },
                            url: preset.url,
                            username: if preset.username.trim().is_empty() {
                                None
                            } else {
                                Some(preset.username)
                            },
                            password: if preset.password.trim().is_empty() {
                                None
                            } else {
                                Some(preset.password)
                            },
                        })
                        .collect()
                })
                .map_err(async_graphql::Error::new)
        }

        async fn user_agent_pass_through_settings(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlUserAgentPassThroughSettings> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            self.operational
                .user_agent_pass_through()
                .map(|enabled| AdminGraphqlUserAgentPassThroughSettings { enabled })
                .map_err(async_graphql::Error::new)
        }

        async fn channel_probe_data(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlGetChannelProbeDataInput,
        ) -> async_graphql::Result<Vec<AdminGraphqlChannelProbeData>> {
            require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
            self.operational
                .channel_probe_data(&input.channel_ids)
                .map(|items| items.into_iter().map(AdminGraphqlChannelProbeData::from).collect())
                .map_err(async_graphql::Error::new)
        }

        async fn models(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<AdminGraphqlModel>> {
            require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
            let models = self
                .foundation
                .channel_models()
                .list_enabled_model_records()
                .map_err(|error| async_graphql::Error::new(format!("failed to list models: {error}")))?;

            Ok(models.into_iter().map(AdminGraphqlModel::from).collect())
        }

        async fn requests(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<Vec<AdminGraphqlRequestSummaryObject>> {
            let project_id = require_admin_graphql_project_id(ctx)?;
            require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
            let requests = self
                .foundation
                .requests()
                .list_requests_by_project(project_id)
                .map_err(|error| async_graphql::Error::new(format!("failed to list requests: {error}")))?;

            Ok(requests
                .into_iter()
                .map(AdminGraphqlRequestSummaryObject::from)
                .collect())
        }

        async fn traces(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<AdminGraphqlTrace>> {
            let project_id = require_admin_graphql_project_id(ctx)?;
            require_admin_project_scope(ctx, project_id, SCOPE_READ_REQUESTS)?;
            let db = self.foundation.seaorm();
            let traces = db
                .run_sync(move |db| async move {
                    let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                    list_traces_by_project_graphql(&connection, project_id).await
                })
                .map_err(|error| async_graphql::Error::new(format!("failed to list traces: {error}")))?;

            Ok(traces.into_iter().map(AdminGraphqlTrace::from).collect())
        }

        async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlUserInfo> {
            let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
            let user_id = request_context.user.id;
            let db = self.foundation.seaorm();
            db.run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                load_admin_graphql_user_info(&connection, user_id).await
            })
            .map_err(async_graphql::Error::new)
        }

        async fn roles(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlRoleConnection> {
            require_admin_system_scope(ctx, SCOPE_READ_ROLES)?;
            let db = self.foundation.seaorm();
            let rows = db
                .run_sync(move |db| async move {
                    let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                    roles::Entity::find()
                        .filter(roles::Column::DeletedAt.eq(0_i64))
                        .order_by_asc(roles::Column::Id)
                        .into_partial_model::<roles::GraphqlRoleSummary>()
                        .all(&connection)
                        .await
                        .map_err(|error| error.to_string())
                        .map(|rows| {
                            rows.into_iter()
                                .map(|row| AdminGraphqlRoleInfo {
                                    id: graphql_gid("role", row.id),
                                    name: row.name,
                                    scopes: parse_scopes_json(&row.scopes),
                                })
                                .collect::<Vec<_>>()
                        })
                })
                .map_err(async_graphql::Error::new)?;

            let edges = rows
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect();

            Ok(AdminGraphqlRoleConnection {
                edges,
                page_info: AdminGraphqlPageInfo {
                    has_next_page: false,
                    has_previous_page: false,
                    start_cursor: None,
                    end_cursor: None,
                },
            })
        }

        async fn all_scopes(
            &self,
            ctx: &Context<'_>,
            level: Option<String>,
        ) -> async_graphql::Result<Vec<AdminGraphqlScopeInfo>> {
            require_admin_system_scope(ctx, SCOPE_READ_SETTINGS)?;
            filter_admin_graphql_scope_info(level.as_deref()).map_err(async_graphql::Error::new)
        }

        async fn query_models(
            &self,
            ctx: &Context<'_>,
            _input: AdminGraphqlQueryModelsInput,
        ) -> async_graphql::Result<Vec<AdminGraphqlModelIdentityWithStatus>> {
            require_admin_system_scope(ctx, SCOPE_READ_CHANNELS)?;
            let db = self.foundation.seaorm();
            db.run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                models::Entity::find()
                    .filter(models::Column::DeletedAt.eq(0_i64))
                    .into_partial_model::<models::GraphqlStatus>()
                    .all(&connection)
                    .await
                    .map_err(|error| error.to_string())
                    .map(|rows| {
                        rows.into_iter()
                            .map(|row| AdminGraphqlModelIdentityWithStatus {
                                id: graphql_gid("model", row.id),
                                status: row.status,
                            })
                            .collect::<Vec<_>>()
                    })
            })
            .map_err(async_graphql::Error::new)
        }

        async fn users(&self, ctx: &Context<'_>) -> async_graphql::Result<AdminGraphqlUserConnection> {
            require_admin_system_scope(ctx, SCOPE_READ_USERS)?;
            let connection = self
                .foundation
                .open_connection(true)
                .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;

            let user_rows = connection
                .prepare(
                    "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE deleted_at = 0 ORDER BY id ASC",
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to prepare users: {error}")))?
                .query_map([], |row| {
                    let id: i64 = row.get(0)?;
                    let email: String = row.get(1)?;
                    let first_name: String = row.get(2)?;
                    let last_name: String = row.get(3)?;
                    let is_owner = row.get::<_, i64>(4)? != 0;
                    let prefer_language: String = row.get(5)?;
                    let status: String = row.get(6)?;
                    let created_at: String = row.get(7)?;
                    let updated_at: String = row.get(8)?;
                    let scopes_json: String = row.get(9)?;
                    let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
                    Ok((
                        id,
                        email,
                        first_name,
                        last_name,
                        is_owner,
                        prefer_language,
                        status,
                        created_at,
                        updated_at,
                        scopes,
                    ))
                })
                .map_err(|error| async_graphql::Error::new(format!("failed to query users: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| async_graphql::Error::new(format!("failed to parse users: {error}")))?;

            let mut edges = Vec::new();
            for (
                id,
                email,
                first_name,
                last_name,
                is_owner,
                prefer_language,
                status,
                created_at,
                updated_at,
                scopes,
            ) in user_rows.into_iter()
            {
                let roles_vec = connection
                    .prepare(
                        "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
                    )
                    .map_err(|error| {
                        async_graphql::Error::new(format!(
                            "failed to prepare roles for user {}: {error}",
                            id
                        ))
                    })?
                    .query_map([id], |row| {
                        let role_id: i64 = row.get(0)?;
                        let name: String = row.get(1)?;
                        let role_scopes_json: String = row.get(2)?;
                        let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                        Ok(AdminGraphqlRoleInfo {
                            id: graphql_gid("role", role_id),
                            name,
                            scopes: role_scopes,
                        })
                    })
                    .map_err(|error| {
                        async_graphql::Error::new(format!(
                            "failed to query roles for user {}: {error}",
                            id
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| {
                        async_graphql::Error::new(format!(
                            "failed to parse roles for user {}: {error}",
                            id
                        ))
                    })?;

                let roles_connection = AdminGraphqlRoleConnection {
                    edges: roles_vec
                        .into_iter()
                        .map(|node| AdminGraphqlRoleEdge {
                            cursor: None,
                            node: Some(node),
                        })
                        .collect(),
                    page_info: AdminGraphqlPageInfo {
                        has_next_page: false,
                        has_previous_page: false,
                        start_cursor: None,
                        end_cursor: None,
                    },
                };

                let node = AdminGraphqlUser {
                    id: graphql_gid("user", id),
                    created_at,
                    updated_at,
                    email,
                    status,
                    first_name,
                    last_name,
                    is_owner,
                    prefer_language,
                    scopes,
                    roles: roles_connection,
                };

                edges.push(AdminGraphqlUserEdge {
                    cursor: None,
                    node: Some(node),
                });
            }

            let page_info = AdminGraphqlPageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            };

            Ok(AdminGraphqlUserConnection { edges, page_info })
        }
    }

    #[Object]
    impl AdminGraphqlMutationRoot {
        async fn update_storage_policy(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateStoragePolicyInput,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            self.operational
                .update_storage_policy(input)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn update_retry_policy(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateRetryPolicyInput,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            let foundation = &self.operational.foundation;
            let mut policy = foundation
                .system_settings()
                .value("retry_policy")
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to read retry policy: {error}"))
                })?
                .map(|raw: String| {
                    serde_json::from_str::<StoredRetryPolicy>(raw.as_str()).map_err(|error| {
                        async_graphql::Error::new(format!("failed to parse retry policy: {error}"))
                    })
                })
                .transpose()?
                .unwrap_or_else(default_retry_policy);

            if let Some(enabled) = input.enabled {
                policy.enabled = enabled;
            }
            if let Some(max_channel_retries) = input.max_channel_retries {
                policy.max_channel_retries = max_channel_retries;
            }
            if let Some(max_single_channel_retries) = input.max_single_channel_retries {
                policy.max_single_channel_retries = max_single_channel_retries;
            }
            if let Some(retry_delay_ms) = input.retry_delay_ms {
                policy.retry_delay_ms = retry_delay_ms;
            }
            if let Some(load_balancer_strategy) = input.load_balancer_strategy {
                policy.load_balancer_strategy = normalize_retry_policy_load_balancer_strategy(&load_balancer_strategy);
            }
            if let Some(auto_disable_channel) = input.auto_disable_channel {
                if let Some(enabled) = auto_disable_channel.enabled {
                    policy.auto_disable_channel.enabled = enabled;
                }
                if let Some(statuses) = auto_disable_channel.statuses {
                    policy.auto_disable_channel.statuses = statuses
                        .into_iter()
                        .map(|status| crate::foundation::admin::StoredAutoDisableChannelStatus {
                            status: status.status,
                            times: status.times,
                        })
                        .collect();
                }
            }

            self.operational
                .foundation
                .system_settings()
                .set_value("retry_policy", &serde_json::to_string(&policy).map_err(|error| {
                    async_graphql::Error::new(format!("failed to serialize retry policy: {error}"))
                })?)
                .map_err(|error| async_graphql::Error::new(format!("failed to persist retry policy: {error}")))?;
            Ok(true)
        }

        async fn update_brand_settings(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateBrandSettingsInput,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;

            if let Some(brand_name) = input.brand_name {
                self.operational
                    .foundation
                    .system_settings()
                    .set_value(crate::foundation::shared::SYSTEM_KEY_BRAND_NAME, &brand_name)
                    .map_err(|error| {
                        async_graphql::Error::new(format!("failed to persist brand name: {error}"))
                    })?;
            }

            if let Some(brand_logo) = input.brand_logo {
                self.operational
                    .foundation
                    .system_settings()
                    .set_value(crate::foundation::shared::SYSTEM_KEY_BRAND_LOGO, &brand_logo)
                    .map_err(|error| {
                        async_graphql::Error::new(format!("failed to persist brand logo: {error}"))
                    })?;
            }

            Ok(true)
        }

        async fn update_auto_backup_settings(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateAutoBackupSettingsInput,
        ) -> async_graphql::Result<bool> {
            require_admin_owner(ctx)?;
            self.operational
                .update_auto_backup_settings(input)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn trigger_auto_backup(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<AdminGraphqlTriggerBackupPayload> {
            require_admin_owner(ctx)?;
            self.operational
                .trigger_backup_now()
                .map_err(async_graphql::Error::new)?;
            Ok(AdminGraphqlTriggerBackupPayload {
                success: true,
                message: Some("Backup completed successfully".to_owned()),
            })
        }

        async fn update_system_channel_settings(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateSystemChannelSettingsInput,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            self.operational
                .update_system_channel_settings(input)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn save_proxy_preset(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlSaveProxyPresetInput,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            let url = input.url.trim();
            if url.is_empty() {
                return Err(async_graphql::Error::new("invalid url: expected non-empty string"));
            }
            let preset = StoredProxyPreset {
                name: input.name.unwrap_or_default().trim().to_owned(),
                url: url.to_owned(),
                username: input.username.unwrap_or_default().trim().to_owned(),
                password: input.password.unwrap_or_default(),
            };
            self.operational
                .save_proxy_preset(preset)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn delete_proxy_preset(
            &self,
            ctx: &Context<'_>,
            url: String,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            let url = url.trim();
            if url.is_empty() {
                return Err(async_graphql::Error::new("invalid url: expected non-empty string"));
            }
            self.operational
                .delete_proxy_preset(url)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn update_user_agent_pass_through_settings(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateUserAgentPassThroughSettingsInput,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            self.operational
                .set_user_agent_pass_through(input.enabled)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn check_provider_quotas(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            self.operational
                .run_provider_quota_check_tick(true, Duration::from_secs(20 * 60))
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn reset_provider_quota(
            &self,
            ctx: &Context<'_>,
            #[graphql(name = "channelID")] channel_id: String,
        ) -> async_graphql::Result<bool> {
            require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS)?;
            let channel_id = parse_graphql_resource_id(channel_id.as_str(), "channel")?;
            self.operational
                .reset_provider_quota_status(channel_id)
                .map_err(async_graphql::Error::new)
        }

        async fn trigger_gc_cleanup(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
            if require_admin_system_scope(ctx, SCOPE_WRITE_SETTINGS).is_err() {
                return Err(async_graphql::Error::new(
                    "permission denied: requires write:settings scope",
                ));
            }
            self.operational
                .run_gc_cleanup_now(false, false)
                .map(|_| true)
                .map_err(async_graphql::Error::new)
        }

        async fn update_me(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlUpdateMeInput,
        ) -> async_graphql::Result<AdminGraphqlUserInfo> {
            let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
            let user_id = request_context.user.id;

            if input.first_name.is_none()
                && input.last_name.is_none()
                && input.prefer_language.is_none()
                && input.avatar.is_none()
            {
                return Err(async_graphql::Error::new("no fields to update"));
            }

            let db = self.operational.foundation.seaorm();
            let first_name = input.first_name.clone();
            let last_name = input.last_name.clone();
            let prefer_language = input.prefer_language.clone();
            let avatar = input.avatar.clone();
            db.run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                let Some(existing) = users::Entity::find_by_id(user_id)
                    .filter(users::Column::DeletedAt.eq(0_i64))
                    .one(&connection)
                    .await
                    .map_err(|error| error.to_string())?
                else {
                    return Err("user not found".to_owned());
                };
                let mut active: users::ActiveModel = existing.into();
                if let Some(first_name) = first_name {
                    active.first_name = Set(first_name);
                }
                if let Some(last_name) = last_name {
                    active.last_name = Set(last_name);
                }
                if let Some(prefer_language) = prefer_language {
                    active.prefer_language = Set(prefer_language);
                }
                if let Some(avatar) = avatar {
                    active.avatar = Set(Some(avatar));
                }
                active.update(&connection).await.map_err(|error| error.to_string())?;
                load_admin_graphql_user_info(&connection, user_id).await
            })
            .map_err(async_graphql::Error::new)
        }

        async fn update_user_status(
            &self,
            ctx: &Context<'_>,
            id: String,
            status: UserStatus,
        ) -> async_graphql::Result<AdminGraphqlUser> {
            require_admin_system_scope(ctx, SCOPE_WRITE_USERS)?;

            let id_parts: Vec<&str> = id.split('/').collect();
            let user_id_str = id_parts
                .last()
                .ok_or_else(|| async_graphql::Error::new("invalid user id format"))?;
            let user_id: i64 = user_id_str
                .parse()
                .map_err(|_| async_graphql::Error::new("invalid user id"))?;

            let status_str = match status {
                UserStatus::Activated => "activated",
                UserStatus::Deactivated => "deactivated",
            };
            let db = self.operational.foundation.seaorm();
            db.run_sync(move |db| async move {
                let connection = db.connect_migrated().await.map_err(|error| error.to_string())?;
                let Some(existing) = users::Entity::find_by_id(user_id)
                    .filter(users::Column::DeletedAt.eq(0_i64))
                    .one(&connection)
                    .await
                    .map_err(|error| error.to_string())?
                else {
                    return Err("user not found".to_owned());
                };
                let mut active: users::ActiveModel = existing.into();
                active.status = Set(status_str.to_owned());
                active.update(&connection).await.map_err(|error| error.to_string())?;
                load_admin_graphql_user(&connection, user_id).await
            })
            .map_err(async_graphql::Error::new)
        }

        async fn create_user(
            &self,
            ctx: &Context<'_>,
            input: AdminGraphqlCreateUserInput,
        ) -> async_graphql::Result<AdminGraphqlUser> {
            require_admin_system_scope(ctx, SCOPE_WRITE_USERS)?;

            let foundation = &self.operational.foundation;
            let mut connection = foundation
                .open_connection(true)
                .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
            let transaction = connection.transaction().map_err(|error| {
                async_graphql::Error::new(format!("failed to start user transaction: {error}"))
            })?;

            let hashed_password = hash_password(&input.password)
                .map_err(|error| async_graphql::Error::new(format!("failed to hash password: {error}")))?;

            let status = match input.status {
                Some(UserStatus::Activated) => "activated",
                Some(UserStatus::Deactivated) => "deactivated",
                None => "activated",
            };

            let scopes_json = if let Some(scopes) = input.scopes {
                serde_json::to_string(&scopes).unwrap_or_default()
            } else {
                "[]".to_string()
            };

            transaction
                .execute(
                    "INSERT INTO users (email, status, prefer_language, password, first_name, last_name, avatar, is_owner, scopes, deleted_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)",
                    (
                        input.email,
                        status,
                        input.prefer_language.unwrap_or_else(|| "en".to_string()),
                        hashed_password,
                        input.first_name.unwrap_or_else(|| "".to_string()),
                        input.last_name.unwrap_or_else(|| "".to_string()),
                        input.avatar.unwrap_or_else(|| "".to_string()),
                        input.is_owner.unwrap_or(false) as i64,
                        scopes_json,
                    ),
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to create user: {error}")))?;

            let user_id = transaction.last_insert_rowid();

            if let Some(project_ids) = input.project_ids {
                for project_gid in project_ids {
                    let parts: Vec<&str> = project_gid.split('/').collect();
                    if let Some(id_str) = parts.last() {
                        if let Ok(project_id) = id_str.parse::<i64>() {
                            let is_owner = false;
                            transaction
                                .execute(
                                    "INSERT INTO user_projects (user_id, project_id, is_owner, scopes) VALUES (?1, ?2, ?3, ?4)",
                                    (user_id, project_id, if is_owner { 1 } else { 0 }, "[]"),
                                )
                                .map_err(|error| {
                                    async_graphql::Error::new(format!(
                                        "failed to assign user project membership: {error}"
                                    ))
                                })?;
                        }
                    }
                }
            }

            if let Some(role_ids) = input.role_ids {
                for role_gid in role_ids {
                    let parts: Vec<&str> = role_gid.split('/').collect();
                    if let Some(id_str) = parts.last() {
                        if let Ok(role_id) = id_str.parse::<i64>() {
                            transaction
                                .execute(
                                    "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                                    (user_id, role_id),
                                )
                                .map_err(|error| {
                                    async_graphql::Error::new(format!(
                                        "failed to assign user role: {error}"
                                    ))
                                })?;
                        }
                    }
                }
            }

            let user_row = transaction
                .query_row(
                    "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
                    [user_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                        ))
                    },
                )
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to query created user: {error}"))
                })?;

            let (
                id,
                email,
                first_name,
                last_name,
                is_owner_i64,
                prefer_language,
                status,
                created_at,
                updated_at,
                scopes_json,
            ) = user_row;
            let is_owner = is_owner_i64 != 0;
            let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

            let roles_vec = transaction
                .prepare(
                    "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
                .query_map([user_id], |row| {
                    let role_id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let role_scopes_json: String = row.get(2)?;
                    let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                    Ok(AdminGraphqlRoleInfo {
                        id: graphql_gid("role", role_id),
                        name,
                        scopes: role_scopes,
                    })
                })
                .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;

            let roles_connection = AdminGraphqlRoleConnection {
                edges: roles_vec
                    .into_iter()
                    .map(|node| AdminGraphqlRoleEdge {
                        cursor: None,
                        node: Some(node),
                    })
                    .collect(),
                page_info: AdminGraphqlPageInfo {
                    has_next_page: false,
                    has_previous_page: false,
                    start_cursor: None,
                    end_cursor: None,
                },
            };

            let created_user = AdminGraphqlUser {
                id: graphql_gid("user", id),
                created_at,
                updated_at,
                email,
                status,
                first_name,
                last_name,
                is_owner,
                prefer_language,
                scopes,
                roles: roles_connection,
            };

            transaction.commit().map_err(|error| {
                async_graphql::Error::new(format!("failed to commit user transaction: {error}"))
            })?;

            Ok(created_user)
        }

        async fn update_user(
            &self,
            ctx: &Context<'_>,
            id: String,
            input: AdminGraphqlUpdateUserInput,
        ) -> async_graphql::Result<AdminGraphqlUser> {
            require_admin_system_scope(ctx, SCOPE_WRITE_USERS)?;

            let id_parts: Vec<&str> = id.split('/').collect();
            let user_id_str = id_parts
                .last()
                .ok_or_else(|| async_graphql::Error::new("invalid user id format"))?;
            let user_id: i64 = user_id_str
                .parse()
                .map_err(|_| async_graphql::Error::new("invalid user id"))?;

            let foundation = &self.operational.foundation;
            let mut connection = foundation
                .open_connection(true)
                .map_err(|error| async_graphql::Error::new(format!("failed to open database: {error}")))?;
            let transaction = connection.transaction().map_err(|error| {
                async_graphql::Error::new(format!("failed to start user transaction: {error}"))
            })?;

            if input.first_name.is_none()
                && input.last_name.is_none()
                && input.prefer_language.is_none()
                && input.avatar.is_none()
                && input.scopes.is_none()
                && input.role_ids.is_none()
            {
                return Err(async_graphql::Error::new("no fields to update"));
            }

            let mut touched = false;
            if let Some(first_name) = &input.first_name {
                let rows_affected = transaction
                    .execute(
                        "UPDATE users SET first_name = ?1 WHERE id = ?2 AND deleted_at = 0",
                        (first_name.as_str(), user_id),
                    )
                    .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
                if rows_affected == 0 {
                    return Err(async_graphql::Error::new("user not found"));
                }
                touched = true;
            }
            if let Some(last_name) = &input.last_name {
                let rows_affected = transaction
                    .execute(
                        "UPDATE users SET last_name = ?1 WHERE id = ?2 AND deleted_at = 0",
                        (last_name.as_str(), user_id),
                    )
                    .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
                if rows_affected == 0 {
                    return Err(async_graphql::Error::new("user not found"));
                }
                touched = true;
            }
            if let Some(prefer_language) = &input.prefer_language {
                let rows_affected = transaction
                    .execute(
                        "UPDATE users SET prefer_language = ?1 WHERE id = ?2 AND deleted_at = 0",
                        (prefer_language.as_str(), user_id),
                    )
                    .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
                if rows_affected == 0 {
                    return Err(async_graphql::Error::new("user not found"));
                }
                touched = true;
            }
            if let Some(avatar) = &input.avatar {
                let rows_affected = transaction
                    .execute(
                        "UPDATE users SET avatar = ?1 WHERE id = ?2 AND deleted_at = 0",
                        (avatar.as_str(), user_id),
                    )
                    .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
                if rows_affected == 0 {
                    return Err(async_graphql::Error::new("user not found"));
                }
                touched = true;
            }
            if let Some(scopes) = &input.scopes {
                let scopes_json = serde_json::to_string(scopes).unwrap_or_default();
                let rows_affected = transaction
                    .execute(
                        "UPDATE users SET scopes = ?1 WHERE id = ?2 AND deleted_at = 0",
                        (scopes_json, user_id),
                    )
                    .map_err(|error| async_graphql::Error::new(format!("failed to update user: {error}")))?;
                if rows_affected == 0 {
                    return Err(async_graphql::Error::new("user not found"));
                }
                touched = true;
            }
            if !touched {
                return Err(async_graphql::Error::new("no fields to update"));
            }

            if let Some(role_ids) = &input.role_ids {
                transaction
                    .execute("DELETE FROM user_roles WHERE user_id = ?", [user_id])
                    .map_err(|error| {
                        async_graphql::Error::new(format!(
                            "failed to clear existing user roles: {error}"
                        ))
                    })?;

                for role_gid in role_ids {
                    let parts: Vec<&str> = role_gid.split('/').collect();
                    if let Some(id_str) = parts.last() {
                        if let Ok(role_id) = id_str.parse::<i64>() {
                            transaction
                                .execute(
                                    "INSERT INTO user_roles (user_id, role_id) VALUES (?1, ?2)",
                                    (user_id, role_id),
                                )
                                .map_err(|error| {
                                    async_graphql::Error::new(format!(
                                        "failed to replace user role assignments: {error}"
                                    ))
                                })?;
                        }
                    }
                }
            }

            let user_row = transaction
                .query_row(
                    "SELECT id, email, first_name, last_name, is_owner, prefer_language, status, created_at, updated_at, scopes FROM users WHERE id = ?1 AND deleted_at = 0",
                    [user_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                        ))
                    },
                )
                .map_err(|error| {
                    async_graphql::Error::new(format!("failed to query updated user: {error}"))
                })?;

            let (
                id,
                email,
                first_name,
                last_name,
                is_owner_i64,
                prefer_language,
                status,
                created_at,
                updated_at,
                scopes_json,
            ) = user_row;
            let is_owner = is_owner_i64 != 0;
            let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();

            let roles_vec = transaction
                .prepare(
                    "SELECT r.id, r.name, r.scopes FROM roles r JOIN user_roles ur ON ur.role_id = r.id WHERE ur.user_id = ?1 AND r.deleted_at = 0",
                )
                .map_err(|error| async_graphql::Error::new(format!("failed to prepare roles: {error}")))?
                .query_map([user_id], |row| {
                    let role_id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let role_scopes_json: String = row.get(2)?;
                    let role_scopes: Vec<String> = serde_json::from_str(&role_scopes_json).unwrap_or_default();
                    Ok(AdminGraphqlRoleInfo {
                        id: graphql_gid("role", role_id),
                        name,
                        scopes: role_scopes,
                    })
                })
                .map_err(|error| async_graphql::Error::new(format!("failed to query roles: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| async_graphql::Error::new(format!("failed to parse roles: {error}")))?;

            let roles_connection = AdminGraphqlRoleConnection {
                edges: roles_vec
                    .into_iter()
                    .map(|node| AdminGraphqlRoleEdge {
                        cursor: None,
                        node: Some(node),
                    })
                    .collect(),
                page_info: AdminGraphqlPageInfo {
                    has_next_page: false,
                    has_previous_page: false,
                    start_cursor: None,
                    end_cursor: None,
                },
            };

            let updated_user = AdminGraphqlUser {
                id: graphql_gid("user", id),
                created_at,
                updated_at,
                email,
                status,
                first_name,
                last_name,
                is_owner,
                prefer_language,
                scopes,
                roles: roles_connection,
            };

            transaction.commit().map_err(|error| {
                async_graphql::Error::new(format!("failed to commit user transaction: {error}"))
            })?;

            Ok(updated_user)
        }
    }

    #[Object]
    impl OpenApiGraphqlQueryRoot {
        async fn service_account_project(
            &self,
            ctx: &Context<'_>,
        ) -> async_graphql::Result<GraphqlProject> {
            let request_context = ctx.data_unchecked::<OpenApiGraphqlRequestContext>();
            Ok(GraphqlProject::from(request_context.owner_api_key.project.clone()))
        }
    }

    #[Object]
    impl OpenApiGraphqlMutationRoot {
        #[graphql(name = "createLLMAPIKey")]
        async fn create_llm_api_key(
            &self,
            ctx: &Context<'_>,
            name: String,
        ) -> async_graphql::Result<OpenApiGraphqlApiKey> {
            let request_context = ctx.data_unchecked::<OpenApiGraphqlRequestContext>();
            create_llm_api_key(self.foundation.as_ref(), &request_context.owner_api_key, &name)
                .map_err(|error| match error {
                    CreateLlmApiKeyError::InvalidName => {
                        async_graphql::Error::new("api key name is required")
                    }
                    CreateLlmApiKeyError::PermissionDenied => {
                        async_graphql::Error::new("permission denied")
                    }
                    CreateLlmApiKeyError::Internal(message) => async_graphql::Error::new(message),
                })
        }
    }

    pub(crate) fn require_admin_graphql_project_id(ctx: &Context<'_>) -> async_graphql::Result<i64> {
        ctx.data_unchecked::<AdminGraphqlRequestContext>()
            .project_id
            .ok_or_else(|| async_graphql::Error::new("project context is required for this query"))
    }

    pub(crate) fn require_admin_system_scope(
        ctx: &Context<'_>,
        scope: ScopeSlug,
    ) -> async_graphql::Result<()> {
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        if authorize_user_system_scope(&request_context.user, scope).is_ok() {
            Ok(())
        } else {
            Err(async_graphql::Error::new("permission denied"))
        }
    }

    pub(crate) fn require_admin_owner(ctx: &Context<'_>) -> async_graphql::Result<()> {
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        if require_owner_bypass(&request_context.user).is_ok() {
            Ok(())
        } else {
            Err(async_graphql::Error::new(
                "permission denied: owner access required",
            ))
        }
    }

    pub(crate) fn require_admin_project_scope(
        ctx: &Context<'_>,
        project_id: i64,
        scope: ScopeSlug,
    ) -> async_graphql::Result<()> {
        let request_context = ctx.data_unchecked::<AdminGraphqlRequestContext>();
        if require_user_project_scope(&request_context.user, project_id, scope).is_ok() {
            Ok(())
        } else {
            Err(async_graphql::Error::new("permission denied"))
        }
    }

    pub(crate) fn create_llm_api_key(
        foundation: &SqliteFoundation,
        owner_api_key: &AuthApiKeyContext,
        name: &str,
    ) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(CreateLlmApiKeyError::InvalidName);
        }
        if require_service_api_key_write_access(owner_api_key).is_err() {
            return Err(CreateLlmApiKeyError::PermissionDenied);
        }

        create_llm_api_key_sqlite(foundation, owner_api_key, trimmed_name)
    }

    fn create_llm_api_key_sqlite(
        foundation: &SqliteFoundation,
        owner_api_key: &AuthApiKeyContext,
        trimmed_name: &str,
    ) -> Result<OpenApiGraphqlApiKey, CreateLlmApiKeyError> {
        let owner_record = foundation
            .identities()
            .find_api_key_by_value(owner_api_key.key.as_str())
            .map_err(|error| {
                CreateLlmApiKeyError::Internal(format!(
                    "failed to load owner api key: {error:?}"
                ))
            })?;
        if owner_record.key_type != "service_account" || owner_record.project_id != owner_api_key.project.id {
            return Err(CreateLlmApiKeyError::PermissionDenied);
        }

        let connection = foundation.open_connection(true).map_err(|error| {
            CreateLlmApiKeyError::Internal(format!("failed to open database: {error}"))
        })?;
        ensure_identity_tables(&connection).map_err(|error| {
            CreateLlmApiKeyError::Internal(format!("failed to ensure identity schema: {error}"))
        })?;

        let generated_key: String = connection
            .query_row("SELECT 'ah-' || lower(hex(randomblob(32)))", [], |row| row.get(0))
            .map_err(|error| {
                CreateLlmApiKeyError::Internal(format!("failed to generate api key: {error}"))
            })?;
        let scopes = scope_strings(LLM_API_KEY_SCOPES);
        let scopes_json = serialize_scope_slugs(LLM_API_KEY_SCOPES).map_err(|error| {
            CreateLlmApiKeyError::Internal(format!("failed to serialize scopes: {error}"))
        })?;

        connection
            .execute(
                "INSERT INTO api_keys (user_id, project_id, key, name, type, status, scopes, profiles, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, 'user', 'enabled', ?5, '{}', 0)",
                (
                    owner_record.user_id,
                    owner_api_key.project.id,
                    generated_key.clone(),
                    trimmed_name,
                    scopes_json,
                ),
            )
            .map_err(|error| {
                CreateLlmApiKeyError::Internal(format!("failed to create api key: {error}"))
            })?;

        Ok(OpenApiGraphqlApiKey {
            key: generated_key,
            name: trimmed_name.to_owned(),
            scopes,
        })
    }

    fn parse_scopes_json(raw: &str) -> Vec<String> {
        serde_json::from_str(raw).unwrap_or_default()
    }

    async fn load_admin_graphql_user_info(
        db: &impl ConnectionTrait,
        user_id: i64,
    ) -> Result<AdminGraphqlUserInfo, String> {
        let user = users::Entity::find_by_id(user_id)
            .filter(users::Column::DeletedAt.eq(0_i64))
            .into_partial_model::<users::GraphqlProfile>()
            .one(db)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "user not found".to_owned())?;

        let roles = roles::Entity::find()
            .join(sea_orm::JoinType::InnerJoin, roles::Relation::UserRoles.def())
            .filter(user_roles::Column::UserId.eq(user_id))
            .filter(roles::Column::DeletedAt.eq(0_i64))
            .into_partial_model::<roles::GraphqlRoleSummary>()
            .all(db)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|role| AdminGraphqlRoleInfo {
                id: graphql_gid("role", role.id),
                name: role.name,
                scopes: parse_scopes_json(&role.scopes),
            })
            .collect::<Vec<_>>();

        let memberships = user_projects::Entity::find()
            .filter(user_projects::Column::UserId.eq(user_id))
            .find_also_related(projects::Entity)
            .all(db)
            .await
            .map_err(|error| error.to_string())?;

        let mut projects_out = Vec::new();
        for (membership, project) in memberships {
            let Some(project) = project else {
                continue;
            };
            if project.deleted_at != 0 {
                continue;
            }
            let project_roles = roles::Entity::find()
                .join(sea_orm::JoinType::InnerJoin, roles::Relation::UserRoles.def())
                .filter(user_roles::Column::UserId.eq(user_id))
                .filter(roles::Column::ProjectId.eq(project.id))
                .filter(roles::Column::DeletedAt.eq(0_i64))
                .into_partial_model::<roles::GraphqlRoleSummary>()
                .all(db)
                .await
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|role| AdminGraphqlRoleInfo {
                    id: graphql_gid("role", role.id),
                    name: role.name,
                    scopes: parse_scopes_json(&role.scopes),
                })
                .collect::<Vec<_>>();

            projects_out.push(AdminGraphqlUserProjectInfo {
                project_id: graphql_gid("project", project.id),
                is_owner: membership.is_owner,
                scopes: parse_scopes_json(&membership.scopes),
                roles: project_roles,
            });
        }

        Ok(AdminGraphqlUserInfo {
            id: graphql_gid("user", user.id),
            email: user.email,
            first_name: user.first_name,
            last_name: user.last_name,
            is_owner: user.is_owner,
            prefer_language: user.prefer_language,
            avatar: user.avatar.filter(|value| !value.is_empty()),
            scopes: parse_scopes_json(&user.scopes),
            roles,
            projects: projects_out,
        })
    }

    async fn load_admin_graphql_roles(
        db: &impl ConnectionTrait,
        user_id: i64,
    ) -> Result<Vec<AdminGraphqlRoleInfo>, String> {
        roles::Entity::find()
            .join(sea_orm::JoinType::InnerJoin, roles::Relation::UserRoles.def())
            .filter(user_roles::Column::UserId.eq(user_id))
            .filter(roles::Column::DeletedAt.eq(0_i64))
            .into_partial_model::<roles::GraphqlRoleSummary>()
            .all(db)
            .await
            .map_err(|error| error.to_string())
            .map(|rows| {
                rows.into_iter()
                    .map(|role| AdminGraphqlRoleInfo {
                        id: graphql_gid("role", role.id),
                        name: role.name,
                        scopes: parse_scopes_json(&role.scopes),
                    })
                    .collect()
            })
    }

    async fn load_admin_graphql_user(
        db: &impl ConnectionTrait,
        user_id: i64,
    ) -> Result<AdminGraphqlUser, String> {
        let user = users::Entity::find_by_id(user_id)
            .filter(users::Column::DeletedAt.eq(0_i64))
            .into_partial_model::<users::GraphqlUserListItem>()
            .one(db)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "user not found".to_owned())?;
        let roles_vec = load_admin_graphql_roles(db, user_id).await?;
        let roles = AdminGraphqlRoleConnection {
            edges: roles_vec
                .into_iter()
                .map(|node| AdminGraphqlRoleEdge {
                    cursor: None,
                    node: Some(node),
                })
                .collect(),
            page_info: AdminGraphqlPageInfo {
                has_next_page: false,
                has_previous_page: false,
                start_cursor: None,
                end_cursor: None,
            },
        };

        Ok(AdminGraphqlUser {
            id: graphql_gid("user", user.id),
            created_at: user.created_at,
            updated_at: user.updated_at,
            email: user.email,
            status: user.status,
            first_name: user.first_name,
            last_name: user.last_name,
            is_owner: user.is_owner,
            prefer_language: user.prefer_language,
            scopes: parse_scopes_json(&user.scopes),
            roles,
        })
    }
}
