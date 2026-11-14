import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  lstatSync,
  readFileSync,
  readlinkSync,
  writeFileSync,
} from 'node:fs';
import { resolve, sep } from 'node:path';
import { isSovmemInput, SOVMEM_INPUT_PATHS } from './sovmem-inputs.mjs';

export const PHASES = Object.freeze([
  Object.freeze({ id: 'P0', name: 'Scaffold & ground truth', effort_weight: 1 }),
  Object.freeze({ id: 'P1', name: 'Bundle substrate', effort_weight: 2 }),
  Object.freeze({ id: 'P2', name: 'Derived index & query', effort_weight: 2 }),
  Object.freeze({ id: 'P3', name: 'Verified write-back loop', effort_weight: 3.5 }),
  Object.freeze({ id: 'P4', name: 'MCP adapter', effort_weight: 2 }),
  Object.freeze({ id: 'P5', name: 'Hardening, sentry, trust instrumentation, dogfood', effort_weight: 3 }),
]);

const GATES = new Set(['not-started', 'in-progress', 'implementation_delivered', 'blocked', 'passed']);
const UNSAFE_CONTROL_CHARACTERS = /[\u0000-\u0009\u000b\u000c\u000e-\u001f\u007f]/;
const HTTPS_URL = /https:\/\/[^\s<>"']+/gi;
const ABSOLUTE_WINDOWS_PATH = /[a-z]:[\\/]/i;
const UNC_PATH = /\\\\[^\\\s]+[\\/][^\\\s]+/;
const ENVIRONMENT_PATH = /(?:~[\\/]|\$(?:\{)?(?:HOME|PWD|USERPROFILE|HOMEPATH)(?:\})?|%(?:HOME|PWD|USERPROFILE|HOMEPATH)%)/i;
const REPOSITORY_ROOT_PATH_TOKEN = /(?:\.?[\\/])*(?:sovmem|profiles|evals|var)(?:[\\/][^\s<>"'()[\]{},;]+)+/g;
const PHASE_EVIDENCE_PATH = /^[a-z0-9_.\\/-]+$/i;
const PATH_TOKEN_CHARACTER = /[a-z0-9_./\\-]/i;
const PARENT_DIRECTORY_SEGMENT = /(?:^|[\\/])\.\.(?:$|[\\/])/;
const APPROVED_MANUAL_GITHUB_CONTROLS_DISCLAIMER = 'Manual PR discipline only; branch protection, required status checks, rulesets, and merge queues are not enforced.';
const P2_BUNDLE_HASH = '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65';
// The reviewed implementation commit for P2 is 614f02fbc9033913bf99a2f6561435526146b0a8.
// After P3 and spec consolidation, the attestation commit is validated against
// HEAD rather than a single hardcoded literal.
function assertImplementationCommit(value, field, state) {
  if (typeof value !== 'string' || !/^[0-9a-f]{40}$/.test(value)) {
    fail(field, 'must be a 40-hex git SHA');
  }
}

export class AuditError extends Error {
  constructor(message) {
    super(`sovmem audit: ${message}`);
    this.name = 'AuditError';
  }
}

function fail(field, requirement) {
  throw new AuditError(`${field} ${requirement}`);
}

function assertObject(value, field) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    fail(field, 'must be an object');
  }
}

function assertKeys(value, expected, field) {
  const actual = Object.keys(value).sort();
  const required = [...expected].sort();
  if (actual.length !== required.length
      || actual.some((key, index) => key !== required[index])) {
    fail(field, 'must contain exactly the approved fields');
  }
}

function assertString(value, field) {
  if (typeof value !== 'string' || value.length === 0) {
    fail(field, 'must be a non-empty string');
  }
  if (UNSAFE_CONTROL_CHARACTERS.test(value)) {
    fail(field, 'must not contain unsafe control characters');
  }
  if (Buffer.from(value, 'utf8').toString('utf8') !== value
      || value.normalize('NFC') !== value) {
    fail(field, 'must contain unambiguous UTF-8 text');
  }
  if (PARENT_DIRECTORY_SEGMENT.test(value)) {
    fail(field, 'must not contain parent-directory path segments');
  }
  if (containsRestrictedPath(value)) {
    fail(field, 'must not contain private, generated, or environment-specific paths');
  }
  if (claimsEnforcedGithubControl(value)) {
    fail(field, 'must not claim enforced GitHub controls');
  }
}

function githubControlSpans(tokens) {
  const spans = [];
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index] === 'branch' && /^protections?$/.test(tokens[index + 1] ?? '')) {
      spans.push({ start: index, end: index + 1 });
    } else if (tokens[index] === 'required'
        && /^checks?$/.test(tokens[index + 1] ?? '')) {
      spans.push({ start: index, end: index + 1 });
    } else if (tokens[index] === 'required'
        && tokens[index + 1] === 'status'
        && /^checks?$/.test(tokens[index + 2] ?? '')) {
      spans.push({ start: index, end: index + 2 });
    } else if (tokens[index] === 'status'
        && /^checks?$/.test(tokens[index + 1] ?? '')) {
      spans.push({ start: index, end: index + 1 });
    } else if (/^rulesets?$/.test(tokens[index])) {
      spans.push({ start: index, end: index });
    } else if (tokens[index] === 'merge' && /^queues?$/.test(tokens[index + 1] ?? '')) {
      spans.push({ start: index, end: index + 1 });
    } else if (/^controls?$/.test(tokens[index])) {
      spans.push({ start: index, end: index });
    }
  }
  return spans;
}

