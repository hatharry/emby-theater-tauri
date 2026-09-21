#!/usr/bin/env node
// Run a single arch's Docker build leg: node scripts/build-one.mjs <arch>.
// Used by build-all.mjs (one child process per arch, in parallel) and handy
// for manual one-off builds. The deb dependency fixup is deliberately NOT
// done here — build-all runs it once after all legs finish, so parallel legs
// never touch each other's freshly-copied debs in out/.

import { dockerLeg } from './lib.mjs';

const arch = process.argv[2];
if (!arch) {
  console.error('usage: node scripts/build-one.mjs <amd64|arm64|armhf>');
  process.exit(2);
}
dockerLeg(arch);
