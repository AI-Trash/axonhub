use anyhow::{Context, Result};
use actix_web::{HttpServer, dev::ServerHandle};
use axonhub_config::load;
use axonhub_http::{
    HttpCorsSettings, HttpMetricsCapability, HttpState, TraceConfig,
    router_with_metrics_and_base_path,
};
use chrono::{Datelike, Duration as ChronoDuration, Local, Timelike};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::process;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

use super::build_info::version;
use super::capabilities::build_server_capabilities;
use super::metrics::MetricsRuntime;
use super::tracing::init_tracing;
use crate::foundation::{
    admin::{BackupFrequencySetting, StoredAutoBackupSettings, StoredVideoStorageSettings},
    admin_operational::SeaOrmOperationalService,
    seaorm::SeaOrmConnectionFactory,
    shared::current_unix_timestamp,
};

pub(crate) const ACTIX_REQUEST_HEAD_TIMEOUT_FALLBACK: Duration = Duration::from_secs(5);

fn runtime_cors_settings(config: &axonhub_config::CorsConfig) -> HttpCorsSettings {
    let max_age_seconds = humantime::parse_duration(&config.max_age)
        .ok()
        .and_then(|duration| duration.as_secs().try_into().ok());

    HttpCorsSettings {
        enabled: config.enabled,
        debug: config.debug,
        allowed_origins: config.allowed_origins.clone(),
        allowed_methods: config.allowed_methods.clone(),
        allowed_headers: config.allowed_headers.clone(),
        exposed_headers: config.exposed_headers.clone(),
        allow_credentials: config.allow_credentials,
        max_age_seconds,
    }
}

pub(crate) async fn start_server() -> Result<()> {
    let loaded = load().unwrap_or_else(|error| {
        eprintln!("Failed to load config: {error}");
        process::exit(1);
    });
    let tracing_runtime = init_tracing(
        &loaded.config.log,
        &loaded.config.db,
        &loaded.config.traces,
        &loaded.config.server.name,
    )?;
    let port: u16 = loaded
        .config
        .server
        .port
        .try_into()
        .context("server.port must be between 1 and 65535")?;

    let address = format!("{}:{port}", loaded.config.server.host);
    let request_timeout = parse_duration_setting(loaded.config.server.request_timeout.as_str());
    let read_timeout = parse_duration_setting(loaded.config.server.read_timeout.as_str());
    let llm_request_timeout = parse_duration_setting(loaded.config.server.llm_request_timeout.as_str());
    let capabilities = build_server_capabilities(
        &loaded.config.db.dsn,
        loaded.config.db.debug,
        loaded.config.server.api.auth.allow_no_auth,
        version(),
        llm_request_timeout,
    );
    let state = HttpState {
        service_name: loaded.config.server.name.clone(),
        version: version().to_owned(),
        config_source: loaded
            .source
            .as_ref()
            .map(|path| path.display().to_string()),
        system_bootstrap: capabilities.system_bootstrap,
        identity: capabilities.identity,
        request_context: capabilities.request_context,
        openai_v1: capabilities.openai_v1,
        admin: capabilities.admin,
        admin_graphql: capabilities.admin_graphql,
        openapi_graphql: capabilities.openapi_graphql,
        oauth_provider_admin: capabilities.oauth_provider_admin,
        allow_no_auth: loaded.config.server.api.auth.allow_no_auth,
        cors: runtime_cors_settings(&loaded.config.server.cors),
        request_timeout,
        llm_request_timeout,
        trace_config: TraceConfig {
            thread_header: Some(loaded.config.server.trace.thread_header.clone()),
            trace_header: Some(loaded.config.server.trace.trace_header.clone()),
            request_header: Some(loaded.config.server.trace.request_header.clone()),
            extra_trace_headers: loaded.config.server.trace.extra_trace_headers.clone(),
            extra_trace_body_fields: loaded.config.server.trace.extra_trace_body_fields.clone(),
            claude_code_trace_enabled: loaded.config.server.trace.claude_code_trace_enabled,
            codex_trace_enabled: loaded.config.server.trace.codex_trace_enabled,
        },
    };

    let metrics_runtime = MetricsRuntime::new(&loaded.config.metrics, &loaded.config.server.name)?;
    let http_metrics = if let Some(metrics_runtime) = metrics_runtime.as_ref() {
        HttpMetricsCapability::Available {
            recorder: metrics_runtime.recorder(),
        }
    } else {
        HttpMetricsCapability::Disabled
    };

    let state_for_server = state.clone();
    let base_path = loaded.config.server.base_path.clone();
    let server = HttpServer::new(move || {
        router_with_metrics_and_base_path(
            state_for_server.clone(),
            http_metrics.clone(),
            &base_path,
        )
    })
    .client_request_timeout(server_request_head_timeout(read_timeout, request_timeout))
    .disable_signals()
    .bind(&address)
    .with_context(|| format!("Failed to bind {address}"))?;

    let listener_address = server.addrs().first().copied().context("No HTTP listener address bound")?;
    let service_name = loaded.config.server.name.clone();
    let background_runtime = Some(BackgroundOperationalRuntime::start(
        background_operational_db_factory(&loaded.config.db),
        &loaded.config.gc,
        &loaded.config.provider_quota,
    ));

    for line in startup_messages(
        &service_name,
        listener_address,
        loaded.config.metrics.enabled,
    ) {
        tracing::info!("{line}");
    }

    let server = server.run();
    let server_handle = server.handle();
    tracing::info!(listen.address = %listener_address, "http server started");
    let server_result = run_server_with_shutdown(server, server_handle)
        .await
        .context("HTTP server exited unexpectedly");

    if let Some(background_runtime) = background_runtime {
        background_runtime.shutdown();
    }

    if let Some(metrics_runtime) = metrics_runtime {
        metrics_runtime.shutdown()?;
    }

    tracing_runtime.shutdown()?;

    server_result
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn run_server_with_shutdown(
    server: actix_web::dev::Server,
    handle: ServerHandle,
) -> std::io::Result<()> {
    tokio::select! {
        result = server => result,
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received");
            handle.stop(true).await;
            Ok(())
        }
    }
}

