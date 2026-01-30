use diesel::{sql_query, sqlite::SqliteConnection, Connection, RunQueryDsl};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use log::error;

static DB_CONNECTION: Lazy<Mutex<SqliteConnection>> = Lazy::new(|| {
    let mut conn = SqliteConnection::establish("input.db").expect("Error connecting to input.db");
    // Use DELETE journal mode to avoid -shm/-wal files triggering Tauri dev rebuilds
    // WAL is unnecessary since we use a single shared connection with mutex
    let _ = sql_query("PRAGMA journal_mode=DELETE;").execute(&mut conn);
    let _ = sql_query("PRAGMA busy_timeout=5000;").execute(&mut conn);
    // 自动建表 input
    sql_query(r#"
        CREATE TABLE IF NOT EXISTS input (
            id TEXT PRIMARY KEY,
            window_id TEXT,
            window_app TEXT,
            window_title TEXT,
            window_class_name TEXT,
            window_x INTEGER,
            window_y INTEGER,
            window_width INTEGER,
            window_height INTEGER,
            input_id TEXT,
            input_title TEXT,
            input_control_type INTEGER,
            input_x INTEGER,
            input_y INTEGER,
            input_width INTEGER,
            input_height INTEGER,
            input_content TEXT,
            timestamp BIGINT
        );
    "#).execute(&mut conn).expect("Failed to create input table");
    Mutex::new(conn)
});

/// Get exclusive access to the database connection
pub fn with_connection<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut SqliteConnection) -> R,
{
    match DB_CONNECTION.lock() {
        Ok(mut conn) => Ok(f(&mut conn)),
        Err(e) => {
            error!("[with_connection] failed to lock database: {}", e);
            Err(format!("Database lock failed: {}", e))
        }
    }
}

#[deprecated(note = "Use with_connection() instead for thread-safe access")]
pub fn establish_connection() -> SqliteConnection {
    let mut conn = SqliteConnection::establish("input.db").expect("Error connecting to input.db");
    let _ = sql_query("PRAGMA journal_mode=DELETE;").execute(&mut conn);
    let _ = sql_query("PRAGMA busy_timeout=5000;").execute(&mut conn);
    sql_query(r#"
        CREATE TABLE IF NOT EXISTS input (
            id TEXT PRIMARY KEY,
            window_id TEXT,
            window_app TEXT,
            window_title TEXT,
            window_class_name TEXT,
            window_x INTEGER,
            window_y INTEGER,
            window_width INTEGER,
            window_height INTEGER,
            input_id TEXT,
            input_title TEXT,
            input_control_type INTEGER,
            input_x INTEGER,
            input_y INTEGER,
            input_width INTEGER,
            input_height INTEGER,
            input_content TEXT,
            timestamp BIGINT
        );
    "#).execute(&mut conn).expect("Failed to create input table");
    conn
}