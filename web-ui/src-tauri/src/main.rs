#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mock_api;

use tauri::Manager;

#[tauri::command]
fn get_api_base_url(mock_api: tauri::State<'_, mock_api::MockApiHandle>) -> String {
    mock_api.base_url().to_string()
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let mock_api = tauri::async_runtime::block_on(mock_api::start())?;
            app.manage(mock_api);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_api_base_url])
        .run(tauri::generate_context!())
        .expect("error while running RikkaDesk");
}
