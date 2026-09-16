use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::UdpSocket;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
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

/// CEC keypress queue: filled by the reader thread, drained by the page's
/// inputmanager plugin (cec_poll).
fn cec_queue() -> &'static Mutex<VecDeque<String>> {
    static QUEUE: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    QUEUE.get_or_init(Default::default)
}

/// Map a CEC UI command code to the client's inputmanager command name
/// (same mapping as the official Emby Theater cec/command-map.js).
fn cec_command(code: u8) -> Option<&'static str> {
    Some(match code {
        0x00 | 0x2B => "select",
        0x01 => "up",
        0x02 => "down",
        0x03 => "left",
        0x04 => "right",
        0x09 => "menu",
        0x0A => "settings",
        0x0C => "favorites",
        0x0D | 0x1D => "back",
        0x10 => "channelup",
        0x11 => "channeldown",
        0x24 => "previous",
        0x25 => "next",
        0x37 => "pageup",
        0x38 => "pagedown",
        0x41 => "volumeup",
        0x42 => "volumedown",
        0x43 => "togglemute",
        0x44 => "play",
        0x45 => "stop",
        0x46 => "pause",
        0x47 => "record",
        0x48 => "rewind",
        0x49 => "fastforward",
        0x4B => "next",
        0x4C => "previous",
        0x53 => "guide",
        0x61 => "playpause",
        _ => return None,
    })
}

/// The HDMI port (physical address first octet) the user selected on the
/// plugin's settings page; None = auto.
fn cec_hdmi_port() -> &'static Mutex<Option<String>> {
    static PORT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    PORT.get_or_init(Default::default)
}

/// The running cec-client, so a port change can kill it and make the reader
/// loop respawn it with the new -p argument.
fn cec_child() -> &'static Mutex<Option<std::process::Child>> {
    static CHILD: OnceLock<Mutex<Option<std::process::Child>>> = OnceLock::new();
    CHILD.get_or_init(Default::default)
}

/// The CEC adapter (a /dev/cecN path) the user picked on the plugin's
/// settings page; None = auto (probe for the connected one).
fn cec_device() -> &'static Mutex<Option<String>> {
    static DEVICE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    DEVICE.get_or_init(Default::default)
}

/// Called by the CEC plugin settings page (and at startup) with the chosen
/// adapter (""/None = auto). Restarts cec-client on change.
#[tauri::command]
fn cec_set_device(device: Option<String>) {
    let device = device.filter(|d| !d.is_empty());
    let changed = {
        let mut g = cec_device().lock().unwrap_or_else(|e| e.into_inner());
        let changed = *g != device;
        *g = device;
        changed
    };
    if changed {
        if let Some(child) = cec_child().lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = child.kill();
        }
    }
}

/// Called by the CEC plugin (at startup and when the settings page saves) with
/// the configured HDMI port (""/None = auto). Restarts cec-client on change.
#[tauri::command]
fn cec_set_hdmi_port(port: Option<String>) {
    let port = port.filter(|p| !p.is_empty());
    let changed = {
        let mut g = cec_hdmi_port().lock().unwrap_or_else(|e| e.into_inner());
        let changed = *g != port;
        *g = port;
        changed
    };
    if changed {
        if let Some(child) = cec_child().lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = child.kill();
        }
    }
}

/// The CEC adapters present as /dev/cecN, in index order. On a Raspberry Pi 4
/// there is one per HDMI output (cec0 = HDMI0, cec1 = HDMI1); only the one the
/// TV is plugged into is usable.
fn cec_adapters() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = fs::read_dir("/dev") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.len() > 3
                && name.starts_with("cec")
                && name[3..].chars().all(|c| c.is_ascii_digit())
            {
                v.push(format!("/dev/{}", name));
            }
        }
    }
    v.sort();
    v
}

/// Outcome of probing one CEC adapter with a short-lived monitor client.
enum CecProbe {
    /// Opens and reports a real physical address: the TV is on this adapter.
    Connected,
    /// Opens but the driver reports f.f.f.f: nothing is plugged in here.
    Disconnected,
    /// Another client (usually our own reader) holds the adapter: it is in
    /// use, which for display purposes counts as connected.
    Busy,
}

