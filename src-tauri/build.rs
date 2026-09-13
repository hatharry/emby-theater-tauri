fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            // Generate ACL permissions (allow-<command>) so the remote Emby URL
            // can invoke these app commands via a capability's `remote` context.
            tauri_build::AppManifest::new().commands(&["discover_servers", "wake_on_lan"]),
        ),
    )
    .expect("failed to run tauri-build");
}
