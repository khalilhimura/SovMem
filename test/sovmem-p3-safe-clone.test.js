import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  chmodSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import test from 'node:test';
import {
  HarnessError,
  copyVerifiedSeed,
  provisionSafeClone,
  resolveProvisionRoots,
} from '../scripts/provision-sovmem-p3-safe-clone.mjs';
import {
  ProofError,
  publishProof,
  validateEvidence,
  verifySafeClone,
} from '../scripts/verify-sovmem-p3-safe-clone.mjs';

const ROOT = mkdtempSync(join(tmpdir(), 'sovmem-p3-safe-clone-test-'));
const CARGO_TARGET_DIR = process.env.CARGO_TARGET_DIR
  ?? join(process.cwd(), 'sovmem', 'target');
const HASH_BINARY = join(CARGO_TARGET_DIR, 'debug', 'examples', 'sovmem-p3-raw-hash');
const HASH = 'a'.repeat(64);
const PROPOSAL_ID = `prp-${'b'.repeat(64)}`;
const VERDICT_ID = `ver-${'c'.repeat(64)}`;
const SOURCE_COMMIT = '9'.repeat(40);
const INTAKE_HEAD = '3'.repeat(64);
const RETRIEVAL_MARKER_BLAKE3 = '8'.repeat(64);

execFileSync('cargo', [
  'build', '--locked', '--manifest-path', 'sovmem/Cargo.toml', '--example', 'sovmem-p3-raw-hash',
], { stdio: 'pipe' });

function tempRoot(name) {
  return mkdtempSync(join(ROOT, `${name}-`));
}

function write(path, bytes = '') {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, bytes);
}

function fakeSovmem(root) {
  const path = join(root, 'fake-sovmem.mjs');
  write(path, `#!/usr/bin/env node
const a = process.argv.slice(2); const h = '${HASH}';
if (process.env.CALLS) require('node:fs').appendFileSync(process.env.CALLS, a[0] + '\\n');
if (process.env.MUTATE_PATH && a[0] === 'eval') require('node:fs').appendFileSync(process.env.MUTATE_PATH, 'x');
const out = value => process.stdout.write(JSON.stringify(value) + '\\n');
if (a[0] === 'verify' && a.includes('--bundle')) out({status:'ok',command:'verify',level:3,documents:62,warnings:0,errors:0,bundle_hash:h});
else if (a[0] === 'verify') out({status:'ok',command:'verify',level:4,events:2,verdicts:1,accept:1,reject:0,rewrite:0,tp:0,fp:0,applied_generations:1,current_generation:'gen-0000000000000001',bundle_hash:h,ledger_head:'${'d'.repeat(64)}',errors:0});
else if (a[0] === 'verdicts') out({status:'ok',command:'verdicts.list',verdicts:[{proposal_id:'${PROPOSAL_ID}',verdict_id:'${VERDICT_ID}',target_new_hash:'${'e'.repeat(64)}',judgment_hash:'${'f'.repeat(64)}',generation_id:'gen-0000000000000001',bundle_hash:h,reindex_receipt_hash:'${'1'.repeat(64)}'}]});
else if (a[0] === 'index') out({status:'ok',command:'index',schema_version:'sovmem-index-manifest-v2',documents:64,graph_nodes:64,graph_edges:1,external_typed_links:0,bundle_hash:h,index_config:h,replaced_invalid_current:false,store_id:'audy-sovmem-writeback-v1',generation:'gen-0000000000000001',manifest_hash:h,receipt_hash:'${'1'.repeat(64)}'});
else if (a[0] === 'query' && a.includes('--bundle')) out({status:'ok',command:'query',top_k:5,expansion:'none',bundle_hash:h,results:[],expanded:[]});
else if (a[0] === 'query') { const q=a[a.indexOf('--query')+1]; out({status:'ok',command:'query',top_k:5,expansion:'none',bundle_hash:h,store_id:'audy-sovmem-writeback-v1',generation:'gen-0000000000000001',manifest_hash:h,results:q==='target-token'?[{id:'ssa-0/target.md'}]:q==='judgment-token'?[{id:'ssa-0/judgments/${VERDICT_ID}.md'}]:[],expanded:[]}); }
else if (a[0] === 'eval') out({status:'go',command:'eval',evaluation_set:'audy-sovmem-v0',queries:25,k:5,queries_with_hit:23,required_queries_with_hit:20,precision_numerator:23,precision_denominator:125,recall_numerator:23,recall_denominator:25,baseline_written:true});
else process.exit(2);`);
  chmodSync(path, 0o755);
  return path;
}

function tree(root) {
  if (!existsSync(root)) return [];
  const entries = [];
  const visit = (directory, relative = '') => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const next = join(directory, entry.name);
      const label = relative ? `${relative}/${entry.name}` : entry.name;
      entries.push(`${label}:${entry.isDirectory() ? 'd' : entry.isSymbolicLink() ? 'l' : readFileSync(next).toString('hex')}`);
      if (entry.isDirectory()) visit(next, label);
    }
  };
  visit(root);
  return entries;
}