#[derive(Debug)]
struct BackgroundOperationalRuntime {
    shutdown: Arc<AtomicBool>,
    tasks: Vec<thread::JoinHandle<()>>,
}

impl BackgroundOperationalRuntime {
    fn start(
        db: SeaOrmConnectionFactory,
        gc: &axonhub_config::GcConfig,
        provider_quota: &axonhub_config::ProviderQuotaConfig,
    ) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut tasks = Vec::new();

        if let Some(gc_schedule) = gc_schedule_from_cron(gc.cron.as_str()) {
            tasks.push(spawn_gc_scheduler(
                db.clone(),
                gc_schedule,
                gc.vacuum_enabled,
                shutdown.clone(),
            ));
        }

        if let Some(check_interval) = parse_duration_setting(provider_quota.check_interval.as_str()) {
            tasks.push(spawn_provider_quota_scheduler(
                db.clone(),
                check_interval,
                shutdown.clone(),
            ));
        }

        tasks.push(spawn_video_storage_scheduler(db.clone(), shutdown.clone()));
        tasks.push(spawn_channel_probe_scheduler(db.clone(), shutdown.clone()));
        tasks.push(spawn_auto_backup_scheduler(db, shutdown.clone()));

        Self { shutdown, tasks }
    }

    fn shutdown(self) {
        self.shutdown.store(true, Ordering::Relaxed);
        for task in self.tasks {
            let _ = task.join();
        }
    }
}

fn spawn_gc_scheduler(
    db: SeaOrmConnectionFactory,
    schedule: GcSchedule,
    vacuum_enabled: bool,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            let wait_duration = next_gc_wait_duration(&schedule);
            if wait_for_shutdown_or_timeout(wait_duration, &shutdown) {
                break;
            }

            if let Err(error) = run_gc_scheduler_tick(db.clone(), vacuum_enabled) {
                tracing::warn!(error = %error, "scheduled gc cleanup failed");
            }
        }
    })
}

fn spawn_provider_quota_scheduler(
    db: SeaOrmConnectionFactory,
    interval: Duration,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run_periodic_task(interval, shutdown, move || {
            let db = db.clone();
            move || {
                if let Err(error) = run_provider_quota_scheduler_tick(db.clone(), interval) {
                    tracing::warn!(error = %error, "scheduled provider quota check failed");
                }
            }
        });
    })
}

fn spawn_auto_backup_scheduler(
    db: SeaOrmConnectionFactory,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            let wait_duration = next_two_am_wait_duration(Local::now());
            if wait_for_shutdown_or_timeout(wait_duration, &shutdown) {
                break;
            }

            let settings = match SeaOrmOperationalService::new(db.clone()).auto_backup_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    tracing::warn!(error = %error, "failed to load auto backup settings");
                    continue;
                }
            };

            if !auto_backup_should_run_now(&settings, Local::now()) {
                continue;
            }

            if let Err(error) = run_auto_backup_scheduler_tick(db.clone()) {
                tracing::warn!(error = %error, "scheduled auto backup failed");
            }
        }
    })
}

