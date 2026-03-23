use axonhub_http::{ThreadContext, TraceContext};
use rusqlite::{params, Connection, OptionalExtension};

use super::{shared::SqliteConnectionFactory, system::ensure_trace_tables};

#[derive(Debug, Clone)]
pub struct TraceContextStore {
    connection_factory: SqliteConnectionFactory,
}

impl TraceContextStore {
    pub(crate) fn new(connection_factory: SqliteConnectionFactory) -> Self {
        Self { connection_factory }
    }

    #[cfg(test)]
    pub fn ensure_schema(&self) -> rusqlite::Result<()> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)
    }

    pub fn get_or_create_thread(
        &self,
        project_id: i64,
        thread_id: &str,
    ) -> rusqlite::Result<ThreadContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        get_or_create_thread(&connection, project_id, thread_id)
    }

    pub fn get_or_create_trace(
        &self,
        project_id: i64,
        trace_id: &str,
        thread_db_id: Option<i64>,
    ) -> rusqlite::Result<TraceContext> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        get_or_create_trace(&connection, project_id, trace_id, thread_db_id)
    }

    pub fn list_traces_by_project(&self, project_id: i64) -> rusqlite::Result<Vec<TraceContext>> {
        let connection = self.connection_factory.open(true)?;
        ensure_trace_tables(&connection)?;
        let mut statement = connection.prepare(
            "SELECT id, trace_id, project_id, thread_id
             FROM traces
             WHERE project_id = ?1
             ORDER BY id DESC",
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(TraceContext {
                id: row.get(0)?,
                trace_id: row.get(1)?,
                project_id: row.get(2)?,
                thread_id: row.get(3)?,
            })
        })?;
        rows.collect()
    }
}

pub(crate) fn get_or_create_thread(
    connection: &Connection,
    project_id: i64,
    thread_id: &str,
) -> rusqlite::Result<ThreadContext> {
    let existing = connection
        .query_row(
            "SELECT id, thread_id, project_id FROM threads WHERE thread_id = ?1 LIMIT 1",
            [thread_id],
            |row| {
                Ok(ThreadContext {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    project_id: row.get(2)?,
                })
            },
        )
        .optional()?;

    if let Some(thread) = existing {
        if thread.project_id == project_id {
            return Ok(thread);
        }
        return Err(rusqlite::Error::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO threads (project_id, thread_id) VALUES (?1, ?2)",
        params![project_id, thread_id],
    )?;

    Ok(ThreadContext {
        id: connection.last_insert_rowid(),
        thread_id: thread_id.to_owned(),
        project_id,
    })
}

pub(crate) fn get_or_create_trace(
    connection: &Connection,
    project_id: i64,
    trace_id: &str,
    thread_db_id: Option<i64>,
) -> rusqlite::Result<TraceContext> {
    let existing = connection
        .query_row(
            "SELECT id, trace_id, project_id, thread_id FROM traces WHERE trace_id = ?1 LIMIT 1",
            [trace_id],
            |row| {
                Ok(TraceContext {
                    id: row.get(0)?,
                    trace_id: row.get(1)?,
                    project_id: row.get(2)?,
                    thread_id: row.get(3)?,
                })
            },
        )
        .optional()?;

    if let Some(trace) = existing {
        if trace.project_id == project_id
            && (thread_db_id.is_none() || trace.thread_id == thread_db_id)
        {
            return Ok(trace);
        }
        return Err(rusqlite::Error::InvalidQuery);
    }

    connection.execute(
        "INSERT INTO traces (project_id, trace_id, thread_id) VALUES (?1, ?2, ?3)",
        params![project_id, trace_id, thread_db_id],
    )?;

    Ok(TraceContext {
        id: connection.last_insert_rowid(),
        trace_id: trace_id.to_owned(),
        project_id,
        thread_id: thread_db_id,
    })
}