function hashFile(path) {
  return execFileSync(HASH_BINARY, [path], { encoding: 'utf8' }).trim();
}

function rawHashes(root) {
  const output = new Map();
  const visit = (directory, relative = '') => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const next = join(directory, entry.name);
      const label = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.isDirectory()) visit(next, label);
      else if (entry.isFile()) output.set(label, hashFile(next));
    }
  };
  visit(root);
  return output;
}

function makeProvisionInputs() {
  const root = tempRoot('provision');
  const seed = join(root, 'seed');
  const protectedRoot = join(root, 'vault');
  const frozen = join(root, 'frozen');
  const store = join(root, 'store');
  const policy = join(frozen, 'policy.toml');
  const p2Profile = join(frozen, 'p2.yaml');
  const p3Profile = join(frozen, 'p3.yaml');
  write(join(seed, 'bundle', 'zero.bin'), Buffer.alloc(0));
  write(join(seed, 'bundle', 'bytes.bin'), Buffer.from([0, 255, 10, 1]));
  write(join(protectedRoot, 'private.md'), 'read only');
  write(policy, 'policy = "frozen"\n');
  write(p2Profile, 'p2');
  write(p3Profile, 'p3');
  return { root, seed, protectedRoot, frozen, store, policy, p2Profile, p3Profile };
}

function validEvidence() {
  return {
    schema_version: 'sovmem.p3.safe-clone-proof.v2',
    status: 'passed',
    source_commit: SOURCE_COMMIT,
    store_id: 'audy-sovmem-writeback-v2',
    profile_version: 'audy-ssa0-p3-v2',
    seed_bundle_hash: HASH,
    proposal_id: PROPOSAL_ID,
    verdict_id: VERDICT_ID,
    intake_sequence: 1,
    intake_head_hash: INTAKE_HEAD,
    ledger_head: 'd'.repeat(64),
    generation: 'gen-0000000000000001',
    target_hash: 'e'.repeat(64),
    judgment_hash: 'f'.repeat(64),
    reindex_receipt_hash: '1'.repeat(64),
    retrieval_marker_blake3: RETRIEVAL_MARKER_BLAKE3,
    level4: { events: 2, verdicts: 1, applied_generations: 1, errors: 0 },
    query_assertions: {
      seed_hit: false,
      target_hit: true,
      judgment_hit: true,
      rebuilt_target_hit: true,
      rebuilt_judgment_hit: true,
    },
    vault_unchanged: true,
    frozen_artifacts_unchanged: true,
    frozen_q3: {
      queries: 25,
      hits: 23,
      k: 5,
      precision: '23/125',
      recall: '23/25',
      misses: ['q-005', 'q-011'],
      status: 'GO',
    },
  };
}

test('safe roots reject equality and every ancestor or descendant overlap', () => {
  const input = makeProvisionInputs();
  const cases = [
    ['store equals seed', input.seed],
    ['store below seed', join(input.seed, 'child')],
    ['store contains seed', input.root],
    ['store equals protected', input.protectedRoot],
    ['store below protected', join(input.protectedRoot, 'child')],
    ['store contains protected', input.root],
    ['store equals frozen input', input.frozen],
    ['store below frozen input', join(input.frozen, 'child')],
  ];
  for (const [name, store] of cases) {
    assert.throws(() => resolveProvisionRoots({ ...input, store }), HarnessError, name);
  }
});

test('safe roots resolve symlinks before rejecting protected overlap', () => {
  const input = makeProvisionInputs();
  const linkedParent = join(input.root, 'linked-parent');
  symlinkSync(input.protectedRoot, linkedParent);
  assert.throws(
    () => resolveProvisionRoots({ ...input, store: join(linkedParent, 'new-store') }),
    HarnessError,
  );
});

test('safe roots reject seed and protected roots that contain frozen inputs', () => {
  const seedCollision = makeProvisionInputs();
  write(join(seedCollision.seed, 'p2.yaml'), 'frozen');
  assert.throws(() => resolveProvisionRoots({ ...seedCollision, p2Profile: join(seedCollision.seed, 'p2.yaml') }), HarnessError);
  const protectedCollision = makeProvisionInputs();
  write(join(protectedCollision.protectedRoot, 'policy.toml'), 'frozen');
  assert.throws(() => resolveProvisionRoots({ ...protectedCollision, policy: join(protectedCollision.protectedRoot, 'policy.toml') }), HarnessError);
});

