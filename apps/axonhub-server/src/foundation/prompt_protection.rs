use std::collections::HashMap;

use axonhub_http::{OpenAiRequestBody, OpenAiV1Error};
use regex::Regex;
use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::repositories::prompt_protection::{
    list_enabled_prompt_protection_rules_seaorm, StoredPromptProtectionRuleRecord,
};

pub(crate) const PROMPT_PROTECTION_REJECTED_MESSAGE: &str =
    "request blocked by prompt protection policy";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredPromptProtectionSettings {
    pub(crate) action: PromptProtectionAction,
    #[serde(default)]
    pub(crate) replacement: Option<String>,
    #[serde(default)]
    pub(crate) scopes: Vec<PromptProtectionScope>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PromptProtectionAction {
    Mask,
    Reject,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PromptProtectionScope {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub(crate) struct PromptProtectionRule {
    pub(crate) id: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) pattern: String,
    pub(crate) status: String,
    pub(crate) settings: StoredPromptProtectionSettings,
}

#[derive(Debug, Clone)]
struct CompiledPromptProtectionRule {
    rule: PromptProtectionRule,
    regex: Regex,
}

pub(crate) async fn load_enabled_prompt_protection_rules_seaorm(
    db: &impl ConnectionTrait,
) -> Result<Vec<PromptProtectionRule>, OpenAiV1Error> {
    list_enabled_prompt_protection_rules_seaorm(db)
        .await
        .map_err(|message| OpenAiV1Error::Internal { message })?
        .into_iter()
        .map(prompt_protection_rule_from_record)
        .collect()
}

pub(crate) fn validate_prompt_protection_settings(
    settings: &StoredPromptProtectionSettings,
    field: &str,
) -> Result<(), String> {
    if settings.scopes.is_empty() {
        return Err(format!("{field} must target at least one scope"));
    }
    if matches!(settings.action, PromptProtectionAction::Mask)
        && settings
            .replacement
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err("mask action requires replacement".to_owned());
    }
    Ok(())
}

pub(crate) fn validate_prompt_protection_rule(
    name: &str,
    pattern: &str,
    settings: &StoredPromptProtectionSettings,
    field: &str,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{field} name is required"));
    }
    if pattern.trim().is_empty() {
        return Err(format!("{field} pattern is required"));
    }
    Regex::new(pattern.trim()).map_err(|error| format!("invalid prompt protection pattern: {error}"))?;
    validate_prompt_protection_settings(settings, field)
}

pub(crate) fn normalize_prompt_protection_status(value: Option<&str>) -> Result<&'static str, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("disabled") => Ok("disabled"),
        Some("enabled") => Ok("enabled"),
        Some("archived") => Ok("archived"),
        Some(other) => Err(format!("invalid prompt protection rule status: {other}")),
    }
}

