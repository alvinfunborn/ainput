pub fn is_edit_element_in_app(app: &str, app_class_name: &str, input_text: &str, control_type: i32, content: &str) -> bool {
    let ui_automation_config = crate::config::get_config().unwrap().ui_automation;
    if ui_automation_config.ignore_apps.contains(&app.to_string()) {
        return false;
    }
    if ui_automation_config.default_edit_control_types.contains(&control_type) {
        return true;
    }
    if ui_automation_config.hastext_edit_control_types.contains(&control_type) {
        return !content.is_empty();
    }
    if ui_automation_config.app_edit_control_types.contains_key(app) {
        let app_edit_control_types = ui_automation_config.app_edit_control_types.get(app).unwrap();
        if app_edit_control_types.contains(&control_type) {
            return true;
        }
    }
    false
}