test('provision refuses non-empty, sealed, unsupported, and cross-filesystem stores', () => {
  const input = makeProvisionInputs();
  mkdirSync(input.store);
  write(join(input.store, 'unexpected'), 'x');
  assert.throws(() => provisionSafeClone({ ...input, hashBinary: HASH_BINARY }), HarnessError);

  const sealed = makeProvisionInputs();
  mkdirSync(sealed.store);
  write(join(sealed.store, 'SEALED'), 'true');
  assert.throws(() => provisionSafeClone({ ...sealed, hashBinary: HASH_BINARY }), HarnessError);

  const unsupported = makeProvisionInputs();
  mkdirSync(unsupported.store);
  write(join(unsupported.store, 'CURRENT'), 'seed\n');
  assert.throws(() => provisionSafeClone({ ...unsupported, hashBinary: HASH_BINARY }), HarnessError);

  const atomicParent = makeProvisionInputs();
  provisionSafeClone({ ...atomicParent, binary: fakeSovmem(atomicParent.root), hashBinary: HASH_BINARY });
  assert.equal(existsSync(join(atomicParent.root, `.${basename(atomicParent.store)}.safe-clone-probe-${process.pid}`)), false);
});

test('provision copies verified seed raw bytes and leaves protected inputs untouched', () => {
  const input = makeProvisionInputs();
  const beforeSeed = tree(input.seed);
  const beforeVault = tree(input.protectedRoot);
  const beforeFrozen = tree(input.frozen);
  provisionSafeClone({ ...input, binary: fakeSovmem(input.root), hashBinary: HASH_BINARY });
  assert.deepEqual(
    readdirSync(input.store).sort(),
    [
      'canon',
      'derived',
      'evidence',
      'intake',
      'ledger',
      'locks',
      'policies',
      'policy.toml',
      'seed',
      'staging',
      'transactions',
    ],
  );
  assert.deepEqual(readdirSync(join(input.store, 'staging')).sort(), ['inbox']);
  assert.deepEqual(
    readdirSync(join(input.store, 'intake')).sort(),
    ['events.jsonl', 'proposals', 'transactions'],
  );
  assert.equal(readFileSync(join(input.store, 'intake', 'events.jsonl')).length, 0);
  assert.deepEqual(rawHashes(join(input.store, 'seed')), rawHashes(input.seed));
  assert.deepEqual(tree(input.seed), beforeSeed);
  assert.deepEqual(tree(input.protectedRoot), beforeVault);
  assert.deepEqual(tree(input.frozen), beforeFrozen);
  assert.equal(readFileSync(join(input.store, 'ledger', 'verdicts.jsonl')).length, 0);
  assert.equal(readFileSync(join(input.store, 'canon', 'CURRENT'), 'utf8'), 'seed\n');
  assert.equal(readFileSync(join(input.store, 'policy.toml'), 'utf8'), 'policy = "frozen"\n');
});

test('copy guard preserves relative raw hashes and rejects a symlinked seed file', () => {
  const input = makeProvisionInputs();
  const destination = join(input.root, 'copy');
  copyVerifiedSeed({ source: input.seed, destination, hashBinary: HASH_BINARY });
  assert.deepEqual(rawHashes(destination), rawHashes(input.seed));
  symlinkSync(join(input.seed, 'bundle', 'zero.bin'), join(input.seed, 'bundle', 'linked.bin'));
  assert.throws(
    () => copyVerifiedSeed({ source: input.seed, destination: join(input.root, 'copy-two'), hashBinary: HASH_BINARY }),
    HarnessError,
  );
});

test('raw hash helper is raw-byte exact and every error is redacted', () => {
  const zero = join(tempRoot('raw-hash'), 'zero');
  write(zero, Buffer.alloc(0));
  assert.equal(hashFile(zero), 'af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262');
  for (const args of [[], [zero, zero], [ROOT], [join(ROOT, 'missing')]]) {
    const output = spawnSync(HASH_BINARY, args, { encoding: 'utf8' });
    assert.equal(output.status, 2);
    assert.equal(output.stdout, '');
    assert.equal(output.stderr, 'safe-clone hash: invalid input\n');
  }
  const linked = join(tempRoot('raw-hash-link'), 'linked');
  symlinkSync(zero, linked);
  const output = spawnSync(HASH_BINARY, [linked], { encoding: 'utf8' });
  assert.equal(output.status, 2);
  assert.equal(output.stderr, 'safe-clone hash: invalid input\n');
});

test('proof validation refuses every mandatory disagreement and never publishes partial evidence', () => {
  const evidence = validEvidence();
  assert.deepEqual(validateEvidence(evidence), evidence);
  const fields = [
    ['source_commit', 'not-a-commit'],
    ['store_id', 'audy-sovmem-writeback-v1'],
    ['profile_version', 'audy-ssa0-p3-v1'],
    ['intake_sequence', 0],
    ['intake_head_hash', 'not-a-hash'],
    ['level4', { events: 1, verdicts: 1, applied_generations: 1, errors: 0 }],
    ['target_hash', 'not-a-hash'],
    ['judgment_hash', 'not-a-hash'],
    ['reindex_receipt_hash', 'not-a-hash'],
    ['retrieval_marker_blake3', 'not-a-hash'],
    ['query_assertions', { ...evidence.query_assertions, rebuilt_target_hit: false }],
    ['vault_unchanged', false],
    ['frozen_artifacts_unchanged', false],
    ['frozen_q3', { ...evidence.frozen_q3, hits: 22 }],
  ];
  for (const [field, value] of fields) {
    assert.throws(() => validateEvidence({ ...evidence, [field]: value }), ProofError, field);
  }
  const output = join(tempRoot('proof'), 'proof.json');
  assert.throws(
    () => publishProof({ output, evidence: { ...evidence, vault_unchanged: false } }),
    ProofError,
  );
  assert.equal(existsSync(output), false);
});

