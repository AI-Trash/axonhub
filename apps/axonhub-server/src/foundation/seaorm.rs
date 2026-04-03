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
    Sqlite { dsn: String, instance_id: u64 },
    Postgres { dsn: String, instance_id: u64 },
}

static NEXT_FACTORY_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

fn next_factory_instance_id() -> u64 {
    NEXT_FACTORY_INSTANCE_ID.fetch_add(1, Ordering::Relaxed)
}

impl SeaOrmConnectionFactory {
    pub(crate) fn sqlite(dsn: String) -> Self {
        Self::Sqlite {
            dsn,
            instance_id: next_factory_instance_id(),
        }
    }

    pub(crate) fn postgres(dsn: String) -> Self {
        Self::Postgres {
            dsn,
            instance_id: next_factory_instance_id(),
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

    pub(crate) fn runtime_dsn(&self) -> String {
        match self {
            Self::Sqlite { dsn, .. } => {
                if dsn == ":memory:" {
                    "sqlite::memory:".to_owned()
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
        let mut options = ConnectOptions::new(self.runtime_dsn());
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .idle_timeout(Duration::from_secs(8))
            .max_lifetime(Duration::from_secs(30))
            .sqlx_logging(false);

        Database::connect(options).await
    }

    pub(crate) async fn connect_migrated(&self) -> Result<DatabaseConnection, DbErr> {
        let db = self.connect().await?;
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
