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
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
    // Copy only the .deb files — the bundle dir also contains the bundler's
    // unpacked work directory, which we don't want in out/.
    const listing = capture('docker', [
      'run',
      '--rm',
      '--entrypoint',
      'bash',
      image,
      '-c',
      'ls /app/src/target/release/bundle/deb/*.deb',
    ]);
    for (const path of listing.split('\n').filter(Boolean)) {
      const name = path.split('/').pop();
      console.log(`\n$ docker cp <container>:${path} out/${name}`);
      execFileSync('docker', ['cp', `${cid}:${path}`, join(outDir, name)], {
        cwd: root,
        stdio: 'inherit',
      });
    }
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

// Debian trixie renamed libgtk-3-0 -> libgtk-3-0t64 (the t64 transition)
// without providing the old name, so the bundler's auto-added dependency
// makes apt fail there. Rewrite it as an alternative so the deb installs on
// both Ubuntu and Debian. Applied to every deb in out/ after collection.
function fixupDebDeps() {
  for (const deb of readdirSync(outDir).filter((f) => f.endsWith('.deb'))) {
    const path = join(outDir, deb);
    const dir = join(outDir, `${deb}.fix`);
    rmSync(dir, { recursive: true, force: true });
    execFileSync('dpkg-deb', ['-R', path, dir], { stdio: 'inherit' });
    const control = join(dir, 'DEBIAN', 'control');
    const before = readFileSync(control, 'utf8');
    // Collapse any existing libgtk-3-0 / t64 alternative to the canonical
    // "libgtk-3-0 | libgtk-3-0t64" form (idempotent across rebuilds).
    const after = before.replace(
      /\blibgtk-3-0(?:t64)?(?:\s*\|\s*libgtk-3-0(?:t64)?)*/g,
      'libgtk-3-0 | libgtk-3-0t64'
    );
    if (after === before) {
      rmSync(dir, { recursive: true, force: true });
      continue;
    }
    writeFileSync(control, after);
    execFileSync('dpkg-deb', ['-b', '--root-owner-group', dir, path], {
      stdio: 'inherit',
    });
    rmSync(dir, { recursive: true, force: true });
    console.log(`  fixed libgtk-3-0t64 dependency in ${deb}`);
  }
}
fixupDebDeps();

console.log('\nAll debs in out/:');
for (const f of readdirSync(outDir).filter((f) => f.endsWith('.deb'))) console.log(`  ${f}`);
