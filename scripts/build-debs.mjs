#!/usr/bin/env node
// Build every .deb package this project ships:
//   1. amd64  — native `tauri build` on the host toolchain
//   2. arm64  — Docker + QEMU emulation via docker/Dockerfile.arm64
// Artifacts are copied into out/ with their bundle names.
//
// The arm64 leg is slow from scratch (~1h under emulation) but cached by
// Docker afterwards. Pass --skip-arm64 to build only the native deb.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = join(root, 'out');
const skipArm64 = process.argv.includes('--skip-arm64');
const arm64Only = process.argv.includes('--arm64-only');

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

// 1. Native build (host arch, amd64 on typical dev machines).
if (!arm64Only) {
  run('npm', ['run', 'build:native']);
  copyDebsFrom(join(root, 'src', 'target', 'release', 'bundle', 'deb'), 'native');
}

// 2. arm64 build via Docker/QEMU.
if (!skipArm64) {
  const binfmt = '/proc/sys/fs/binfmt_misc/qemu-aarch64';
  if (!existsSync(binfmt)) {
    console.error(
      '\nqemu-aarch64 binfmt is not registered. Enable it once with:\n' +
        '  docker run --privileged --rm tonistiigi/binfmt --install arm64\n'
    );
    process.exit(1);
  }

  const image = 'embytheater-arm64';
  run('docker', ['build', '-f', join('docker', 'Dockerfile.arm64'), '-t', image, '.']);

  // Extract the deb from the image without running the smoke-test CMD.
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

console.log('\nAll debs in out/:');
for (const f of readdirSync(outDir).filter((f) => f.endsWith('.deb'))) console.log(`  ${f}`);
