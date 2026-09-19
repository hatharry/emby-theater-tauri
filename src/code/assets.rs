//! The JavaScript/HTML assets served to the web client over the `embyhost://`
//! scheme, plus the startup script template. Each file lives next to this
//! module in `code/` (with the CEC page assets under `code/cec/`).

/// Raspberry Pi device profile, served over `embyhost://` and loaded through
/// `appStartInfo.plugins` (the same mechanism the official Theater apps use).
/// It replaces the client's `browserdeviceprofile.js` builder wholesale by
/// overriding the player's `getDeviceProfile`, so there is no `canPlayType`
/// probing and no `Emby.importModule` wrapping. The profile is a static object
/// captured from the client's own builder running on this exact WebKitGTK
/// build, then tuned for the Pi's hardware:
///
/// - Direct play H.264/VP8/VP9 (the vc4 block decodes them), but NOT HEVC or
///   AV1 — the Pi 4 has no hardware block for either, so omitting them makes
///   the server transcode to H.264, which the Pi decodes in hardware.
/// - The first video TranscodingProfile is a progressive Matroska stream: the
///   server emits /videos/…/stream.mkv and GStreamer plays it in hardware via a
///   plain video.src. HLS is unplayable in this webview (hls.js/MSE throws
///   mediadecodeerror, native HLS throws "no compatible streams"). Matroska,
///   not MP4, because ffmpeg cannot mux AC3/E-AC3 into MP4 (its encoder is
///   experimental there) — an MP4 profile fails the transcode for any title
///   with AC3/E-AC3 audio, which the client reports as "no streams available".
///
/// NOTE: the server must have transcode THROTTLING enabled (Dashboard ->
/// Playback -> Transcoding). Unthrottled, ffmpeg writes the whole film to a
/// temp file as fast as it can encode (observed speed=15.9x, throttle=off —
/// 4.9 GB in about 5 minutes) and the webview buffers that firehose until the
/// kernel OOM-kills the WebKit web process, which looks like playback freezing.
/// Throttled it runs at ~1.3x and swap stays empty.
pub(crate) const PI_DEVICE_PROFILE_JS: &str = include_str!("./deviceprofiles/pi.js");

/// Desktop (amd64) device profile, served over `embyhost://` and loaded through
/// `appStartInfo.plugins` on non-Pi builds. Its DirectPlayProfiles were derived
/// empirically: every container x codec combination was generated with ffmpeg
/// and played through this exact WebKitGTK+GStreamer engine (320x240 and
/// 1280x720), keeping only the combinations that reach 'ended' at >= ~1x real
/// time. Unlike the Pi, the desktop decodes HEVC/AV1 in software, so those
/// direct-play instead of transcoding. Native HLS is unplayable here, so the
/// first video TranscodingProfile is progressive Matroska.
pub(crate) const DEFAULT_DEVICE_PROFILE_JS: &str = include_str!("./deviceprofiles/default.js");

/// AMD module served over the `embyhost://` custom protocol and referenced from
/// `appStartInfo.paths.serverdiscovery`. The client's loader resolves it instead
/// of its built-in no-op discovery module (browsers cannot UDP broadcast), and
/// it calls our native `discover_servers` command.
pub(crate) const SERVER_DISCOVERY_JS: &str = include_str!("./serverdiscovery.js");

/// Host apphost module: wraps the client's built-in apphost and enables the
/// `exit` capability, which the web apphost only reports on native LG/Tizen.
/// With it the TV client shows its "are you ready to exit" back menu on Esc
/// instead of having no way to quit.
///
/// The dependency id must match the one the client itself uses
/// (`importFromPath("./modules/apphost.js")` normalizes to `modules/apphost.js`);
/// a leading slash would create a second instance whose servicelocator was never
/// initialized, and appHost.init() would reject, leaving the splash screen up.
pub(crate) const APPHOST_JS: &str = include_str!("./apphost.js");

/// Host Wake-on-LAN module (same rationale as serverdiscovery: browsers cannot
/// send UDP magic packets). `send(info)` receives the server's WakeInfo, whose
/// MacAddress/Address/Port we forward to the native command.
pub(crate) const WAKE_ON_LAN_JS: &str = include_str!("./wakeonlan.js");

/// Host CEC plugin, loaded through `appStartInfo.plugins` (the same mechanism
/// the official Theater apps use: the client does `new require(url)`). The
/// constructor polls the native cec_poll queue and feeds the client's own
/// inputmanager singleton, so TV remote keys drive navigation exactly like
/// keyboard input. The dependency id must match the one the client itself
/// resolves to (modules/common/inputmanager.js) or we would bind a second,
/// uninitialized instance.
pub(crate) const CEC_JS: &str = include_str!("./cec/plugin.js");

/// Settings page controller for the CEC plugin: renders/saves the HDMI port
/// select and pushes the value to the native reader. Dependencies use path
/// ids, not the bare ids the Electron app uses ("loading", "baseView", ...):
/// the web client's alameda loader has no paths config, so bare ids would 404
/// against the site root, while these normalize to the exact module instances
/// the client itself uses.
pub(crate) const CEC_PAGE_JS: &str = include_str!("./cec/settings.js");

/// Settings page markup for the CEC plugin, fetched by the router via its
/// `text!` loader (the custom scheme is CORS-enabled by wry, so the XHR works).
/// The root must carry class="view" (or data-role="page") — that is the
/// element viewmanager extracts from the template.
pub(crate) const CEC_PAGE_HTML: &str = include_str!("./cec/settings.html");

/// Resolve an `embyhost://` request path to the asset bytes and content type.
/// Unknown paths fall back to the server-discovery module, matching the
/// `appStartInfo.paths.serverdiscovery` URL.
pub(crate) fn response_for(path: &str) -> (&'static [u8], &'static str) {
    match path {
        "/wakeonlan.js" => (WAKE_ON_LAN_JS.as_bytes(), "application/javascript"),
        "/apphost.js" => (APPHOST_JS.as_bytes(), "application/javascript"),
        "/cec.js" => (CEC_JS.as_bytes(), "application/javascript"),
        "/cec/cec.js" => (CEC_PAGE_JS.as_bytes(), "application/javascript"),
        "/cec/cec.html" => (CEC_PAGE_HTML.as_bytes(), "text/html"),
        "/deviceprofiles/pi.js" => (PI_DEVICE_PROFILE_JS.as_bytes(), "application/javascript"),
        "/deviceprofiles/default.js" => {
            (DEFAULT_DEVICE_PROFILE_JS.as_bytes(), "application/javascript")
        }
        _ => (SERVER_DISCOVERY_JS.as_bytes(), "application/javascript"),
    }
}

/// The init script injected before any page script runs, so the Emby app sees
/// `window.appStartInfo` on first load. Once the page's Emby.App is ready it
/// starts the app itself with that info. On the Pi it also registers the
/// static device-profile plugin (see PI_DEVICE_PROFILE_JS) through the same
/// plugin mechanism the official Theater apps use.
pub(crate) fn startup_script(version: &str, did: &str, device_name: &str, is_pi: bool) -> String {
    include_str!("./startup.js")
        .replace("{version}", version)
        .replace("{did}", did)
        .replace("{device_name}", device_name)
        .replace(
            "{profile_plugin}",
            if is_pi {
                ", \"embyhost://host/deviceprofiles/pi.js\""
            } else {
                ", \"embyhost://host/deviceprofiles/default.js\""
            },
        )
}
