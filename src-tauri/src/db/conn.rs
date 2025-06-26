use diesel::{sql_query, sqlite::SqliteConnection, Connection, RunQueryDsl};

pub fn establish_connection() -> SqliteConnection {
    let mut conn = SqliteConnection::establish("input.db").expect("Error connecting to input.db");
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
    conn
} 