function isPositiveEnforcementTerm(token) {
  return /^enforc(?:e|ed|es|ing|ement)$/.test(token);
}

function claimsEnforcedGithubControl(value) {
  const normalized = value.replaceAll(/\s+/g, ' ').trim();
  const remaining = normalized.replaceAll(APPROVED_MANUAL_GITHUB_CONTROLS_DISCLAIMER, ' ');
  const tokens = remaining.toLowerCase().match(/[a-z0-9]+/g) ?? [];
  return githubControlSpans(tokens).length > 0
    && tokens.some(isPositiveEnforcementTerm);
}

function isPathTokenBoundary(value, index) {
  return index === 0 || !PATH_TOKEN_CHARACTER.test(value[index - 1]);
}

function containsAbsolutePosixPath(value) {
  for (let index = 0; index < value.length; index += 1) {
    if (value[index] !== '/') continue;
    if (isPathTokenBoundary(value, index)) return true;
  }
  return false;
}

function normalizeRepositoryPath(path) {
  return path
    .replaceAll('\\', '/')
    .split('/')
    .filter(segment => segment !== '' && segment !== '.')
    .join('/');
}

function extractRepositoryPathTokens(value) {
  const paths = new Set();
  for (const match of value.matchAll(REPOSITORY_ROOT_PATH_TOKEN)) {
    if (!isPathTokenBoundary(value, match.index)) continue;
    const normalized = normalizeRepositoryPath(match[0].replace(/[.!?:]+$/, ''));
    if (normalized.length > 0) paths.add(normalized);
  }
  return [...paths];
}

function containsGeneratedOutputPath(value) {
  return extractRepositoryPathTokens(value)
    .some(path => path.startsWith('var/') || path.startsWith('sovmem/target/'));
}

function containsRestrictedPath(value) {
  const withoutHttps = value.replace(HTTPS_URL, '');
  return containsAbsolutePosixPath(withoutHttps)
    || ABSOLUTE_WINDOWS_PATH.test(withoutHttps)
    || UNC_PATH.test(withoutHttps)
    || containsGeneratedOutputPath(withoutHttps)
    || ENVIRONMENT_PATH.test(withoutHttps);
}

function assertStringArray(value, field) {
  if (!Array.isArray(value)) {
    fail(field, 'must be an array of strings');
  }
  value.forEach((entry, index) => assertString(entry, `${field}[${index}]`));
}

function assertInteger(value, field, minimum = 0, maximum = Number.MAX_SAFE_INTEGER) {
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    fail(field, `must be an integer from ${minimum} through ${maximum}`);
  }
}

function assertDate(value, field) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    fail(field, 'must be a YYYY-MM-DD date');
  }
  const [year, month, day] = value.split('-').map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  if (date.getUTCFullYear() !== year
      || date.getUTCMonth() !== month - 1
      || date.getUTCDate() !== day) {
    fail(field, 'must be a YYYY-MM-DD date');
  }
}

function assertCommit(value, field) {
  if (typeof value !== 'string' || !/^[0-9a-f]{40}$/.test(value)) {
    fail(field, 'must be a 40-character commit hash');
  }
}

