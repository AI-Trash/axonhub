use anyhow::{anyhow, Context, Result};
use axonhub_config::{DbConfig, LogConfig};
use std::sync::OnceLock;
use tracing_log::LogTracer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

static TRACING_INITIALIZED: OnceLock<()> = OnceLock::new();

pub(crate) fn init_tracing(log: &LogConfig, db: &DbConfig, service_name: &str) -> Result<()> {
    if TRACING_INITIALIZED.get().is_some() {
        return Ok(());
    }

    let filter = build_env_filter(log, db)?;
    let format = tracing_format(log);
    let subscriber = tracing_subscriber::registry().with(filter).with(format);

    let _ = LogTracer::init();
    subscriber
        .try_init()
        .map_err(|error| anyhow!(error.to_string()))?;
    let _ = TRACING_INITIALIZED.set(());

    tracing::info!(
        service.name = %service_name,
        log.level = %normalize_level(log.level.as_str()),
        log.encoding = %log.encoding,
        db.dialect = %db.dialect,
        db.debug = db.debug,
        "tracing initialized"
    );
    Ok(())
}

fn build_env_filter(log: &LogConfig, db: &DbConfig) -> Result<EnvFilter> {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter_directives(log, db)))
        .context("invalid tracing filter directives")
}

fn default_filter_directives(log: &LogConfig, db: &DbConfig) -> String {
    let mut directives = vec![
        normalize_level(log.level.as_str()).to_owned(),
        "actix_web=info".to_owned(),
        "actix_server=info".to_owned(),
    ];

    if log.debug || db.debug {
        directives.push("sqlx=debug".to_owned());
        directives.push("sea_orm=debug".to_owned());
    }

    directives.join(",")
}

fn tracing_format<S>(
    log: &LogConfig,
) -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let base = fmt::layer()
        .with_target(log.debug)
        .with_thread_ids(log.debug)
        .with_thread_names(log.debug)
        .with_span_events(FmtSpan::CLOSE);

    if log.encoding.trim().eq_ignore_ascii_case("console") {
        base.compact().boxed()
    } else {
        base.json().boxed()
    }
}

fn normalize_level(level: &str) -> &'static str {
    match level.trim().to_ascii_lowercase().as_str() {
        "panic" | "fatal" | "error" => "error",
        "warn" | "warning" => "warn",
        "debug" => "debug",
        _ => "info",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_enables_sql_debug_when_db_debug_enabled() {
        let log = LogConfig::default();
        let db = DbConfig {
            debug: true,
            ..DbConfig::default()
        };

        let directives = default_filter_directives(&log, &db);

        assert!(directives.contains("sqlx=debug"));
        assert!(directives.contains("sea_orm=debug"));
    }

    #[test]
    fn default_filter_normalizes_warning_alias() {
        let log = LogConfig {
            level: "warning".to_owned(),
            ..LogConfig::default()
        };

        let directives = default_filter_directives(&log, &DbConfig::default());

        assert!(directives.starts_with("warn,"));
    }
}
