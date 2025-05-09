use std::panic;

use config::{get_config_for_frontend, save_config_for_frontend};
use log::{error, info, warn};
use os::{element, monitor};
use tauri::{image::Image, menu::{MenuBuilder, MenuItemBuilder}, tray::{TrayIconBuilder, TrayIconEvent}, AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use utils::logger::init_logger;
use windows::Win32::{Foundation::HWND, Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE}, System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED}, UI::WindowsAndMessaging::{GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TRANSPARENT}};

mod ai;
mod input;
mod config;
mod context;
mod os;
mod utils;

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
  let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app_handle)?;

  let tray_menu = MenuBuilder::new(app_handle)
      .item(&settings_item)
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

fn setup_shortcut(
  app_handle: &AppHandle,
  config: &config::Config,
  main_window: tauri::WebviewWindow,
) -> Result<(), Box<dyn std::error::Error>> {
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
  tauri::Builder::default()
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
      ])
      .on_window_event(|window, event| {
          if let WindowEvent::CloseRequested { api, .. } = event {
              window.hide().unwrap();
              api.prevent_close();
          }
      })
}

fn create_overlay_window(
  app_handle: &AppHandle,
  window_label: &str,
  monitor: &monitor::MonitorInfo,
) {
  // 如果已存在，先关闭
  if let Some(existing_window) = app_handle.get_webview_window(&window_label) {
      warn!("[create_overlay_window] close existing window: {}", window_label);
      if let Err(e) = existing_window.close() {
          error!(
              "[create_overlay_window] close existing window failed: {}",
              e
          );
      }
  }

  let width = monitor.width as f64 / monitor.scale_factor;
  let height = monitor.height as f64 / monitor.scale_factor;
  let position_x = monitor.x;
  let position_y = monitor.y;
  info!(
      "[create_overlay_window] create overlay window {}: position({}, {}), size{}x{}",
      window_label, position_x, position_y, width, height
  );
  let window = tauri::WebviewWindowBuilder::new(
      app_handle,
      window_label,
      tauri::WebviewUrl::App(format!("overlay.html?window_label={}", window_label).into()),
  )
  .title(window_label)
  .transparent(true)
  .decorations(false)
  // must disable shadow, otherwise the window will be offset
  .shadow(false)
  .resizable(true)
  .inner_size(width, height)
  .focused(false)
  .build();

  if let Err(e) = window {
      panic!(
          "[create_overlay_window] create overlay window failed: {}",
          e
      );
  }
  
  let window = window.unwrap();
  if let Err(e) = window.set_position(tauri::PhysicalPosition::new(position_x, position_y)) {
      error!("[create_overlay_window] set position failed: {}", e);
  }
  // 确保窗口位置正确
  if let Ok(hwnd) = window.hwnd() {
    let hwnd_raw = hwnd.0;
    const DWMWA_WINDOW_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
    const DWMWCP_DONOTROUND: u32 = 1;
    let preference: u32 = DWMWCP_DONOTROUND;
    unsafe {
      // 去掉 Windows 11 圆角
      let _ = DwmSetWindowAttribute(
          HWND(hwnd_raw as *mut _),
          DWMWA_WINDOW_CORNER_PREFERENCE,
          &preference as *const _ as _,
          std::mem::size_of_val(&preference) as u32,
      );
      if !config::get_config().unwrap().system.debug_mode {
          set_window_transparent_style(&window, hwnd_raw as i64);
      }
    }
  }
}

fn set_window_transparent_style(window: &tauri::WebviewWindow, hwnd_raw: i64) {
  // 设置无任务栏图标并确保在最顶层
  if let Err(e) = window.set_skip_taskbar(true) {
      error!("[set_overlay_style] set skip taskbar failed: {}", e);
  }
  if let Err(e) = window.set_always_on_top(true) {
      error!("[set_overlay_style] set always on top failed: {}", e);
  }

  // 设置扩展窗口样式
  unsafe {
      let style = GetWindowLongW(HWND(hwnd_raw as *mut _), GWL_EXSTYLE);
      // 确保WS_EX_TRANSPARENT样式被正确设置
      SetWindowLongW(
          HWND(hwnd_raw as *mut _),
          GWL_EXSTYLE,
          style | (WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as i32,
      );
  }
}

pub fn run() {
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

      // Handle window visibility
      if config.system.start_in_tray {
          if let Err(e) = main_window.hide() {
              error!("[✗] hide main window failed: {}", e);
          }
          info!("[✓] minimized to tray (if show_tray_icon is true)");
      } else {
          if let Err(e) = main_window.show() {
              error!("[✗] show main window failed: {}", e);
          }
      }

      // Initialize panic handler
      setup_panic_handler(app_handle.clone());
      info!("[✓] panic handler initialized");

      monitor::init_monitors(&main_window);
      info!("[✓] monitors initialized");

      // 监听输入焦点
      element::listen_input_focus();
      info!("[✓] input focus listener initialized");

      input::listen_input_state();
      info!("[✓] input state listener initialized");

    //   // Create overlay windows
    //   create_overlay_window(&app_handle, "main", &monitor::MONITORS_STORAGE.lock().unwrap()[0]);
    //   info!("[✓] overlay windows created");

      // Setup shortcuts
      setup_shortcut(&app_handle, &config, main_window.clone())
          .expect("Failed to setup shortcuts");
      info!("[✓] shortcuts setup");

      // set autostart
      set_auto_start(&app_handle, &config).expect("Failed to setup auto start");
      info!("[✓] auto start setup");

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

          unsafe {
              CoUninitialize();
              info!("[✓] COM uninitialized");
          }
      }
  });
}
