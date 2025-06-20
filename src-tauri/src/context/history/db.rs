use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use log::error;
use serde::{Deserialize, Serialize};
use diesel::connection::SimpleConnection;

// diesel schema
// (建议用diesel_cli自动生成，但这里手写)
table! {
    input (id) {
        id -> Text,
        window_id -> Text,
        window_app -> Text,
        window_title -> Text,
        window_class_name -> Text,
        window_x -> Integer,
        window_y -> Integer,
        window_width -> Integer,
        window_height -> Integer,
        input_id -> Text,
        input_title -> Text,
        input_control_type -> Integer,
        input_x -> Integer,
        input_y -> Integer,
        input_width -> Integer,
        input_height -> Integer,
        input_content -> Text,
        timestamp -> BigInt,
    }
}

#[derive(Queryable, Insertable, AsChangeset, Serialize, Deserialize, Debug, Clone)]
#[diesel(table_name = input)]
pub struct Input {
    pub id: String,
    pub window_id: String,
    pub window_app: String,
    pub window_title: String,
    pub window_class_name: String,
    pub window_x: i32,
    pub window_y: i32,
    pub window_width: i32,
    pub window_height: i32,
    pub input_id: String,
    pub input_title: String,
    pub input_control_type: i32,
    pub input_x: i32,
    pub input_y: i32,
    pub input_width: i32,
    pub input_height: i32,
    pub input_content: String,
    pub timestamp: i64,
}

pub fn establish_connection() -> SqliteConnection {
    let mut conn = SqliteConnection::establish("input.db").expect("Error connecting to input.db");
    // 自动建表（如不存在）
    conn.batch_execute(r#"
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
        )
    "#).expect("Failed to create input table");
    conn
}

pub fn insert_history(conn: &mut SqliteConnection, record: &Input) {
    use self::input::dsl::*;
    if let Err(e) = diesel::replace_into(input).values(record).execute(conn) {
        error!("Failed to insert history: {}", e);
    }

    // 自动清理过期历史
    let ttl_days = crate::config::get_config().unwrap().system.history_ttl;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let expire = now - (ttl_days as i64) * 24 * 60 * 60 * 1000;
    use diesel::sql_query;
    if let Err(e) = sql_query("DELETE FROM input WHERE timestamp < ?")
        .bind::<diesel::sql_types::BigInt, _>(expire)
        .execute(conn) {
        error!("Failed to delete expired history: {}", e);
    }
}
