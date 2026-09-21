#!/usr/bin/env node
// Build every architecture's packages in parallel: one child process per
// arch, each running its own Docker build leg concurrently, then the shared
// post-processing (libgtk dependency fixup + listing) once, after all legs
// have finished copying into out/.
//
// QEMU legs (arm64/armhf) need binfmt registered; check it up front so the
// whole run fails fast rather than after a long amd64 build.

import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { fixupDebDeps, listPackages, root } from './lib.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const ARCHES = ['amd64', 'arm64', 'armhf'];
const BINFMT = { arm64: '/proc/sys/fs/binfmt_misc/qemu-aarch64', armhf: '/proc/sys/fs/binfmt_misc/qemu-arm' };

const missing = Object.entries(BINFMT).filter(([, p]) => !existsSync(p));
if (missing.length) {
  const hints = missing.map(([a]) => (a === 'arm64' ? 'arm64' : 'arm')).join(',');
  console.error(
    `\nMissing binfmt for: ${missing.map(([a]) => a).join(', ')}\n` +
      `Enable it with:\n  docker run --privileged --rm tonistiigi/binfmt --install ${hints}\n`
  );
  process.exit(1);
}

console.log(`Launching ${ARCHES.length} parallel build legs: ${ARCHES.join(', ')}`);

const children = ARCHES.map((arch) => {
  const child = spawn(process.execPath, [join(here, 'build-one.mjs'), arch], {
    cwd: root,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const prefix = `[${arch}] `;
  const stamp = (stream) => (chunk) => {
    for (const line of chunk.toString().split('\n')) {
      if (line.trim()) stream.write(prefix + line + '\n');
    }
  };
  child.stdout.on('data', stamp(process.stdout));
  child.stderr.on('data', stamp(process.stderr));
  return new Promise((resolvePromise, rejectPromise) => {
    child.on('exit', (code, signal) =>
      code === 0
        ? resolvePromise(arch)
        : rejectPromise(new Error(`${arch} leg failed (exit ${code ?? signal})`))
    );
  });
});

const results = await Promise.allSettled(children);
const failures = results.filter((r) => r.status === 'rejected').map((r) => r.reason.message);

if (failures.length) {
  console.error(`\n${failures.length} leg(s) failed:\n  ` + failures.join('\n  '));
  process.exit(1);
}

fixupDebDeps();
listPackages();
console.log('\nAll legs built in parallel.');
