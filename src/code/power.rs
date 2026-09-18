//! System power actions surfaced through the client's back menu
//! (appHost.shutdown / appHost.restart). These power the whole machine, not
//! just the app — matching the official Emby Theater desktop apps, whose
//! "Shutdown"/"Restart" items call shutdownSystem()/restartSystem().

use std::process::Command;

/// Run a systemctl power action, escalating through passwordless sudo if the
/// direct call is refused (a kiosk session without a polkit agent). Returns
/// whether the command was accepted (exit status 0).
fn systemctl(action: &str) -> bool {
    if let Ok(status) = Command::new("systemctl").arg(action).status() {
        if status.success() {
            return true;
        }
    }
    match Command::new("sudo").args(["-n", "systemctl", action]).status() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

/// Power off the machine.
pub(crate) fn shutdown() -> bool {
    systemctl("poweroff")
}

/// Reboot the machine.
pub(crate) fn restart() -> bool {
    systemctl("reboot")
}