/// Probe one adapter. A monitor client on a disconnected adapter logs
/// "physical address is invalid" (the vc4 driver reports f.f.f.f); one on a
/// busy adapter logs "could not open a connection"; the connected adapter logs
/// neither. Monitor mode does not claim a logical address or send keys, so
/// probing is side-effect-free.
fn probe_cec_adapter(dev: &str) -> CecProbe {
    let child = std::process::Command::new("cec-client")
        .args(["-m", "-d", "15", dev])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(_) => return CecProbe::Disconnected,
    };
    std::thread::sleep(Duration::from_millis(1200));
    let _ = child.kill();
    match child.wait_with_output() {
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if text.contains("physical address is invalid") {
                CecProbe::Disconnected
            } else if text.contains("could not open") {
                CecProbe::Busy
            } else {
                CecProbe::Connected
            }
        }
        Err(_) => CecProbe::Disconnected,
    }
}

/// List the CEC adapters for the settings page: every /dev/cecN with a flag
/// for whether it is usable (connected, or held by our running client).
#[tauri::command]
fn cec_devices() -> Vec<serde_json::Value> {
    cec_adapters()
        .into_iter()
        .map(|path| {
            let usable = !matches!(probe_cec_adapter(&path), CecProbe::Disconnected);
            serde_json::json!({ "path": path, "connected": usable })
        })
        .collect()
}

/// Pick the adapter to talk to in auto mode. With a single adapter (or none
/// enumerable) return None and let cec-client autodetect; with several, probe
/// each and use the connected one.
fn pick_cec_adapter() -> Option<String> {
    let adapters = cec_adapters();
    if adapters.len() <= 1 {
        return None;
    }
    adapters
        .into_iter()
        .find(|d| matches!(probe_cec_adapter(d), CecProbe::Connected))
}

/// Run cec-client (from the cec-utils/libcec package) as a playback device and
/// queue the UI commands the TV forwards from its remote. Restarts the client
/// if it exits and retries periodically when no adapter is present, so
/// plugging one in later works without relaunching.
fn cec_reader_loop() {
    loop {
        let port = cec_hdmi_port()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let device = cec_device()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut builder = std::process::Command::new("cec-client");
        builder
            // -t takes a device-type letter (p=playback); the logical-address
            // name "playback1" is not valid here and libcec silently falls back
            // to a recording device.
            .args(["-t", "p", "-d", "15", "-o", "EmbyTheater"])
            .stdin(Stdio::piped()) // kept open: EOF would make cec-client exit
            .stderr(Stdio::null())
            .stdout(Stdio::piped());
        // The adapter chosen on the settings page wins; auto probes instead.
        match device.or_else(pick_cec_adapter) {
            Some(dev) => builder.arg(dev),
            None => &mut builder,
        };
        if let Some(p) = &port {
            // Physical address N.0.0.0 = the TV's HDMI input N.
            builder.arg("-p").arg(p);
        }
        match builder.spawn() {
            Ok(mut child) => {
                let started = Instant::now();
                let stdin = child.stdin.take();
                let out = child.stdout.take();
                *cec_child().lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
                if let Some(out) = out {
                    for line in BufReader::new(out).lines() {
                        let Ok(line) = line else { break };
                        if let Some(cmd) = parse_cec_line(&line) {
                            cec_queue()
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push_back(cmd.to_string());
                        }
                    }
                }
                let _ = stdin;
                // Reap the child (dropping a Child does not wait, which would
                // leave a zombie behind on every restart).
                if let Some(mut child) = cec_child()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                // Exited almost immediately: no adapter -> back off; otherwise
                // (e.g. killed for a port change) restart quickly.
                let backoff = started.elapsed() < Duration::from_secs(3);
                std::thread::sleep(Duration::from_secs(if backoff { 30 } else { 2 }));
            }
            Err(_) => std::thread::sleep(Duration::from_secs(30)),
        }
    }
}