fn spawn_video_storage_scheduler(
    db: SeaOrmConnectionFactory,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            let settings = match SeaOrmOperationalService::new(db.clone()).video_storage_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    tracing::warn!(error = %error, "failed to load video storage settings");
                    if wait_for_shutdown_or_timeout(Duration::from_secs(60), &shutdown) {
                        break;
                    }
                    continue;
                }
            };

            let wait_duration = video_storage_wait_duration(&settings);
            if wait_for_shutdown_or_timeout(wait_duration, &shutdown) {
                break;
            }

            if let Err(error) = run_video_storage_scheduler_tick(db.clone()) {
                tracing::warn!(error = %error, "scheduled video storage scan failed");
            }
        }
    })
}

fn spawn_channel_probe_scheduler(
    db: SeaOrmConnectionFactory,
    shutdown: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_aligned_timestamp = None;
        loop {
            if wait_for_shutdown_or_timeout(Duration::from_secs(60), &shutdown) {
                break;
            }

            let settings = match SeaOrmOperationalService::new(db.clone()).system_channel_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    tracing::warn!(error = %error, "failed to load channel probe settings");
                    continue;
                }
            };
            if !settings.probe.enabled {
                last_aligned_timestamp = None;
                continue;
            }

            let aligned_timestamp = channel_probe_aligned_timestamp(settings.probe.interval_minutes());
            if last_aligned_timestamp == Some(aligned_timestamp) {
                continue;
            }

            match run_channel_probe_scheduler_tick(db.clone()) {
                Ok(_) => last_aligned_timestamp = Some(aligned_timestamp),
                Err(error) => {
                    tracing::warn!(error = %error, "scheduled channel probe sampling failed");
                }
            }
        }
    })
}

fn run_periodic_task<Build, Task>(
    interval: Duration,
    shutdown: Arc<AtomicBool>,
    build: Build,
) where
    Build: Fn() -> Task,
    Task: FnOnce(),
{
    loop {
        if wait_for_shutdown_or_timeout(interval, &shutdown) {
            break;
        }
        build()();
    }
}

fn wait_for_shutdown_or_timeout(
    duration: Duration,
    shutdown: &AtomicBool,
) -> bool {
    if shutdown.load(Ordering::Relaxed) {
        return true;
    }

    let sleep_step = Duration::from_millis(100).min(duration.max(Duration::from_millis(1)));
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if shutdown.load(Ordering::Relaxed) {
            return true;
        }

        let current_step = sleep_step.min(remaining);
        thread::sleep(current_step);
        remaining = remaining.saturating_sub(current_step);
    }

    shutdown.load(Ordering::Relaxed)
}

fn background_operational_db_factory(db: &axonhub_config::DbConfig) -> SeaOrmConnectionFactory {
    SeaOrmConnectionFactory::postgres_with_debug(db.dsn.clone(), db.debug)
}

pub(crate) fn parse_duration_setting(value: &str) -> Option<Duration> {
    humantime::parse_duration(value).ok().filter(|duration| !duration.is_zero())
}

pub(crate) fn server_request_head_timeout(
    read_timeout: Option<Duration>,
    request_timeout: Option<Duration>,
) -> Duration {
    match (read_timeout, request_timeout) {
        (Some(read_timeout), Some(request_timeout)) => read_timeout.max(request_timeout),
        (Some(read_timeout), None) => read_timeout,
        (None, Some(request_timeout)) => request_timeout,
        (None, None) => ACTIX_REQUEST_HEAD_TIMEOUT_FALLBACK,
    }
}

pub(crate) fn run_gc_scheduler_tick(
    db: SeaOrmConnectionFactory,
    vacuum_enabled: bool,
) -> Result<crate::foundation::admin::StoredGcCleanupSummary, String> {
    SeaOrmOperationalService::new(db).run_scheduled_gc_cleanup(vacuum_enabled)
}

pub(crate) fn run_provider_quota_scheduler_tick(
    db: SeaOrmConnectionFactory,
    check_interval: Duration,
) -> Result<usize, String> {
    SeaOrmOperationalService::new(db).run_provider_quota_check_tick(false, check_interval, None)
}

pub(crate) fn run_auto_backup_scheduler_tick(db: SeaOrmConnectionFactory) -> Result<bool, String> {
    let service = SeaOrmOperationalService::new(db);
    let settings = service.auto_backup_settings()?;
    if !auto_backup_should_run_now(&settings, Local::now()) {
        return Ok(false);
    }
    service.trigger_scheduled_backup()?;
    Ok(true)
}

