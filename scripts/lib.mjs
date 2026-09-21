#!/usr/bin/env node
// Shared plumbing for the build-*.mjs packaging scripts:
//   - dockerLeg(name): build a Docker image for one arch and copy its
//     bundle artifacts out of the image into out/ (without running the
//     image's smoke-test CMD).
//   - fixupDebDeps(): normalize the libgtk-3-0t64 dependency in every deb.
//   - listPackages(): print what ended up in out/.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const outDir = join(root, 'out');

export function run(cmd, args, opts = {}) {
  console.log(`\n$ ${cmd} ${args.join(' ')}`);
  execFileSync(cmd, args, { cwd: root, stdio: 'inherit', ...opts });
}

export function capture(cmd, args) {
  return execFileSync(cmd, args, { cwd: root, encoding: 'utf8' }).trim();
}

// One Docker build leg per arch. `binfmt` (when set) is checked first so
// QEMU-emulated legs fail fast with instructions instead of a cryptic error.
export const legs = {
  amd64: {
    dockerfile: 'Dockerfile.amd64',
    image: 'embytheater-amd64',
    globs: [
      '/app/src/target/release/bundle/deb/*.deb',
      '/app/src/target/release/bundle/rpm/*.rpm',
      '/app/src/target/release/bundle/appimage/*.AppImage',
    ],
  },
  arm64: {
    dockerfile: 'Dockerfile.arm64',
    image: 'embytheater-arm64',
    binfmt: '/proc/sys/fs/binfmt_misc/qemu-aarch64',
    binfmtHint: 'arm64',
    globs: ['/app/src/target/release/bundle/deb/*.deb'],
  },
  armhf: {
    dockerfile: 'Dockerfile.armhf',
    image: 'embytheater-armhf',
    binfmt: '/proc/sys/fs/binfmt_misc/qemu-arm',
    binfmtHint: 'arm',
    globs: ['/app/src/target/release/bundle/deb/*.deb'],
  },
};

export function dockerLeg(name) {
  const { dockerfile, image, binfmt, binfmtHint, globs } = legs[name];
  if (binfmt && !existsSync(binfmt)) {
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
    // Copy only the bundle artifacts — the bundle dirs also contain the
    // bundler's unpacked work directories, which we don't want in out/.
    const listing = capture('docker', [
      'run',
      '--rm',
      '--entrypoint',
      'bash',
      image,
      '-c',
      `ls ${globs.join(' ')}`,
    ]);
    for (const path of listing.split('\n').filter(Boolean)) {
      const file = path.split('/').pop();
      console.log(`\n$ docker cp <container>:${path} out/${file}`);
      execFileSync('docker', ['cp', `${cid}:${path}`, join(outDir, file)], {
        cwd: root,
        stdio: 'inherit',
      });
    }
  } finally {
    execFileSync('docker', ['rm', cid], { stdio: 'ignore' });
  }
}

// Debian trixie renamed libgtk-3-0 -> libgtk-3-0t64 (the t64 transition)
// without providing the old name, so the bundler's auto-added dependency
// makes apt fail there. Rewrite it as an alternative so the deb installs on
// both Ubuntu and Debian. Applied to every deb in out/ after collection.
export function fixupDebDeps() {
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

export function listPackages() {
  console.log('\nAll packages in out/:');
  for (const f of readdirSync(outDir).filter((f) => /\.(deb|rpm)$/.test(f))) console.log(`  ${f}`);
}
