use std::{panic, sync::Mutex};

use config::{get_config_for_frontend, save_config_for_frontend};
use log::{error, info, warn};
use once_cell::sync::Lazy;
use os::{element};
use tauri::{image::Image, menu::{MenuBuilder, MenuItemBuilder}, tray::{TrayIconBuilder, TrayIconEvent}, AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use utils::logger::init_logger;
use windows::Win32::{System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED}};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, SetWindowLongPtrW, GWLP_WNDPROC, WM_CLIPBOARDUPDATE, GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW
};

mod ai;
mod input;
mod config;
mod context;
mod os;
mod utils;
mod overlay;

static APP_HANDLE: Lazy<Mutex<Option<AppHandle>>> = Lazy::new(|| Mutex::new(None));
static mut ORIGINAL_WNDPROC: Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT> = None;

fn setup_tray(
  app_handle: &AppHandle,
  config: &config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
  if !config.system.show_tray_icon {
      info!("[setup_tray] tray icon is not enabled");
      return Ok(());
  }

  let exit_item = MenuItemBuilder::with_id("exit", "Exit").build(app_handle)?;
  let restart_item = MenuItemBuilder::with_id("restart", "Restart").build(app_handle)?;
//   let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app_handle)?;

  let tray_menu = MenuBuilder::new(app_handle)
    //   .item(&settings_item)
      .item(&restart_item)
      .item(&exit_item)
      .build()?;

  let tray_icon = Image::from_bytes(include_bytes!("../icons/icon.ico"))?;

  let _tray_icon = TrayIconBuilder::new()
      .menu(&tray_menu)
      .on_menu_event(move |tray_handle, event| {
          let app_handle = tray_handle.app_handle();
          match event.id.as_ref() {
              "exit" => {
                  app_handle.exit(0);
              }
              "settings" => {
                  let window = app_handle.get_webview_window("main").unwrap();
                  window.show().unwrap();
                  window.set_focus().unwrap();
              }
              "restart" => {
                  app_handle.restart();
              }
              _ => {}
          }
      })
      .icon(tray_icon)
      .on_tray_icon_event(move |tray_handle, event| {
          let app_handle = tray_handle.app_handle();
          match event {
              TrayIconEvent::DoubleClick { .. } => {
                  let window = app_handle.get_webview_window("main").unwrap();
                  window.show().unwrap();
                  window.set_focus().unwrap();
              }
              _ => {}
          }
      })
      .show_menu_on_left_click(true)
      .build(app_handle)?;
  Ok(())
}

fn set_auto_start(
  app_handle: &AppHandle,
  config: &config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
  let auto_start = config.system.start_at_login;
  let autostart_manager = app_handle.autolaunch();
  info!("[set_auto_start] auto start: {}", auto_start);
  if auto_start {
      let _ = autostart_manager.enable();
  } else {
      let _ = autostart_manager.disable();
  }
  Ok(())
}

fn setup_panic_handler(app_handle: tauri::AppHandle) {
  panic::set_hook(Box::new(move |panic_info| {
      let location = panic_info
          .location()
          .unwrap_or_else(|| panic::Location::caller());
      let message = match panic_info.payload().downcast_ref::<&str>() {
          Some(s) => *s,
          None => match panic_info.payload().downcast_ref::<String>() {
              Some(s) => &s[..],
              None => "Box<Any>",
          },
      };

      let error_info = format!(
          "program panic:\nlocation: {}:{}\nerror: {}",
          location.file(),
          location.line(),
          message
      );

      error!("{}", error_info);

      // 发送错误到前端
      let window = app_handle.get_webview_window("main").unwrap();
      window.emit("rust-panic", error_info).unwrap_or_else(|e| {
          error!(
              "[setup_panic_handler] send panic info to frontend failed: {}",
              e
          );
      });
  }));
}

fn create_app_builder() -> tauri::Builder<tauri::Wry> {
  info!("[create_app_builder] creating app builder");
  tauri::Builder::default()
      .plugin(tauri_plugin_notification::init())
      .plugin(tauri_plugin_autostart::init(
          MacosLauncher::LaunchAgent,
          None,
      ))
      .plugin(tauri_plugin_process::init())
      .plugin(tauri_plugin_positioner::init())
      .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
          let _ = app
              .get_webview_window("main")
              .expect("no main window")
              .set_focus();
      }))
      .invoke_handler(tauri::generate_handler![
          get_config_for_frontend,
          save_config_for_frontend,
          overlay::overlay::resize_overlay_window,
          overlay::overlay::get_overlay_style,
      ])
      .on_window_event(|window, event| {
          if let WindowEvent::CloseRequested { api, .. } = event {
              window.hide().unwrap();
              api.prevent_close();
          }
      })
}

