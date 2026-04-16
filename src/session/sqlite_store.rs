use std::sync::Mutex;

use super::{SessionData, SessionStore};
use crate::error::Result;

pub struct SqliteSessionStore {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteSessionStore {
    pub fn open(path: &str) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                thread_id TEXT PRIMARY KEY,
                messages TEXT NOT NULL,
                state TEXT NOT NULL,
                summary TEXT
            );",
        )?;
        // Migration: add summary column if it doesn't exist (for databases created without it)
        let has_summary_column: bool = {
            let stmt = conn.prepare("SELECT summary FROM sessions LIMIT 0");
            stmt.is_ok()
        };
        if !has_summary_column {
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN summary TEXT")?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl SessionStore for SqliteSessionStore {
    fn save(&self, thread_id: &str, data: &SessionData) -> Result<()> {
        let messages_json = serde_json::to_string(&data.messages)?;
        let state_json = serde_json::to_string(&data.state)?;
        let summary_json: Option<String> = data
            .summary
            .as_ref()
            .map(|s| serde_json::to_string(s))
            .transpose()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sessions (thread_id, messages, state, summary) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![thread_id, messages_json, state_json, summary_json],
        )?;
        Ok(())
    }

    fn load(&self, thread_id: &str) -> Result<Option<SessionData>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT messages, state, summary FROM sessions WHERE thread_id = ?1")?;
        let result = stmt.query_row(rusqlite::params![thread_id], |row| {
            let messages_str: String = row.get(0)?;
            let state_str: String = row.get(1)?;
            let summary_str: Option<String> = row.get(2)?;
            Ok((messages_str, state_str, summary_str))
        });

        match result {
            Ok((messages_str, state_str, summary_str)) => {
                let messages: Vec<serde_json::Value> = serde_json::from_str(&messages_str)?;
                let state: serde_json::Value = serde_json::from_str(&state_str)?;
                let summary: Option<String> =
                    summary_str.and_then(|s| serde_json::from_str::<String>(&s).ok());
                Ok(Some(SessionData {
                    messages,
                    state,
                    summary,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
