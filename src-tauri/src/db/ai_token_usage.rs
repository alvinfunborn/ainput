use diesel::{connection::SimpleConnection, prelude::*};
use log::info;

// ai_token_usage 表结构和 schema

table! {
    ai_token_usage (apikey) {
        apikey -> Text,
        used_token -> BigInt,
    }
}

#[derive(Queryable, Insertable, AsChangeset, Debug, Clone)]
#[diesel(table_name = ai_token_usage)]
pub struct AiTokenUsage {
    pub apikey: String,
    pub used_token: i64,
}

pub fn ensure_ai_token_usage_table(conn: &mut SqliteConnection) {
    conn.batch_execute(r#"
        CREATE TABLE IF NOT EXISTS ai_token_usage (
            apikey TEXT PRIMARY KEY,
            used_token INTEGER
        )
    "#).expect("Failed to create ai_token_usage table");
}

pub fn get_used_token(conn: &mut SqliteConnection, apikey: &str) -> i64 {
    use self::ai_token_usage::dsl::*;
    ensure_ai_token_usage_table(conn);
    ai_token_usage
        .filter(apikey.eq(apikey))
        .select(used_token)
        .first::<i64>(conn)
        .unwrap_or(0)
}

pub fn set_used_token(conn: &mut SqliteConnection, apikey_: &str, value: i64) {
    use self::ai_token_usage::dsl::*;
    ensure_ai_token_usage_table(conn);
    let usage = AiTokenUsage { apikey: apikey_.to_string(), used_token: value };
    diesel::replace_into(ai_token_usage).values(&usage).execute(conn).ok();
}

pub fn increment_used_token(conn: &mut SqliteConnection, apikey_: &str, delta: i64) {
    let current = get_used_token(conn, apikey_);
    set_used_token(conn, apikey_, current + delta);
} 
    
#[tauri::command]
pub fn get_used_token_command(apikey: String) -> i64 {
    let mut conn = crate::db::conn::establish_connection();
    crate::db::ai_token_usage::get_used_token(&mut conn, &apikey)
}