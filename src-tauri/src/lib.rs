use std::collections::HashSet;
use std::fs;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

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

/// LAN server discovery: UDP broadcast "who is EmbyServer?" on the standard
/// Emby discovery ports and collect the JSON replies ({Address, Id, Name}).
fn discover_servers_sync() -> Vec<serde_json::Value> {
    const MSG: &[u8] = b"who is EmbyServer?";
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let _ = sock.set_broadcast(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));

    for port in [7359u16, 32410u16] {
        let _ = sock.send_to(MSG, format!("255.255.255.255:{port}"));
        if let Ok(addrs) = local_subnet_broadcasts() {
            for addr in addrs {
                let _ = sock.send_to(MSG, (addr, port));
            }
        }
    }

    let mut servers = Vec::new();
    let mut seen = HashSet::new();
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((len, peer)) => {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
                    let id = v
                        .get("Id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() && seen.insert(id) {
                        let mut server = v;
                        if let Some(obj) = server.as_object_mut() {
                            obj.insert(
                                "EndpointAddress".to_string(),
                                serde_json::Value::String(peer.ip().to_string()),
                            );
                        }
                        servers.push(server);
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
            || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    servers
}

/// Directed broadcast addresses for our interfaces (255.255.255.255 is often
/// dropped on multi-interface or strict-routing hosts).
fn local_subnet_broadcasts() -> Result<Vec<std::net::IpAddr>, std::io::Error> {
    let mut out = Vec::new();
    let hostname = hostname::get()?;
    for addr in std::net::ToSocketAddrs::to_socket_addrs(&hostname.to_string_lossy() as &str)? {
        if let std::net::IpAddr::V4(v4) = addr.ip() {
            let o = v4.octets();
            out.push(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                o[0], o[1], o[2], 255,
            )));
        }
    }
    Ok(out)
}

#[tauri::command]
async fn discover_servers() -> Vec<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(discover_servers_sync)
        .await
        .unwrap_or_default()
}

/// Parse a MAC address in any common notation into 6 bytes.
fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if hex.len() != 12 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(mac)
}

/// Build and broadcast a Wake-on-LAN magic packet: 6x 0xFF followed by the
/// target MAC repeated 16 times, sent to the limited broadcast address, our
/// subnet-directed broadcasts, and (best-effort) unicast to the server host.
fn wake_on_lan_sync(mac: &str, address: Option<String>, port: Option<u16>) -> bool {
    let Some(mac) = parse_mac(mac) else {
        return false;
    };
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }
    let port = port.unwrap_or(9);

    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = sock.set_broadcast(true);

    let mut sent = false;
    if sock.send_to(&packet, format!("255.255.255.255:{port}")).is_ok() {
        sent = true;
    }
    if let Ok(addrs) = local_subnet_broadcasts() {
        for addr in addrs {
            if sock.send_to(&packet, (addr, port)).is_ok() {
                sent = true;
            }
        }
    }
    // Many NICs also accept a WoL frame unicasted to their own address, and
    // this survives networks that filter broadcasts.
    if let Some(address) = address {
        let host = address
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(['/', ':'])
            .next()
            .unwrap_or(&address)
            .to_string();
        if sock.send_to(&packet, format!("{host}:{port}")).is_ok() {
            sent = true;
        }
    }
    sent
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn wake_on_lan(
    mac_address: String,
    address: Option<String>,
    port: Option<u16>,
) -> bool {
    tauri::async_runtime::spawn_blocking(move || wake_on_lan_sync(&mac_address, address, port))
        .await
        .unwrap_or(false)
}

/// AMD module served over the `embyhost://` custom protocol and referenced from
/// `appStartInfo.paths.serverdiscovery`. The client's loader resolves it instead
/// of its built-in no-op discovery module (browsers cannot UDP broadcast), and
/// it calls our native `discover_servers` command.
const SERVER_DISCOVERY_JS: &str = r#"define(function () {
  return {
    findServers: function () {
      try {
        return window.__TAURI_INTERNALS__.invoke("discover_servers");
      } catch (e) {
        return Promise.resolve([]);
      }
    },
  };
});
"#;

/// Host apphost module: wraps the client's built-in apphost and enables the
/// `exit` capability, which the web apphost only reports on native LG/Tizen.
/// With it the TV client shows its "are you ready to exit" back menu on Esc
/// instead of having no way to quit.
///
/// The dependency id must match the one the client itself uses
/// (`importFromPath("./modules/apphost.js")` normalizes to `modules/apphost.js`);
/// a leading slash would create a second instance whose servicelocator was never
/// initialized, and appHost.init() would reject, leaving the splash screen up.
const APPHOST_JS: &str = r#"define(["modules/apphost.js"], function (mod) {
  var inner = mod && mod.default ? mod.default : mod;
  var baseSupports = inner.supports;
  inner.supports = function (feature) {
    if (feature === "exit") return true;
    return baseSupports.call(inner, feature);
  };
  inner.exit = function () {
    try {
      window.__TAURI_INTERNALS__.invoke("quit_app");
    } catch (e) {
      window.close();
    }
    return Promise.resolve();
  };
  return inner;
});
"#;

