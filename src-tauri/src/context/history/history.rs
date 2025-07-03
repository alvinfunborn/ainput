use crate::db::input_history::{Input, insert_history};
use std::collections::HashSet;
use diesel::prelude::*;
use tantivy::schema::Value;
use super::search;

pub struct InputHistory {
    pub window_id: i64,
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

pub fn save_history(conn: &mut SqliteConnection, input_history: &InputHistory) {
    if input_history.input_content.trim().is_empty() {
        return;
    }
    use crate::db::input_history::input::dsl::*;
    // 查询最后一条记录
    let last: Option<Input> = input
        .order(timestamp.desc())
        .first::<Input>(conn)
        .ok();
    // 将 InputHistory 转换为 Input（只填充有的字段，id 用当前时间戳字符串）
    let new_input = Input {
        id: format!("{}", input_history.timestamp),
        window_id: input_history.window_id.to_string(),
        window_app: input_history.window_app.clone(),
        window_title: input_history.window_title.clone(),
        window_class_name: input_history.window_class_name.clone(),
        window_x: input_history.window_x,
        window_y: input_history.window_y,
        window_width: input_history.window_width,
        window_height: input_history.window_height,
        input_id: input_history.input_id.clone(),
        input_title: input_history.input_title.clone(),
        input_control_type: input_history.input_control_type,
        input_x: input_history.input_x,
        input_y: input_history.input_y,
        input_width: input_history.input_width,
        input_height: input_history.input_height,
        input_content: input_history.input_content.clone(),
        timestamp: input_history.timestamp,
    };
    let is_same = if let Some(last) = last {
        last.window_id == new_input.window_id &&
        last.window_app == new_input.window_app &&
        last.input_id == new_input.input_id &&
        last.input_title == new_input.input_title &&
        last.input_content == new_input.input_content
    } else {
        false
    };
    if !is_same {
        insert_history(conn, &new_input);
    }
}

pub fn get_history(
    conn: &mut SqliteConnection,
    window_id_: &str,
    window_app_: &str,
    window_title_: &str,
    input_id_: &str,
    input_title_: &str,
    input_content_: &str,
) -> QueryResult<Vec<Input>> {
    use crate::db::input_history::input::dsl::*;
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    // 5. 当前content (使用 Tantivy 全文搜索)
    if let Some(mut search_index_guard) = search::get_search_index() {
        if let Some(search_index) = search_index_guard.as_mut() {
            if !input_content_.trim().is_empty() {
                match search_index.search(input_content_, 10) {
                    Ok(docs) => {
                        for doc in docs {
                            if let Some(id_val) = doc.get_first(search_index.id_field).and_then(|v| v.as_str()) {
                                use crate::db::input_history::input::dsl::*;
                                let retrieved_input: QueryResult<Input> = input.filter(id.eq(id_val)).first::<Input>(conn);
                                match retrieved_input {
                                    Ok(retrieved_record) => {
                                        if seen.insert(retrieved_record.id.clone()) {
                                            result.push(retrieved_record);
                                        }
                                    },
                                    Err(e) => {
                                        log::error!("Failed to retrieve full Input from DB for id {}: {:?}", id_val, e);
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => {
                        log::error!("Tantivy search failed: {:?}", e);
                    }
                }
            }
        }
    }

    // // 5. 当前content
    // let q = input
    //     .filter(input_content.like(format!("%{}%", input_content_)))
    //     .order(timestamp.desc())
    //     .limit(10)
    //     .load::<Input>(conn)?;
    // for r in q {
    //     if seen.insert(r.id.clone()) {
    //         result.push(r);
    //     }
    // }

    // 1. 当前window+input
    let q = input
        .filter(window_id.eq(window_id_))
        .filter(input_id.eq(input_id_))
        .order(timestamp.desc())
        .limit(10)
        .load::<Input>(conn)?;
    for r in q {
        if seen.insert(r.id.clone()) {
            result.push(r);
        }
    }

    // 1. 同一window+input(重启窗口可能窗口和元素handle会变, 即id会变)
    let q = input
        .filter(window_title.eq(window_title_))
        .filter(window_id.ne(window_id_))
        .filter(input_title.eq(input_title_))
        .order(timestamp.desc())
        .limit(10)
        .load::<Input>(conn)?;
    for r in q {
        if seen.insert(r.id.clone()) {
            result.push(r);
        }
    }

    // 2. 当前window（不限定input）
    let q = input
        .filter(window_id.eq(window_id_))
        .filter(input_id.ne(input_id_))
        .order(timestamp.desc())
        .limit(5)
        .load::<Input>(conn)?;
    for r in q {
        if seen.insert(r.id.clone()) {
            result.push(r);
        }
    }

     // 2. 同一window（不限定input）
     let q = input
        .filter(window_title.eq(window_title_))
        .filter(window_id.ne(window_id_))
        .filter(input_title.ne(input_title_))
        .order(timestamp.desc())
        .limit(5)
        .load::<Input>(conn)?;
    for r in q {
        if seen.insert(r.id.clone()) {
            result.push(r);
        }
    }

    // 3. 当前app（不限定window）
    let q = input
        .filter(window_app.eq(window_app_))
        .filter(window_title.ne(window_title_))
        .order(timestamp.desc())
        .limit(3)
        .load::<Input>(conn)?;
    for r in q {
        if seen.insert(r.id.clone()) {
            result.push(r);
        }
    }

    // 4. 其它应用
    let q = input
        .filter(window_app.ne(window_app_))
        .order(timestamp.desc())
        .limit(3)
        .load::<Input>(conn)?;
    for r in q {
        if seen.insert(r.id.clone()) {
            result.push(r);
        }
    }

    Ok(result)
}
