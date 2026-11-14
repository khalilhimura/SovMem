#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import {
  closeSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { basename, dirname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const OWNER_MARKER = '.sovmem-p3-safe-clone-owner';
const REQUIRED_DIRECTORIES = [
  'policies',
  'staging/inbox',
  'intake/proposals',
  'intake/transactions',
  'ledger',
  'canon/generations',
  'evidence/reindex',
  'derived/indexes',
  'transactions',
  'locks',
];

export class HarnessError extends Error {
  constructor(code = 'refused') {
    super(`safe-clone harness: ${code}`);
    this.name = 'HarnessError';
  }
}

function fail(code) {
  throw new HarnessError(code);
}

function fsyncPath(path) {
  const descriptor = openSync(path, 'r');
  try {
    fsyncSync(descriptor);
  } finally {
    closeSync(descriptor);
  }
}

function fsyncParents(root) {
  const directories = [root];
  const visit = directory => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        const child = resolve(directory, entry.name);
        visit(child);
        directories.push(child);
      }
    }
  };
  visit(root);
  for (const directory of directories) fsyncPath(directory);
}

function regularFile(path) {
  let metadata;
  try {
    metadata = lstatSync(path);
  } catch {
    fail('input unavailable');
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) fail('input unavailable');
  return path;
}

function existingDirectory(path) {
  let metadata;
  try {
    metadata = lstatSync(path);
  } catch {
    fail('input unavailable');
  }
  if (metadata.isSymbolicLink() || !metadata.isDirectory()) fail('input unavailable');
  return realpathSync(path);
}

function storeIdentity(path) {
  const absolute = resolve(path);
  try {
    const metadata = lstatSync(absolute);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) fail('store unavailable');
    return { path: realpathSync(absolute), exists: true, parent: realpathSync(dirname(absolute)) };
  } catch (error) {
    if (error instanceof HarnessError) throw error;
    const parent = dirname(absolute);
    return { path: resolve(existingDirectory(parent), basename(absolute)), exists: false, parent: existingDirectory(parent) };
  }
}

function overlaps(left, right) {
  return left === right || left.startsWith(`${right}${sep}`) || right.startsWith(`${left}${sep}`);
}

function assertDisjoint(paths) {
  for (let index = 0; index < paths.length; index += 1) {
    for (let other = index + 1; other < paths.length; other += 1) {
      if (overlaps(paths[index], paths[other])) fail('root overlap');
    }
  }
}

export function resolveProvisionRoots(options) {
  const store = storeIdentity(options.store);
  const seed = existingDirectory(options.seed);
  const protectedRoots = (options.protectedRoots ?? options.protectedRoot ? [
    ...(options.protectedRoots ?? []),
    ...(options.protectedRoot ? [options.protectedRoot] : []),
  ] : []).map(existingDirectory);
  if (protectedRoots.length === 0) fail('protected root required');
  const frozenInputs = [options.p2Profile, options.p3Profile, options.policy].map(path => {
    regularFile(path);
    return realpathSync(path);
  });
  const frozenRoots = frozenInputs.map(path => realpathSync(dirname(path)));
  assertDisjoint([store.path, seed, ...protectedRoots, ...frozenInputs]);
  if (frozenRoots.some(path => overlaps(store.path, path))) fail('root overlap');
  return Object.freeze({ store, seed, protectedRoots: Object.freeze(protectedRoots), frozenInputs: Object.freeze(frozenInputs), frozenRoots: Object.freeze(frozenRoots) });
}

function assertSafeTree(root) {
  const visit = directory => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = resolve(directory, entry.name);
      const metadata = lstatSync(path);
      if (metadata.isSymbolicLink() || (!metadata.isDirectory() && !metadata.isFile())) fail('seed shape invalid');
      if (metadata.isDirectory()) visit(path);
    }
  };
  visit(root);
}

function requireHashBinary(path) {
  regularFile(path);
  return realpathSync(path);
}

