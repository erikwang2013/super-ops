use tauri::menu::{Menu, MenuItemBuilder};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

#[tauri::command]
fn get_system_info() -> String {
    serde_json::json!({
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH
    })
    .to_string()
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let show_item = MenuItemBuilder::with_id("show", "显示面板").build(app.handle())?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app.handle())?;
            let menu = Menu::with_items(app.handle(), &[&show_item, &quit_item])?;
            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_system_info])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error running tauri");
}