pub(crate) fn run_video_storage_scheduler_tick(db: SeaOrmConnectionFactory) -> Result<usize, String> {
    SeaOrmOperationalService::new(db).run_video_storage_scan_tick()
}

pub(crate) fn run_channel_probe_scheduler_tick(db: SeaOrmConnectionFactory) -> Result<usize, String> {
    SeaOrmOperationalService::new(db).run_channel_probe_sampling_tick()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GcSchedule {
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CronField {
    allowed: BTreeSet<u32>,
}

pub(crate) fn gc_schedule_from_cron(cron: &str) -> Option<GcSchedule> {
    let trimmed = cron.trim();
    if trimmed.is_empty() {
        return None;
    }

    let fields = trimmed.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 5 {
        return None;
    }

    Some(GcSchedule {
        minute: parse_cron_field(fields[0], 0, 59, false)?,
        hour: parse_cron_field(fields[1], 0, 23, false)?,
        day_of_month: parse_cron_field(fields[2], 1, 31, false)?,
        month: parse_cron_field(fields[3], 1, 12, false)?,
        day_of_week: parse_cron_field(fields[4], 0, 6, true)?,
    })
}

pub(crate) fn gc_interval_from_cron(cron: &str) -> Option<Duration> {
    let schedule = gc_schedule_from_cron(cron)?;
    gc_schedule_min_interval(&schedule, Local::now())
}

fn parse_cron_field(
    field: &str,
    min: u32,
    max: u32,
    normalize_seven_to_zero: bool,
) -> Option<CronField> {
    let mut allowed = BTreeSet::new();

    for part in field.split(',') {
        parse_cron_field_part(part.trim(), min, max, normalize_seven_to_zero, &mut allowed)?;
    }

    if allowed.is_empty() {
        return None;
    }

    Some(CronField { allowed })
}

fn parse_cron_field_part(
    part: &str,
    min: u32,
    max: u32,
    normalize_seven_to_zero: bool,
    allowed: &mut BTreeSet<u32>,
) -> Option<()> {
    if part == "*" {
        for value in min..=max {
            allowed.insert(value);
        }
        return Some(());
    }

    let (range_part, step) = if let Some((range_part, step_part)) = part.split_once('/') {
        let step = step_part.parse::<u32>().ok()?;
        if step == 0 {
            return None;
        }
        (range_part, step)
    } else {
        (part, 1)
    };

    let (start, end) = if range_part == "*" {
        (min, max)
    } else if let Some((start_part, end_part)) = range_part.split_once('-') {
        (
            normalize_cron_value(start_part.trim().parse::<u32>().ok()?, min, max, normalize_seven_to_zero)?,
            normalize_cron_value(end_part.trim().parse::<u32>().ok()?, min, max, normalize_seven_to_zero)?,
        )
    } else {
        let value = normalize_cron_value(
            range_part.parse::<u32>().ok()?,
            min,
            max,
            normalize_seven_to_zero,
        )?;
        (value, value)
    };

    if start > end {
        return None;
    }

    let mut value = start;
    while value <= end {
        allowed.insert(value);
        value = match value.checked_add(step) {
            Some(next) => next,
            None => break,
        };
    }

    Some(())
}

fn normalize_cron_value(
    value: u32,
    min: u32,
    max: u32,
    normalize_seven_to_zero: bool,
) -> Option<u32> {
    let normalized = if normalize_seven_to_zero && value == 7 { 0 } else { value };
    (min..=max).contains(&normalized).then_some(normalized)
}

pub(crate) fn auto_backup_wait_duration(settings: &StoredAutoBackupSettings) -> Option<Duration> {
    if !settings.enabled || settings.data_storage_id <= 0 {
        return None;
    }

    let fallback = backup_frequency_interval(settings.frequency);
    let last_backup_at = settings.last_backup_at?;
    let now = crate::foundation::shared::current_unix_timestamp();
    let elapsed = now.saturating_sub(last_backup_at) as u64;
    let target = fallback.as_secs();
    if elapsed >= target {
        return Some(Duration::from_secs(0));
    }
    Some(Duration::from_secs(target - elapsed))
}

pub(crate) fn auto_backup_should_run_now(
    settings: &StoredAutoBackupSettings,
    now: chrono::DateTime<Local>,
) -> bool {
    if !settings.enabled || settings.data_storage_id <= 0 {
        return false;
    }

    if !auto_backup_frequency_matches(settings.frequency, now) {
        return false;
    }

    let Some(last_backup_at) = settings.last_backup_at else {
        return true;
    };

    let elapsed = now.timestamp().saturating_sub(last_backup_at) as u64;
    elapsed >= backup_frequency_interval(settings.frequency).as_secs()
}

fn video_storage_wait_duration(settings: &StoredVideoStorageSettings) -> Duration {
    if !settings.enabled || settings.data_storage_id <= 0 {
        return Duration::from_secs(60);
    }

    Duration::from_secs(u64::try_from(settings.scan_interval_minutes.max(1)).unwrap_or(1) * 60)
}

fn channel_probe_aligned_timestamp(interval_minutes: i32) -> i64 {
    let interval_seconds = i64::from(interval_minutes.max(1)) * 60;
    let now = current_unix_timestamp();
    now - now.rem_euclid(interval_seconds)
}

pub(crate) fn backup_frequency_interval(frequency: BackupFrequencySetting) -> Duration {
    match frequency {
        BackupFrequencySetting::Daily => Duration::from_secs(86_400),
        BackupFrequencySetting::Weekly => Duration::from_secs(7 * 86_400),
        BackupFrequencySetting::Monthly => Duration::from_secs(30 * 86_400),
    }
}

fn gc_schedule_min_interval(
    schedule: &GcSchedule,
    now: chrono::DateTime<Local>,
) -> Option<Duration> {
    let first = next_gc_occurrence(schedule, now)?;
    let second = next_gc_occurrence(schedule, first)?;
    Some(chrono_duration_to_std(second - first))
}

fn next_gc_wait_duration(schedule: &GcSchedule) -> Duration {
    let now = Local::now();
    let Some(next_run) = next_gc_occurrence(schedule, now) else {
        return Duration::from_secs(60);
    };
    chrono_duration_to_std(next_run - now)
}

fn next_gc_occurrence(
    schedule: &GcSchedule,
    now: chrono::DateTime<Local>,
) -> Option<chrono::DateTime<Local>> {
    let mut candidate = now
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))?
        + ChronoDuration::minutes(1);

    for _ in 0..(366 * 24 * 60) {
        if schedule.matches(candidate) {
            return Some(candidate);
        }
        candidate += ChronoDuration::minutes(1);
    }

    None
}

