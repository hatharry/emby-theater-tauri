//! Emby Theater desktop wrapper: a Tauri window around the Emby Theater TV web
//! client, with native bridges for LAN discovery, Wake-on-LAN, HDMI-CEC and a
//! Raspberry Pi device profile.
//!
//! The JavaScript/HTML the client loads lives in `code/` (see the `assets`
//! module); the native feature code is split across `cec`, `discovery`,
//! `platform` and this file.

mod assets;
mod cec;
mod discovery;
mod platform;
mod power;

use std::fs;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use assets::response_for;
use platform::{apply_webkit_nvidia_workarounds, has_nvidia_gpu, is_raspberry_pi};

/// Emby Theater TV web client with autostart disabled; startup parameters are
/// supplied through the injected `window.appStartInfo` object (runs before page
/// scripts), and the app is started explicitly via `Emby.App.start`.
///
/// Loaded over HTTP (as the official Theater desktop apps do) so connecting to a
/// plain-HTTP Emby Server on the LAN is not blocked as mixed content; WebKitGTK
/// exposes no mixed-content setting.
const EMBY_URL: &str = "http://tv.emby.media/index.html?autostart=false";

/// Return the Emby application URL so the frontend (or other code) can use it.
#[tauri::command]
fn emby_url() -> String {
    EMBY_URL.to_string()
}

#[tauri::command]
async fn discover_servers() -> Vec<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(discovery::discover_servers_sync)
        .await
        .unwrap_or_default()
}

#[tauri::command]
async fn wake_on_lan(
    mac_address: String,
    address: Option<String>,
    port: Option<u16>,
) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        discovery::wake_on_lan_sync(&mac_address, address, port)
    })
    .await
    .unwrap_or(false)
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Power off the whole machine (client back menu -> appHost.shutdown).
#[tauri::command]
fn shutdown_system() -> bool {
    power::shutdown()
}

/// Reboot the whole machine (client back menu -> appHost.restart).
#[tauri::command]
fn restart_system() -> bool {
    power::restart()
}

/// Drain queued CEC keypresses for the page plugin.
#[tauri::command]
fn cec_poll() -> Vec<String> {
    cec::poll()
}

/// List the CEC adapters for the settings page.
#[tauri::command]
fn cec_devices() -> Vec<serde_json::Value> {
    cec::devices()
}

/// Called by the CEC plugin settings page (and at startup) with the chosen
/// adapter (""/None = auto). Restarts cec-client on change.
#[tauri::command]
fn cec_set_device(device: Option<String>) {
    cec::set_device(device)
}

/// Called by the CEC plugin (at startup and when the settings page saves) with
/// the configured HDMI port (""/None = auto). Restarts cec-client on change.
#[tauri::command]
fn cec_set_hdmi_port(port: Option<String>) {
    cec::set_hdmi_port(port)
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

/// The client's view-mode setting ("tv" or "normal"), persisted at startup by
/// the init script. Defaults to "normal": with no stored choice, the client's
/// own auto-detection resolves to the desktop/mobile layout, which runs in a
/// normal window.
fn saved_layout_mode(app: &tauri::AppHandle) -> String {
    if let Ok(dir) = app.path().app_config_dir() {
        if let Ok(txt) = fs::read_to_string(dir.join("layout.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(mode) = v.get("mode").and_then(|m| m.as_str()) {
                    return mode.to_string();
                }
            }
        }
    }
    "normal".to_string()
}

