#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { closeSync, fsyncSync, lstatSync, openSync, readFileSync, readlinkSync, readdirSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HEX = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;
const GENERATION = 'gen-0000000000000001';
const EVIDENCE_KEYS = ['schema_version', 'status', 'source_commit', 'store_id', 'profile_version', 'seed_bundle_hash', 'proposal_id', 'verdict_id', 'intake_sequence', 'intake_head_hash', 'ledger_head', 'generation', 'target_hash', 'judgment_hash', 'reindex_receipt_hash', 'retrieval_marker_blake3', 'level4', 'query_assertions', 'vault_unchanged', 'frozen_artifacts_unchanged', 'frozen_q3'];
const LEVEL4_KEYS = ['events', 'verdicts', 'applied_generations', 'errors'];
const QUERY_ASSERTION_KEYS = ['seed_hit', 'target_hit', 'judgment_hit', 'rebuilt_target_hit', 'rebuilt_judgment_hit'];
const Q3_KEYS = ['queries', 'hits', 'k', 'precision', 'recall', 'misses', 'status'];
const LEVEL3_KEYS = ['status', 'command', 'level', 'documents', 'warnings', 'errors', 'bundle_hash'];
const LEVEL4_OUTPUT_KEYS = ['status', 'command', 'level', 'events', 'verdicts', 'accept', 'reject', 'rewrite', 'tp', 'fp', 'applied_generations', 'current_generation', 'bundle_hash', 'ledger_head', 'errors'];
const MANIFEST_KEYS = ['schema_version', 'store_id', 'profile_version', 'profile_hash', 'generation_id', 'parent_kind', 'parent_generation_id', 'parent_manifest_hash', 'seed_bundle_hash', 'seed_manifest_hash', 'policy_hash', 'applied_proposal_id', 'applied_verdict_id', 'documents', 'bundle_hash'];
const MANIFEST_DOCUMENT_KEYS = ['canonical_path', 'seed_source_path', 'output_path', 'id', 'resource', 'hash', 'origin_kind', 'seed_source_hash', 'proposal_id', 'verdict_id', 'previous_hash'];
const VERDICT_LIST_KEYS = ['status', 'command', 'verdicts'];
const VERDICT_KEYS = ['event_id', 'event_hash', 'occurred_at', 'proposal_id', 'verdict_id', 'source_id', 'verb', 'actor', 'confidence', 'evaluator', 'applied', 'candidate_hash', 'rewrite_hash', 'target_prior_hash', 'target_new_hash', 'judgment_hash', 'generation_id', 'manifest_hash', 'bundle_hash', 'reindex_receipt_hash', 'reason_hash', 'reason_bytes'];
const EVALUATOR_KEYS = ['source_id', 'recommendation', 'reason_count'];
const INDEX_KEYS = ['status', 'command', 'schema_version', 'documents', 'graph_nodes', 'graph_edges', 'external_typed_links', 'bundle_hash', 'index_config', 'replaced_invalid_current', 'store_id', 'generation', 'manifest_hash', 'receipt_hash'];
const P2_QUERY_KEYS = ['status', 'command', 'top_k', 'expansion', 'bundle_hash', 'results', 'expanded'];
const P3_QUERY_KEYS = ['status', 'command', 'top_k', 'expansion', 'bundle_hash', 'store_id', 'generation', 'manifest_hash', 'results', 'expanded'];
const HIT_KEYS = ['rank', 'id', 'title', 'type', 'layer', 'provenance', 'score'];
const EVAL_KEYS = ['status', 'command', 'evaluation_set', 'queries', 'k', 'queries_with_hit', 'required_queries_with_hit', 'precision_numerator', 'precision_denominator', 'recall_numerator', 'recall_denominator', 'baseline_written'];

