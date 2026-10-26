use crate::os::WindowElement;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct UIElement {
    pub id: String,
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub width: i32,
    pub height: i32,
    pub window_id: i64,
    pub control_type: i32,
    // element_type: 0-default, 1-window, 2-pane, 3-tab, 4-button, 5-scrollbar
    pub element_type: usize,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusedInput {
    #[cfg(target_os = "macos")]
    #[serde(skip)]
    pub selection: TextRange,
    pub window_element: WindowElement,
    pub input_element: UIElement,
}

impl Eq for FocusedInput {}

impl PartialEq for FocusedInput {
    fn eq(&self, other: &Self) -> bool {
        self.same_selection(other)
            && self.same_focus_labels(other)
            && self.window_element.id == other.window_element.id
            && self.window_element.class_name == other.window_element.class_name
            && self.input_element.id == other.input_element.id
            && self.input_element.control_type == other.input_element.control_type
    }
}

impl FocusedInput {
    fn same_focus_labels(&self, other: &Self) -> bool {
        #[cfg(target_os = "macos")]
        {
            // Browser tab titles and AX labels can change without changing the
            // focused native window/control. They are context, not identity.
            let _ = other;
            true
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.window_element.title == other.window_element.title
                && self.input_element.text == other.input_element.text
        }
    }
    #[cfg(target_os = "macos")]
    pub fn matches_for_insertion(&self, expected: &Self) -> bool {
        self == expected && self.input_element.content == expected.input_element.content
    }

    fn same_selection(&self, other: &Self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.selection == other.selection
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = other;
            true
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextRange {
    pub location: usize,
    pub length: usize,
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    fn focus() -> FocusedInput {
        FocusedInput {
            selection: TextRange {
                location: 3,
                length: 0,
            },
            window_element: WindowElement {
                id: 7,
                app: "TextEdit".into(),
                title: "Fixture".into(),
                class_name: "AXTextArea".into(),
                x: 0,
                y: 0,
                width: 400,
                height: 400,
            },
            input_element: UIElement {
                id: "123:4".into(),
                text: "".into(),
                x: 20,
                y: 20,
                z: 0,
                width: 200,
                height: 100,
                window_id: 7,
                control_type: 50004,
                element_type: 0,
                content: "abc".into(),
            },
        }
    }
    #[test]
    fn changing_browser_titles_and_labels_does_not_replace_a_completion() {
        let original = focus();
        let mut updated = original.clone();
        updated.window_element.title = "Background status updated".into();
        updated.input_element.text = "New accessible label".into();
        assert_eq!(original, updated);
        assert!(updated.matches_for_insertion(&original));
    }
    #[test]
    fn insertion_rejects_changed_window_control_caret_or_text() {
        let original = focus();
        assert!(original.matches_for_insertion(&original));
        let mut other = original.clone();
        other.window_element.id += 1;
        assert!(!other.matches_for_insertion(&original));
        let mut other = original.clone();
        other.input_element.id = "123:5".into();
        assert!(!other.matches_for_insertion(&original));
        let mut other = original.clone();
        other.selection.location = 1;
        assert!(!other.matches_for_insertion(&original));
        let mut other = original.clone();
        other.input_element.content = "xyz".into();
        assert!(!other.matches_for_insertion(&original));
    }
}
