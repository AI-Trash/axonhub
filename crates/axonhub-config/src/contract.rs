use anyhow::{anyhow, Result};
use serde_yaml::{Mapping, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedConfigKey {
    pub key: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedConfigAlias {
    pub key: &'static str,
    pub canonical_key: &'static str,
    pub description: &'static str,
}

const SUPPORTED_CONFIG_KEYS: &[SupportedConfigKey] = &[
    SupportedConfigKey {
        key: "server.host",
        description: "Server bind host",
    },
    SupportedConfigKey {
        key: "server.port",
        description: "Server port number",
    },
    SupportedConfigKey {
        key: "server.name",
        description: "Server name",
    },
    SupportedConfigKey {
        key: "server.base_path",
        description: "Server base path",
    },
    SupportedConfigKey {
        key: "server.read_timeout",
        description: "Server read timeout duration",
    },
    SupportedConfigKey {
        key: "server.request_timeout",
        description: "Request timeout duration",
    },
    SupportedConfigKey {
        key: "server.llm_request_timeout",
        description: "LLM request timeout duration",
    },
    SupportedConfigKey {
        key: "server.trace.thread_header",
        description: "Primary thread trace header",
    },
    SupportedConfigKey {
        key: "server.trace.trace_header",
        description: "Primary trace header",
    },
    SupportedConfigKey {
        key: "server.trace.request_header",
        description: "Request ID header",
    },
    SupportedConfigKey {
        key: "server.trace.extra_trace_headers",
        description: "Fallback trace headers",
    },
    SupportedConfigKey {
        key: "server.trace.extra_trace_body_fields",
        description: "Fallback trace body fields",
    },
    SupportedConfigKey {
        key: "server.trace.claude_code_trace_enabled",
        description: "Enable Claude Code trace extraction",
    },
    SupportedConfigKey {
        key: "server.trace.codex_trace_enabled",
        description: "Enable Codex trace extraction",
    },
    SupportedConfigKey {
        key: "server.dashboard.all_time_token_stats_soft_ttl",
        description: "Dashboard stale-while-revalidate TTL",
    },
    SupportedConfigKey {
        key: "server.dashboard.all_time_token_stats_hard_ttl",
        description: "Dashboard hard cache TTL",
    },
    SupportedConfigKey {
        key: "server.debug",
        description: "Server debug mode",
    },
    SupportedConfigKey {
        key: "server.disable_ssl_verify",
        description: "Disable outbound SSL verification",
    },
    SupportedConfigKey {
        key: "server.cors.enabled",
        description: "Enable CORS handling",
    },
    SupportedConfigKey {
        key: "server.cors.debug",
        description: "CORS debug mode",
    },
    SupportedConfigKey {
        key: "server.cors.allowed_origins",
        description: "Allowed CORS origins",
    },
    SupportedConfigKey {
        key: "server.cors.allowed_methods",
        description: "Allowed CORS methods",
    },
    SupportedConfigKey {
        key: "server.cors.allowed_headers",
        description: "Allowed CORS headers",
    },
    SupportedConfigKey {
        key: "server.cors.exposed_headers",
        description: "Exposed CORS headers",
    },
    SupportedConfigKey {
        key: "server.cors.allow_credentials",
        description: "Allow credentialed CORS requests",
    },
    SupportedConfigKey {
        key: "server.cors.max_age",
        description: "CORS preflight cache duration",
    },
    SupportedConfigKey {
        key: "server.api.auth.allow_no_auth",
        description: "Allow unauthenticated API access",
    },
    SupportedConfigKey {
        key: "db.dsn",
        description: "Database DSN",
    },
    SupportedConfigKey {
        key: "db.debug",
        description: "Enable database debug logging",
    },
    SupportedConfigKey {
        key: "log.name",
        description: "Logger name",
    },
    SupportedConfigKey {
        key: "log.debug",
        description: "Logger debug mode",
    },
    SupportedConfigKey {
        key: "log.skip_level",
        description: "Logger caller skip level",
    },
    SupportedConfigKey {
        key: "log.level",
        description: "Log level",
    },
    SupportedConfigKey {
        key: "log.level_key",
        description: "Structured level field name",
    },
    SupportedConfigKey {
        key: "log.time_key",
        description: "Structured time field name",
    },
    SupportedConfigKey {
        key: "log.caller_key",
        description: "Structured caller field name",
    },
    SupportedConfigKey {
        key: "log.function_key",
        description: "Structured function field name",
    },
    SupportedConfigKey {
        key: "log.name_key",
        description: "Structured logger-name field name",
    },
    SupportedConfigKey {
        key: "log.encoding",
        description: "Log encoding (json or console)",
    },
    SupportedConfigKey {
        key: "log.includes",
        description: "Logger include filters",
    },
    SupportedConfigKey {
        key: "log.excludes",
        description: "Logger exclude filters",
    },
    SupportedConfigKey {
        key: "log.output",
        description: "Log output (stdio or file)",
    },
    SupportedConfigKey {
        key: "log.file.path",
        description: "Log file path",
    },
    SupportedConfigKey {
        key: "log.file.max_size",
        description: "Log file max size in MB",
    },
    SupportedConfigKey {
        key: "log.file.max_age",
        description: "Log file retention in days",
    },
    SupportedConfigKey {
        key: "log.file.max_backups",
        description: "Log file backup count",
    },
    SupportedConfigKey {
        key: "log.file.local_time",
        description: "Use local time for log rotation",
    },
    SupportedConfigKey {
        key: "metrics.enabled",
        description: "Enable metrics export",
    },
    SupportedConfigKey {
        key: "metrics.exporter.type",
        description: "Metrics exporter type (stdout, otlpgrpc, otlphttp)",
    },
    SupportedConfigKey {
        key: "metrics.exporter.endpoint",
        description: "Metrics exporter endpoint",
    },
    SupportedConfigKey {
        key: "metrics.exporter.insecure",
        description: "Disable TLS verification for metrics exporter",
    },
    SupportedConfigKey {
        key: "traces.enabled",
        description: "Enable trace export",
    },
    SupportedConfigKey {
        key: "traces.exporter.type",
        description: "Trace exporter type (stdout, otlpgrpc, otlphttp)",
    },
    SupportedConfigKey {
        key: "traces.exporter.endpoint",
        description: "Trace exporter endpoint",
    },
    SupportedConfigKey {
        key: "traces.exporter.insecure",
        description: "Disable TLS verification for trace exporter",
    },
    SupportedConfigKey {
        key: "gc.cron",
        description: "GC cron schedule",
    },
    SupportedConfigKey {
        key: "gc.vacuum_enabled",
        description: "Enable DB vacuum during GC",
    },
    SupportedConfigKey {
        key: "gc.vacuum_full",
        description: "Use full vacuum during GC",
    },
    SupportedConfigKey {
        key: "cache.mode",
        description: "Cache mode (memory, redis, two-level)",
    },
    SupportedConfigKey {
        key: "cache.default_expiration",
        description: "Default in-memory cache expiration",
    },
    SupportedConfigKey {
        key: "cache.cleanup_interval",
        description: "In-memory cache cleanup interval",
    },
    SupportedConfigKey {
        key: "cache.memory.expiration",
        description: "In-memory cache expiration",
    },
    SupportedConfigKey {
        key: "cache.memory.cleanup_interval",
        description: "In-memory cache cleanup interval",
    },
    SupportedConfigKey {
        key: "cache.redis.addr",
        description: "Redis cache address",
    },
    SupportedConfigKey {
        key: "cache.redis.url",
        description: "Redis cache URL",
    },
    SupportedConfigKey {
        key: "cache.redis.username",
        description: "Redis username",
    },
    SupportedConfigKey {
        key: "cache.redis.password",
        description: "Redis password",
    },
    SupportedConfigKey {
        key: "cache.redis.db",
        description: "Redis database number",
    },
    SupportedConfigKey {
        key: "cache.redis.tls",
        description: "Enable Redis TLS",
    },
    SupportedConfigKey {
        key: "cache.redis.tls_insecure_skip_verify",
        description: "Skip Redis TLS certificate verification",
    },
    SupportedConfigKey {
        key: "cache.redis.expiration",
        description: "Redis cache expiration",
    },
    SupportedConfigKey {
        key: "provider_quota.check_interval",
        description: "Provider quota poll interval",
    },
    SupportedConfigKey {
        key: "provider_edge.codex.authorize_url",
        description: "Codex OAuth authorization URL",
    },
    SupportedConfigKey {
        key: "provider_edge.codex.token_url",
        description: "Codex OAuth token URL",
    },
    SupportedConfigKey {
        key: "provider_edge.codex.client_id",
        description: "Codex OAuth client ID",
    },
    SupportedConfigKey {
        key: "provider_edge.codex.redirect_uri",
        description: "Codex OAuth redirect URI",
    },
    SupportedConfigKey {
        key: "provider_edge.codex.scopes",
        description: "Codex OAuth scopes",
    },
    SupportedConfigKey {
        key: "provider_edge.codex.user_agent",
        description: "Codex OAuth user agent",
    },
    SupportedConfigKey {
        key: "provider_edge.claudecode.authorize_url",
        description: "Claude Code OAuth authorization URL",
    },
    SupportedConfigKey {
        key: "provider_edge.claudecode.token_url",
        description: "Claude Code OAuth token URL",
    },
    SupportedConfigKey {
        key: "provider_edge.claudecode.client_id",
        description: "Claude Code OAuth client ID",
    },
    SupportedConfigKey {
        key: "provider_edge.claudecode.redirect_uri",
        description: "Claude Code OAuth redirect URI",
    },
    SupportedConfigKey {
        key: "provider_edge.claudecode.scopes",
        description: "Claude Code OAuth scopes",
    },
    SupportedConfigKey {
        key: "provider_edge.claudecode.user_agent",
        description: "Claude Code OAuth user agent",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.authorize_url",
        description: "Antigravity OAuth authorization URL",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.token_url",
        description: "Antigravity OAuth token URL",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.client_id",
        description: "Antigravity OAuth client ID",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.client_secret",
        description: "Antigravity OAuth client secret",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.redirect_uri",
        description: "Antigravity OAuth redirect URI",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.scopes",
        description: "Antigravity OAuth scopes",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.load_endpoints",
        description: "Antigravity project lookup endpoints",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.user_agent",
        description: "Antigravity OAuth user agent",
    },
    SupportedConfigKey {
        key: "provider_edge.antigravity.client_metadata",
        description: "Antigravity OAuth client metadata JSON",
    },
    SupportedConfigKey {
        key: "provider_edge.copilot.device_code_url",
        description: "Copilot OAuth device-code URL",
    },
    SupportedConfigKey {
        key: "provider_edge.copilot.access_token_url",
        description: "Copilot OAuth access-token URL",
    },
    SupportedConfigKey {
        key: "provider_edge.copilot.client_id",
        description: "Copilot OAuth client ID",
    },
    SupportedConfigKey {
        key: "provider_edge.copilot.scope",
        description: "Copilot OAuth scope",
    },
];

const SUPPORTED_CONFIG_ALIASES: &[SupportedConfigAlias] = &[
    SupportedConfigAlias {
        key: "cache.memory.expiration",
        canonical_key: "cache.default_expiration",
        description: "Accepted nested alias for cache.default_expiration",
    },
    SupportedConfigAlias {
        key: "cache.memory.cleanup_interval",
        canonical_key: "cache.cleanup_interval",
        description: "Accepted nested alias for cache.cleanup_interval",
    },
];

pub fn supported_config_keys() -> &'static [SupportedConfigKey] {
    SUPPORTED_CONFIG_KEYS
}

pub fn supported_config_aliases() -> &'static [SupportedConfigAlias] {
    SUPPORTED_CONFIG_ALIASES
}

