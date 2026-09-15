#!/usr/bin/env node
// Build every .deb package this project ships:
//   1. amd64  — native `tauri build` on the host toolchain
//   2. arm64  — Docker + QEMU emulation via docker/Dockerfile.arm64
//   3. armhf  — Docker + QEMU emulation via docker/Dockerfile.armhf
//               (Ubuntu 22.04 armhf — last LTS with full armhf coverage)
// Artifacts are copied into out/ with their bundle names.
//
// The emulated legs are slow from scratch (arm64 ~1h, armhf several hours
// under qemu-arm); the Docker layer cache makes rebuilds cheap.
// Flags: --skip-arm64, --skip-armhf, --arm64-only, --armhf-only.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = join(root, 'out');
const skipArm64 = process.argv.includes('--skip-arm64');
const skipArmhf = process.argv.includes('--skip-armhf');
const arm64Only = process.argv.includes('--arm64-only');
const armhfOnly = process.argv.includes('--armhf-only');

function run(cmd, args, opts = {}) {
  console.log(`\n$ ${cmd} ${args.join(' ')}`);
  execFileSync(cmd, args, { cwd: root, stdio: 'inherit', ...opts });
}

function capture(cmd, args) {
  return execFileSync(cmd, args, { cwd: root, encoding: 'utf8' }).trim();
}

function copyDebsFrom(dir, label) {
  if (!existsSync(dir)) {
    console.error(`No debs found in ${dir} (${label} build may have failed).`);
    process.exit(1);
  }
  mkdirSync(outDir, { recursive: true });
  const debs = readdirSync(dir).filter((f) => f.endsWith('.deb'));
  if (debs.length === 0) {
    console.error(`No .deb files in ${dir} (${label}).`);
    process.exit(1);
  }
  for (const deb of debs) {
    copyFileSync(join(dir, deb), join(outDir, deb));
    console.log(`  -> out/${deb}`);
  }
}

// Docker/QEMU legs: build the image, then copy the debs out of it
// without running the smoke-test CMD.
function dockerDebLeg({ dockerfile, image, binfmt, binfmtHint }) {
  if (!existsSync(binfmt)) {
    console.error(
      `\n${binfmt.split('/').pop()} binfmt is not registered. Enable it with:\n` +
        `  docker run --privileged --rm tonistiigi/binfmt --install ${binfmtHint}\n`
    );
    process.exit(1);
  }

  run('docker', ['build', '-f', join('docker', dockerfile), '-t', image, '.']);

  const cid = capture('docker', ['create', image]);
  try {
    mkdirSync(outDir, { recursive: true });
    console.log(`\n$ docker cp <container>:/app/src/target/release/bundle/deb out/`);
    execFileSync(
      'docker',
      ['cp', `${cid}:/app/src/target/release/bundle/deb/.`, outDir],
      { cwd: root, stdio: 'inherit' }
    );
  } finally {
    execFileSync('docker', ['rm', cid], { stdio: 'ignore' });
  }
}

// 1. Native build (host arch, amd64 on typical dev machines).
if (!arm64Only && !armhfOnly) {
  run('npm', ['run', 'build:native']);
  copyDebsFrom(join(root, 'src', 'target', 'release', 'bundle', 'deb'), 'native');
}

// 2. arm64 build via Docker/QEMU.
if (!skipArm64 && !armhfOnly) {
  dockerDebLeg({
    dockerfile: 'Dockerfile.arm64',
    image: 'embytheater-arm64',
    binfmt: '/proc/sys/fs/binfmt_misc/qemu-aarch64',
    binfmtHint: 'arm64',
  });
}

// 3. armhf (32-bit Raspberry Pi OS) build via Docker/QEMU.
if (!skipArmhf && !arm64Only) {
  dockerDebLeg({
    dockerfile: 'Dockerfile.armhf',
    image: 'embytheater-armhf',
    binfmt: '/proc/sys/fs/binfmt_misc/qemu-arm',
    binfmtHint: 'arm',
  });
}

console.log('\nAll debs in out/:');
for (const f of readdirSync(outDir).filter((f) => f.endsWith('.deb'))) console.log(`  ${f}`);