test('proof verifier cross-checks level 3, level 4, raw bytes, Judgment, receipt, queries, and frozen Q3', () => {
  const evidence = validEvidence();
  const checks = {
    level3: { level: 3, documents: 62, warnings: 226, errors: 0 },
    level4: { level: 4, ...evidence.level4, ledger_head: evidence.ledger_head },
    intake: {
      proposal_id: evidence.proposal_id,
      sequence: evidence.intake_sequence,
      head_hash: evidence.intake_head_hash,
    },
    applied: { target_hash: evidence.target_hash, judgment_hash: evidence.judgment_hash, reindex_receipt_hash: evidence.reindex_receipt_hash },
    verdict_intake: {
      proposal_id: evidence.proposal_id,
      verdict_id: evidence.verdict_id,
      sequence: evidence.intake_sequence,
      head_hash: evidence.intake_head_hash,
    },
    retrieval_marker_blake3: evidence.retrieval_marker_blake3,
    query_assertions: evidence.query_assertions,
    vault_unchanged: true,
    frozen_artifacts_unchanged: true,
    frozen_q3: evidence.frozen_q3,
  };
  assert.deepEqual(verifySafeClone({ evidence, checks }), evidence);
  for (const [section, key] of [
    ['level3', 'warnings'], ['level3', 'errors'], ['level4', 'ledger_head'], ['intake', 'head_hash'],
    ['verdict_intake', 'head_hash'], ['applied', 'target_hash'],
    ['applied', 'judgment_hash'], ['applied', 'reindex_receipt_hash'],
    [null, 'retrieval_marker_blake3'],
    ['query_assertions', 'rebuilt_judgment_hit'], ['frozen_q3', 'hits'],
  ]) {
    const changed = structuredClone(checks);
    const target = section === null ? changed : changed[section];
    target[key] = typeof target[key] === 'boolean'
      ? !target[key]
      : typeof target[key] === 'number' ? target[key] + 1 : '0'.repeat(64);
    assert.throws(() => verifySafeClone({ evidence, checks: changed }), ProofError, `${section}.${key}`);
  }
});

test('proof publication is redacted and atomically refuses cross-filesystem publication', () => {
  const output = join(tempRoot('redaction'), 'proof.json');
  publishProof({ output, evidence: validEvidence() });
  const serialized = readFileSync(output, 'utf8');
  for (const forbidden of [
    ROOT, tmpdir(), basename(ROOT), process.env.USER ?? '', process.env.HOSTNAME ?? '', String(process.pid),
    'candidate body', 'rewrite body', 'reason body', 'advice body', 'private query', 'ENOENT', 'Error:',
  ].filter(Boolean)) {
    assert.equal(serialized.includes(forbidden), false, `proof must redact ${forbidden}`);
  }
  const crossFilesystemOutput = join(tempRoot('cross-device-proof'), 'proof.json');
  assert.throws(() => publishProof({
    output: crossFilesystemOutput,
    evidence: validEvidence(),
    filesystem: { sameDevice: () => false },
  }), ProofError);
  assert.equal(existsSync(crossFilesystemOutput), false);
});

test('verification refuses protected output overlap before any writer command', () => {
  const input = makeProvisionInputs(); const binary = fakeSovmem(input.root); const generation = join(input.store, 'canon', 'generations', 'gen-0000000000000001');
  cpSync(input.seed, join(input.store, 'seed'), { recursive: true });
  write(join(generation, 'manifest.json'), JSON.stringify({ schema_version:'sovmem-canon-manifest-v1', store_id:'audy-sovmem-writeback-v1', profile_version:'audy-ssa0-p3-v1', profile_hash:hashFile(input.p3Profile), generation_id:'gen-0000000000000001', seed_bundle_hash:HASH, applied_proposal_id:PROPOSAL_ID, applied_verdict_id:VERDICT_ID, documents:[{id:'ssa-0/target.md',hash:'e'.repeat(64),proposal_id:PROPOSAL_ID,verdict_id:VERDICT_ID},{id:`ssa-0/judgments/${VERDICT_ID}.md`,hash:'f'.repeat(64),proposal_id:PROPOSAL_ID,verdict_id:VERDICT_ID}] }));
  write(join(input.root, 'target.txt'), 'target-token'); write(join(input.root, 'judgment.txt'), 'judgment-token'); write(join(input.root, 'golden.jsonl'), '{}'); write(join(input.root, 'thresholds.json'), '{}'); write(join(input.root, 'baseline.json'), '{}'); mkdirSync(join(input.root, 'frozen-index'));
  const calls = join(input.root, 'calls.txt'); const args = ['scripts/verify-sovmem-p3-safe-clone.mjs','--binary',binary,'--hash-binary',HASH_BINARY,'--store',input.store,'--seed',input.seed,'--p2-profile',input.p2Profile,'--p3-profile',input.p3Profile,'--golden',join(input.root,'golden.jsonl'),'--thresholds',join(input.root,'thresholds.json'),'--baseline',join(input.root,'baseline.json'),'--frozen-index',join(input.root,'frozen-index'),'--vault',input.protectedRoot,'--target-query-file',join(input.root,'target.txt'),'--judgment-query-file',join(input.root,'judgment.txt'),'--output',join(input.store,'proof.json')];
  const run = spawnSync(process.execPath, args, { encoding:'utf8', env:{ ...process.env, CALLS:calls } });
  assert.notEqual(run.status, 0); assert.equal(existsSync(calls), false);
});

