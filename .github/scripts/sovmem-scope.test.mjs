import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, mkdirSync, renameSync, rmSync, readFileSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
const load = () => import('./sovmem-scope.mjs');

test('site, account, runtime and ordinary documentation changes skip full SovMem checks', async () => {
  const { requiresSovmem } = await load();
  assert.equal(requiresSovmem(['apps/site/src/app/page.tsx', 'apps/account/worker/index.ts', 'packages/account-platform/src/auth.ts', 'packages/runtime/src/index.ts', 'README.md', 'docs/site-migration/operations.md', 'test/site-migration.test.js', 'scripts/verify-site.mjs']), false);
});

test('SovMem source, private-input contracts, regressions and audit tooling require full checks', async () => {
  const { requiresSovmem } = await load();
  for (const file of ['sovmem/src/lib.rs', 'sovmem/tests/cli.rs', 'profiles/audy-ssa0-v0/profile.yaml', 'evals/audy-sovmem-v0/golden.jsonl', 'test/sovmem-p3-safe-clone.test.js', 'test/retrieval-corpus-version.test.js', 'scripts/lib/sovmem-build-audit.mjs', 'scripts/generate-sovmem-build-audit.mjs', 'scripts/sovmem-sentry.sh', 'docs/sovmem_v0.3_execution_plan.md', 'docs/spec/source-lineage-june-23-spec-bundle-verbatim.md', 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md']) assert.equal(requiresSovmem([file]), true, file);
});

test('shared build configuration and the CI policy itself require full checks', async () => {
  const { requiresSovmem } = await load();
  for (const file of ['package.json', 'package-lock.json', 'rust-toolchain.toml', 'scripts/run-js-tests.mjs', 'scripts/lib/sovmem-inputs.mjs', '.github/workflows/sovmem-audit.yml', '.github/scripts/sovmem-scope.mjs', '.github/scripts/sovmem-scope.test.mjs']) assert.equal(requiresSovmem([file]), true, file);
});

test('generated build files and unrelated root tests are outside the SovMem scope', async () => {
  const { requiresSovmem } = await load();
  for (const file of ['sovmem/target/debug/sovmem', 'test/index.test.js', 'test/site-migration.test.js', 'test/sovmem-fixtures/unrelated.test.js']) {
    assert.equal(requiresSovmem([file]), false, file);
  }
  assert.equal(requiresSovmem(['test/sovmem-new.test.js']), true);
});

test('canonical audit state and executable safe-clone boundaries cannot skip regressions', async () => {
  const { requiresSovmem } = await load();
  for (const file of ['docs/sovmem-build-state.json', 'scripts/provision-sovmem-p3-safe-clone.mjs', 'scripts/verify-sovmem-p3-safe-clone.mjs']) assert.equal(requiresSovmem([file]), true, file);
});

test('manual dispatch forces a full run, while missing/invalid comparisons fail closed', async () => {
  const { selectRun } = await load();
  assert.equal(selectRun({ event: 'workflow_dispatch' }), true);
  assert.throws(() => selectRun({ event: 'pull_request' }), /valid base and head/);
  assert.throws(() => selectRun({ event: 'merge_group', base: 'main; echo unsafe', head: 'a'.repeat(40) }), /valid base and head/);
  assert.throws(() => selectRun({ event: 'unknown' }), /unsupported event/);
});

test('renaming a SovMem file out of its directory still requires full checks', async () => {
  const { changedPaths, requiresSovmem } = await load();
  const dir = mkdtempSync(join(tmpdir(), 'nous-ci-scope-'));
  const git = (...args) => execFileSync('git', ['-C', dir, ...args], { encoding: 'utf8' }).trim();
  try {
    git('init', '-q'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
    mkdirSync(join(dir, 'sovmem')); writeFileSync(join(dir, 'sovmem/source.rs'), 'fixture');
    git('add', '.'); git('commit', '-qm', 'base'); const base = git('rev-parse', 'HEAD');
    renameSync(join(dir, 'sovmem/source.rs'), join(dir, 'unrelated.rs'));
    git('add', '-A'); git('commit', '-qm', 'rename'); const head = git('rev-parse', 'HEAD');
    const files = changedPaths(base, head, dir);
    assert.ok(files.includes('sovmem/source.rs'));
    assert.ok(files.includes('unrelated.rs'));
    assert.equal(requiresSovmem(files), true);
    writeFileSync(join(dir, 'README.md'), 'site-only'); git('add', '.'); git('commit', '-qm', 'docs');
    assert.equal(requiresSovmem(changedPaths(head, git('rev-parse', 'HEAD'), dir)), false);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('CLI reports the selected full-run output without requiring Git for a manual run', async () => {
  await load();
  const dir = mkdtempSync(join(tmpdir(), 'nous-ci-output-'));
  try {
    const output = join(dir, 'output');
    const result = spawnSync(process.execPath, [new URL('./sovmem-scope.mjs', import.meta.url).pathname], { encoding: 'utf8', env: { ...process.env, GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_OUTPUT: output } });
    assert.equal(result.status, 0, result.stderr);
    const { readFileSync } = await import('node:fs');
    assert.equal(readFileSync(output, 'utf8'), 'full=true\n');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

function lockFixture(run, root = { name: 'fixture', workspaces: ['packages/*'] }) {
  const cwd = mkdtempSync(join(tmpdir(), 'nous-lock-scope-'));
  const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
  const lock = { name: 'fixture', lockfileVersion: 3, requires: true, packages: { '': root, 'node_modules/stripe': { version: '1.0.0' } } };
  const write = value => writeFileSync(join(cwd, 'package-lock.json'), JSON.stringify(value));
  const commit = () => { git('add', '.'); git('commit', '-qm', 'fixture'); return git('rev-parse', 'HEAD'); };
  try {
    git('init', '-q'); git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
    mkdirSync(join(cwd, 'docs/archive/audits'), { recursive: true });
    writeFileSync(join(cwd, 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md'), `# Audit\n- Source-tree fingerprint: \`${'a'.repeat(64)}\`\nReviewed state unchanged.\n`);
    write(lock); const base = commit();
    run({ cwd, git, lock, write, commit, base });
  } finally { rmSync(cwd, { recursive: true, force: true }); }
}

test('workspace-only lockfile updates skip Rust while explicit manual runs still select it', async () => {
  const { selectRun } = await load();
  lockFixture(({ cwd, lock, write, commit, base }) => {
    lock.packages['node_modules/stripe'].version = '2.0.0'; write(lock);
    const head = commit();
    assert.equal(selectRun({ event: 'pull_request', base, head, cwd }), false);
    assert.equal(selectRun({ event: 'merge_group', base, head, cwd }), false);
    assert.equal(selectRun({ event: 'workflow_dispatch', base, head, cwd }), true);
    const report = join(cwd, 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md');
    writeFileSync(report, readFileSync(report, 'utf8').replace('a'.repeat(64), 'b'.repeat(64)));
    assert.equal(selectRun({ event: 'pull_request', base, head: commit(), cwd }), false);
    mkdirSync(join(cwd, 'sovmem')); writeFileSync(join(cwd, 'sovmem', 'source.rs'), 'changed');
    assert.equal(selectRun({ event: 'pull_request', base, head: commit(), cwd }), true);
  });
});

test('audit edits beyond the derived fingerprint still require full checks alongside workspace lock updates', async () => {
  const { selectRun } = await load();
  lockFixture(({ cwd, lock, write, commit, base }) => {
    lock.packages['node_modules/stripe'].version = '2.0.0'; write(lock);
    const report = join(cwd, 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md');
    writeFileSync(report, readFileSync(report, 'utf8').replace('Reviewed state unchanged.', 'Different gate claim.'));
    assert.equal(selectRun({ event: 'pull_request', base, head: commit(), cwd }), true);
  });
});

test('lockfile-only changes to shared root configuration or dependency graphs require Rust', async () => {
  const { selectRun } = await load();
  lockFixture(({ cwd, lock, write, commit, base }) => {
    lock.packages[''].engines = { node: '>=22' }; write(lock);
    assert.equal(selectRun({ event: 'pull_request', base, head: commit(), cwd }), true);
  });
  lockFixture(({ cwd, lock, write, commit, base }) => {
    lock.packages['node_modules/stripe'].version = '2.0.0'; write(lock);
    assert.equal(selectRun({ event: 'pull_request', base, head: commit(), cwd }), true);
  }, { name: 'fixture', dependencies: { stripe: '*' } });
});

test('unreadable, missing, and unsupported lockfiles cannot skip full checks', async () => {
  const { selectRun } = await load();
  for (const mutate of [
    ({ cwd }) => writeFileSync(join(cwd, 'package-lock.json'), '{'),
    ({ cwd }) => unlinkSync(join(cwd, 'package-lock.json')),
    ({ lock, write }) => { lock.lockfileVersion = 2; write(lock); },
    ({ lock, write }) => { delete lock.packages['']; write(lock); },
    ({ lock, write }) => { lock.requires = false; write(lock); },
  ]) lockFixture(fixture => {
    mutate(fixture);
    assert.equal(selectRun({ event: 'pull_request', base: fixture.base, head: fixture.commit(), cwd: fixture.cwd }), true);
  });
});

test('lockfile routing compares against the merge base when main has advanced', async () => {
  const { selectRun } = await load();
  lockFixture(({ cwd, git, lock, write, commit, base }) => {
    // The branch changes shared metadata. Main later gains the same metadata;
    // comparing only main's latest file to HEAD would incorrectly skip checks.
    lock.packages[''].engines = { node: '>=22' }; write(lock); const head = commit();
    git('checkout', '-qb', 'advanced-main', base);
    write(lock); writeFileSync(join(cwd, 'README.md'), 'main advances'); const advancedBase = commit();
    assert.equal(selectRun({ event: 'pull_request', base: advancedBase, head, cwd }), true);
  });
});

test('CLI reports workspace-only lock updates as skipped without needing Cargo', async () => {
  const scopeScript = new URL('./sovmem-scope.mjs', import.meta.url).pathname;
  await load();
  lockFixture(({ cwd, lock, write, commit, base }) => {
    lock.packages['node_modules/stripe'].version = '2.0.0'; write(lock); const head = commit();
    const output = join(cwd, 'scope-output');
    const result = spawnSync(process.execPath, [scopeScript], { cwd, encoding: 'utf8', env: { ...process.env, GITHUB_EVENT_NAME: 'pull_request', BASE_SHA: base, HEAD_SHA: head, GITHUB_OUTPUT: output } });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(readFileSync(output, 'utf8'), 'full=false\n');
  });
});