pub(crate) fn canonical_key_for_get(key: &str) -> Option<&'static str> {
    SUPPORTED_CONFIG_KEYS
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.key)
        .or_else(|| {
            SUPPORTED_CONFIG_ALIASES
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.canonical_key)
        })
}

pub(crate) fn validate_supported_config_shape(root: &Value) -> Result<()> {
    let Some(root_map) = root.as_mapping() else {
        return Ok(());
    };

    validate_mapping(&[], root_map)
}

fn validate_mapping(prefix: &[String], mapping: &Mapping) -> Result<()> {
    for (key, value) in mapping {
        let Some(segment) = key.as_str() else {
            continue;
        };

        let mut path = prefix.to_vec();
        path.push(segment.to_owned());
        let joined = path.join(".");

        match value {
            Value::Mapping(child) => {
                if !has_documented_descendant(&joined) {
                    return Err(anyhow!(unsupported_config_key_message(&joined)));
                }
                validate_mapping(&path, child)?;
            }
            _ => {
                if !SUPPORTED_CONFIG_KEYS
                    .iter()
                    .any(|entry| entry.key == joined)
                {
                    return Err(anyhow!(unsupported_config_key_message(&joined)));
                }
            }
        }
    }

    Ok(())
}

fn has_documented_descendant(prefix: &str) -> bool {
    let dotted_prefix = format!("{prefix}.");
    SUPPORTED_CONFIG_KEYS
        .iter()
        .any(|entry| entry.key.starts_with(&dotted_prefix))
}

fn unsupported_config_key_message(key: &str) -> String {
    format!(
        "unsupported config key '{key}': supported AxonHub config keys must match the Go config contract rooted in conf/conf.go"
    )
}