/// Extract the inputmanager command from a cec-client output line. Handles
/// both the decoded form (`... 'User Control Pressed' (0x44)`) and the raw
/// traffic form (`>> 10:44:00` = <init:dest>:<opcode 44=press>:<keycode>).
fn parse_cec_line(line: &str) -> Option<&'static str> {
    if let Some(pos) = line.find("'User Control Pressed' (0x") {
        let rest = &line[pos + 26..];
        let code = u8::from_str_radix(rest.get(..2)?, 16).ok()?;
        return cec_command(code);
    }
    if let Some(pos) = line.find(">> ") {
        let mut it = line[pos + 3..].split(':');
        let route = it.next()?.trim();
        // First byte is (initiator << 4) | destination; remote keys come from
        // the TV (0x0_) or an audio system (0x5_).
        if !matches!(route.as_bytes().first(), Some(b'0') | Some(b'5')) {
            return None;
        }
        if it.next()?.trim().eq_ignore_ascii_case("44") {
            let code = u8::from_str_radix(it.next()?.trim(), 16).ok()?;
            return cec_command(code);
        }
    }
    None
}

/// Drain queued CEC keypresses for the page plugin.
#[tauri::command]
fn cec_poll() -> Vec<String> {
    let mut q = cec_queue().lock().unwrap_or_else(|e| e.into_inner());
    q.drain(..).collect()
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

/// Host CEC plugin, loaded through `appStartInfo.plugins` (the same mechanism
/// the official Theater apps use: the client does `new require(url)`). The
/// constructor polls the native cec_poll queue and feeds the client's own
/// inputmanager singleton, so TV remote keys drive navigation exactly like
/// keyboard input. The dependency id must match the one the client itself
/// resolves to (modules/common/inputmanager.js) or we would bind a second,
/// uninitialized instance.
const CEC_JS: &str = r#"define(["modules/common/inputmanager.js"], function (mod) {
  var im = mod && mod.default ? mod.default : mod;
  return function () {
    this.id = "cecinput";
    this.name = "cec";
    this.type = "input";
    this.getRoutes = function () {
      return [
        {
          path: "cec/cec.html",
          transition: "slide",
          controller: "embyhost://host/cec/cec.js",
          type: "settings",
          title: "HDMI-CEC",
          category: "Playback",
          thumbImage: "",
          icon: "tv",
          settingsTheme: true,
          adjustHeaderForEmbeddedScroll: true,
        },
      ];
    };
    // Apply the saved HDMI port and CEC adapter (if any) at startup.
    try {
      var p = localStorage.getItem("cec-hdmiport") || "";
      window.__TAURI_INTERNALS__.invoke("cec_set_hdmi_port", { port: p });
      var d = localStorage.getItem("cec-device") || "";
      window.__TAURI_INTERNALS__.invoke("cec_set_device", { device: d });
    } catch (e) {}
    var failures = 0;
    function poll() {
      try {
        window.__TAURI_INTERNALS__.invoke("cec_poll").then(
          function (keys) {
            failures = 0;
            for (var i = 0; i < keys.length; i++) {
              try {
                im.trigger(keys[i]);
              } catch (e) {}
            }
            setTimeout(poll, 100);
          },
          function () {
            if (++failures < 50) setTimeout(poll, 1000);
          }
        );
      } catch (e) {
        if (++failures < 50) setTimeout(poll, 1000);
      }
    }
    poll();
  };
});
"#;

