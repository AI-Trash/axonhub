use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axonhub_db_migration::{Migrator, MigratorTrait};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr,
    Statement, TransactionTrait,
};

#[derive(Debug, Clone)]
pub(crate) enum SeaOrmConnectionFactory {
    Sqlite {
        dsn: String,
        instance_id: u64,
        sqlx_logging: bool,
    },
    Postgres {
        dsn: String,
        instance_id: u64,
        sqlx_logging: bool,
    },
}

static NEXT_FACTORY_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

fn next_factory_instance_id() -> u64 {
    NEXT_FACTORY_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

impl SeaOrmConnectionFactory {
    pub(crate) fn sqlite(dsn: String) -> Self {
        Self::sqlite_with_debug(dsn, false)
    }

    pub(crate) fn sqlite_with_debug(dsn: String, sqlx_logging: bool) -> Self {
        Self::Sqlite {
            dsn,
            instance_id: next_factory_instance_id(),
            sqlx_logging,
        }
    }

    pub(crate) fn postgres(dsn: String) -> Self {
        Self::postgres_with_debug(dsn, false)
    }

    pub(crate) fn postgres_with_debug(dsn: String, sqlx_logging: bool) -> Self {
        Self::Postgres {
            dsn,
            instance_id: next_factory_instance_id(),
            sqlx_logging,
        }
    }

    pub(crate) fn backend(&self) -> DatabaseBackend {
        match self {
            Self::Sqlite { .. } => DatabaseBackend::Sqlite,
            Self::Postgres { .. } => DatabaseBackend::Postgres,
        }
    }

    pub(crate) fn instance_id(&self) -> u64 {
        match self {
            Self::Sqlite { instance_id, .. } | Self::Postgres { instance_id, .. } => *instance_id,
        }
    }

    fn sqlx_logging(&self) -> bool {
        match self {
            Self::Sqlite { sqlx_logging, .. } | Self::Postgres { sqlx_logging, .. } => *sqlx_logging,
        }
    }

    pub(crate) fn runtime_dsn(&self) -> String {
        match self {
            Self::Sqlite { dsn, .. } => {
                if dsn == ":memory:" {
                    "sqlite::memory:".to_owned()
                } else if dsn.starts_with("file:") {
                    normalize_sqlite_file_dsn(dsn)
                } else if dsn.starts_with("sqlite:") {
                    dsn.clone()
                } else {
                    format!("sqlite://{}?mode=rwc", dsn)
                }
            }
            Self::Postgres { dsn, .. } => dsn.clone(),
        }
    }

    pub(crate) async fn connect(&self) -> Result<DatabaseConnection, DbErr> {
        let runtime_dsn = self.runtime_dsn();
        let backend = self.backend();
        let instance_id = self.instance_id();
        tracing::debug!(
            seaorm.instance_id = instance_id,
            db.backend = ?backend,
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
                db.backend = ?backend,
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
            db.backend = ?self.backend(),
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
            db.backend = ?factory.backend(),
            "bridging SeaORM async work through sync runtime"
        );

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(move || {
                tokio::runtime::Runtime::new()
                    .expect("create runtime for SeaORM sync bridge")
                    .block_on(build(factory))
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

fn normalize_sqlite_file_dsn(dsn: &str) -> String {
    let raw = dsn.strip_prefix("file:").unwrap_or(dsn);
    let (path, query) = raw.split_once('?').unwrap_or((raw, ""));

    let mut params = Vec::new();
    let mut has_mode = false;

    if !query.is_empty() {
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let key = pair.split('=').next().unwrap_or_default();
            if key.starts_with('_') {
                continue;
            }
            if key.eq_ignore_ascii_case("mode") {
                has_mode = true;
            }
            params.push(pair.to_owned());
        }
    }

    if !has_mode {
        params.insert(0, "mode=rwc".to_owned());
    }

    if params.is_empty() {
        format!("sqlite://{path}")
    } else {
        format!("sqlite://{path}?{}", params.join("&"))
    }
}

pub(crate) async fn query_scalar_string<C>(db: &C, backend: DatabaseBackend, sql: &str) -> Result<String, DbErr>
where
    C: ConnectionTrait,
{
    let row = db
        .query_one(Statement::from_string(backend, sql.to_owned()))
        .await?
        .ok_or_else(|| DbErr::RecordNotFound(sql.to_owned()))?;
    row.try_get_by_index(0)
}

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