// 自定义的窗口过程
unsafe extern "system" fn sub_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        info!("[sub_wnd_proc] clipboard update message received");
        os::clipboard::windows_clipboard::handle_clipboard_update();
    }
    
    // 调用原始的窗口过程
    if let Some(original_wndproc) = ORIGINAL_WNDPROC {
        CallWindowProcW(Some(original_wndproc), hwnd, msg, wparam, lparam)
    } else {
        // Fallback, though this should ideally not be reached
        windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

pub fn run() {
  info!("[run] starting ainput application");
  // 自动切换到 exe 所在目录, 为了解决windows自动启动时workding directory读取不到配置文件的问题
  if !cfg!(debug_assertions) {
      if let Ok(exe_path) = std::env::current_exe() {
          if let Some(exe_dir) = exe_path.parent() {
              let _ = std::env::set_current_dir(exe_dir);
          }
      }
  }
  // Initialize config first
  config::init_config();
  let config = config::get_config().unwrap();
  let config_for_manage = config.clone();

  // Initialize logger
  let _ = init_logger(config.system.logging_level.clone());
  
  // Initialize COM
  unsafe {
      let result = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
      if result.is_err() {
          error!("COM initialize failed: {:?}", result.message());
      } else {
          info!("COM initialized (APARTMENTTHREADED)");
      }
  }

  // Initialize app
  let mut builder = create_app_builder();
  // Setup application
  builder = builder.setup(move |app| {
      info!("=== application started ===");
      info!("debug mode: {}", cfg!(debug_assertions));

      let app_handle = app.handle();

      // Setup system tray
      setup_tray(&app_handle, &config).expect("Failed to setup system tray");

      // Setup main window
      let main_window = app_handle.get_webview_window("main").unwrap();
      // 在这里设置剪贴板监听
      if let Ok(handle) = main_window.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            let hwnd = HWND(handle.hwnd.get() as *mut _);
            // 获取原有扩展样式
            let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
            // 去掉 WS_EX_APPWINDOW，添加 WS_EX_TOOLWINDOW
            let new_ex_style = (ex_style & !WS_EX_APPWINDOW.0 as isize) | WS_EX_TOOLWINDOW.0 as isize;
            unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style); }
            if os::clipboard::windows_clipboard::register_clipboard_listener(hwnd) {
                info!("[✓] clipboard listener registered");
                unsafe {
                    let original_proc = SetWindowLongPtrW(
                        hwnd,
                        GWLP_WNDPROC,
                        sub_wnd_proc as usize as _,
                    );
                    ORIGINAL_WNDPROC = Some(std::mem::transmute(original_proc));
                }
            } else {
                error!("Failed to register clipboard listener");
            }
        }
      }
      #[cfg(debug_assertions)]
      main_window.open_devtools();
      overlay::overlay::top_window(&main_window);

      // Initialize panic handler
      setup_panic_handler(app_handle.clone());
      info!("[✓] panic handler initialized");

      input::hook::init(app_handle.clone());
      info!("[✓] keyboard listener initialized");

      // 监听输入焦点
      element::collect_input_focus();
      info!("[✓] input focus listener initialized");

      input::listen_input_state();
      info!("[✓] input state listener initialized");

      // set autostart
      set_auto_start(&app_handle, &config).expect("Failed to setup auto start");
      info!("[✓] auto start setup");

      *APP_HANDLE.lock().unwrap() = Some(app_handle.clone());
      info!("=== application initialized ===");
      Ok(())
  });

  // Build and run application
  let app = builder
      .build(tauri::generate_context!("Tauri.toml"))
      .expect("error while building tauri application");

  app.manage(config_for_manage);

  app.run(|_app_handle, event| {
      if let tauri::RunEvent::Exit = event {
          info!("application is exiting, cleaning up resources...");
          input::hook::cleanup();
          info!("[✓] keyboard listener cleaned up");

          unsafe {
              CoUninitialize();
              info!("[✓] COM uninitialized");
          }
      }
  });
}
