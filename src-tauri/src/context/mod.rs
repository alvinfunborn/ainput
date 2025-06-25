pub mod history;

use log::debug;
use serde::Serialize;

use crate::os::clipboard::windows_clipboard::get_clipboard_history;
use crate::os::element::ui_automation::FocusedInput;
use crate::db::conn::establish_connection;
use crate::context::history::history::get_history;

#[derive(Debug, Clone, Serialize)]
pub struct Context {
    pub app: InputContext,
    pub history: Vec<InputContext>,
    pub clipboard_history: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputContext {
    pub window_id: String,
    pub window_app: String,
    pub window_title: String,
    pub input_id: String,
    pub input_title: String,
    pub input_content: String,
}

impl Context {
    pub fn new(app: &FocusedInput) -> Option<Self> {
        let app_context = InputContext {
            window_id: app.window_element.id.to_string(),
            window_app: app.window_element.app.clone(),
            window_title: app.window_element.title.clone(),
            input_id: app.input_element.id.clone(),
            input_title: app.input_element.text.clone(),
            input_content: app.input_element.content.clone(),
        };
        let mut conn = establish_connection();
        debug!("[Context::new] app: {:?}", app);
        let history = match get_history(
            &mut conn,
            &app.window_element.id.to_string(),
            &app.window_element.class_name,
            &app.window_element.title,
            &app.input_element.id,
            &app.input_element.text,
            &app.input_element.content,
        ) {
            Ok(history) => history,
            Err(e) => {
                log::error!("get_history error: {:?}", e);
                vec![]
            }
        };
        let context_input_history = history.iter().map(|h| InputContext {
            window_id: h.window_id.clone(),
            window_app: h.window_app.clone(),
            window_title: h.window_title.clone(),
            input_id: h.input_id.clone(),
            input_title: h.input_title.clone(),
            input_content: h.input_content.clone(),
        }).collect::<Vec<InputContext>>();
        debug!("[Context::new] history: {:?}", context_input_history);
        let clipboard_history = get_clipboard_history();
        debug!("[Context::new] clipboard_history: {:?}", clipboard_history);
        Some(Self {
            app: app_context,
            history: context_input_history,
            clipboard_history,
        })
    }
}