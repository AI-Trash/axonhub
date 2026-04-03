use anyhow::{anyhow, Context, Result};

use crate::contract::validate_db_dialect;
use crate::types::Config;
use http::header::HeaderName;
use http::method::Method;

impl Config {
    pub(crate) fn ensure_cli_loadable(&self) -> Result<()> {
        validate_db_dialect(&self.db.dialect)?;
        ensure_log_level(&self.log.level)?;
        self.ensure_duration_fields_parse()?;

        Ok(())
    }

    pub fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.server.port == 0 || self.server.port > 65_535 {
            errors.push("server.port must be between 1 and 65535".to_owned());
        }

        if self.db.dsn.trim().is_empty() {
            errors.push("db.dsn cannot be empty".to_owned());
        }

        if let Err(error) = validate_db_dialect(&self.db.dialect) {
            errors.push(error.to_string());
        }

        if self.log.name.trim().is_empty() {
            errors.push("log.name cannot be empty".to_owned());
        }

        let encoding = self.log.encoding.trim().to_ascii_lowercase();
        if !matches!(encoding.as_str(), "json" | "console") {
            errors.push("log.encoding must be one of: json, console".to_owned());
        }

        let output = self.log.output.trim().to_ascii_lowercase();
        if !matches!(output.as_str(), "stdio" | "file") {
            errors.push("log.output must be one of: stdio, file".to_owned());
        }

        let cache_mode = self.cache.mode.trim().to_ascii_lowercase();
        if !matches!(cache_mode.as_str(), "memory" | "redis" | "two-level") {
            errors.push("cache.mode must be one of: memory, redis, two-level".to_owned());
        }

        if self.metrics.enabled {
            let exporter_type = self
                .metrics
                .exporter
                .exporter_type
                .trim()
                .to_ascii_lowercase();
            if !matches!(exporter_type.as_str(), "stdout" | "otlpgrpc" | "otlphttp") {
                errors.push(
                    "metrics.exporter.type must be one of: stdout, otlpgrpc, otlphttp when metrics are enabled"
                        .to_owned(),
                );
            }
        }

        if self.server.cors.enabled && self.server.cors.allowed_origins.is_empty() {
            errors.push(
                "server.cors.allowed_origins cannot be empty when CORS is enabled".to_owned(),
            );
        }

        if provider_edge_is_configured(&self.provider_edge) {
            validate_provider_edge(&self.provider_edge, &mut errors);
        }

        validate_cors_methods(&self.server.cors.allowed_methods, &mut errors);
        validate_cors_headers(
            &self.server.cors.allowed_headers,
            "allowed_headers",
            &mut errors,
        );
        validate_cors_headers(
            &self.server.cors.exposed_headers,
            "exposed_headers",
            &mut errors,
        );

        errors
    }

    pub(crate) fn ensure_loadable(&self) -> Result<()> {
        self.ensure_cli_loadable()?;

        match self.log.encoding.trim().to_ascii_lowercase().as_str() {
            "json" | "console" => {}
            value => return Err(anyhow!("invalid log encoding '{value}'")),
        }

        match self.log.output.trim().to_ascii_lowercase().as_str() {
            "stdio" | "file" => {}
            value => return Err(anyhow!("invalid log output '{value}'")),
        }

        match self.cache.mode.trim().to_ascii_lowercase().as_str() {
            "memory" | "redis" | "two-level" => {}
            value => return Err(anyhow!("invalid cache mode '{value}'")),
        }

        if self.metrics.enabled {
            match self
                .metrics
                .exporter
                .exporter_type
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "stdout" | "otlpgrpc" | "otlphttp" => {}
                value => return Err(anyhow!("invalid metrics exporter type '{value}'")),
            }
        }

        ensure_cors_methods(&self.server.cors.allowed_methods)?;
        ensure_cors_headers(&self.server.cors.allowed_headers, "allowed_headers")?;
        ensure_cors_headers(&self.server.cors.exposed_headers, "exposed_headers")?;
        if provider_edge_is_configured(&self.provider_edge) {
            ensure_provider_edge(&self.provider_edge)?;
        }

        Ok(())
    }

    fn ensure_duration_fields_parse(&self) -> Result<()> {
        for (field, value) in self.duration_fields() {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }

            humantime::parse_duration(trimmed)
                .with_context(|| format!("invalid duration for {field}: {trimmed}"))?;
        }

        Ok(())
    }

    fn duration_fields(&self) -> Vec<(&'static str, &str)> {
        vec![
            ("server.read_timeout", &self.server.read_timeout),
            ("server.request_timeout", &self.server.request_timeout),
            (
                "server.llm_request_timeout",
                &self.server.llm_request_timeout,
            ),
            (
                "server.dashboard.all_time_token_stats_soft_ttl",
                &self.server.dashboard.all_time_token_stats_soft_ttl,
            ),
            (
                "server.dashboard.all_time_token_stats_hard_ttl",
                &self.server.dashboard.all_time_token_stats_hard_ttl,
            ),
            ("server.cors.max_age", &self.server.cors.max_age),
            (
                "provider_quota.check_interval",
                &self.provider_quota.check_interval,
            ),
            ("cache.memory.expiration", &self.cache.memory.expiration),
            (
                "cache.memory.cleanup_interval",
                &self.cache.memory.cleanup_interval,
            ),
            ("cache.redis.expiration", &self.cache.redis.expiration),
        ]
    }
}

