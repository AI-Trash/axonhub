use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axonhub_db_migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr, TransactionTrait};

#[derive(Debug, Clone)]
pub(crate) struct SeaOrmConnectionFactory {
    dsn: String,
    instance_id: u64,
    sqlx_logging: bool,
}

static NEXT_FACTORY_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

fn next_factory_instance_id() -> u64 {
    NEXT_FACTORY_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

impl SeaOrmConnectionFactory {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn postgres(dsn: String) -> Self {
        Self::postgres_with_debug(dsn, false)
    }

    pub(crate) fn postgres_with_debug(dsn: String, sqlx_logging: bool) -> Self {
        Self {
            dsn,
            instance_id: next_factory_instance_id(),
            sqlx_logging,
        }
    }

    pub(crate) fn instance_id(&self) -> u64 {
        self.instance_id
    }

    fn sqlx_logging(&self) -> bool {
        self.sqlx_logging
    }

    pub(crate) fn runtime_dsn(&self) -> String {
        self.dsn.clone()
    }

    pub(crate) async fn connect(&self) -> Result<DatabaseConnection, DbErr> {
        let runtime_dsn = self.runtime_dsn();
        let instance_id = self.instance_id();
        tracing::debug!(
            seaorm.instance_id = instance_id,
            db.backend = "postgres",
            db.dsn = %sanitize_runtime_dsn(runtime_dsn.as_str()),
            "opening SeaORM connection"
        );
        let mut options = ConnectOptions::new(runtime_dsn);
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .idle_timeout(Duration::from_secs(8))
            .max_lifetime(Duration::from_secs(30))
            .sqlx_logging(self.sqlx_logging());

        let connection = Database::connect(options).await;
        if let Err(error) = &connection {
            tracing::error!(
                seaorm.instance_id = instance_id,
                db.backend = "postgres",
                error = %error,
                "failed to open SeaORM connection"
            );
        }
        connection
    }

    pub(crate) async fn connect_migrated(&self) -> Result<DatabaseConnection, DbErr> {
        let db = self.connect().await?;
        tracing::debug!(
            seaorm.instance_id = self.instance_id(),
            db.backend = "postgres",
            "running SeaORM migrations"
        );
        Migrator::up(&db, None).await?;
        Ok(db)
    }

    pub(crate) fn run_sync<T, E, Fut, Build>(&self, build: Build) -> Result<T, E>
    where
        T: Send + 'static,
        E: Send + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        Build: FnOnce(Self) -> Fut + Send + 'static,
    {
        let factory = self.clone();
        tracing::debug!(
            seaorm.instance_id = factory.instance_id(),
            db.backend = "postgres",
            "bridging SeaORM async work through sync runtime"
        );

        if tokio::runtime::Handle::try_current().is_ok() {
            let dispatch = tracing::dispatcher::get_default(|dispatch| dispatch.clone());
            let span = tracing::Span::current();
            std::thread::spawn(move || {
                tracing::dispatcher::with_default(&dispatch, || {
                    let _enter = span.enter();
                    tokio::runtime::Runtime::new()
                        .expect("create runtime for SeaORM sync bridge")
                        .block_on(build(factory))
                })
            })
            .join()
            .unwrap_or_else(|_| panic!("SeaORM worker thread panicked"))
        } else {
            tokio::runtime::Runtime::new()
                .expect("create runtime for SeaORM sync bridge")
                .block_on(build(factory))
        }
    }
}

fn sanitize_runtime_dsn(dsn: &str) -> String {
    if let Some((prefix, _)) = dsn.split_once('@') {
        return format!("{prefix}@***");
    }
    dsn.to_owned()
}

#[allow(dead_code)]
pub(crate) async fn with_transaction<T, F, Fut>(
    db: &DatabaseConnection,
    build: F,
) -> Result<T, DbErr>
where
    F: FnOnce(&sea_orm::DatabaseTransaction) -> Fut,
    Fut: Future<Output = Result<T, DbErr>>,
{
    let txn = db.begin().await?;
    let result = build(&txn).await?;
    txn.commit().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{TraceContextExt as _, TracerProvider as _};
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use std::convert::Infallible;
    use std::sync::Mutex;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    #[test]
    pub(crate) fn seaorm_run_sync_preserves_trace_context_across_bridge() {
        let _guard = SEAORM_TRACE_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let provider = SdkTracerProvider::builder().build();
        let tracer = provider.tracer("seaorm-tests");
        let subscriber = Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer));

        tracing::subscriber::with_default(subscriber, || {
            let parent_span = tracing::span!(tracing::Level::INFO, "seaorm-sync-bridge-parent");
            let _enter = parent_span.enter();
            let expected_trace_id = tracing::Span::current()
                .context()
                .span()
                .span_context()
                .trace_id()
                .to_string();

            let factory = SeaOrmConnectionFactory::postgres("postgres://localhost/axonhub".to_owned());
            let bridged_trace_id = tokio::runtime::Runtime::new()
                .expect("create runtime for SeaORM trace bridge test")
                .block_on(async move {
                    factory.run_sync(move |_| async move {
                        Ok::<_, Infallible>(
                            tracing::Span::current()
                                .context()
                                .span()
                                .span_context()
                                .trace_id()
                                .to_string(),
                        )
                    })
                })
                .expect("run sync through tracing bridge");

            assert_ne!(
                expected_trace_id,
                "00000000000000000000000000000000",
                "expected parent span to carry a valid trace id"
            );
            assert_eq!(bridged_trace_id, expected_trace_id);
        });

        let _ = provider.force_flush();
        let _ = provider.shutdown();
    }
}

#[cfg(test)]
static SEAORM_TRACE_TEST_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn seaorm_run_sync_preserves_trace_context_across_bridge_inner() {
    tests::seaorm_run_sync_preserves_trace_context_across_bridge();
}
