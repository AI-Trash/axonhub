use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, ExecResult, QueryResult, Statement, Value};

pub(crate) fn sql_for_backend<'a>(
    backend: DatabaseBackend,
    sqlite: &'a str,
    postgres: &'a str,
    mysql: &'a str,
) -> &'a str {
    match backend {
        DatabaseBackend::Sqlite => sqlite,
        DatabaseBackend::Postgres => postgres,
        DatabaseBackend::MySql => mysql,
    }
}

pub(crate) async fn query_one(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<Value>,
) -> Result<Option<QueryResult>, DbErr> {
    db.query_one(Statement::from_sql_and_values(
        backend,
        sql_for_backend(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
}

pub(crate) async fn query_all(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<Value>,
) -> Result<Vec<QueryResult>, DbErr> {
    db.query_all(Statement::from_sql_and_values(
        backend,
        sql_for_backend(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
}

pub(crate) async fn execute(
    db: &impl ConnectionTrait,
    backend: DatabaseBackend,
    sqlite_sql: &str,
    postgres_sql: &str,
    mysql_sql: &str,
    values: Vec<Value>,
) -> Result<ExecResult, DbErr> {
    db.execute(Statement::from_sql_and_values(
        backend,
        sql_for_backend(backend, sqlite_sql, postgres_sql, mysql_sql),
        values,
    ))
    .await
}

pub(crate) fn last_insert_id(result: &ExecResult, context: &str) -> Result<i64, DbErr> {
    let id = result.last_insert_id();
    if id == 0 {
        Err(DbErr::Custom(format!("missing last insert id for {context}")))
    } else {
        Ok(id as i64)
    }
}