test('both commands require explicit caller roots and hash binary without embedded live paths', () => {
  const provision = spawnSync(process.execPath, ['scripts/provision-sovmem-p3-safe-clone.mjs'], { encoding: 'utf8' });
  const verify = spawnSync(process.execPath, ['scripts/verify-sovmem-p3-safe-clone.mjs'], { encoding: 'utf8' });
  assert.notEqual(provision.status, 0);
  assert.notEqual(verify.status, 0);
  for (const output of [provision.stderr, verify.stderr]) {
    assert.match(output, /^safe-clone harness: invalid arguments\n$/);
    assert.equal(output.includes('/Users/'), false);
  }
});

// This fixture deliberately separates the immutable Level-3 seed bundle from the
// applied P3 generation bundle. The old verifier incorrectly treated them as the
// same commitment, which this black-box contract must reject.
const SEED_BUNDLE = 'a'.repeat(64);
const P3_BUNDLE = 'b'.repeat(64);
const MANIFEST_HASH = 'c'.repeat(64);
const RECEIPT_HASH = '1'.repeat(64);

function strictFakeSovmem(root) {
  const path = join(root, 'strict-fake-sovmem.mjs');
  write(path, `#!/usr/bin/env node
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
const a = process.argv.slice(2); const mode = process.env.MODE || ''; const seed = '${SEED_BUNDLE}'; const bundle = '${P3_BUNDLE}'; const manifest = '${MANIFEST_HASH}';
if (process.env.CALLS) appendFileSync(process.env.CALLS, a[0] + '\\n');
const out = value => process.stdout.write(JSON.stringify(value) + '\\n');
const hit = id => ({rank:1,id,title:'redacted',type:'memory',layer:'canon',provenance:'verified',score:'1.0'});
const query = (id, frozen=false) => ({status:'ok',command:'query',top_k:5,expansion:'none',bundle_hash:frozen ? seed : (mode === 'query-metadata' ? seed : bundle),...(frozen ? {} : {store_id:'audy-sovmem-writeback-v2',generation:'gen-0000000000000001',manifest_hash:manifest}),results:(frozen && mode === 'seed-hit') ? [hit('ssa-0/target.md')] : (!frozen && id ? [hit(id)] : []),expanded:[]});
if (a[0] === 'verify' && a.includes('--bundle')) out({status:'ok',command:'verify',level:3,documents:62,warnings:mode === 'level3-warnings' ? 225 : 226,errors:0,bundle_hash:seed});
else if (a[0] === 'verify') out({status:'ok',command:'verify',level:4,events:2,verdicts:1,accept:1,reject:0,rewrite:0,tp:0,fp:0,applied_generations:1,current_generation:mode === 'level4' ? 'gen-0000000000000002' : 'gen-0000000000000001',bundle_hash:mode === 'level4' ? seed : bundle,ledger_head:'${'d'.repeat(64)}',errors:0});
else if (a[0] === 'verdicts') out({status:'ok',command:'verdicts.list',verdicts:[{event_id:'evt-0000000000000001',event_hash:'${'2'.repeat(64)}',occurred_at:'2026-07-19T00:00:00.000000000Z',proposal_id:'${PROPOSAL_ID}',verdict_id:'${VERDICT_ID}',source_id:'src:human:test',verb:'ACCEPT',actor:'src:human:test',confidence:null,evaluator:mode === 'evaluator-schema' ? {source_id:'src:test',recommendation:'accept',reason_count:0,extra:true} : null,applied:true,candidate_hash:'${'3'.repeat(64)}',rewrite_hash:null,target_prior_hash:null,target_new_hash:mode === 'verdict' ? '${'0'.repeat(64)}' : '${'e'.repeat(64)}',judgment_hash:'${'f'.repeat(64)}',generation_id:'gen-0000000000000001',manifest_hash:manifest,bundle_hash:bundle,reindex_receipt_hash:'${RECEIPT_HASH}',reason_hash:'${'4'.repeat(64)}',reason_bytes:0}]});
else if (a[0] === 'index') { const count = process.env.CALLS ? readFileSync(process.env.CALLS,'utf8').split('\\n').filter(x=>x==='index').length : 1; const bad = mode === 'first-index' || (mode === 'rebuilt-index' && count > 1); out({status:'ok',command:'index',schema_version:'sovmem-derived-index-v2',documents:64,graph_nodes:64,graph_edges:1,external_typed_links:0,bundle_hash:bad ? seed : bundle,index_config:'${'5'.repeat(64)}',replaced_invalid_current:false,store_id:'audy-sovmem-writeback-v2',generation:'gen-0000000000000001',manifest_hash:manifest,receipt_hash:'${RECEIPT_HASH}'}); }
else if (a[0] === 'query' && a.includes('--bundle')) out(query(null,true));
else if (a[0] === 'query') { const text=a[a.indexOf('--query')+1]; out(query(text === 'target-token' ? 'ssa-0/target.md' : text === 'judgment-token' ? 'ssa-0/judgments/${VERDICT_ID}.md' : null)); }
else if (a[0] === 'eval') { if (mode === 'vault-git-metadata-change') writeFileSync(process.env.VAULT_GIT_METADATA, 'after'); if (mode === 'vault-working-tree-change') writeFileSync(process.env.VAULT_WORKING_TREE, 'after'); writeFileSync(a[a.indexOf('--baseline') + 1], mode === 'baseline-mismatch' ? 'different-baseline' : 'frozen-baseline'); out({status:'go',command:'eval',evaluation_set:'audy-sovmem-v0',queries:25,k:5,queries_with_hit:23,required_queries_with_hit:20,precision_numerator:23,precision_denominator:125,recall_numerator:23,recall_denominator:25,baseline_written:true}); }
else process.exit(2);`);
  chmodSync(path, 0o755);
  return path;
}

