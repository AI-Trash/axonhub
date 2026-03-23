use anyhow::{anyhow, Context, Result};

use crate::types::Config;

impl Config {
    pub fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.server.port == 0 || self.server.port > 65_535 {
            errors.push("server.port must be between 1 and 65535".to_owned());
        }

        if self.db.dsn.trim().is_empty() {
            errors.push("db.dsn cannot be empty".to_owned());
        }

        if self.log.name.trim().is_empty() {
            errors.push("log.name cannot be empty".to_owned());
        }

        if self.server.cors.enabled && self.server.cors.allowed_origins.is_empty() {
            errors.push(
                "server.cors.allowed_origins cannot be empty when CORS is enabled".to_owned(),
            );
        }

        errors
    }

    pub(crate) fn ensure_loadable(&self) -> Result<()> {
        ensure_log_level(&self.log.level)?;

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