function rawHash(hashBinary, path) {
  const output = execFileSync(hashBinary, [path], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  if (!/^[0-9a-f]{64}\n$/.test(output)) fail('hash unavailable');
  return output.slice(0, -1);
}

function relativeHashes(root, hashBinary) {
  const hashes = new Map();
  const visit = (directory, prefix = '') => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = resolve(directory, entry.name);
      const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
      const metadata = lstatSync(path);
      if (metadata.isSymbolicLink() || (!metadata.isDirectory() && !metadata.isFile())) fail('seed shape invalid');
      if (metadata.isDirectory()) visit(path, relativePath);
      else hashes.set(relativePath, rawHash(hashBinary, path));
    }
  };
  visit(root);
  return hashes;
}

function equalMaps(left, right) {
  return left.size === right.size && [...left].every(([key, value]) => right.get(key) === value);
}

function copyTree(source, destination) {
  mkdirSync(destination, { recursive: false, mode: 0o700 });
  for (const entry of readdirSync(source, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const from = resolve(source, entry.name);
    const to = resolve(destination, entry.name);
    const metadata = lstatSync(from);
    if (metadata.isSymbolicLink() || (!metadata.isDirectory() && !metadata.isFile())) fail('seed shape invalid');
    if (metadata.isDirectory()) copyTree(from, to);
    else {
      writeFileSync(to, readFileSync(from), { mode: metadata.mode & 0o777 });
      fsyncPath(to);
    }
  }
  fsyncPath(destination);
}

export function copyVerifiedSeed({ source, destination, hashBinary }) {
  const verifiedSource = existingDirectory(source);
  const verifiedHashBinary = requireHashBinary(hashBinary);
  assertSafeTree(verifiedSource);
  const sourceHashes = relativeHashes(verifiedSource, verifiedHashBinary);
  copyTree(verifiedSource, destination);
  const copiedHashes = relativeHashes(destination, verifiedHashBinary);
  if (!equalMaps(sourceHashes, copiedHashes)) fail('seed copy mismatch');
  fsyncParents(destination);
  return sourceHashes;
}

function verifyLevel3(options, seed) {
  const output = execFileSync(options.binary, [
    'verify', '--profile', options.p2Profile, '--bundle', seed, '--level', '3', '--require-corpus',
  ], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  let result;
  try { result = JSON.parse(output); } catch { fail('level 3 verification failed'); }
  const expected = ['status', 'command', 'level', 'documents', 'warnings', 'errors', 'bundle_hash'];
  if (Object.keys(result).length !== expected.length || expected.some(key => !(key in result))
      || result.status !== 'ok' || result.command !== 'verify' || result.level !== 3
      || result.documents !== 62 || result.errors !== 0 || !/^[0-9a-f]{64}$/.test(result.bundle_hash)) {
    fail('level 3 verification failed');
  }
  return result;
}

function checkStoreTarget(store) {
  if (!store.exists) return;
  const entries = readdirSync(store.path);
  if (entries.length !== 0) fail('store must be empty');
}

function sameDevice(left, right, filesystem) {
  return statSync(left).dev === statSync(right).dev;
}

function assertAtomicParent(parent, store) {
  const token = `.${basename(store)}.safe-clone-probe-${process.pid}-${Date.now()}`;
  const probe = resolve(parent, token);
  const published = resolve(parent, `${token}.published`);
  try {
    writeFileSync(probe, Buffer.alloc(0), { mode: 0o600, flag: 'wx' });
    fsyncPath(probe);
    if (!sameDevice(parent, dirname(probe))) fail('cross-filesystem publication');
    renameSync(probe, published);
    fsyncPath(parent);
  } finally {
    try { unlinkSync(probe); } catch {}
    try { unlinkSync(published); } catch { fail('atomic publication unavailable'); }
  }
}

function parseArguments(argv) {
  const values = {};
  const protectedRoots = [];
  const flags = new Set(['--binary', '--hash-binary', '--seed', '--p2-profile', '--p3-profile', '--policy', '--store', '--protected-root']);
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flags.has(flag) || !value || value.startsWith('--')) fail('invalid arguments');
    if (flag === '--protected-root') protectedRoots.push(value);
    else if (values[flag]) fail('invalid arguments');
    else values[flag] = value;
  }
  if (argv.length % 2 !== 0 || protectedRoots.length === 0
      || ['--binary', '--hash-binary', '--seed', '--p2-profile', '--p3-profile', '--policy', '--store'].some(flag => !values[flag])) {
    fail('invalid arguments');
  }
  return {
    binary: values['--binary'], hashBinary: values['--hash-binary'], seed: values['--seed'],
    p2Profile: values['--p2-profile'], p3Profile: values['--p3-profile'], policy: values['--policy'],
    store: values['--store'], protectedRoots,
  };
}