function strictVerificationFixture(mode = '') {
  const input = makeProvisionInputs();
  const binary = strictFakeSovmem(input.root);
  const generation = join(input.store, 'canon', 'generations', 'gen-0000000000000001');
  const manifestGeneration = mode === 'malicious-generation' ? '../../canon' : 'gen-0000000000000001';
  write(join(generation, 'manifest.json'), JSON.stringify({
    schema_version:'sovmem-canon-manifest-v1', store_id:'audy-sovmem-writeback-v2', profile_version:'audy-ssa0-p3-v2', profile_hash:hashFile(input.p3Profile), generation_id:manifestGeneration, parent_kind:'seed', parent_generation_id:null, parent_manifest_hash:'6'.repeat(64), seed_bundle_hash:SEED_BUNDLE, seed_manifest_hash:'6'.repeat(64), policy_hash:'7'.repeat(64), applied_proposal_id:PROPOSAL_ID, applied_verdict_id:VERDICT_ID, bundle_hash: mode === 'manifest' ? SEED_BUNDLE : P3_BUNDLE,
    documents:[
      {canonical_path:'target.md',seed_source_path:null,output_path:'bundle/target.md',id:'ssa-0/target.md',resource:'vault://ssa-0/target.md',hash:'e'.repeat(64),origin_kind:'verdict',seed_source_hash:null,proposal_id:PROPOSAL_ID,verdict_id:VERDICT_ID,previous_hash:null},
      {canonical_path:`judgments/${VERDICT_ID}.md`,seed_source_path:null,output_path:`bundle/judgments/${VERDICT_ID}.md`,id:`ssa-0/judgments/${VERDICT_ID}.md`,resource:`judgment://ssa-0/${VERDICT_ID}`,hash:'f'.repeat(64),origin_kind:'verdict',seed_source_hash:null,proposal_id:PROPOSAL_ID,verdict_id:VERDICT_ID,previous_hash:null},
    ],
  }));
  const protectedIntakeHead = mode === 'intake-head' ? '0'.repeat(64) : INTAKE_HEAD;
  if (mode !== 'intake-missing') {
    write(join(input.store, 'intake', 'events.jsonl'), `${JSON.stringify({
      schema_version:'sovmem-intake-event-v1', event_type:'intake', sequence:1,
      event_id:'int-0000000000000001', outcome:'STAGED', proposal_id:PROPOSAL_ID,
      event_hash:protectedIntakeHead,
    })}\n`);
  }
  write(join(input.store, 'ledger', 'verdicts.jsonl'), `${JSON.stringify({
    schema_version:'sovmem-ledger-event-v2', event_type:'verdict', sequence:1,
    event_id:'evt-0000000000000001', store_id:'audy-sovmem-writeback-v2',
    proposal_id:PROPOSAL_ID, verdict_id:VERDICT_ID, intake_sequence:1,
    intake_head_hash:mode === 'verdict-intake' ? '0'.repeat(64) : INTAKE_HEAD,
    event_hash:'2'.repeat(64),
  })}\n`);
  cpSync(input.seed, join(input.store, 'seed'), { recursive:true });
  write(join(input.store, 'derived', 'indexes', 'gen-0000000000000001', 'receipt.json'), '{}');
  write(join(input.root, 'target.txt'), 'target-token'); write(join(input.root, 'judgment.txt'), 'judgment-token');
  write(join(input.root, 'golden.jsonl'), '{}\\n'); write(join(input.root, 'thresholds.json'), '{}\\n');
  const frozenBaseline = join(input.frozen, 'baseline.json'); write(frozenBaseline, 'frozen-baseline');
  mkdirSync(join(input.root, 'frozen-index'));
  const outputParent = join(input.root, 'output'); mkdirSync(outputParent);
  const baselineOutputParent = join(input.root, 'baseline-output'); mkdirSync(baselineOutputParent);
  const output = join(outputParent, 'proof.json'); const baselineOutput = join(baselineOutputParent, 'baseline.json'); const calls = join(input.root, 'calls.txt');
  const args = ['scripts/verify-sovmem-p3-safe-clone.mjs','--binary',binary,'--hash-binary',HASH_BINARY,'--store',input.store,'--seed',input.seed,'--p2-profile',input.p2Profile,'--p3-profile',input.p3Profile,'--golden',join(input.root,'golden.jsonl'),'--thresholds',join(input.root,'thresholds.json'),'--frozen-baseline',frozenBaseline,'--baseline-output',baselineOutput,'--frozen-index',join(input.root,'frozen-index'),'--vault',input.protectedRoot,'--target-query-file',join(input.root,'target.txt'),'--judgment-query-file',join(input.root,'judgment.txt'),'--output',output];
  return { input, args, calls, output, frozenBaseline, baselineOutput, mode };
}

