import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isSovmemInput } from '../../scripts/lib/sovmem-inputs.mjs';

const auditReports = new Set([
  'docs/SOVMEM-BUILD-AUDIT.md', 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md',
]);
export function requiresSovmem(files) {
  return files.some(file => isSovmemInput(file) || auditReports.has(file));
}
export function changedPaths(base, head, cwd = process.cwd()) {
  if (![base, head].every(sha => typeof sha === 'string' && /^[a-f\d]{40}$/i.test(sha))) {
    throw new Error('valid base and head commit SHAs are required');
  }
  // Treat renames as delete+add, so moving a protected input cannot evade the gate.
  return execFileSync('git', ['diff', '--name-only', '--no-renames', '-z', `${base}...${head}`, '--'], { cwd, encoding: 'utf8' })
    .split('\0').filter(Boolean);
}
function lockfileAffectsSovmem(base, head, cwd) {
  // Match the merge-base comparison used by changedPaths. Workspace-only npm
  // resolutions do not affect the root's dependency-free SovMem tooling.
  // Unknown formats, root dependency graphs and root metadata changes still
  // select full checks; do not guess at shared transitive dependency ownership.
  try {
    const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 16 * 1024 * 1024 });
    const mergeBase = git('merge-base', base, head).trim();
    const read = ref => {
      const lock = JSON.parse(git('show', `${ref}:package-lock.json`));
      if (lock?.lockfileVersion !== 3 || !lock.packages || typeof lock.packages !== 'object' || Array.isArray(lock.packages)) throw new Error('unsupported lockfile');
      const root = lock.packages[''];
      if (!root || typeof root !== 'object' || Array.isArray(root)) throw new Error('missing lockfile root');
      const { packages, ...metadata } = lock;
      return { root, metadata };
    };
    const before = read(mergeBase), after = read(head);
    if ([before, after].some(({ root }) => root.hasInstallScript || ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies', 'bundledDependencies', 'bundleDependencies']
      .some(field => root[field] != null && (typeof root[field] !== 'object' || Object.keys(root[field]).length > 0)))) return true;
    return JSON.stringify(before) !== JSON.stringify(after);
  } catch {
    return true;
  }
}
function onlyAuditFingerprintChanged(base, head, path, cwd) {
  try {
    const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
    const mergeBase = git('merge-base', base, head).trim();
    const withoutFingerprint = ref => {
      const report = git('show', `${ref}:${path}`);
      const fingerprint = /^- Source-tree fingerprint: `[a-f0-9]{64}`$/gm;
      if (report.match(fingerprint)?.length !== 1) throw new Error('unsupported audit report');
      return report.replace(fingerprint, '- Source-tree fingerprint: <derived>');
    };
    return withoutFingerprint(mergeBase) === withoutFingerprint(head);
  } catch {
    return false;
  }
}
export function selectRun({ event, base, head, cwd }) {
  if (event === 'workflow_dispatch' || event === 'push') return true;
  if (!['pull_request', 'merge_group'].includes(event)) throw new Error('unsupported event for SovMem scope');
  const files = changedPaths(base, head, cwd);
  if (!files.includes('package-lock.json') || lockfileAffectsSovmem(base, head, cwd)) return requiresSovmem(files);
  // A workspace-only lock update also changes the generated source fingerprint.
  // Freshness remains an unconditional cheap check in CI and the local audit.
  return requiresSovmem(files.filter(file => file !== 'package-lock.json'
    && !(auditReports.has(file)
      && onlyAuditFingerprintChanged(base, head, file, cwd))));
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const full = selectRun({ event: process.env.GITHUB_EVENT_NAME, base: process.env.BASE_SHA, head: process.env.HEAD_SHA });
    if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `full=${full}\n`);
    console.log(full ? 'Full SovMem checks selected.' : 'No SovMem inputs changed; full checks skipped.');
  } catch (error) {
    console.error(`SovMem scope failed: ${error.message}`);
    process.exitCode = 1;
  }
}
