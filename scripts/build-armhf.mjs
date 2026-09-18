#!/usr/bin/env node
// Build the armhf .deb (32-bit Raspberry Pi OS) via Docker + QEMU emulation
// using docker/Dockerfile.armhf, then collect it into out/.
//
// Very slow from scratch under qemu-arm (several hours); the Docker layer
// cache makes rebuilds cheap. Requires qemu-arm binfmt registration:
//   docker run --privileged --rm tonistiigi/binfmt --install arm

import { dockerLeg, fixupDebDeps, listPackages } from './lib.mjs';

dockerLeg('armhf');
fixupDebDeps();
listPackages();