/// Settings page controller for the CEC plugin: renders/saves the HDMI port
/// select and pushes the value to the native reader. Dependencies use path
/// ids, not the bare ids the Electron app uses ("loading", "baseView", ...):
/// the web client's alameda loader has no paths config, so bare ids would 404
/// against the site root, while these normalize to the exact module instances
/// the client itself uses.
const CEC_PAGE_JS: &str = r#"define([
  "modules/loading/loading.js",
  "modules/viewmanager/baseview.js",
  "modules/common/appsettings.js",
  "modules/emby-elements/emby-select/emby-select.js",
  "modules/emby-elements/emby-scroller/emby-scroller.js",
], function (loading, BaseView, appSettings) {
  // alameda hands the raw ES-module namespace to the factory; unwrap defaults.
  loading = loading.default || loading;
  BaseView = BaseView.default || BaseView;
  appSettings = appSettings.default || appSettings;
  function onSubmit(e) {
    e.preventDefault();
    return false;
  }
  function renderSettings(view) {
    view.querySelector(".hdmiPort").value = appSettings.get("cec-hdmiport") || "";
    // Populate the adapter select from the native probe (Auto + one option
    // per /dev/cecN, flagged when nothing is plugged into that port).
    var sel = view.querySelector(".cecDevice");
    var saved = appSettings.get("cec-device") || "";
    try {
      window.__TAURI_INTERNALS__.invoke("cec_devices").then(
        function (devs) {
          while (sel.options.length > 1) sel.removeChild(sel.options[1]);
          for (var i = 0; i < devs.length; i++) {
            var o = document.createElement("option");
            o.value = devs[i].path;
            o.textContent = devs[i].connected ? devs[i].path : devs[i].path + " (nothing attached)";
            sel.appendChild(o);
          }
          sel.value = saved;
          if (sel.value !== saved) sel.value = "";
        },
        function () {}
      );
    } catch (e) {}
  }
  function saveSettings(view) {
    var port = view.querySelector(".hdmiPort").value;
    if ((appSettings.get("cec-hdmiport") || "") !== port) {
      appSettings.set("cec-hdmiport", port);
      try {
        window.__TAURI_INTERNALS__.invoke("cec_set_hdmi_port", { port: port });
      } catch (e) {}
    }
    var device = view.querySelector(".cecDevice").value;
    if ((appSettings.get("cec-device") || "") !== device) {
      appSettings.set("cec-device", device);
      try {
        window.__TAURI_INTERNALS__.invoke("cec_set_device", { device: device });
      } catch (e) {}
    }
  }
  function SettingsView(view, params) {
    BaseView.apply(this, arguments);
    view.querySelector("form").addEventListener("submit", onSubmit);
  }
  Object.assign(SettingsView.prototype, BaseView.prototype);
  SettingsView.prototype.onResume = function (options) {
    BaseView.prototype.onResume.apply(this, arguments);
    loading.hide();
    if (options.refresh) {
      renderSettings(this.view);
    }
  };
  SettingsView.prototype.onPause = function () {
    saveSettings(this.view);
    BaseView.prototype.onPause.apply(this, arguments);
  };
  return SettingsView;
});
"#;

/// Settings page markup for the CEC plugin, fetched by the router via its
/// `text!` loader (the custom scheme is CORS-enabled by wry, so the XHR works).
/// The root must carry class="view" (or data-role="page") — that is the
/// element viewmanager extracts from the template.
const CEC_PAGE_HTML: &str = r#"<div is="emby-scroller" class="view flex flex-direction-column scrollFrameY flex-grow" data-mousewheel="true" data-horizontal="false" data-forcescrollbar="true" data-centerfocus="card" data-bindheader="true">
  <div class="scrollSlider flex-grow flex-direction-column padded-left padded-left-page padded-right padded-top-page padded-bottom-page settingsContainer">
    <form class="auto-center">
      <div class="selectContainer">
        <select is="emby-select" class="hdmiPort" label="HDMI port:">
          <option value="">Auto</option>
          <option>1</option>
          <option>2</option>
          <option>3</option>
          <option>4</option>
          <option>5</option>
          <option>6</option>
          <option>7</option>
          <option>8</option>
          <option>9</option>
          <option>10</option>
        </select>
      </div>
      <div class="selectContainer">
        <select is="emby-select" class="cecDevice" label="CEC adapter:">
          <option value="">Auto</option>
        </select>
      </div>
      <div class="fieldDescription">Select the HDMI input on your TV that this computer is connected to, so the TV remote's buttons reach the app over CEC. Leave on Auto to detect.</div>
      <div class="fieldDescription">On boards with one CEC adapter per HDMI port (e.g. Raspberry Pi 4), pick the adapter the TV is cabled to. Leave on Auto to probe.</div>
    </form>
  </div>
</div>
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
    let restart_for_nvidia = fullscreen
        && has_nvidia_gpu()
        && std::env::var_os("WEBKIT_DMABUF_RENDERER_FORCE_SHM").is_none();
    let restart_for_pi = fullscreen
        && is_raspberry_pi()
        && webkit_version_below(2, 50)
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none();
    if restart_for_nvidia || restart_for_pi {
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
            tv_mode().store(fullscreen, Ordering::Relaxed);
            let visible = !fullscreen;
            let _ = w.with_webview(move |wv| set_webview_cursor(&wv.inner(), visible));
        }
    }
}

