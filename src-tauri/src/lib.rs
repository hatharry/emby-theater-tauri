use std::fs;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Emby Theater TV web client with autostart disabled; startup parameters are
/// supplied through the injected `window.appStartInfo` object (runs before page
/// scripts), and the app is started explicitly via `Emby.App.start`.
const EMBY_URL: &str = "https://tv.emby.media/index.html?autostart=false";

/// Return the Emby application URL so the frontend (or other code) can use it.
#[tauri::command]
fn emby_url() -> String {
    EMBY_URL.to_string()
}

/// Stable per-install device id, persisted in the app config directory.
fn device_id(app: &tauri::AppHandle) -> String {
    if let Ok(dir) = app.path().app_config_dir() {
        let file = dir.join("device.json");
        if let Ok(txt) = fs::read_to_string(&file) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(id) = v.get("deviceId").and_then(|i| i.as_str()) {
                    return id.to_string();
                }
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        if fs::create_dir_all(&dir).is_ok() {
            let _ = fs::write(&file, serde_json::json!({ "deviceId": id }).to_string());
        }
        return id;
    }
    uuid::Uuid::new_v4().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![emby_url])
        .setup(|app| {
            let handle = app.handle().clone();
            let version = app.package_info().version.to_string();
            let device_name =
                std::env::var("USER").unwrap_or_else(|_| "Emby Theater".to_string());
            let did = device_id(&handle);

            // Injected before any page script runs, so the Emby app sees
            // window.appStartInfo on first load. Once the page's Emby.App is
            // ready, we start the app ourselves with the injected info.
            let start_info = format!(
                r#"window.appStartInfo = Object.assign({{
  environment: "emby-theater",
  appVersion: "{version}",
  deviceId: "{did}",
  deviceName: "{device_name}",
  platform: "linux",
  architecture: "x64",
  canUpdate: false,
  canRestart: false,
  canQuit: true,
  devToolsEnabled: true,
  supportedCommands: [],
}}, window.appStartInfo || {{}});
(function startEmby() {{
  if (window.Emby && window.Emby.App && typeof window.Emby.App.start === "function") {{
    window.Emby.App.start(window.appStartInfo);
  }} else {{
    setTimeout(startEmby, 50);
  }}
}})();"#
            );

            let _window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(EMBY_URL.parse::<tauri::Url>().unwrap()),
            )
            .title("Emby Theater")
            .inner_size(1280.0, 720.0)
            .min_inner_size(960.0, 540.0)
            .center()
            .initialization_script(&start_info)
            .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