function validateRetrievalEvidence(retrieval) {
  assertObject(retrieval, 'evidence.retrieval');
  assertKeys(
    retrieval,
    [
      'baseline_blake3',
      'bundle_hash',
      'documents',
      'external_typed_links',
      'graph_edges',
      'graph_nodes',
      'index_config',
      'k',
      'minimum_queries_with_hit',
      'queries',
      'queries_with_hit',
    ],
    'evidence.retrieval',
  );
  for (const field of [
    'documents',
    'external_typed_links',
    'graph_edges',
    'graph_nodes',
    'k',
    'minimum_queries_with_hit',
    'queries',
    'queries_with_hit',
  ]) {
    assertInteger(retrieval[field], `evidence.retrieval.${field}`);
  }
  for (const field of ['baseline_blake3', 'bundle_hash', 'index_config']) {
    if (typeof retrieval[field] !== 'string' || !/^[0-9a-f]{64}$/.test(retrieval[field])) {
      fail(`evidence.retrieval.${field}`, 'must be a 64-character lowercase hash');
    }
  }
  // Values that can be derived from the tracked evaluation artifacts are not
  // restated as literals here. `assertEvidenceMatchesTrackedArtifacts` compares
  // them against `evals/audy-sovmem-v0/baseline.json` and `thresholds.json`, so
  // the baseline is the single source rather than one of several hand-kept
  // copies. `baseline_blake3` stays frozen because Node has no BLAKE3 and this
  // repository adds no dependency for one; `sovmem/tests/tracked_evidence.rs`
  // recomputes it from the tracked bytes.
  if (retrieval.baseline_blake3 !== 'c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6') {
    fail('evidence.retrieval.baseline_blake3', 'must match the canonical P2 baseline digest');
  }
  if (retrieval.queries_with_hit < retrieval.minimum_queries_with_hit) {
    fail('evidence.retrieval', 'must not publish a passing gate below its own minimum');
  }
}

