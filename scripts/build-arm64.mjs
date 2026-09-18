#!/usr/bin/env node
// Build the arm64 .deb (Raspberry Pi OS 64-bit / arm64 Linux) via Docker +
// QEMU emulation using docker/Dockerfile.arm64, then collect it into out/.
//
// Slow from scratch (~1h under qemu-aarch64); the Docker layer cache makes
// rebuilds cheap. Requires qemu-aarch64 binfmt registration:
//   docker run --privileged --rm tonistiigi/binfmt --install arm64

import { dockerLeg, fixupDebDeps, listPackages } from './lib.mjs';

dockerLeg('arm64');
fixupDebDeps();
listPackages();
