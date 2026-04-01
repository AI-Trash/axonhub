use anyhow::{anyhow, Context, Result};

use crate::contract::validate_db_dialect;
use crate::types::Config;

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

fn ensure_log_level(level: &str) -> Result<()> {
    match level.trim().to_ascii_lowercase().as_str() {
        "debug" | "info" | "warn" | "warning" | "error" | "panic" | "fatal" => Ok(()),
        value => Err(anyhow!("invalid log level '{value}'")),
    }
}