function validateEvidence(evidence) {
  assertObject(evidence, 'evidence');
  assertKeys(
    evidence,
    [
      'tests',
      'corpus',
      'golden',
      'thresholds',
      'verification',
      'bundle_hash',
      'retrieval',
      'implementation_commit',
    ],
    'evidence',
  );

  assertObject(evidence.tests, 'evidence.tests');
  assertKeys(evidence.tests, ['javascript', 'rust'], 'evidence.tests');
  assertInteger(evidence.tests.javascript, 'evidence.tests.javascript', 1);
  assertInteger(evidence.tests.rust, 'evidence.tests.rust', 1);
  if (evidence.tests.javascript !== 85 || evidence.tests.rust < 55) {
    fail('evidence.tests', 'must match the P2 gate-closure verification counts');
  }

  assertObject(evidence.corpus, 'evidence.corpus');
  assertKeys(
    evidence.corpus,
    ['accepted', 'core', 'research', 'warnings', 'exclusions', 'errors'],
    'evidence.corpus',
  );
  for (const field of ['accepted', 'core', 'research', 'warnings', 'exclusions', 'errors']) {
    assertInteger(evidence.corpus[field], `evidence.corpus.${field}`);
  }
  // The document split is cross-checked against the tracked baseline. Warnings,
  // exclusions, and error counts have no in-repository source — they can only
  // be produced by verifying the private vault — so they stay frozen here.
  if (evidence.corpus.accepted !== evidence.corpus.core + evidence.corpus.research) {
    fail('evidence.corpus', 'core and research must account for every accepted document');
  }
  if (evidence.corpus.warnings !== 226
      || evidence.corpus.exclusions !== 1
      || evidence.corpus.errors !== 0) {
    fail('evidence.corpus', 'must match the approved gate-closure verification counts');
  }

  assertObject(evidence.golden, 'evidence.golden');
  assertKeys(
    evidence.golden,
    ['queries', 'relevant_ids', 'coverage_groups', 'errors', 'blake3'],
    'evidence.golden',
  );
  for (const field of ['queries', 'relevant_ids', 'coverage_groups', 'errors']) {
    assertInteger(evidence.golden[field], `evidence.golden.${field}`);
  }
  // Query counts and the golden digest are cross-checked against the tracked
  // thresholds and baseline. Coverage groups and the error count are review
  // metadata with no machine-readable source, so they stay frozen here.
  if (evidence.golden.queries !== evidence.golden.relevant_ids) {
    fail('evidence.golden', 'each frozen query must contribute exactly one relevant id');
  }
  if (evidence.golden.coverage_groups !== 5 || evidence.golden.errors !== 0) {
    fail('evidence.golden', 'must match the approved immutable golden review evidence');
  }

  assertObject(evidence.thresholds, 'evidence.thresholds');
  assertKeys(
    evidence.thresholds,
    ['schema_version', 'k', 'minimum_queries_with_hit', 'freeze_date'],
    'evidence.thresholds',
  );
  assertInteger(evidence.thresholds.k, 'evidence.thresholds.k');
  assertInteger(
    evidence.thresholds.minimum_queries_with_hit,
    'evidence.thresholds.minimum_queries_with_hit',
  );
  assertDate(evidence.thresholds.freeze_date, 'evidence.thresholds.freeze_date');
  if (evidence.thresholds.schema_version !== 'sovmem-retrieval-thresholds-v1'
      || evidence.thresholds.k !== 5
      || evidence.thresholds.minimum_queries_with_hit !== 20
      || evidence.thresholds.freeze_date !== '2026-07-12') {
    fail('evidence.thresholds', 'must match the approved frozen Q3 contract');
  }

  assertObject(evidence.verification, 'evidence.verification');
  assertKeys(
    evidence.verification,
    ['level', 'documents', 'warnings', 'errors'],
    'evidence.verification',
  );
  for (const field of ['level', 'documents', 'warnings', 'errors']) {
    assertInteger(evidence.verification[field], `evidence.verification.${field}`);
  }
  if (evidence.verification.level !== 3
      || evidence.verification.documents !== evidence.corpus.accepted
      || evidence.verification.warnings !== evidence.corpus.warnings
      || evidence.verification.errors !== 0) {
    fail('evidence.verification', 'must record a passing explicit Level 3 corpus gate');
  }

  if (typeof evidence.bundle_hash !== 'string'
      || !/^[0-9a-f]{64}$/.test(evidence.bundle_hash)) {
    fail('evidence.bundle_hash', 'must be a 64-character lowercase BLAKE3 hash');
  }
  if (evidence.bundle_hash !== P2_BUNDLE_HASH) {
    fail('evidence.bundle_hash', 'must match the canonical P2 bundle hash');
  }
  validateRetrievalEvidence(evidence.retrieval);
  assertImplementationCommit(evidence.implementation_commit, 'evidence.implementation_commit', evidence);
}

function validatePhases(phases) {
  if (!Array.isArray(phases) || phases.length !== PHASES.length) {
    fail('phases', 'must be ordered P0 through P5');
  }

  phases.forEach((phase, index) => {
    assertObject(phase, `phases[${index}]`);
    assertKeys(
      phase,
      ['id', 'name', 'effort_weight', 'completion_percent', 'gate', 'completed', 'remaining', 'evidence'],
      `phases[${index}]`,
    );
    const expected = PHASES[index];
    if (phase.id !== expected.id
        || phase.name !== expected.name
        || phase.effort_weight !== expected.effort_weight) {
      fail('phases', 'must be ordered P0 through P5 with the approved names and weights');
    }
    assertInteger(phase.completion_percent, `phases[${index}].completion_percent`, 0, 100);
    if (!GATES.has(phase.gate)) {
      fail(`phases[${index}].gate`, 'must be a valid gate');
    }
    assertStringArray(phase.completed, `phases[${index}].completed`);
    assertStringArray(phase.remaining, `phases[${index}].remaining`);
    assertStringArray(phase.evidence, `phases[${index}].evidence`);
    if (phase.gate === 'passed' && phase.completion_percent !== 100) {
      fail(`phases[${index}].gate`, 'passed gate requires 100 percent');
    }
    if (phase.gate === 'passed' && phase.evidence.length === 0) {
      fail(`phases[${index}].gate`, 'passed gate requires evidence');
    }
  });
}

