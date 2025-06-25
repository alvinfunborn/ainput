use diesel::{sqlite::SqliteConnection, Connection};

pub fn establish_connection() -> SqliteConnection {
    let mut conn = SqliteConnection::establish("input.db").expect("Error connecting to input.db");
    conn
} 