#!/usr/bin/env node
// Build the amd64 packages (.deb + .rpm) in a clean-room Ubuntu image via
// docker/Dockerfile.amd64, then collect them into out/.
//
// Tauri's RPM bundler is pure Rust (rpm-rs), so the Ubuntu image emits a
// valid .rpm alongside the .deb — no rpmbuild or RPM distro needed. The rpm
// targets Fedora/RHEL/openSUSE installs; its runtime Requires are the RPM
// package names from bundle>linux>rpm>depends plus the webkit2gtk/gtk
// sonames that tauri-cli injects automatically.

import { dockerLeg, fixupDebDeps, listPackages } from './lib.mjs';

dockerLeg('amd64');
fixupDebDeps();
listPackages();