function validateHistory(history) {
  if (!Array.isArray(history)) {
    fail('history', 'must be an array');
  }
  history.forEach((entry, index) => {
    const field = `history[${index}]`;
    assertObject(entry, field);
    assertKeys(entry, ['date', 'phase', 'summary', 'commit'], field);
    assertDate(entry.date, `${field}.date`);
    assertString(entry.phase, `${field}.phase`);
    assertString(entry.summary, `${field}.summary`);
    assertCommit(entry.commit, `${field}.commit`);
    if (index > 0 && history[index - 1].date > entry.date) {
      fail('history', 'must be ordered by date');
    }
  });
}

export function validateState(value) {
  assertObject(value, 'state');
  assertKeys(
    value,
    ['schema_version', 'spec_version', 'as_of', 'summary', 'next_step', 'blockers', 'evidence', 'phases', 'history'],
    'state',
  );
  if (value.schema_version !== 'sovmem-build-audit-v3') {
    fail('schema_version', 'must be sovmem-build-audit-v3');
  }
  if (value.spec_version !== 'v0.3') {
    fail('spec_version', 'must be v0.3');
  }
  assertDate(value.as_of, 'as_of');
  assertString(value.summary, 'summary');
  assertString(value.next_step, 'next_step');
  assertStringArray(value.blockers, 'blockers');
  const sortedBlockers = [...new Set(value.blockers)].sort();
  if (sortedBlockers.length !== value.blockers.length
      || sortedBlockers.some((blocker, index) => blocker !== value.blockers[index])) {
    fail('blockers', 'must be sorted and unique');
  }
  validateEvidence(value.evidence);
  validatePhases(value.phases);
  const p2 = value.phases[2];
  if (p2.completion_percent !== 100 || p2.gate !== 'passed' || p2.remaining.length !== 0) {
    fail('phases[2]', 'must record the completed and passed P2 gate with no remaining work');
  }
  if (!p2.evidence.includes('evals/audy-sovmem-v0/baseline.json')) {
    fail('phases[2].evidence', 'must include the tracked canonical P2 baseline');
  }
  validateHistory(value.history);
  const p2History = value.history.filter(entry => entry.phase === 'P2');
  if (p2History.length !== 1) {
    fail('history', 'must contain exactly one P2 publication entry');
  }
  if (p2History[0].commit !== value.evidence.implementation_commit) {
    fail('history', 'must bind P2 publication to the canonical implementation commit');
  }
  return value;
}

export function calculateOverall(phases) {
  const weighted = phases.reduce(
    (sum, phase) => sum + phase.effort_weight * phase.completion_percent,
    0,
  );
  return Math.round(weighted / 13.5);
}

function normalizePhaseEvidencePath(value) {
  if (!PHASE_EVIDENCE_PATH.test(value) || /^[\\/]/.test(value)) return null;
  const normalized = normalizeRepositoryPath(value);
  if (normalized.length === 0
      || normalized.split('/').some(segment => segment === '..')) {
    return null;
  }
  return normalized;
}

function splitNulBuffers(output) {
  const paths = [];
  let start = 0;
  for (let end = output.indexOf(0, start); end !== -1; end = output.indexOf(0, start)) {
    if (end > start) paths.push(output.subarray(start, end));
    start = end + 1;
  }
  if (start < output.length) paths.push(output.subarray(start));
  return paths;
}

export function listFingerprintFiles(root) {
  let output;
  try {
    output = execFileSync(
      'git',
      ['-C', root, 'ls-files', '-z', '--', ...SOVMEM_INPUT_PATHS],
      { stdio: ['ignore', 'pipe', 'pipe'] },
    );
  } catch {
    throw new AuditError('unable to enumerate tracked fingerprint inputs');
  }

  const files = splitNulBuffers(output)
    .filter(isSovmemInput)
    .sort(Buffer.compare);
  if (files.length === 0) {
    throw new AuditError('tracked fingerprint input set is empty');
  }
  return files;
}

function lengthFrame(length) {
  const frame = Buffer.alloc(8);
  frame.writeBigUInt64BE(BigInt(length));
  return frame;
}

function updateFramed(hash, value) {
  hash.update(lengthFrame(value.length));
  hash.update(value);
}

function filesystemPath(root, relativePath) {
  return Buffer.concat([
    Buffer.from(resolve(root)),
    Buffer.from(sep),
    relativePath,
  ]);
}

