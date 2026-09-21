//! Hardware detection and the WebKitGTK crash workarounds it selects.

use std::fs;
use std::sync::atomic::AtomicBool;

/// True when the machine has an NVIDIA GPU (vendor 0x10de on the PCI bus, or
/// the proprietary driver loaded). The WebKitGTK crash workarounds below are
/// needed there; on Intel/AMD hardware acceleration works fine and forcing
/// SHM/CPU rendering would only slow things down. Raspberry Pi needs its own
/// workaround (see `is_raspberry_pi`).
pub(crate) fn has_nvidia_gpu() -> bool {
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

/// True on a Raspberry Pi (device-tree compatible string). Used to select
/// the Pi device profile and the GStreamer feature-rank fix for the broken
/// vc4 stateless HEVC decoder (see lib.rs setup).
pub(crate) fn is_raspberry_pi() -> bool {
    fs::read_to_string("/proc/device-tree/compatible")
        .map(|c| c.contains("raspberrypi"))
        .unwrap_or(false)
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
pub(crate) fn apply_webkit_nvidia_workarounds() {
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

/// Whether the TV layout is active; read by the load-changed handler to
/// re-apply the hidden cursor after each page load (WebKit installs its own
/// default cursor on the webview window when a new page is created).
#[cfg(target_os = "linux")]
pub(crate) fn tv_mode() -> &'static AtomicBool {
    static TV: AtomicBool = AtomicBool::new(false);
    &TV
}

/// WebKitGTK draws the pointer for the webview's own GDK window, so hiding
/// the cursor on the toplevel window leaves the arrow over the page. Set an
/// empty cursor on the webview widget's window directly.
#[cfg(target_os = "linux")]
pub(crate) fn set_webview_cursor(webview: &webkit2gtk::WebView, visible: bool) {
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