/// Whether the TV layout is active; read by the load-changed handler to
/// re-apply the hidden cursor after each page load (WebKit installs its own
/// default cursor on the webview window when a new page is created).
#[cfg(target_os = "linux")]
fn tv_mode() -> &'static AtomicBool {
    static TV: AtomicBool = AtomicBool::new(false);
    &TV
}

/// WebKitGTK draws the pointer for the webview's own GDK window, so hiding
/// the cursor on the toplevel window leaves the arrow over the page. Set an
/// empty cursor on the webview widget's window directly.
#[cfg(target_os = "linux")]
fn set_webview_cursor(webview: &webkit2gtk::WebView, visible: bool) {
    use gtk::prelude::*;
    let Some(win) = webview.window() else {
        return;
    };
    if visible {
        win.set_cursor(None);
        return;
    }
    let display = win.display();
    let blank = gtk::gdk::Cursor::from_name(&display, "none").or_else(|| {
        // No "none" cursor in the theme: build a 1x1 fully transparent one.
        let bytes = gtk::glib::Bytes::from(&[0u8; 4][..]);
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
            &bytes,
            gtk::gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            1,
            1,
            4,
        );
        Some(gtk::gdk::Cursor::from_pixbuf(&display, &pixbuf, 0, 0))
    });
    win.set_cursor(blank.as_ref());
}

/// True when the machine has an NVIDIA GPU (vendor 0x10de on the PCI bus, or
/// the proprietary driver loaded). The WebKitGTK crash workarounds below are
/// needed there; on Intel/AMD hardware acceleration works fine and forcing
/// SHM/CPU rendering would only slow things down. Raspberry Pi needs its own
/// workaround (see `is_raspberry_pi`).
fn has_nvidia_gpu() -> bool {
    if fs::metadata("/proc/driver/nvidia/version").is_ok() {
        return true;
    }
    match fs::read_dir("/sys/bus/pci/devices") {
        Ok(devs) => devs.flatten().any(|d| {
            fs::read_to_string(d.path().join("vendor"))
                .map(|v| v.trim().eq_ignore_ascii_case("0x10de"))
                .unwrap_or(false)
        }),
        Err(_) => false,
    }
}

/// WebKitGTK + NVIDIA: the first accelerated-compositing trigger on a page
/// (e.g. the TV client's page-transition animation when a menu item is
/// clicked) segfaults the UI process on a null AcceleratedBackingStore —
/// "segfault at 48" (bugs.webkit.org #321683, block/buzz #3654). The crash is
/// only ever reachable in the TV layout, whose transitions animate; the
/// desktop layout does not trigger it.
/// WEBKIT_DMABUF_RENDERER_FORCE_SHM routes the renderer through shared memory
/// and keeps the backing store valid (unlike the old
/// WEBKIT_DISABLE_DMABUF_RENDERER, which empties the transport set and causes
/// exactly this crash). Ubuntu's libwebkit2gtk additionally ships a
/// disable-nvidia-dmabuf patch that bails out before the SHM mode is added, so
/// its own opt-out (WEBKIT_FORCE_DMABUF_RENDERER) must be set alongside for
/// FORCE_SHM to take effect. WEBKIT_SKIA_ENABLE_CPU_RENDERING keeps Skia off
/// the NVIDIA GL path entirely, which also avoids the driver's GPU-worker
/// teardown segfault on exit.
///
/// Must be called before the webview spawns so all helper processes inherit
/// the environment; respects pre-set values.
fn apply_webkit_nvidia_workarounds() {
    for (var, value) in [
        ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
        ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
        ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1"),
    ] {
        if std::env::var_os(var).is_none() {
            std::env::set_var(var, value);
        }
    }
}

/// True on a Raspberry Pi (device-tree compatible string). The Pi's V3D GPU
/// combined with WebKitGTK's dmabuf renderer fails every page load with
/// "internallyFailedLoadTimerFired" (WebLoaderStrategy.cpp), leaving the TV
/// client stuck on its splash screen.
fn is_raspberry_pi() -> bool {
    fs::read_to_string("/proc/device-tree/compatible")
        .map(|c| c.contains("raspberrypi"))
        .unwrap_or(false)
}