function runStrictFixture(fixture) {
  return spawnSync(process.execPath, fixture.args, { encoding:'utf8', env:{ ...process.env, ...fixture.environment, CALLS:fixture.calls, MODE:fixture.mode } });
}

function constantHashBinary(root) {
  const path = join(root, 'constant-hash.mjs');
  write(path, `#!/usr/bin/env node\nprocess.stdout.write('${HASH}\\n');\n`); chmodSync(path, 0o755); return path;
}

test('verification CLI binds a distinct P3 generation bundle rather than equating it to the P2 seed', () => {
  const fixture = strictVerificationFixture();
  const run = runStrictFixture(fixture);
  assert.equal(run.status, 0, run.stderr);
  const proof = JSON.parse(readFileSync(fixture.output, 'utf8'));
  assert.equal(proof.seed_bundle_hash, SEED_BUNDLE);
  assert.match(proof.source_commit, /^[0-9a-f]{40}$/);
  assert.equal(proof.intake_head_hash, INTAKE_HEAD);
  assert.match(proof.retrieval_marker_blake3, /^[0-9a-f]{64}$/);
  assert.deepEqual(readFileSync(fixture.baselineOutput), readFileSync(fixture.frozenBaseline));
});

test('verification fingerprints a relative in-vault symlink without following it', () => {
  const fixture = strictVerificationFixture();
  write(join(fixture.input.protectedRoot, 'linked-target.md'), 'vault target');
  symlinkSync('linked-target.md', join(fixture.input.protectedRoot, 'linked.md'));

  const run = runStrictFixture(fixture);

  assert.equal(run.status, 0, run.stderr);
  assert.equal(existsSync(fixture.output), true);
});

test('verification accepts one terminal newline from each private query file', () => {
  const fixture = strictVerificationFixture();
  write(join(fixture.input.root, 'target.txt'), 'target-token\n');
  write(join(fixture.input.root, 'judgment.txt'), 'judgment-token\r\n');

  const run = runStrictFixture(fixture);

  assert.equal(run.status, 0, run.stderr);
  assert.equal(existsSync(fixture.output), true);
});

test('verification requires a nonempty unique retrieval marker before any subprocess', () => {
  const fixture = strictVerificationFixture();
  write(join(fixture.input.root, 'target.txt'), '\n');

  const run = runStrictFixture(fixture);

  assert.notEqual(run.status, 0);
  assert.equal(existsSync(fixture.calls), false);
  assert.equal(existsSync(fixture.output), false);
});

test('verification excludes root Git administrative metadata from the Vault snapshot', () => {
  const fixture = strictVerificationFixture('vault-git-metadata-change');
  const metadata = join(fixture.input.protectedRoot, '.git', 'index');
  write(metadata, 'before');
  fixture.environment = { VAULT_GIT_METADATA: metadata };

  const run = runStrictFixture(fixture);

  assert.equal(run.status, 0, run.stderr);
  assert.equal(existsSync(fixture.output), true);
});

test('verification rejects a Vault working-tree change during the proof', () => {
  const fixture = strictVerificationFixture('vault-working-tree-change');
  const document = join(fixture.input.protectedRoot, 'private.md');
  fixture.environment = { VAULT_WORKING_TREE: document };

  const run = runStrictFixture(fixture);

  assert.notEqual(run.status, 0);
  assert.equal(existsSync(fixture.output), false);
});

test('verification refuses a relative vault symlink that resolves outside the protected root', () => {
  const fixture = strictVerificationFixture();
  symlinkSync('../outside.md', join(fixture.input.protectedRoot, 'linked.md'));

  const run = runStrictFixture(fixture);

  assert.notEqual(run.status, 0);
  assert.equal(existsSync(fixture.calls), false);
  assert.equal(existsSync(fixture.output), false);
});

