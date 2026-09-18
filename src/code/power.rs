//! System power actions surfaced through the client's back menu
//! (appHost.shutdown / appHost.restart). These power the whole machine, not
//! just the app — matching the official Emby Theater desktop apps, whose
//! "Shutdown"/"Restart" items call shutdownSystem()/restartSystem().

use std::process::Command;

/// Run a systemctl power action. Returns whether the command was accepted
/// (exit status 0).
fn systemctl(action: &str) -> bool {
    match Command::new("systemctl").arg(action).status() {
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