fn validate_provider_edge(config: &crate::types::ProviderEdgeConfig, errors: &mut Vec<String>) {
    validate_required_string(
        "provider_edge.codex.authorize_url",
        &config.codex.authorize_url,
        errors,
    );
    validate_required_string(
        "provider_edge.codex.token_url",
        &config.codex.token_url,
        errors,
    );
    validate_required_string(
        "provider_edge.codex.client_id",
        &config.codex.client_id,
        errors,
    );
    validate_required_string(
        "provider_edge.codex.redirect_uri",
        &config.codex.redirect_uri,
        errors,
    );
    validate_required_string("provider_edge.codex.scopes", &config.codex.scopes, errors);
    validate_required_string(
        "provider_edge.codex.user_agent",
        &config.codex.user_agent,
        errors,
    );

    validate_required_string(
        "provider_edge.claudecode.authorize_url",
        &config.claudecode.authorize_url,
        errors,
    );
    validate_required_string(
        "provider_edge.claudecode.token_url",
        &config.claudecode.token_url,
        errors,
    );
    validate_required_string(
        "provider_edge.claudecode.client_id",
        &config.claudecode.client_id,
        errors,
    );
    validate_required_string(
        "provider_edge.claudecode.redirect_uri",
        &config.claudecode.redirect_uri,
        errors,
    );
    validate_required_string(
        "provider_edge.claudecode.scopes",
        &config.claudecode.scopes,
        errors,
    );
    validate_required_string(
        "provider_edge.claudecode.user_agent",
        &config.claudecode.user_agent,
        errors,
    );

    validate_required_string(
        "provider_edge.antigravity.authorize_url",
        &config.antigravity.authorize_url,
        errors,
    );
    validate_required_string(
        "provider_edge.antigravity.token_url",
        &config.antigravity.token_url,
        errors,
    );
    validate_required_string(
        "provider_edge.antigravity.client_id",
        &config.antigravity.client_id,
        errors,
    );
    validate_required_string(
        "provider_edge.antigravity.client_secret",
        &config.antigravity.client_secret,
        errors,
    );
    validate_required_string(
        "provider_edge.antigravity.redirect_uri",
        &config.antigravity.redirect_uri,
        errors,
    );
    validate_required_string(
        "provider_edge.antigravity.scopes",
        &config.antigravity.scopes,
        errors,
    );
    if config.antigravity.load_endpoints.is_empty() {
        errors.push("provider_edge.antigravity.load_endpoints cannot be empty".to_owned());
    }
    validate_required_string(
        "provider_edge.antigravity.user_agent",
        &config.antigravity.user_agent,
        errors,
    );
    validate_required_string(
        "provider_edge.antigravity.client_metadata",
        &config.antigravity.client_metadata,
        errors,
    );

    validate_required_string(
        "provider_edge.copilot.device_code_url",
        &config.copilot.device_code_url,
        errors,
    );
    validate_required_string(
        "provider_edge.copilot.access_token_url",
        &config.copilot.access_token_url,
        errors,
    );
    validate_required_string(
        "provider_edge.copilot.client_id",
        &config.copilot.client_id,
        errors,
    );
    validate_required_string("provider_edge.copilot.scope", &config.copilot.scope, errors);
}

fn ensure_provider_edge(config: &crate::types::ProviderEdgeConfig) -> Result<()> {
    ensure_required_string(
        "provider_edge.codex.authorize_url",
        &config.codex.authorize_url,
    )?;
    ensure_required_string("provider_edge.codex.token_url", &config.codex.token_url)?;
    ensure_required_string("provider_edge.codex.client_id", &config.codex.client_id)?;
    ensure_required_string(
        "provider_edge.codex.redirect_uri",
        &config.codex.redirect_uri,
    )?;
    ensure_required_string("provider_edge.codex.scopes", &config.codex.scopes)?;
    ensure_required_string("provider_edge.codex.user_agent", &config.codex.user_agent)?;

    ensure_required_string(
        "provider_edge.claudecode.authorize_url",
        &config.claudecode.authorize_url,
    )?;
    ensure_required_string(
        "provider_edge.claudecode.token_url",
        &config.claudecode.token_url,
    )?;
    ensure_required_string(
        "provider_edge.claudecode.client_id",
        &config.claudecode.client_id,
    )?;
    ensure_required_string(
        "provider_edge.claudecode.redirect_uri",
        &config.claudecode.redirect_uri,
    )?;
    ensure_required_string("provider_edge.claudecode.scopes", &config.claudecode.scopes)?;
    ensure_required_string(
        "provider_edge.claudecode.user_agent",
        &config.claudecode.user_agent,
    )?;

    ensure_required_string(
        "provider_edge.antigravity.authorize_url",
        &config.antigravity.authorize_url,
    )?;
    ensure_required_string(
        "provider_edge.antigravity.token_url",
        &config.antigravity.token_url,
    )?;
    ensure_required_string(
        "provider_edge.antigravity.client_id",
        &config.antigravity.client_id,
    )?;
    ensure_required_string(
        "provider_edge.antigravity.client_secret",
        &config.antigravity.client_secret,
    )?;
    ensure_required_string(
        "provider_edge.antigravity.redirect_uri",
        &config.antigravity.redirect_uri,
    )?;
    ensure_required_string(
        "provider_edge.antigravity.scopes",
        &config.antigravity.scopes,
    )?;
    if config.antigravity.load_endpoints.is_empty() {
        return Err(anyhow!(
            "provider_edge.antigravity.load_endpoints cannot be empty"
        ));
    }
    ensure_required_string(
        "provider_edge.antigravity.user_agent",
        &config.antigravity.user_agent,
    )?;
    ensure_required_string(
        "provider_edge.antigravity.client_metadata",
        &config.antigravity.client_metadata,
    )?;

    ensure_required_string(
        "provider_edge.copilot.device_code_url",
        &config.copilot.device_code_url,
    )?;
    ensure_required_string(
        "provider_edge.copilot.access_token_url",
        &config.copilot.access_token_url,
    )?;
    ensure_required_string("provider_edge.copilot.client_id", &config.copilot.client_id)?;
    ensure_required_string("provider_edge.copilot.scope", &config.copilot.scope)?;

    Ok(())
}

