# Emby Theater for Linux (Tauri)

A native Linux client for the [Emby](https://emby.media) Theater TV web
client, built on Tauri + WebKitGTK. It continues the Emby Theater lineage
(the Electron app ended at 3.0.21) as a ground-up rewrite: no bundled
Chromium, ~2.6 MB packages, and a fraction of the memory footprint. It
loads the same web client the Theater apps do, wrapped in a native shell
with Linux integrations.

## Features

- **The real Emby Theater client** — the TV web app in a native window, with
  both TV (fullscreen, remote-driven) and desktop layouts
- **Hardware-accelerated playback** through GStreamer: VA-API/NVDEC on x86,
  V4L2 MPP on Pi/RK platforms, with per-device profiles tuned for the
  Raspberry Pi
- **HDMI-CEC** — control playback with your TV remote: navigation, volume,
  power commands, and TV-aware input handling
- **Wake-on-LAN** and automatic server discovery (UDP 7359) on the login
  screen
- **Server integration** — appears in your Emby server's device list as
  "Emby Theater" with your machine's hostname and the Emby icon;
  shutdown/restart of the host from the in-app back menu
- **Direct-play seeking fixed** — forward seeks and chapter jumps work
  reliably on direct-play streams (a WebKitGTK/GStreamer container-index
  interaction that clamped seeks to the buffer end)

## Download

Grab the latest packages from the
[releases page](https://github.com/hatharry/emby-theater-tauri/releases):

| Platform | Packages |
|---|---|
| x86_64 | `.deb`, `.rpm`, `.AppImage` (self-contained, bundles GStreamer) |
| arm64 (Pi 4/5 64-bit, other aarch64) | `.deb` |
| armhf (32-bit Raspberry Pi OS) | `.deb` |

Install a package:

```sh
sudo apt install ./emby-theater_4.*.deb     # Debian/Ubuntu/Raspberry Pi OS
sudo dnf install ./emby-theater-4.*.rpm     # Fedora/RHEL/openSUSE
```

Or run the AppImage directly:

```sh
chmod a+x emby-theater_4.*.AppImage
./emby-theater_4.*.AppImage
```

## Requirements

- WebKitGTK 4.1 (`libwebkit2gtk-4.1-0`) — pulled in automatically by the
  packages
- GStreamer plugins for media playback (`gstreamer1.0-plugins-{base,good,bad}`,
  `gstreamer1.0-libav`) — declared as package dependencies; the AppImage
  bundles its own set
- `cec-utils` for HDMI-CEC support (optional)

## Raspberry Pi notes

Tested on a Pi 4 running Raspberry Pi OS (bookworm/trixie, labwc or GNOME).
H.264 decodes in hardware; HEVC falls back to software (the Pi 4 has no HEVC
block). Embedded and external subtitles render through the client's own
parsers.

## Building from source

Prerequisites: Docker (with `binfmt` QEMU emulation for the ARM legs),
Node.js, and an x86_64 Linux host.

```sh
npm install

# All architectures in parallel (~15-20 min, bounded by the QEMU legs):
docker run --privileged --rm tonistiigi/binfmt --install arm64,arm
npm run build

# Single leg:
npm run build:amd64     # deb + rpm + AppImage
npm run build:arm64     # deb (QEMU)
npm run build:armhf     # deb (QEMU)

# Development (native debug build + hot reload):
npm run dev
```

Artifacts land in `out/`. Each leg is a clean-room Docker build
(`docker/Dockerfile.*`); `scripts/build-all.mjs` runs them concurrently and
normalizes deb dependencies afterwards.

## Repository layout

```
src/                 Tauri crate root
  code/              Rust modules + all injected JS/HTML assets
    deviceprofiles/  Per-device playback profiles (Pi, desktop)
    cec/             HDMI-CEC plugin for the web client
  capabilities/      Tauri permission set
  gen/               Generated schemas and icons
  ui/                Local splash page
docker/              Clean-room build images per architecture
scripts/             Build orchestration
```

## License

Emby and the Emby logo are trademarks of Emby Media.