/// Called by the init script on every page load with the client's persisted
/// view mode: remembers it for the next launch and applies it immediately if
/// the user changed it in settings (tv layout -> fullscreen window).
#[tauri::command]
fn set_layout_mode(app: tauri::AppHandle, mode: String) {
    if let Ok(dir) = app.path().app_config_dir() {
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(
            dir.join("layout.json"),
            serde_json::json!({ "mode": mode }).to_string(),
        );
    }
    let fullscreen = mode != "normal";
    let restart_for_nvidia = fullscreen
        && has_nvidia_gpu()
        && std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_none();
    if restart_for_nvidia {
        // Switching into TV mode at runtime: the crash workarounds must be in
        // the environment before the webview spawns, so relaunch (the new
        // process reads the persisted "tv" mode and applies them).
        app.restart(); // never returns
    }
    if let Some(w) = app.get_webview_window("main") {
        if w.is_fullscreen().unwrap_or(!fullscreen) != fullscreen {
            let _ = w.set_fullscreen(fullscreen);
        }
        // TV mode is driven by the remote (CEC/keyboard), not a mouse: hide
        // the cursor, which would otherwise sit over the picture.
        let _ = w.set_cursor_visible(!fullscreen);
        #[cfg(target_os = "linux")]
        {
            platform::tv_mode().store(fullscreen, std::sync::atomic::Ordering::Relaxed);
            let visible = !fullscreen;
            let _ = w.with_webview(move |wv| {
                platform::set_webview_cursor(&wv.inner(), visible)
            });
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            emby_url,
            discover_servers,
            wake_on_lan,
            quit_app,
            shutdown_system,
            restart_system,
            set_layout_mode,
            cec_poll,
            cec_set_hdmi_port,
            cec_devices,
            cec_set_device
        ])
        .register_uri_scheme_protocol("embyhost", |_ctx, req| {
            let (body, content_type) = response_for(req.uri().path());
            tauri::http::Response::builder()
                .header("content-type", content_type)
                .header("access-control-allow-origin", "*")
                .body(body.to_vec())
                .unwrap()
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let version = app.package_info().version.to_string();
            // The machine's hostname is reported as the device name and
            // "Emby Theater" as the app/client name (the web client would
            // otherwise derive both from the browser user-agent and report
            // e.g. "Safari" / "Emby Web").
            let device_name = hostname::get()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "Emby Theater".to_string());
            let did = device_id(&handle);
            std::thread::spawn(cec::reader_loop);
            // Start in the mode the user last chose in the client's settings
            // (view mode "TV" -> fullscreen, anything else -> normal window).
            let fullscreen = saved_layout_mode(&handle) != "normal";
            if is_raspberry_pi() {
                // The vc4 stateless HEVC decoder (the v4l2codecs plugin's
                // v4l2slh265dec) silently kills the WebKit web process the
                // moment playback starts: the Pi 4 has no HEVC hardware block,
                // so that path is broken. Drop its rank to NONE so GStreamer's
                // autoplug picks software (libav avdec_h265) for HEVC instead.
                // Feature-rank (not GST_PLUGIN_BLOCKLIST) is required: the
                // blocklist is applied only during a registry scan and is a
                // no-op once the plugin registry is cached, whereas the rank is
                // honoured at element-selection time regardless. H.264 hardware
                // decode is untouched (separate request-API decoder). Must be
                // set before the webview spawns so the web process inherits it.
                match std::env::var("GST_PLUGIN_FEATURE_RANK") {
                    Ok(existing) if !existing.is_empty() => {
                        std::env::set_var(
                            "GST_PLUGIN_FEATURE_RANK",
                            format!("{existing},v4l2slh265dec:0"),
                        );
                    }
                    _ => std::env::set_var("GST_PLUGIN_FEATURE_RANK", "v4l2slh265dec:0"),
                }
            }
            if fullscreen && has_nvidia_gpu() {
                apply_webkit_nvidia_workarounds();
            }

            let start_info =
                assets::startup_script(&version, &did, &device_name, is_raspberry_pi());

            let builder = WebviewWindowBuilder::new(
                app,
                "main",
                // Local splash (ui/index.html, a copy of Emby's own loading
                // screen) navigates to EMBY_URL via the emby_url command. The
                // webview keeps painting the splash until the remote document
                // commits, covering the initial network fetch; Emby's own
                // splash then takes over. WebKit re-runs the injected
                // appStartInfo script on that navigation.
                WebviewUrl::App("index.html".into()),
            )
            .title("Emby Theater")
            .inner_size(1280.0, 720.0)
            .min_inner_size(960.0, 540.0)
            .center()
            .fullscreen(fullscreen)
            .initialization_script(&start_info);
            let window = builder.build()?;

            // wry registers custom schemes as secure but NOT CORS-enabled, and
            // the Emby client fetches plugin page HTML with XHR (its `text!`
            // loader). Without this the request is blocked, the template comes
            // back empty and viewmanager crashes before the controller runs.
            #[cfg(target_os = "linux")]
            {
                use std::sync::atomic::Ordering;
                use webkit2gtk::{LoadEvent, SecurityManagerExt, WebContextExt, WebViewExt};
                let _ = window.with_webview(|wv| {
                    if let Some(ctx) = wv.inner().context() {
                        if let Some(sm) = ctx.security_manager() {
                            sm.register_uri_scheme_as_cors_enabled("embyhost");
                        }
                    }
                    // WebKit sets its own default cursor on the webview window
                    // when each new page is created, which would undo the TV-mode
                    // hide; re-apply it after every load.
                    wv.inner().connect_load_changed(|webview, event| {
                        if matches!(event, LoadEvent::Finished) {
                            platform::set_webview_cursor(
                                webview,
                                !platform::tv_mode().load(Ordering::Relaxed),
                            );
                        }
                    });
                });
            }

            // Route window-manager closes (Alt+F4, swipe-away, session logout)
            // through the same orderly app.exit() as the in-app Exit button.
            // Destroying the window underneath WebKit races the GPU-process
            // teardown and segfaults libnvidia-eglcore's worker threads
            // (crash popups, no data loss); app.exit() tears the webview down
            // first and exits cleanly.
            {
                use tauri::{Manager, WindowEvent};
                if let Some(w) = handle.get_webview_window("main") {
                    let app = handle.clone();
                    w.on_window_event(move |event| {
                        if let WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            app.exit(0);
                        }
                    });
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;
    use std::time::Duration;

    #[test]
    fn parses_cec_traffic_lines() {
        // Decoded form.
        assert_eq!(
            crate::cec::parse_cec_line(
                "TRAFFIC: [ 1] >> A:5 (0): [ 44 01], 'User Control Pressed' (0x01)"
            ),
            Some("up")
        );
        // Raw traffic form: initiator 0 (TV) -> destination 5, opcode 44.
        assert_eq!(crate::cec::parse_cec_line(">> 05:44:41"), Some("volumeup"));
        // Key release (opcode 45) and foreign initiators are ignored.
        assert_eq!(crate::cec::parse_cec_line(">> 05:45:41"), None);
        assert_eq!(crate::cec::parse_cec_line(">> 15:44:41"), None);
        assert_eq!(crate::cec::parse_cec_line("current latency: 24 ms"), None);
    }

    #[test]
    fn parses_mac_notations() {
        let want = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        for s in ["AA:BB:CC:DD:EE:FF", "aa-bb-cc-dd-ee-ff", "aabbccddeeff"] {
            assert_eq!(crate::discovery::parse_mac(s), Some(want), "{s}");
        }
        assert_eq!(crate::discovery::parse_mac("nope"), None);
    }

    #[test]
    fn sends_magic_packet() {
        // Unicast to loopback so we can capture the frame deterministically.
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = rx.local_addr().unwrap().port();
        rx.set_read_timeout(Some(Duration::from_millis(500))).unwrap();

        assert!(crate::discovery::wake_on_lan_sync(
            "AA:BB:CC:DD:EE:FF",
            Some(format!("http://127.0.0.1:{port}")),
            Some(port),
        ));

        let mut buf = [0u8; 1024];
        let (len, _) = rx.recv_from(&mut buf).unwrap();
        assert_eq!(len, 102, "magic packet is 6 sync + 16x MAC");
        assert_eq!(&buf[..6], &[0xFF; 6]);
        for i in 0..16 {
            assert_eq!(
                &buf[6 + i * 6..12 + i * 6],
                &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]
            );
        }
    }

    #[test]
    #[ignore = "requires an Emby server (or fake responder on udp/7359) on the LAN"]
    fn discovers_fake_server() {
        let servers = crate::discovery::discover_servers_sync();
        println!("discovered: {servers:?}");
        assert!(!servers.is_empty(), "expected at least one server");
    }

    #[test]
    fn serves_both_device_profiles() {
        // The Pi and desktop profiles are distinct assets, each routed.
        let (pi, pi_ct) = crate::assets::response_for("/deviceprofiles/pi.js");
        let (def, def_ct) = crate::assets::response_for("/deviceprofiles/default.js");
        assert_eq!(pi_ct, "application/javascript");
        assert_eq!(def_ct, "application/javascript");
        assert_ne!(pi, def, "pi and desktop profiles must differ");
        assert!(String::from_utf8_lossy(pi).contains("pideviceprofile"));
        assert!(String::from_utf8_lossy(def).contains("defaultdeviceprofile"));
    }

    #[test]
    fn startup_selects_profile_by_platform() {
        let pi = crate::assets::startup_script("1.0", "did", "dev", true);
        let desktop = crate::assets::startup_script("1.0", "did", "dev", false);
        assert!(
            pi.contains("deviceprofiles/pi.js"),
            "pi uses the pi profile"
        );
        assert!(
            desktop.contains("deviceprofiles/default.js"),
            "desktop uses the empirically-derived profile"
        );
        assert!(!pi.contains("deviceprofiles/default.js"));
        assert!(!desktop.contains("deviceprofiles/pi.js"));
    }
}