/// True when the linked WebKitGTK is older than `major.minor`.
fn webkit_version_below(major: u32, minor: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let (a, b) = unsafe {
            (
                webkit2gtk::ffi::webkit_get_major_version(),
                webkit2gtk::ffi::webkit_get_minor_version(),
            )
        };
        return (a, b) < (major, minor);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (major, minor);
        false
    }
}

/// Disabling the dmabuf renderer routes WebKit through shared memory, which
/// loads fine on the older (2.48, 32-bit) Pi build where the dmabuf path fails
/// page loads. Newer WebKitGTK (>= 2.50) renders correctly over dmabuf on the
/// Pi's V3D GPU, and forcing SHM there is actively harmful: the shared-memory
/// renderer leaves a null AcceleratedBackingStore that segfaults the UI process
/// the first time a page transition enters accelerated compositing. So the
/// workaround is applied only below 2.50. Must run before the webview spawns so
/// the helper processes inherit the environment; respects pre-set values.
fn apply_webkit_pi_workarounds() {
    if webkit_version_below(2, 50) && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
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
            set_layout_mode,
            cec_poll,
            cec_set_hdmi_port,
            cec_devices,
            cec_set_device
        ])
        .register_uri_scheme_protocol("embyhost", |_ctx, req| {
            let (body, content_type) = match req.uri().path() {
                "/wakeonlan.js" => (WAKE_ON_LAN_JS.as_bytes(), "application/javascript"),
                "/apphost.js" => (APPHOST_JS.as_bytes(), "application/javascript"),
                "/cec.js" => (CEC_JS.as_bytes(), "application/javascript"),
                "/cec/cec.js" => (CEC_PAGE_JS.as_bytes(), "application/javascript"),
                "/cec/cec.html" => (CEC_PAGE_HTML.as_bytes(), "text/html"),
                _ => (SERVER_DISCOVERY_JS.as_bytes(), "application/javascript"),
            };
            tauri::http::Response::builder()
                .header("content-type", content_type)
                .header("access-control-allow-origin", "*")
                .body(body.to_vec())
                .unwrap()
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let version = app.package_info().version.to_string();
            let device_name =
                std::env::var("USER").unwrap_or_else(|_| "Emby Theater".to_string());
            let did = device_id(&handle);
            std::thread::spawn(cec_reader_loop);
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
                // The dmabuf workaround is only reachable in the TV layout.
                if fullscreen {
                    apply_webkit_pi_workarounds();
                }
            }
            if fullscreen && has_nvidia_gpu() {
                apply_webkit_nvidia_workarounds();
            }

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
  plugins: ["embyhost://host/cec.js"],
}}, window.appStartInfo || {{}});
(function startEmby() {{
  if (window.Emby && window.Emby.App && typeof window.Emby.App.start === "function") {{
    window.Emby.App.start(window.appStartInfo);
  }} else {{
    setTimeout(startEmby, 50);
  }}}})();
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

            let window = WebviewWindowBuilder::new(
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

            // wry registers custom schemes as secure but NOT CORS-enabled, and
            // the Emby client fetches plugin page HTML with XHR (its `text!`
            // loader). Without this the request is blocked, the template comes
            // back empty and viewmanager crashes before the controller runs.
            #[cfg(target_os = "linux")]
            {
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
                            set_webview_cursor(webview, !tv_mode().load(Ordering::Relaxed));
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
            super::parse_cec_line(
                "TRAFFIC: [ 1] >> A:5 (0): [ 44 01], 'User Control Pressed' (0x01)"
            ),
            Some("up")
        );
        // Raw traffic form: initiator 0 (TV) -> destination 5, opcode 44.
        assert_eq!(super::parse_cec_line(">> 05:44:41"), Some("volumeup"));
        // Key release (opcode 45) and foreign initiators are ignored.
        assert_eq!(super::parse_cec_line(">> 05:45:41"), None);
        assert_eq!(super::parse_cec_line(">> 15:44:41"), None);
        assert_eq!(super::parse_cec_line("current latency: 24 ms"), None);
    }

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