/// Host Wake-on-LAN module (same rationale as serverdiscovery: browsers cannot
/// send UDP magic packets). `send(info)` receives the server's WakeInfo, whose
/// MacAddress/Address/Port we forward to the native command.
const WAKE_ON_LAN_JS: &str = r#"define(function () {
  return {
    isSupported: function () {
      return true;
    },
    send: function (info) {
      try {
        return window.__TAURI_INTERNALS__.invoke("wake_on_lan", {
          macAddress: info && info.MacAddress,
          address: info && info.Address,
          port: info && info.Port,
        });
      } catch (e) {
        return Promise.resolve(false);
      }
    },
  };
});
"#;

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
    if let Some(w) = app.get_webview_window("main") {
        if w.is_fullscreen().unwrap_or(!fullscreen) != fullscreen {
            let _ = w.set_fullscreen(fullscreen);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK + NVIDIA: the first accelerated-compositing trigger on a page
    // (e.g. the TV client's page-transition animation when a menu item is
    // clicked) segfaults the UI process on a null AcceleratedBackingStore —
    // "segfault at 48" (bugs.webkit.org #321683, block/buzz #3654).
    // WEBKIT_DMABUF_RENDERER_FORCE_SHM routes the renderer through shared
    // memory and keeps the backing store valid (unlike the old
    // WEBKIT_DISABLE_DMABUF_RENDERER, which empties the transport set and
    // causes exactly this crash). Ubuntu's libwebkit2gtk additionally ships a
    // disable-nvidia-dmabuf patch that bails out before the SHM mode is
    // added, so its own opt-out (WEBKIT_FORCE_DMABUF_RENDERER) must be set
    // alongside for FORCE_SHM to take effect. WEBKIT_SKIA_ENABLE_CPU_RENDERING
    // keeps Skia off the NVIDIA GL path entirely, which also avoids the
    // driver's GPU-worker teardown segfault on exit. Set before the webview
    // spawns so all helper processes inherit them; respect pre-set values.
    for (var, value) in [
        ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
        ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
        ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1"),
    ] {
        if std::env::var_os(var).is_none() {
            std::env::set_var(var, value);
        }
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            emby_url,
            discover_servers,
            wake_on_lan,
            quit_app,
            set_layout_mode
        ])
        .register_uri_scheme_protocol("embyhost", |_ctx, req| {
            let body = match req.uri().path() {
                "/wakeonlan.js" => WAKE_ON_LAN_JS,
                "/apphost.js" => APPHOST_JS,
                _ => SERVER_DISCOVERY_JS,
            };
            tauri::http::Response::builder()
                .header("content-type", "application/javascript")
                .header("access-control-allow-origin", "*")
                .body(body.as_bytes().to_vec())
                .unwrap()
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let version = app.package_info().version.to_string();
            let device_name =
                std::env::var("USER").unwrap_or_else(|_| "Emby Theater".to_string());
            let did = device_id(&handle);
            // Start in the mode the user last chose in the client's settings
            // (view mode "TV" -> fullscreen, anything else -> normal window).
            let fullscreen = saved_layout_mode(&handle) != "normal";

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
  paths: {{
    serverdiscovery: "embyhost://host/serverdiscovery.js",
    wakeonlan: "embyhost://host/wakeonlan.js",
    apphost: "embyhost://host/apphost.js",
  }},
}}, window.appStartInfo || {{}});
(function startEmby() {{
  if (window.Emby && window.Emby.App && typeof window.Emby.App.start === "function") {{
    window.Emby.App.start(window.appStartInfo);
  }} else {{
    setTimeout(startEmby, 50);
  }}
}})();
// Report the client's persisted view mode (settings -> "View mode", stored by
// layoutmanager as the "layout" key) so the window starts fullscreen for the
// TV layout. Only an explicit "tv" counts: empty/auto resolves to the
// desktop/mobile layout, which runs in a normal window.
(function () {{
  function send() {{
    try {{
      var l = localStorage.getItem("layout");
      window.__TAURI_INTERNALS__.invoke("set_layout_mode", {{
        mode: l === "tv" ? "tv" : "normal",
      }});
      return true;
    }} catch (e) {{
      return false;
    }}
  }}
  if (!send()) setTimeout(send, 200);
  // The settings page can change the view mode without a page reload; catch
  // the write so the window state follows immediately. Must patch the
  // prototype: assigning localStorage.setItem would just store an ITEM named
  // "setItem" (Storage is an exotic object with a named-property setter).
  var orig = Storage.prototype.setItem;
  Storage.prototype.setItem = function (key, value) {{
    orig.call(this, key, value);
    if (key === "layout") send();
  }};
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
            .fullscreen(fullscreen)
            .initialization_script(&start_info)
            .build()?;

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
    fn parses_mac_notations() {
        let want = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        for s in ["AA:BB:CC:DD:EE:FF", "aa-bb-cc-dd-ee-ff", "aabbccddeeff"] {
            assert_eq!(super::parse_mac(s), Some(want), "{s}");
        }
        assert_eq!(super::parse_mac("nope"), None);
    }

    #[test]
    fn sends_magic_packet() {
        // Unicast to loopback so we can capture the frame deterministically.
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = rx.local_addr().unwrap().port();
        rx.set_read_timeout(Some(Duration::from_millis(500))).unwrap();

        assert!(super::wake_on_lan_sync(
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
        let servers = super::discover_servers_sync();
        println!("discovered: {servers:?}");
        assert!(!servers.is_empty(), "expected at least one server");
    }
}