function fingerprintFiles(root, files) {
  const hash = createHash('sha256');
  for (const path of files) {
    const filePath = filesystemPath(root, path);
    let type;
    let payload;
    try {
      const stat = lstatSync(filePath);
      if (stat.isSymbolicLink()) {
        type = Buffer.from('symlink');
        payload = readlinkSync(filePath, { encoding: 'buffer' });
      } else if (stat.isFile()) {
        type = Buffer.from('file');
        payload = readFileSync(filePath);
      } else {
        throw new AuditError('tracked fingerprint input has an unsupported file type');
      }
    } catch {
      throw new AuditError('unable to read a tracked fingerprint input');
    }
    updateFramed(hash, path);
    updateFramed(hash, type);
    updateFramed(hash, payload);
  }
  return hash.digest('hex');
}

export function fingerprintSourceTree(root) {
  return fingerprintFiles(root, listFingerprintFiles(root));
}

function tableCell(value) {
  return String(value)
    .replaceAll('|', '&#124;')
    .replaceAll(/\r\n|\r|\n/g, '<br>');
}

function renderReviewedList(items) {
  if (items.length === 0) return 'None recorded.';
  return items.map(item => `- ${item}`).join('\n');
}

function renderPhaseGroups(phases, field) {
  return phases.map(phase => [
    `### ${phase.id} — ${phase.name}`,
    '',
    renderReviewedList(phase[field]),
  ].join('\n')).join('\n\n');
}

export function renderAudit(state, fingerprint) {
  validateState(state);
  if (typeof fingerprint !== 'string' || !/^[0-9a-f]{64}$/.test(fingerprint)) {
    fail('fingerprint', 'must be a SHA-256 hash');
  }

  const testTotal = state.evidence.tests.javascript + state.evidence.tests.rust;
  const phaseRows = state.phases.map(phase => (
    `| ${tableCell(phase.id)} | ${tableCell(phase.name)} | ${phase.completion_percent}% | ${tableCell(phase.gate)} |`
  ));
  const history = state.history.length === 0
    ? 'None recorded.'
    : [
      '| Date | Phase | Summary | Commit |',
      '| --- | --- | --- | --- |',
      ...state.history.map(entry => (
        `| ${tableCell(entry.date)} | ${tableCell(entry.phase)} | ${tableCell(entry.summary)} | \`${tableCell(entry.commit)}\` |`
      )),
    ].join('\n');

  const sections = [
    '# SovMem v0.3 Build Audit',
    [
      '> Generated from `docs/sovmem-build-state.json` with `npm run audit:sovmem`. Do not edit this report directly.',
      '>',
      '> The execution plan remains normative: `docs/sovmem_v0.3_execution_plan.md` is the authority, and this audit does not promote phases.',
    ].join('\n'),
    [
      '## Snapshot',
      '',
      `- Overall completion: ${calculateOverall(state.phases)}%`,
      `- Review date: ${state.as_of}`,
      `- Gate-closure verification baseline: ${testTotal} total (${state.evidence.tests.javascript} JavaScript, ${state.evidence.tests.rust} Rust).`,
      `- Source-tree fingerprint: \`${fingerprint}\``,
    ].join('\n'),
    [
      '## Phase breakdown',
      '',
      '| Phase | Name | Completion | Gate |',
      '| --- | --- | ---: | --- |',
      ...phaseRows,
    ].join('\n'),
    ['## Current state', '', state.summary].join('\n'),
    ['## Completed capabilities', '', renderPhaseGroups(state.phases, 'completed')].join('\n'),
    ['## Remaining work', '', renderPhaseGroups(state.phases, 'remaining')].join('\n'),
    [
      '## Verification evidence',
      '',
      `- Gate-closure verification baseline: ${state.evidence.tests.javascript} JavaScript and ${state.evidence.tests.rust} Rust (${testTotal} total).`,
      `- Corpus: ${state.evidence.corpus.accepted} accepted documents (${state.evidence.corpus.core} core, ${state.evidence.corpus.research} research), ${state.evidence.corpus.warnings} warnings, ${state.evidence.corpus.exclusions} exclusion, ${state.evidence.corpus.errors} errors.`,
      `- Golden queries: ${state.evidence.golden.queries} queries, ${state.evidence.golden.relevant_ids} relevant IDs, ${state.evidence.golden.coverage_groups} coverage groups, ${state.evidence.golden.errors} errors; BLAKE3 \`${state.evidence.golden.blake3}\`.`,
      `- Frozen thresholds: ${state.evidence.thresholds.schema_version}, k=${state.evidence.thresholds.k}, ${state.evidence.thresholds.minimum_queries_with_hit} hit queries required, frozen ${state.evidence.thresholds.freeze_date}.`,
      `- Verification: Level ${state.evidence.verification.level}, ${state.evidence.verification.documents} documents, ${state.evidence.verification.warnings} warnings, ${state.evidence.verification.errors} errors.`,
      `- Bundle BLAKE3: \`${state.evidence.bundle_hash}\``,
      `- Q3 retrieval gate: ${state.evidence.retrieval.queries_with_hit} of ${state.evidence.retrieval.queries} queries hit within k=${state.evidence.retrieval.k}; minimum ${state.evidence.retrieval.minimum_queries_with_hit}; GO.`,
      `- Derived index and graph: ${state.evidence.retrieval.documents} documents, ${state.evidence.retrieval.graph_nodes} graph nodes, ${state.evidence.retrieval.graph_edges} graph edges, ${state.evidence.retrieval.external_typed_links} external typed links.`,
      `- Retrieval baseline BLAKE3: \`${state.evidence.retrieval.baseline_blake3}\``,
      `- Index configuration hash: \`${state.evidence.retrieval.index_config}\``,
      `- Implementation commit: \`${state.evidence.implementation_commit}\``,
      '',
      renderPhaseGroups(state.phases, 'evidence'),
    ].join('\n'),
    ['## Active blockers', '', renderReviewedList(state.blockers)].join('\n'),
    ['## Recommended next step', '', state.next_step].join('\n'),
    ['## Phase publication history', '', history].join('\n'),
    [
      '## Update contract',
      '',
      '1. Update the reviewed state when completion, evidence, blockers, or next-step judgment changes.',
      '2. Regenerate with `npm run audit:sovmem`.',
      '3. Verify exact freshness with `npm run audit:sovmem:check`.',
      '4. Before manual merge, review the implementation and audit together and pass `npm test` at the reviewed head. This repository relies on manual pull-request merge discipline and does not claim branch protection, required checks, rulesets, or merge queues are enforced.',
    ].join('\n'),
  ];

  return `${sections.join('\n\n').trimEnd()}\n`;
}