test('verification refuses frozen-baseline and baseline-output prewrite collisions before any subprocess', () => {
  const cases = [
    ['frozen target', fixture => { fixture.args[fixture.args.indexOf('--baseline-output') + 1] = fixture.frozenBaseline; }],
    ['existing output', fixture => { write(fixture.baselineOutput, 'existing'); }],
    ['store output parent', fixture => { fixture.args[fixture.args.indexOf('--baseline-output') + 1] = join(fixture.input.store, 'baseline.json'); }],
    ['proof output parent', fixture => { fixture.args[fixture.args.indexOf('--baseline-output') + 1] = join(dirname(fixture.output), 'baseline.json'); }],
    ['vault alias parent', fixture => { const alias = join(fixture.input.root, 'baseline-alias'); symlinkSync(fixture.input.protectedRoot, alias); fixture.args[fixture.args.indexOf('--baseline-output') + 1] = join(alias, 'baseline.json'); }],
  ];
  for (const [name, configure] of cases) {
    const fixture = strictVerificationFixture(); configure(fixture); const before = readFileSync(fixture.frozenBaseline);
    const run = runStrictFixture(fixture);
    assert.notEqual(run.status, 0, name); assert.equal(existsSync(fixture.calls), false, name); assert.equal(existsSync(fixture.output), false, name); assert.deepEqual(readFileSync(fixture.frozenBaseline), before, name);
  }
});

test('verification refuses a generated baseline byte mismatch without changing frozen baseline', () => {
  const fixture = strictVerificationFixture('baseline-mismatch'); const before = readFileSync(fixture.frozenBaseline);
  const run = runStrictFixture(fixture);
  assert.notEqual(run.status, 0); assert.equal(existsSync(fixture.output), false); assert.deepEqual(readFileSync(fixture.frozenBaseline), before); assert.equal(readFileSync(fixture.baselineOutput, 'utf8'), 'different-baseline');
});

test('verification requires exact baseline bytes even if the supplied helper collides', () => {
  const fixture = strictVerificationFixture('baseline-mismatch');
  fixture.args[fixture.args.indexOf('--hash-binary') + 1] = constantHashBinary(fixture.input.root);
  const manifestPath = join(fixture.input.store, 'canon', 'generations', 'gen-0000000000000001', 'manifest.json'); const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.profile_hash = HASH; write(manifestPath, JSON.stringify(manifest));
  const run = runStrictFixture(fixture);
  assert.notEqual(run.status, 0); assert.equal(existsSync(fixture.output), false); assert.notDeepEqual(readFileSync(fixture.baselineOutput), readFileSync(fixture.frozenBaseline));
});

test('verification refuses preflight output overlaps and symlink aliases before any subprocess or proof', () => {
  for (const placement of ['store', 'seed', 'vault', 'frozen']) {
    const fixture = strictVerificationFixture();
    const root = placement === 'store' ? fixture.input.store : placement === 'seed' ? fixture.input.seed : placement === 'vault' ? fixture.input.protectedRoot : fixture.input.frozen;
    fixture.args[fixture.args.indexOf('--output') + 1] = join(root, 'proof.json');
    const run = runStrictFixture(fixture);
    assert.notEqual(run.status, 0, placement);
    assert.equal(existsSync(fixture.calls), false, placement);
    assert.equal(existsSync(join(root, 'proof.json')), false, placement);
  }
  const fixture = strictVerificationFixture(); const alias = join(fixture.input.root, 'output-alias'); symlinkSync(fixture.input.protectedRoot, alias);
  fixture.args[fixture.args.indexOf('--output') + 1] = join(alias, 'proof.json');
  const run = runStrictFixture(fixture);
  assert.notEqual(run.status, 0); assert.equal(existsSync(fixture.calls), false); assert.equal(existsSync(join(alias, 'proof.json')), false);
});

test('verification refuses contradictory snapshot bindings without publishing proof', () => {
  for (const mode of [
    'level3-warnings', 'seed-hit', 'level4', 'manifest', 'verdict',
    'evaluator-schema', 'first-index', 'rebuilt-index', 'query-metadata',
    'intake-missing', 'intake-head', 'verdict-intake',
  ]) {
    const fixture = strictVerificationFixture(mode); const run = runStrictFixture(fixture);
    assert.notEqual(run.status, 0, mode); assert.equal(existsSync(fixture.output), false, mode);
  }
});

test('verification validates a malicious manifest generation before deleting any canon path', () => {
  const fixture = strictVerificationFixture('malicious-generation'); const sentinel = join(fixture.input.store, 'canon', 'sentinel'); write(sentinel, 'keep');
  const run = runStrictFixture(fixture);
  assert.notEqual(run.status, 0); assert.equal(readFileSync(sentinel, 'utf8'), 'keep'); assert.equal(existsSync(fixture.output), false);
});