pub(crate) fn next_two_am_wait_duration(now: chrono::DateTime<Local>) -> Duration {
    next_daily_wait_duration(now, 2, 0)
}

fn next_daily_wait_duration(now: chrono::DateTime<Local>, hour: u32, minute: u32) -> Duration {
    let today = now.date_naive();
    let Some(today_target) = today.and_hms_opt(hour, minute, 0) else {
        return Duration::from_secs(60);
    };

    let next_target = if now.naive_local() < today_target {
        today_target
    } else {
        today_target + ChronoDuration::days(1)
    };

    chrono_duration_to_std(next_target - now.naive_local())
}

fn chrono_duration_to_std(duration: ChronoDuration) -> Duration {
    duration
        .to_std()
        .unwrap_or_else(|_| Duration::from_secs(60))
        .max(Duration::from_secs(1))
}

fn auto_backup_frequency_matches(
    frequency: BackupFrequencySetting,
    now: chrono::DateTime<Local>,
) -> bool {
    match frequency {
        BackupFrequencySetting::Daily => true,
        BackupFrequencySetting::Weekly => now.weekday().num_days_from_sunday() == 0,
        BackupFrequencySetting::Monthly => now.day() == 1,
    }
}

impl GcSchedule {
    fn matches(&self, timestamp: chrono::DateTime<Local>) -> bool {
        self.minute.matches(timestamp.minute())
            && self.hour.matches(timestamp.hour())
            && self.day_of_month.matches(timestamp.day())
            && self.month.matches(timestamp.month())
            && self
                .day_of_week
                .matches(timestamp.weekday().num_days_from_sunday())
    }
}

impl CronField {
    fn matches(&self, value: u32) -> bool {
        self.allowed.contains(&value)
    }
}

pub(crate) fn startup_messages(
    service_name: &str,
    listener_address: SocketAddr,
    metrics_enabled: bool,
) -> Vec<String> {
    let mut messages = Vec::new();

    if metrics_enabled {
        messages.push("Metrics exporter initialized for Rust server runtime.".to_owned());
    }

    messages.push(format!(
        "{service_name} listening on http://{listener_address}"
    ));

    messages
}
