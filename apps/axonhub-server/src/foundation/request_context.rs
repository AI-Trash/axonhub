use axonhub_http::{ThreadContext, TraceContext};
use serde::{Deserialize, Serialize};

pub(crate) fn normalize_context_key(value: &str) -> String {
    value.trim().to_owned()
}

pub(crate) fn thread_belongs_to_project(thread: &ThreadContext, project_id: i64) -> bool {
    thread.project_id == project_id
}

pub(crate) fn trace_matches_project_and_thread(
    trace: &TraceContext,
    project_id: i64,
    thread_db_id: Option<i64>,
) -> bool {
    trace.project_id == project_id && (thread_db_id.is_none() || trace.thread_id == thread_db_id)
}

#[cfg(any())]
pub(crate) mod sqlite_test_support {
    pub(crate) use super::super::repositories::request_context::sqlite_test_support::TraceContextStore;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OnboardingModule {
    pub onboarded: bool,
    #[serde(rename = "completed_at", skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OnboardingRecord {
    pub onboarded: bool,
    #[serde(rename = "completed_at", skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(
        rename = "system_model_setting",
        skip_serializing_if = "Option::is_none"
    )]
    pub system_model_setting: Option<OnboardingModule>,
    #[serde(
        rename = "auto_disable_channel",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_disable_channel: Option<OnboardingModule>,
}

pub(crate) fn serialize_onboarding_record(
    value: &OnboardingRecord,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

pub(crate) fn parse_onboarding_record(raw: &str) -> Result<OnboardingRecord, serde_json::Error> {
    serde_json::from_str(raw)
}
