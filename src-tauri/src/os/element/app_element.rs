pub fn edit_in_app(app_class_name: &str, input_text: &str, control_type: i32, content: &str) -> bool {
    match control_type {
        50033 => {
            match app_class_name {
                "XLMAIN" => {
                    return true;
                }
                "PPTFrameClass" => {
                    return true;
                }
                _ => {
                    return false;
                }
            }
        }
        50026 => {
            return !content.is_empty();
        }
        50029 => {
            match app_class_name {
                "XLMAIN" => {
                    return true;
                }
                "PPTFrameClass" => {
                    return true;
                }
                _ => {
                    return false;
                }
            }
        }
        50004 => {
            return true;
        }
        _ => {
            return false;
        }
    }
}