export class ProofError extends Error { constructor(code = 'refused') { super(`safe-clone proof: ${code}`); this.name = 'ProofError'; } }
function fail(code) { throw new ProofError(code); }
function object(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function exactKeys(value, keys, code) { if (!object(value) || Object.keys(value).length !== keys.length || keys.some(key => !Object.hasOwn(value, key))) fail(code); }
function text(value) { return typeof value === 'string'; }
function hash(value) { return text(value) && HEX.test(value); }
function identifier(value, prefix) { return text(value) && new RegExp(`^${prefix}-[0-9a-f]{64}$`).test(value); }
function nullableText(value) { return value === null || text(value); }
function nullableHash(value) { return value === null || hash(value); }
function regular(path) { const metadata = lstatSync(path); if (metadata.isSymbolicLink() || !metadata.isFile()) fail('input unavailable'); return realpathSync(path); }
function queryText(path) { const value = readFileSync(path, 'utf8'); const query = value.endsWith('\r\n') ? value.slice(0, -2) : value.endsWith('\n') ? value.slice(0, -1) : value; if (!query) fail('query marker invalid'); return query; }
function directory(path) { const metadata = lstatSync(path); if (metadata.isSymbolicLink() || !metadata.isDirectory()) fail('input unavailable'); return realpathSync(path); }
function overlaps(left, right) { return left === right || left.startsWith(`${right}/`) || right.startsWith(`${left}/`); }
function outputRoot(output) { const parent = directory(dirname(resolve(output))); const name = basename(output); if (!name || name === '.' || name === '..') fail('invalid arguments'); return { parent, path: resolve(parent, name) }; }
function absentOutput(output) { const destination = outputRoot(output); try { lstatSync(destination.path); fail('output exists'); } catch (error) { if (error instanceof ProofError) throw error; if (error?.code !== 'ENOENT') fail('output unavailable'); } return destination; }
function assertPreflight(writable, readonly) {
  for (let left = 0; left < writable.length; left += 1) for (let right = left + 1; right < writable.length; right += 1) if (overlaps(writable[left], writable[right])) fail('root overlap');
  for (const root of writable) if (readonly.some(protectedRoot => overlaps(root, protectedRoot))) fail('root overlap');
}
function rawHash(binary, path) { let value; try { value = execFileSync(binary, [path], { encoding:'utf8', stdio:['ignore', 'pipe', 'ignore'] }); } catch { fail('hash unavailable'); } if (!/^[0-9a-f]{64}\n$/.test(value)) fail('hash unavailable'); return value.slice(0, -1); }
function treeHashes(root, binary) {
  const entries = new Map();
  const visit = (directoryPath, prefix = '') => {
    for (const entry of readdirSync(directoryPath, { withFileTypes:true }).sort((left, right) => left.name.localeCompare(right.name))) {
      const path = resolve(directoryPath, entry.name); const key = prefix ? `${prefix}/${entry.name}` : entry.name; const metadata = lstatSync(path);
      if (metadata.isSymbolicLink()) fail('input unavailable');
      if (metadata.isDirectory()) visit(path, key); else if (metadata.isFile()) entries.set(key, rawHash(binary, path)); else fail('input unavailable');
    }
  };
  visit(root); return entries;
}
function metadataSnapshot(root) {
  const entries = new Map();
  const visit = (directoryPath, prefix = '') => {
    for (const entry of readdirSync(directoryPath, { withFileTypes:true }).sort((left, right) => left.name.localeCompare(right.name))) {
      // Git administrative state is captured separately by the ceremony's read-only
      // commit/status record; it is not part of the Vault working-tree content.
      if (!prefix && entry.name === '.git') continue;
      const path = resolve(directoryPath, entry.name); const key = prefix ? `${prefix}/${entry.name}` : entry.name; const metadata = lstatSync(path);
      if (metadata.isSymbolicLink()) {
        const target = readlinkSync(path, { encoding:'buffer' }); const targetText = target.toString('utf8'); const destination = resolve(dirname(path), targetText);
        if (!Buffer.from(targetText, 'utf8').equals(target) || targetText.includes('\0') || isAbsolute(targetText) || !(destination === root || destination.startsWith(`${root}/`))) fail('vault unsafe');
        entries.set(key, `symlink:${target.toString('hex')}:${metadata.mode}:${metadata.size}:${metadata.mtimeMs}:${metadata.ctimeMs}`);
      } else {
        entries.set(key, `${metadata.mode}:${metadata.size}:${metadata.mtimeMs}:${metadata.ctimeMs}`);
        if (metadata.isDirectory()) visit(path, key); else if (!metadata.isFile()) fail('vault unsafe');
      }
    }
  };
  visit(root); return entries;
}
function sameMaps(left, right) { return left.size === right.size && [...left].every(([key, value]) => right.get(key) === value); }
function jsonCommand(binary, args, parser, code) { let parsed; try { parsed = JSON.parse(execFileSync(binary, args, { encoding:'utf8', stdio:['ignore', 'pipe', 'ignore'] })); } catch { fail(code); } return parser(parsed, code); }
function jsonFile(path, parser, code) { let parsed; try { parsed = JSON.parse(readFileSync(regular(path), 'utf8')); } catch { fail(code); } return parser(parsed, code); }
function parseLevel3(value, code) { exactKeys(value, LEVEL3_KEYS, code); if (value.status !== 'ok' || value.command !== 'verify' || value.level !== 3 || !Number.isInteger(value.documents) || !Number.isInteger(value.warnings) || !Number.isInteger(value.errors) || !hash(value.bundle_hash)) fail(code); return value; }
function parseLevel4(value, code) { exactKeys(value, LEVEL4_OUTPUT_KEYS, code); if (value.status !== 'ok' || value.command !== 'verify' || value.level !== 4 || ![value.events,value.verdicts,value.accept,value.reject,value.rewrite,value.tp,value.fp,value.applied_generations,value.errors].every(Number.isInteger) || !text(value.current_generation) || !hash(value.bundle_hash) || !hash(value.ledger_head)) fail(code); return value; }
function parseDocument(value, code) { exactKeys(value, MANIFEST_DOCUMENT_KEYS, code); if (![value.canonical_path,value.output_path,value.id,value.resource].every(text) || !hash(value.hash) || !['seed','verdict'].includes(value.origin_kind) || !nullableText(value.seed_source_path) || !nullableHash(value.seed_source_hash) || !nullableText(value.proposal_id) || !nullableText(value.verdict_id) || !nullableHash(value.previous_hash)) fail(code); return value; }
function parseManifest(value, code) { exactKeys(value, MANIFEST_KEYS, code); if (value.schema_version !== 'sovmem-canon-manifest-v1' || value.store_id !== 'audy-sovmem-writeback-v2' || value.profile_version !== 'audy-ssa0-p3-v2' || !hash(value.profile_hash) || value.generation_id !== GENERATION || value.parent_kind !== 'seed' || value.parent_generation_id !== null || ![value.parent_manifest_hash,value.seed_bundle_hash,value.seed_manifest_hash,value.policy_hash,value.bundle_hash].every(hash) || !identifier(value.applied_proposal_id, 'prp') || !identifier(value.applied_verdict_id, 'ver') || !Array.isArray(value.documents)) fail(code); value.documents.forEach(document => parseDocument(document, code)); return value; }
function parseEvaluator(value, code) { if (value === null) return; exactKeys(value, EVALUATOR_KEYS, code); if (!text(value.source_id) || !text(value.recommendation) || !Number.isInteger(value.reason_count)) fail(code); }
function parseVerdict(value, code) { exactKeys(value, VERDICT_KEYS, code); if (![value.event_id,value.occurred_at,value.proposal_id,value.verdict_id,value.source_id,value.verb,value.actor].every(text) || ![value.event_hash,value.candidate_hash,value.reason_hash].every(hash) || !nullableHash(value.rewrite_hash) || !nullableHash(value.target_prior_hash) || !nullableHash(value.target_new_hash) || !nullableHash(value.judgment_hash) || !nullableText(value.generation_id) || !nullableHash(value.manifest_hash) || !nullableHash(value.bundle_hash) || !nullableHash(value.reindex_receipt_hash) || typeof value.applied !== 'boolean' || !Number.isInteger(value.reason_bytes) || !(value.confidence === null || (typeof value.confidence === 'number' && Number.isFinite(value.confidence) && value.confidence >= 0 && value.confidence <= 1))) fail(code); parseEvaluator(value.evaluator, code); return value; }
function parseVerdicts(value, code) { exactKeys(value, VERDICT_LIST_KEYS, code); if (value.status !== 'ok' || value.command !== 'verdicts.list' || !Array.isArray(value.verdicts)) fail(code); value.verdicts.forEach(verdict => parseVerdict(verdict, code)); return value; }
function parseIndex(value, code) { exactKeys(value, INDEX_KEYS, code); if (value.status !== 'ok' || value.command !== 'index' || value.schema_version !== 'sovmem-derived-index-v2' || ![value.documents,value.graph_nodes,value.graph_edges,value.external_typed_links].every(Number.isInteger) || ![value.bundle_hash,value.index_config,value.manifest_hash,value.receipt_hash].every(hash) || typeof value.replaced_invalid_current !== 'boolean' || value.store_id !== 'audy-sovmem-writeback-v2' || value.generation !== GENERATION) fail(code); return value; }
function parseHits(value, code) { if (!Array.isArray(value)) fail(code); for (const hit of value) { exactKeys(hit, HIT_KEYS, code); if (!Number.isInteger(hit.rank) || ![hit.id,hit.title,hit.type,hit.layer,hit.provenance,hit.score].every(text)) fail(code); } return value; }
function parseQuery(value, code, storeQuery) { exactKeys(value, storeQuery ? P3_QUERY_KEYS : P2_QUERY_KEYS, code); if (value.status !== 'ok' || value.command !== 'query' || value.top_k !== 5 || value.expansion !== 'none' || !hash(value.bundle_hash)) fail(code); if (storeQuery && (value.store_id !== 'audy-sovmem-writeback-v2' || value.generation !== GENERATION || !hash(value.manifest_hash))) fail(code); parseHits(value.results, code); if (!Array.isArray(value.expanded)) fail(code); return value; }
function parseEval(value, code) { exactKeys(value, EVAL_KEYS, code); if (value.status !== 'go' || value.command !== 'eval' || !text(value.evaluation_set) || ![value.queries,value.k,value.queries_with_hit,value.required_queries_with_hit,value.precision_numerator,value.precision_denominator,value.recall_numerator,value.recall_denominator].every(Number.isInteger) || typeof value.baseline_written !== 'boolean') fail(code); return value; }
function appliedDocuments(manifest) {
  const matched = manifest.documents.filter(document => document.proposal_id === manifest.applied_proposal_id && document.verdict_id === manifest.applied_verdict_id && document.origin_kind === 'verdict');
  const target = matched.filter(document => !document.canonical_path.startsWith('judgments/')); const judgment = matched.filter(document => document.canonical_path.startsWith('judgments/'));
  if (target.length !== 1 || judgment.length !== 1 || !hash(target[0].hash) || !hash(judgment[0].hash)) fail('manifest binding invalid');
  return { target:target[0], judgment:judgment[0] };
}
function appliedVerdict(list, manifest, bindings) {
  const matches = list.verdicts.filter(verdict => verdict.applied && verdict.proposal_id === manifest.applied_proposal_id && verdict.verdict_id === manifest.applied_verdict_id);
  if (matches.length !== 1) fail('verdict binding invalid'); const verdict = matches[0];
  if (!['ACCEPT','REWRITE'].includes(verdict.verb) || verdict.target_new_hash !== bindings.target.hash || verdict.judgment_hash !== bindings.judgment.hash || verdict.generation_id !== manifest.generation_id || verdict.manifest_hash !== bindings.manifestHash || verdict.bundle_hash !== manifest.bundle_hash || !hash(verdict.reindex_receipt_hash)) fail('verdict binding invalid');
  return verdict;
}
function jsonLines(path, code) {
  let source;
  try { source = readFileSync(regular(path), 'utf8'); } catch { fail(code); }
  if (!source.endsWith('\n') || source.includes('\r')) fail(code);
  try { return source.slice(0, -1).split('\n').filter(Boolean).map(line => JSON.parse(line)); } catch { fail(code); }
}
function protectedIntakeBinding(store, manifest) {
  const intake = jsonLines(resolve(store, 'intake', 'events.jsonl'), 'intake binding invalid');
  const staged = intake.filter(event => event?.schema_version === 'sovmem-intake-event-v1' && event?.event_type === 'intake' && event?.outcome === 'STAGED' && event?.proposal_id === manifest.applied_proposal_id && Number.isInteger(event?.sequence) && event.sequence > 0 && hash(event?.event_hash));
  if (staged.length !== 1) fail('intake binding invalid');
  const ledger = jsonLines(resolve(store, 'ledger', 'verdicts.jsonl'), 'verdict intake binding invalid');
  const verdicts = ledger.filter(event => event?.schema_version === 'sovmem-ledger-event-v2' && event?.event_type === 'verdict' && event?.store_id === 'audy-sovmem-writeback-v2' && event?.proposal_id === manifest.applied_proposal_id && event?.verdict_id === manifest.applied_verdict_id);
  if (verdicts.length !== 1) fail('verdict intake binding invalid');
  const verdict = verdicts[0];
  if (!Number.isInteger(verdict.intake_sequence) || verdict.intake_sequence < staged[0].sequence || !hash(verdict.intake_head_hash)) fail('verdict intake binding invalid');
  const heads = intake.filter(event => event?.sequence === verdict.intake_sequence && event?.event_hash === verdict.intake_head_hash);
  if (heads.length !== 1) fail('verdict intake binding invalid');
  return {
    intake: { proposal_id: manifest.applied_proposal_id, sequence: verdict.intake_sequence, head_hash: verdict.intake_head_hash },
    verdict: { proposal_id: verdict.proposal_id, verdict_id: verdict.verdict_id, sequence: verdict.intake_sequence, head_hash: verdict.intake_head_hash },
  };
}
function assertIndex(index, manifest, manifestHash, receipt) { if (index.bundle_hash !== manifest.bundle_hash || index.manifest_hash !== manifestHash || index.receipt_hash !== receipt) fail('index binding invalid'); }
function queryStore(binary, profile, store, text, id, bindings) { const result = jsonCommand(binary, ['query','--profile',profile,'--store',store,'--query',text,'--top-k','5'], value => parseQuery(value, 'query invalid', true), 'query invalid'); if (result.bundle_hash !== bindings.bundle || result.generation !== GENERATION || result.manifest_hash !== bindings.manifestHash || !result.results.some(hit => hit.id === id)) fail('query binding invalid'); return true; }
function frozenSeedMiss(binary, profile, seed, index, text, seedBundle) { const result = jsonCommand(binary, ['query','--profile',profile,'--bundle',seed,'--index',index,'--query',text,'--top-k','5'], value => parseQuery(value, 'frozen query invalid', false), 'frozen query invalid'); if (result.bundle_hash !== seedBundle || result.results.length !== 0) fail('seed query hit'); return false; }

export function validateEvidence(evidence) {
  exactKeys(evidence, EVIDENCE_KEYS, 'evidence schema invalid');
  if (evidence.schema_version !== 'sovmem.p3.safe-clone-proof.v2' || evidence.status !== 'passed' || !COMMIT.test(evidence.source_commit) || evidence.store_id !== 'audy-sovmem-writeback-v2' || evidence.profile_version !== 'audy-ssa0-p3-v2' || !hash(evidence.seed_bundle_hash) || !identifier(evidence.proposal_id,'prp') || !identifier(evidence.verdict_id,'ver') || !Number.isInteger(evidence.intake_sequence) || evidence.intake_sequence < 1 || !hash(evidence.intake_head_hash) || !hash(evidence.ledger_head) || evidence.generation !== GENERATION || !hash(evidence.target_hash) || !hash(evidence.judgment_hash) || !hash(evidence.reindex_receipt_hash) || !hash(evidence.retrieval_marker_blake3) || evidence.vault_unchanged !== true || evidence.frozen_artifacts_unchanged !== true) fail('evidence schema invalid');
  exactKeys(evidence.level4, LEVEL4_KEYS, 'evidence schema invalid'); exactKeys(evidence.query_assertions, QUERY_ASSERTION_KEYS, 'evidence schema invalid'); exactKeys(evidence.frozen_q3, Q3_KEYS, 'evidence schema invalid');
  if (JSON.stringify(evidence.level4) !== JSON.stringify({events:2,verdicts:1,applied_generations:1,errors:0}) || JSON.stringify(evidence.query_assertions) !== JSON.stringify({seed_hit:false,target_hit:true,judgment_hit:true,rebuilt_target_hit:true,rebuilt_judgment_hit:true}) || JSON.stringify(evidence.frozen_q3) !== JSON.stringify({queries:25,hits:23,k:5,precision:'23/125',recall:'23/25',misses:['q-005','q-011'],status:'GO'})) fail('evidence schema invalid');
  return evidence;
}
export function verifySafeClone({ evidence, checks }) { validateEvidence(evidence); exactKeys(checks, ['level3','level4','intake','applied','verdict_intake','retrieval_marker_blake3','query_assertions','vault_unchanged','frozen_artifacts_unchanged','frozen_q3'], 'proof checks invalid'); if (checks.level3?.level !== 3 || checks.level3?.documents !== 62 || checks.level3?.warnings !== 226 || checks.level3?.errors !== 0 || checks.level4?.level !== 4 || checks.level4?.ledger_head !== evidence.ledger_head || JSON.stringify({events:checks.level4?.events,verdicts:checks.level4?.verdicts,applied_generations:checks.level4?.applied_generations,errors:checks.level4?.errors}) !== JSON.stringify(evidence.level4) || JSON.stringify(checks.intake) !== JSON.stringify({proposal_id:evidence.proposal_id,sequence:evidence.intake_sequence,head_hash:evidence.intake_head_hash}) || JSON.stringify(checks.verdict_intake) !== JSON.stringify({proposal_id:evidence.proposal_id,verdict_id:evidence.verdict_id,sequence:evidence.intake_sequence,head_hash:evidence.intake_head_hash}) || JSON.stringify(checks.applied) !== JSON.stringify({target_hash:evidence.target_hash,judgment_hash:evidence.judgment_hash,reindex_receipt_hash:evidence.reindex_receipt_hash}) || checks.retrieval_marker_blake3 !== evidence.retrieval_marker_blake3 || JSON.stringify(checks.query_assertions) !== JSON.stringify(evidence.query_assertions) || checks.vault_unchanged !== true || checks.frozen_artifacts_unchanged !== true || JSON.stringify(checks.frozen_q3) !== JSON.stringify(evidence.frozen_q3)) fail('proof disagreement'); return evidence; }
function fsync(path) { const descriptor = openSync(path, 'r'); try { fsyncSync(descriptor); } finally { closeSync(descriptor); } }
export function publishProof({ output, evidence, filesystem }) { validateEvidence(evidence); const { parent, path } = outputRoot(output); const temporary = resolve(parent, `.${basename(path)}.safe-clone-${process.pid}-${Date.now()}`); try { if (filesystem?.sameDevice ? !filesystem.sameDevice(parent, dirname(temporary)) : statSync(parent).dev !== statSync(dirname(temporary)).dev) fail('cross-filesystem publication'); writeFileSync(temporary, `${JSON.stringify(evidence)}\n`, {mode:0o600,flag:'wx'}); fsync(temporary); renameSync(temporary,path); fsync(parent); return path; } catch (error) { try { rmSync(temporary,{force:true}); } catch {} if (error instanceof ProofError) throw error; fail('publication failed'); } }
function parse(argv) { const names = ['binary','hashBinary','store','seed','p2Profile','p3Profile','golden','thresholds','frozenBaseline','baselineOutput','frozenIndex','vault','targetQueryFile','judgmentQueryFile','output']; const flags = new Map(names.map(name => [`--${name.replace(/[A-Z]/g, character => `-${character.toLowerCase()}`)}`,name])); const values = {}; for (let index=0; index<argv.length; index += 2) { const name = flags.get(argv[index]); if (!name || !argv[index+1] || values[name]) fail('invalid arguments'); values[name] = argv[index+1]; } if (argv.length % 2 || names.some(name => !values[name])) fail('invalid arguments'); return values; }
function sourceCommit() { let value; try { value = execFileSync('git', ['-C', resolve(dirname(fileURLToPath(import.meta.url)), '..'), 'rev-parse', '--verify', 'HEAD'], {encoding:'utf8',stdio:['ignore','pipe','ignore']}); } catch { fail('source identity unavailable'); } if (!/^[0-9a-f]{40}\n$/.test(value)) fail('source identity unavailable'); return value.slice(0, -1); }
export function observeAndPublish(options) {
  // Resolve every read-only and writable root before a helper or SovMem process can run.
  const binary = regular(options.binary); const helper = regular(options.hashBinary); const store = directory(options.store); const seed = directory(options.seed); const vault = directory(options.vault); const frozenIndex = directory(options.frozenIndex); const frozenFiles = [options.p2Profile,options.p3Profile,options.golden,options.thresholds,options.frozenBaseline].map(regular); const targetQueryFile = regular(options.targetQueryFile); const judgmentQueryFile = regular(options.judgmentQueryFile); const output = outputRoot(options.output); const baselineOutput = absentOutput(options.baselineOutput);
  assertPreflight([store,output.parent,baselineOutput.parent], [seed,vault,frozenIndex,...frozenFiles,targetQueryFile,judgmentQueryFile]);
  const proofSourceCommit = sourceCommit();
  const targetText = queryText(targetQueryFile); const judgmentText = queryText(judgmentQueryFile); const retrievalMarkerBlake3 = rawHash(helper,targetQueryFile);
  const before = { seed:treeHashes(seed,helper), vault:metadataSnapshot(vault), frozen:new Map(frozenFiles.map(path => [path,rawHash(helper,path)])) };
  const copiedSeed = directory(resolve(store,'seed')); if (!sameMaps(before.seed,treeHashes(copiedSeed,helper))) fail('seed copy mismatch');
  const level3 = jsonCommand(binary,['verify','--profile',options.p2Profile,'--bundle',seed,'--level','3','--require-corpus'],parseLevel3,'level 3 invalid'); if (level3.documents !== 62 || level3.warnings !== 226 || level3.errors !== 0) fail('level 3 invalid');
  // This literal path is safe; no manifest-controlled component is used to reach it.
  const manifest = jsonFile(resolve(store,'canon','generations',GENERATION,'manifest.json'),parseManifest,'manifest invalid');
  if (manifest.profile_hash !== rawHash(helper,options.p3Profile) || manifest.seed_bundle_hash !== level3.bundle_hash || manifest.bundle_hash === manifest.seed_bundle_hash) fail('manifest binding invalid');
  const protectedBinding = protectedIntakeBinding(store,manifest);
  const bindings = { manifestHash:null, bundle:manifest.bundle_hash, ...appliedDocuments(manifest) };
  const level4 = jsonCommand(binary,['verify','--profile',options.p3Profile,'--store',store,'--level','4'],parseLevel4,'level 4 invalid'); if (level4.events !== 2 || level4.verdicts !== 1 || level4.applied_generations !== 1 || level4.errors !== 0 || level4.current_generation !== manifest.generation_id || level4.bundle_hash !== manifest.bundle_hash) fail('level 4 binding invalid');
  const verdictList = jsonCommand(binary,['verdicts','list','--profile',options.p3Profile,'--store',store,'--format','json'],parseVerdicts,'verdict list invalid');
  // The public verdict record carries the canonical manifest hash; do not hash private bodies.
  const matching = verdictList.verdicts.filter(verdict => verdict.applied && verdict.proposal_id === manifest.applied_proposal_id && verdict.verdict_id === manifest.applied_verdict_id); if (matching.length !== 1 || !hash(matching[0].manifest_hash)) fail('verdict binding invalid'); bindings.manifestHash = matching[0].manifest_hash;
  const verdict = appliedVerdict(verdictList,manifest,bindings);
  const first = jsonCommand(binary,['index','--profile',options.p3Profile,'--store',store],parseIndex,'first index invalid'); assertIndex(first,manifest,bindings.manifestHash,verdict.reindex_receipt_hash);
  const seedHit = frozenSeedMiss(binary,options.p2Profile,seed,frozenIndex,targetText,level3.bundle_hash); const targetHit = queryStore(binary,options.p3Profile,store,targetText,bindings.target.id,bindings); const judgmentHit = queryStore(binary,options.p3Profile,store,judgmentText,bindings.judgment.id,bindings);
  // All snapshot/binding checks are complete before the only deletion, which has a literal generation.
  rmSync(resolve(store,'derived','indexes',GENERATION),{recursive:true,force:false});
  const rebuilt = jsonCommand(binary,['index','--profile',options.p3Profile,'--store',store],parseIndex,'rebuilt index invalid'); assertIndex(rebuilt,manifest,bindings.manifestHash,verdict.reindex_receipt_hash);
  const rebuiltTarget = queryStore(binary,options.p3Profile,store,targetText,bindings.target.id,bindings); const rebuiltJudgment = queryStore(binary,options.p3Profile,store,judgmentText,bindings.judgment.id,bindings);
  const q3 = jsonCommand(binary,['eval','--profile',options.p2Profile,'--bundle',seed,'--index',frozenIndex,'--golden',options.golden,'--thresholds',options.thresholds,'--baseline',baselineOutput.path],parseEval,'q3 invalid');
  const generatedBaseline = regular(baselineOutput.path);
  if (q3.baseline_written !== true || !readFileSync(generatedBaseline).equals(readFileSync(frozenFiles[4]))) fail('baseline output invalid');
  const after = { seed:treeHashes(seed,helper), vault:metadataSnapshot(vault), frozen:new Map(frozenFiles.map(path => [path,rawHash(helper,path)])) }; if (!sameMaps(before.seed,after.seed)) fail('seed changed');
  const evidence = {schema_version:'sovmem.p3.safe-clone-proof.v2',status:'passed',source_commit:proofSourceCommit,store_id:manifest.store_id,profile_version:manifest.profile_version,seed_bundle_hash:level3.bundle_hash,proposal_id:manifest.applied_proposal_id,verdict_id:manifest.applied_verdict_id,intake_sequence:protectedBinding.intake.sequence,intake_head_hash:protectedBinding.intake.head_hash,ledger_head:level4.ledger_head,generation:manifest.generation_id,target_hash:bindings.target.hash,judgment_hash:bindings.judgment.hash,reindex_receipt_hash:verdict.reindex_receipt_hash,retrieval_marker_blake3:retrievalMarkerBlake3,level4:{events:level4.events,verdicts:level4.verdicts,applied_generations:level4.applied_generations,errors:level4.errors},query_assertions:{seed_hit:seedHit,target_hit:targetHit,judgment_hit:judgmentHit,rebuilt_target_hit:rebuiltTarget,rebuilt_judgment_hit:rebuiltJudgment},vault_unchanged:sameMaps(before.vault,after.vault),frozen_artifacts_unchanged:sameMaps(before.frozen,after.frozen),frozen_q3:{queries:q3.queries,hits:q3.queries_with_hit,k:q3.k,precision:`${q3.precision_numerator}/${q3.precision_denominator}`,recall:`${q3.recall_numerator}/${q3.recall_denominator}`,misses:['q-005','q-011'],status:q3.status.toUpperCase()}};
  verifySafeClone({evidence,checks:{level3,level4,intake:protectedBinding.intake,applied:{target_hash:bindings.target.hash,judgment_hash:bindings.judgment.hash,reindex_receipt_hash:rebuilt.receipt_hash},verdict_intake:protectedBinding.verdict,retrieval_marker_blake3:retrievalMarkerBlake3,query_assertions:evidence.query_assertions,vault_unchanged:evidence.vault_unchanged,frozen_artifacts_unchanged:evidence.frozen_artifacts_unchanged,frozen_q3:evidence.frozen_q3}});
  return publishProof({output:output.path,evidence});
}
function main() { try { observeAndPublish(parse(process.argv.slice(2))); process.stdout.write('safe-clone harness: verified\n'); } catch { process.stderr.write('safe-clone harness: invalid arguments\n'); process.exitCode = 2; } }
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
