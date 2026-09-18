//! HDMI-CEC support: runs `cec-client` (from cec-utils/libcec) as a playback
//! device, queues the UI commands the TV forwards from its remote, and probes
//! adapters for the settings page.

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
pub(crate) fn set_device(device: Option<String>) {
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
pub(crate) fn set_hdmi_port(port: Option<String>) {
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
pub(crate) fn devices() -> Vec<serde_json::Value> {
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

/// Run cec-client as a playback device and queue the UI commands the TV
/// forwards from its remote. Restarts the client if it exits and retries
/// periodically when no adapter is present, so plugging one in later works
/// without relaunching.
pub(crate) fn reader_loop() {
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
pub(crate) fn parse_cec_line(line: &str) -> Option<&'static str> {
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
pub(crate) fn poll() -> Vec<String> {
    let mut q = cec_queue().lock().unwrap_or_else(|e| e.into_inner());
    q.drain(..).collect()
}