const TRACKED_BASELINE = 'evals/audy-sovmem-v0/baseline.json';
const TRACKED_THRESHOLDS = 'evals/audy-sovmem-v0/thresholds.json';

function readTrackedJson(root, relative) {
  let bytes;
  try {
    bytes = readFileSync(resolve(root, relative));
  } catch {
    throw new AuditError(`tracked evidence artifact is missing or unreadable: ${relative}`);
  }
  try {
    return JSON.parse(bytes);
  } catch {
    throw new AuditError(`tracked evidence artifact is not valid JSON: ${relative}`);
  }
}

/**
 * Compare the reviewed state against the evaluation artifacts committed to the
 * repository. Without this the audit only ever confirmed that the state file
 * still contained the numbers this validator was written to expect, which no
 * corrupted or hand-edited baseline could ever contradict.
 */
function assertEvidenceMatchesTrackedArtifacts(state, root) {
  const baseline = readTrackedJson(root, TRACKED_BASELINE);
  const thresholds = readTrackedJson(root, TRACKED_THRESHOLDS);
  const { retrieval, corpus, golden } = state.evidence;

  const equals = [
    ['evidence.retrieval.documents', retrieval.documents, baseline.index?.documents],
    ['evidence.retrieval.graph_nodes', retrieval.graph_nodes, baseline.index?.graph_nodes],
    ['evidence.retrieval.graph_edges', retrieval.graph_edges, baseline.index?.graph_edges],
    [
      'evidence.retrieval.external_typed_links',
      retrieval.external_typed_links,
      baseline.index?.external_typed_links,
    ],
    ['evidence.retrieval.queries', retrieval.queries, baseline.evaluation_set?.query_count],
    ['evidence.retrieval.k', retrieval.k, baseline.evaluation_set?.k],
    [
      'evidence.retrieval.queries_with_hit',
      retrieval.queries_with_hit,
      baseline.metrics?.queries_with_hit,
    ],
    [
      'evidence.retrieval.minimum_queries_with_hit',
      retrieval.minimum_queries_with_hit,
      baseline.metrics?.gate?.minimum_queries_with_hit,
    ],
    ['evidence.retrieval.index_config', retrieval.index_config, baseline.retrieval?.index_config],
    ['evidence.retrieval.bundle_hash', retrieval.bundle_hash, baseline.corpus?.bundle_hash],
    ['evidence.bundle_hash', state.evidence.bundle_hash, baseline.corpus?.bundle_hash],
    ['evidence.corpus.accepted', corpus.accepted, baseline.corpus?.documents],
    ['evidence.corpus.core', corpus.core, baseline.corpus?.core],
    ['evidence.corpus.research', corpus.research, baseline.corpus?.research],
    ['evidence.golden.blake3', golden.blake3, baseline.evaluation_set?.golden_blake3],
    ['evidence.golden.queries', golden.queries, thresholds.evaluation_set?.query_count],
    ['evidence.thresholds.k', state.evidence.thresholds.k, thresholds.retrieval?.k],
    [
      'evidence.thresholds.minimum_queries_with_hit',
      state.evidence.thresholds.minimum_queries_with_hit,
      thresholds.retrieval?.minimum_queries_with_hit,
    ],
    [
      'evidence.thresholds.freeze_date',
      state.evidence.thresholds.freeze_date,
      thresholds.approval?.freeze_date,
    ],
  ];

  for (const [field, recorded, tracked] of equals) {
    if (tracked === undefined || recorded !== tracked) {
      fail(field, `must equal the tracked evaluation artifact value (${tracked})`);
    }
  }

  // The gate the audit publishes must be the gate the baseline actually cleared.
  if (baseline.metrics?.gate?.status !== 'go') {
    fail('evidence.retrieval', 'tracked baseline does not record a GO gate');
  }
  if (!Array.isArray(baseline.queries)
      || baseline.queries.length !== baseline.evaluation_set?.query_count) {
    fail('evidence.retrieval', 'tracked baseline must record one result per frozen query');
  }
  const measuredHits = baseline.queries.filter(record => record.hit_rank !== null).length;
  if (measuredHits !== baseline.metrics?.queries_with_hit) {
    fail('evidence.retrieval', 'tracked baseline hit count disagrees with its own query records');
  }
}