pub(crate) fn apply_prompt_protection(
    body: &OpenAiRequestBody,
    rules: &[PromptProtectionRule],
) -> Result<OpenAiRequestBody, OpenAiV1Error> {
    if rules.is_empty() {
        return Ok(body.clone());
    }

    let compiled = rules
        .iter()
        .cloned()
        .map(|rule| {
            let rule_name = rule.name.clone();
            let pattern = rule.pattern.clone();
            match Regex::new(pattern.as_str()) {
                Ok(regex) => Ok(CompiledPromptProtectionRule { rule, regex }),
                Err(error) => Err(OpenAiV1Error::Internal {
                    message: format!(
                        "invalid prompt protection pattern for rule `{}`: {error}",
                        rule_name
                    ),
                }),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    match body {
        OpenAiRequestBody::Json(value) => apply_prompt_protection_json(value, &compiled).map(OpenAiRequestBody::Json),
        OpenAiRequestBody::Multipart(_) => Ok(body.clone()),
    }
}

pub(crate) fn prompt_protection_rule_from_record(
    record: StoredPromptProtectionRuleRecord,
) -> Result<PromptProtectionRule, OpenAiV1Error> {
    let settings = serde_json::from_str::<StoredPromptProtectionSettings>(&record.settings).map_err(|error| {
        OpenAiV1Error::Internal {
            message: format!(
                "failed to decode prompt protection settings for rule `{}`: {error}",
                record.name
            ),
        }
    })?;
    validate_prompt_protection_settings(&settings, "prompt protection settings").map_err(|message| {
        OpenAiV1Error::Internal { message }
    })?;
    Ok(PromptProtectionRule {
        id: record.id,
        created_at: record.created_at,
        updated_at: record.updated_at,
        name: record.name,
        description: record.description,
        pattern: record.pattern,
        status: record.status,
        settings,
    })
}

pub(crate) fn prompt_protection_rule_json(rule: &PromptProtectionRule) -> Value {
    json!({
        "id": super::shared::graphql_gid("promptProtectionRule", rule.id),
        "createdAt": rule.created_at,
        "updatedAt": rule.updated_at,
        "name": rule.name,
        "description": rule.description,
        "pattern": rule.pattern,
        "status": rule.status,
        "settings": {
            "action": match rule.settings.action {
                PromptProtectionAction::Mask => "mask",
                PromptProtectionAction::Reject => "reject",
            },
            "replacement": rule.settings.replacement,
            "scopes": rule
                .settings
                .scopes
                .iter()
                .map(|scope| prompt_protection_scope_name(*scope).to_owned())
                .collect::<Vec<_>>(),
        }
    })
}

pub(crate) fn prompt_protection_scope_name(scope: PromptProtectionScope) -> &'static str {
    match scope {
        PromptProtectionScope::System => "system",
        PromptProtectionScope::Developer => "developer",
        PromptProtectionScope::User => "user",
        PromptProtectionScope::Assistant => "assistant",
        PromptProtectionScope::Tool => "tool",
    }
}

fn apply_prompt_protection_json(
    value: &Value,
    rules: &[CompiledPromptProtectionRule],
) -> Result<Value, OpenAiV1Error> {
    let mut rewritten = value.clone();
    let Some(object) = rewritten.as_object_mut() else {
        return Ok(rewritten);
    };

    if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            apply_prompt_protection_message(message, rules)?;
        }
    }

    if let Some(input) = object.get_mut("input") {
        apply_prompt_protection_input(input, rules)?;
    }

    if let Some(prompt) = object.get_mut("prompt") {
        apply_prompt_protection_text_value(prompt, PromptProtectionScope::User, rules)?;
    }

    Ok(rewritten)
}

fn apply_prompt_protection_message(
    message: &mut Value,
    rules: &[CompiledPromptProtectionRule],
) -> Result<(), OpenAiV1Error> {
    let Some(object) = message.as_object_mut() else {
        return Ok(());
    };
    let scope = object
        .get("role")
        .and_then(Value::as_str)
        .and_then(prompt_protection_scope_for_role)
        .unwrap_or(PromptProtectionScope::User);
    if let Some(content) = object.get_mut("content") {
        apply_prompt_protection_content(content, scope, rules)?;
    }
    Ok(())
}

