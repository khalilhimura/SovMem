import { spawnSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const sovmem = process.argv[2] === '--sovmem';
if (process.argv.length > (sovmem ? 3 : 2)) {
  console.error('Usage: node scripts/run-js-tests.mjs [--sovmem]');
  process.exit(1);
}

// Keep the Rust-dependent harness explicit without moving historical test paths.
const rustTests = new Set(['sovmem-p3-safe-clone.test.js']);
const files = readdirSync(new URL('../test/', import.meta.url)).sort()
  .filter(file => file.endsWith('.test.js') && rustTests.has(file) === sovmem)
  .map(file => `test/${file}`);
if (!files.length) throw new Error('No JavaScript tests selected');

if (sovmem) {
  const cargo = spawnSync('cargo', ['--version'], { cwd: root, encoding: 'utf8' });
  if (cargo.error || cargo.status !== 0) {
    console.error('SovMem tests require Cargo. In a prepared Codex workspace, run:');
    console.error(`source "${root}scripts/codex-env.sh"`);
    console.error('For a fresh workspace, first run bash scripts/setup-codex.sh.');
    process.exit(1);
  }
}

const result = spawnSync(process.execPath, ['--test', ...files], { cwd: root, stdio: 'inherit' });
if (result.error) console.error(result.error.message);
process.exitCode = result.status ?? 1;