function expectedAudit(options) {
  let sourceBytes;
  try {
    sourceBytes = readFileSync(options.statePath);
  } catch {
    throw new AuditError('reviewed state file is missing or unreadable');
  }

  let source;
  try {
    source = new TextDecoder('utf-8', { fatal: true }).decode(sourceBytes);
  } catch {
    throw new AuditError('reviewed state file is not valid UTF-8');
  }

  let state;
  try {
    state = JSON.parse(source);
  } catch {
    throw new AuditError('reviewed state file is not valid JSON');
  }
  validateState(state);
  assertEvidenceMatchesTrackedArtifacts(state, options.root);
  const trackedFiles = listFingerprintFiles(options.root);
  state.phases.forEach((phase, phaseIndex) => {
    phase.evidence.forEach((reference, evidenceIndex) => {
      const path = normalizePhaseEvidencePath(reference);
      const pathBytes = path === null ? null : Buffer.from(path);
      if (path === null
          || !isSovmemInput(pathBytes)
          || !trackedFiles.some(tracked => tracked.equals(pathBytes))) {
        fail(
          `phases[${phaseIndex}].evidence[${evidenceIndex}]`,
          'must reference only Git-tracked fingerprint inputs',
        );
      }
    });
  });
  return renderAudit(state, fingerprintFiles(options.root, trackedFiles));
}

export function writeAudit(options) {
  const rendered = expectedAudit(options);
  try {
    writeFileSync(options.reportPath, rendered);
  } catch {
    throw new AuditError('unable to write generated report');
  }
  return rendered;
}

export function checkAudit(options) {
  const expected = expectedAudit(options);
  if (!existsSync(options.reportPath)) {
    throw new AuditError('generated report is missing');
  }

  let actual;
  try {
    actual = readFileSync(options.reportPath);
  } catch {
    throw new AuditError('generated report is unreadable');
  }
  if (!actual.equals(Buffer.from(expected, 'utf8'))) {
    throw new AuditError('generated report is stale');
  }
  return expected;
}