fn provider_edge_is_configured(config: &crate::types::ProviderEdgeConfig) -> bool {
    [
        config.codex.authorize_url.as_str(),
        config.codex.token_url.as_str(),
        config.codex.client_id.as_str(),
        config.codex.redirect_uri.as_str(),
        config.codex.scopes.as_str(),
        config.codex.user_agent.as_str(),
        config.claudecode.authorize_url.as_str(),
        config.claudecode.token_url.as_str(),
        config.claudecode.client_id.as_str(),
        config.claudecode.redirect_uri.as_str(),
        config.claudecode.scopes.as_str(),
        config.claudecode.user_agent.as_str(),
        config.antigravity.authorize_url.as_str(),
        config.antigravity.token_url.as_str(),
        config.antigravity.client_id.as_str(),
        config.antigravity.client_secret.as_str(),
        config.antigravity.redirect_uri.as_str(),
        config.antigravity.scopes.as_str(),
        config.antigravity.user_agent.as_str(),
        config.antigravity.client_metadata.as_str(),
        config.copilot.device_code_url.as_str(),
        config.copilot.access_token_url.as_str(),
        config.copilot.client_id.as_str(),
        config.copilot.scope.as_str(),
    ]
    .iter()
    .any(|value| !value.trim().is_empty())
        || !config.antigravity.load_endpoints.is_empty()
}

fn validate_required_string(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} cannot be empty"));
    }
}

fn ensure_required_string(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("{field} cannot be empty"));
    }
    Ok(())
}

fn ensure_log_level(level: &str) -> Result<()> {
    match level.trim().to_ascii_lowercase().as_str() {
        "debug" | "info" | "warn" | "warning" | "error" | "panic" | "fatal" => Ok(()),
        value => Err(anyhow!("invalid log level '{value}'")),
    }
}

fn validate_cors_methods(methods: &[String], errors: &mut Vec<String>) {
    for method in methods {
        if method.trim().is_empty() {
            errors.push("server.cors.allowed_methods contains empty method".to_owned());
            continue;
        }
        if Method::from_bytes(method.as_bytes()).is_err() {
            errors.push(format!(
                "server.cors.allowed_methods contains invalid method '{method}'"
            ));
        }
    }
}

fn validate_cors_headers(headers: &[String], field_name: &str, errors: &mut Vec<String>) {
    for header in headers {
        if header.trim().is_empty() {
            errors.push(format!(
                "server.cors.{} contains empty header name",
                field_name
            ));
            continue;
        }
        if HeaderName::from_bytes(header.as_bytes()).is_err() {
            errors.push(format!(
                "server.cors.{} contains invalid header name '{header}'",
                field_name
            ));
        }
    }
}

fn ensure_cors_methods(methods: &[String]) -> Result<()> {
    for method in methods {
        if method.trim().is_empty() {
            return Err(anyhow!("server.cors.allowed_methods contains empty method"));
        }
        if Method::from_bytes(method.as_bytes()).is_err() {
            return Err(anyhow!(
                "server.cors.allowed_methods contains invalid method '{}'",
                method
            ));
        }
    }
    Ok(())
}

fn ensure_cors_headers(headers: &[String], field_name: &str) -> Result<()> {
    for header in headers {
        if header.trim().is_empty() {
            return Err(anyhow!(
                "server.cors.{} contains empty header name",
                field_name
            ));
        }
        if HeaderName::from_bytes(header.as_bytes()).is_err() {
            return Err(anyhow!(
                "server.cors.{} contains invalid header name '{}'",
                field_name,
                header
            ));
        }
    }
    Ok(())
}
