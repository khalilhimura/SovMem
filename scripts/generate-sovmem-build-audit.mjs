#!/usr/bin/env node
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { checkAudit, writeAudit } from './lib/sovmem-build-audit.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const options = {
  root,
  statePath: resolve(root, 'docs/sovmem-build-state.json'),
  reportPath: resolve(root, 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md'),
};

try {
  if (process.argv.length === 3 && process.argv[2] === '--check') {
    checkAudit(options);
    process.stdout.write('SovMem build audit is current.\n');
  } else if (process.argv.length === 2) {
    writeAudit(options);
    process.stdout.write('Updated docs/SOVMEM-BUILD-AUDIT.md.\n');
  } else {
    throw new Error('usage: generate-sovmem-build-audit.mjs [--check]');
  }
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
}