fn apply_prompt_protection_input(
    input: &mut Value,
    rules: &[CompiledPromptProtectionRule],
) -> Result<(), OpenAiV1Error> {
    match input {
        Value::String(_) => apply_prompt_protection_text_value(input, PromptProtectionScope::User, rules),
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(_) => apply_prompt_protection_text_value(item, PromptProtectionScope::User, rules)?,
                    Value::Object(object) => {
                        let scope = object
                            .get("role")
                            .and_then(Value::as_str)
                            .and_then(prompt_protection_scope_for_role)
                            .unwrap_or(PromptProtectionScope::User);
                        if let Some(content) = object.get_mut("content") {
                            apply_prompt_protection_content(content, scope, rules)?;
                        }
                        if let Some(text) = object.get_mut("text") {
                            apply_prompt_protection_text_value(text, scope, rules)?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        Value::Object(object) => {
            let scope = object
                .get("role")
                .and_then(Value::as_str)
                .and_then(prompt_protection_scope_for_role)
                .unwrap_or(PromptProtectionScope::User);
            if let Some(content) = object.get_mut("content") {
                apply_prompt_protection_content(content, scope, rules)?;
            }
            if let Some(text) = object.get_mut("text") {
                apply_prompt_protection_text_value(text, scope, rules)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn apply_prompt_protection_content(
    content: &mut Value,
    scope: PromptProtectionScope,
    rules: &[CompiledPromptProtectionRule],
) -> Result<(), OpenAiV1Error> {
    match content {
        Value::String(_) => apply_prompt_protection_text_value(content, scope, rules),
        Value::Array(parts) => {
            for part in parts {
                if let Some(object) = part.as_object_mut() {
                    let part_scope = object
                        .get("type")
                        .and_then(Value::as_str)
                        .and_then(prompt_protection_scope_for_content_type)
                        .unwrap_or(scope);
                    if let Some(text) = object.get_mut("text") {
                        apply_prompt_protection_text_value(text, part_scope, rules)?;
                    }
                    if let Some(input) = object.get_mut("input") {
                        apply_prompt_protection_text_value(input, PromptProtectionScope::Tool, rules)?;
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn apply_prompt_protection_text_value(
    text: &mut Value,
    scope: PromptProtectionScope,
    rules: &[CompiledPromptProtectionRule],
) -> Result<(), OpenAiV1Error> {
    let Some(current) = text.as_str() else {
        return Ok(());
    };
    let mut masked = current.to_owned();
    for compiled in rules {
        if !compiled.rule.settings.scopes.contains(&scope) {
            continue;
        }
        if !compiled.regex.is_match(masked.as_str()) {
            continue;
        }
        match compiled.rule.settings.action {
            PromptProtectionAction::Reject => {
                return Err(OpenAiV1Error::InvalidRequest {
                    message: PROMPT_PROTECTION_REJECTED_MESSAGE.to_owned(),
                })
            }
            PromptProtectionAction::Mask => {
                let replacement = compiled
                    .rule
                    .settings
                    .replacement
                    .clone()
                    .unwrap_or_default();
                masked = compiled
                    .regex
                    .replace_all(masked.as_str(), replacement.as_str())
                    .into_owned();
            }
        }
    }
    *text = Value::String(masked);
    Ok(())
}

fn prompt_protection_scope_for_role(role: &str) -> Option<PromptProtectionScope> {
    match role {
        "system" => Some(PromptProtectionScope::System),
        "developer" => Some(PromptProtectionScope::Developer),
        "user" => Some(PromptProtectionScope::User),
        "assistant" => Some(PromptProtectionScope::Assistant),
        "tool" => Some(PromptProtectionScope::Tool),
        _ => None,
    }
}

fn prompt_protection_scope_for_content_type(value: &str) -> Option<PromptProtectionScope> {
    match value {
        "input_text" | "output_text" | "text" => Some(PromptProtectionScope::User),
        "tool_result" | "tool_call" | "input_tool_result" => Some(PromptProtectionScope::Tool),
        _ => None,
    }
}

pub(crate) fn default_prompt_protection_connection_json(rules: Vec<Value>) -> Value {
    let edges = rules
        .into_iter()
        .map(|node| {
            let cursor = node
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            json!({"node": node, "cursor": cursor})
        })
        .collect::<Vec<_>>();
    let start_cursor = edges
        .first()
        .and_then(|edge| edge.get("cursor"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let end_cursor = edges
        .last()
        .and_then(|edge| edge.get("cursor"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    json!({
        "edges": edges,
        "pageInfo": {
            "hasNextPage": false,
            "hasPreviousPage": false,
            "startCursor": start_cursor,
            "endCursor": end_cursor,
        },
        "totalCount": edges.len(),
    })
}

pub(crate) fn prompt_protection_settings_from_variables(
    variables: &HashMap<String, Value>,
) -> Option<Value> {
    variables.get("settings").cloned()
}
