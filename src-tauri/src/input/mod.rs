// 候选词 UI 相关模块 

use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::os::element;

static INPUT_STATE: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
static FORMER_FOCUSED_INPUT: Lazy<Mutex<Option<element::FocusedInput>>> = Lazy::new(|| Mutex::new(None));

pub fn set_input_state(state: bool) {
    let mut input_state = INPUT_STATE.lock().unwrap();
    *input_state = state;
}

pub fn get_input_state() -> bool {
    let input_state = INPUT_STATE.lock().unwrap();
    *input_state
}

pub fn listen_input_state() {
    std::thread::spawn(|| {
        loop {
            match element::get_current_focus_info() {
                Some(focused_input) => {
                    if let Some(former_focused_input) = FORMER_FOCUSED_INPUT.lock().unwrap().as_ref() {
                        if !former_focused_input.eq(&focused_input) {
                            FORMER_FOCUSED_INPUT.lock().unwrap().replace(focused_input);
                            set_input_state(true);
                        }
                    } else {
                        FORMER_FOCUSED_INPUT.lock().unwrap().replace(focused_input);
                        set_input_state(true);
                    }
                }
                None => {
                    *FORMER_FOCUSED_INPUT.lock().unwrap() = None;
                    set_input_state(false);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });
}