export function provisionSafeClone(options) {
  const roots = resolveProvisionRoots(options);
  regularFile(options.binary ?? process.execPath);
  const hashBinary = requireHashBinary(options.hashBinary);
  checkStoreTarget(roots.store);
  assertAtomicParent(roots.store.parent, roots.store.path);
  verifyLevel3(options, roots.seed);
  let created = false;
  try {
    if (!roots.store.exists) {
      mkdirSync(roots.store.path, { mode: 0o700 });
      created = true;
    }
    writeFileSync(resolve(roots.store.path, OWNER_MARKER), 'owned\n', { mode: 0o600 });
    fsyncPath(roots.store.path);
    const copiedHashes = copyVerifiedSeed({ source: roots.seed, destination: resolve(roots.store.path, 'seed'), hashBinary });
    verifyLevel3(options, resolve(roots.store.path, 'seed'));
    writeFileSync(resolve(roots.store.path, 'policy.toml'), readFileSync(options.policy), { mode: 0o600 });
    fsyncPath(resolve(roots.store.path, 'policy.toml'));
    for (const directory of REQUIRED_DIRECTORIES) mkdirSync(resolve(roots.store.path, directory), { recursive: true, mode: 0o700 });
    writeFileSync(resolve(roots.store.path, 'intake/events.jsonl'), Buffer.alloc(0), { mode: 0o600 });
    fsyncPath(resolve(roots.store.path, 'intake/events.jsonl'));
    writeFileSync(resolve(roots.store.path, 'ledger/verdicts.jsonl'), Buffer.alloc(0), { mode: 0o600 });
    fsyncPath(resolve(roots.store.path, 'ledger/verdicts.jsonl'));
    writeFileSync(resolve(roots.store.path, 'canon/CURRENT'), 'seed\n', { mode: 0o600 });
    fsyncPath(resolve(roots.store.path, 'canon/CURRENT'));
    const policyHash = rawHash(hashBinary, resolve(roots.store.path, 'policy.toml'));
    writeFileSync(resolve(roots.store.path, `policies/${policyHash}.toml`), readFileSync(options.policy), { mode: 0o600 });
    fsyncPath(resolve(roots.store.path, `policies/${policyHash}.toml`));
    unlinkSync(resolve(roots.store.path, OWNER_MARKER));
    fsyncParents(roots.store.path);
    return Object.freeze({ store: roots.store.path, seedHashes: copiedHashes, p3ProfileRawHash: rawHash(hashBinary, options.p3Profile) });
  } catch (error) {
    if (created) {
      try {
        if (realpathSync(roots.store.path) === roots.store.path
            && readFileSync(resolve(roots.store.path, OWNER_MARKER), 'utf8') === 'owned\n') {
          rmSync(roots.store.path, { recursive: true, force: false });
        }
      } catch { /* retain anything whose ownership cannot be proven */ }
    }
    if (error instanceof HarnessError) throw error;
    fail('provision failed');
  }
}

function main() {
  try {
    provisionSafeClone(parseArguments(process.argv.slice(2)));
    process.stdout.write('safe-clone harness: provisioned\n');
  } catch {
    process.stderr.write('safe-clone harness: invalid arguments\n');
    process.exitCode = 2;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
