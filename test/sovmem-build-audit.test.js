import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import {
  AuditError,
  PHASES,
  calculateOverall,
  checkAudit,
  fingerprintSourceTree,
  listFingerprintFiles,
  renderAudit,
  validateState,
  writeAudit,
} from '../scripts/lib/sovmem-build-audit.mjs';
import { requiresSovmem, selectRun } from '../.github/scripts/sovmem-scope.mjs';

const EXPECTED_PHASES = [
  { id: 'P0', name: 'Scaffold & ground truth', effort_weight: 1 },
  { id: 'P1', name: 'Bundle substrate', effort_weight: 2 },
  { id: 'P2', name: 'Derived index & query', effort_weight: 2 },
  { id: 'P3', name: 'Verified write-back loop', effort_weight: 3.5 },
  { id: 'P4', name: 'MCP adapter', effort_weight: 2 },
  { id: 'P5', name: 'Hardening, sentry, trust instrumentation, dogfood', effort_weight: 3 },
];

const CANONICAL_AUDIT_WORKFLOW = `name: SovMem audit

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
  merge_group:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  scope:
    runs-on: ubuntu-latest
    timeout-minutes: 5
    outputs:
      full: \${{ steps.scope.outputs.full }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - name: Check bootstrap shell syntax
        run: bash -n scripts/setup-codex.sh scripts/codex-env.sh
      - name: Verify change-routing policy
        run: node --test .github/scripts/sovmem-scope.test.mjs
      - name: Select full regression checks
        id: scope
        env:
          BASE_SHA: \${{ github.event.pull_request.base.sha || github.event.merge_group.base_sha }}
          HEAD_SHA: \${{ github.event.pull_request.head.sha || github.event.merge_group.head_sha }}
        run: node .github/scripts/sovmem-scope.mjs
      - name: Verify audit freshness without Rust
        run: node scripts/generate-sovmem-build-audit.mjs --check

  verify:
    needs: scope
    # Keep the check visible as skipped, rather than leaving path-filtered
    # required checks pending. Manual dispatch always selects the full job.
    if: needs.scope.outputs.full == 'true'
    runs-on: ubuntu-latest
    # Cold Tantivy builds plus ~10 minutes of Rust test execution exceed the
    # previous 20-minute budget whenever the cargo cache misses.
    timeout-minutes: 40
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm

      # rust-toolchain.toml is the single source of truth for the channel and
      # for the clippy/rustfmt components. Installing via rustup keeps that pin
      # authoritative instead of duplicating the version in this workflow.
      - name: Install pinned Rust toolchain
        run: rustup show active-toolchain || rustup toolchain install
      - name: Report toolchain
        run: |
          rustc --version
          cargo --version
          cargo clippy --version
          cargo fmt --version
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: sovmem

      - run: npm ci
      - run: npm run audit:sovmem:check

      - name: Formatting
        run: cargo fmt --manifest-path sovmem/Cargo.toml --check
      - name: Lints
        run: cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --locked --features test-support -- -D warnings

      - name: Build with MCP
        run: cargo build --locked --manifest-path sovmem/Cargo.toml
      - name: Build without MCP
        run: cargo build --locked --manifest-path sovmem/Cargo.toml --no-default-features
      - run: npm test

      - name: Baseline integrity gate (readability check — not a regression comparison)
        run: |
          # This control verifies the stored baseline file is readable and
          # has the expected format.  It is NOT a regression comparison:
          # a full regression (fresh eval against baseline) requires a built
          # Tantivy index from the Audy vault, which CI does not have.
          #
          # See sovmem/tests/eval.rs for regression-simulation tests.
          #
          BASELINE_HITS=$(jq -r '.metrics.queries_with_hit' evals/audy-sovmem-v0/baseline.json)
          echo "Baseline hits: $BASELINE_HITS"
          if [ -z "$BASELINE_HITS" ] || [ "$BASELINE_HITS" -lt 1 ]; then
            echo "FAIL: could not read baseline"
            exit 1
          fi
          echo "PASS: eval baseline file is readable ($BASELINE_HITS hits)"
`;

const MANUAL_PUBLICATION_RULE = 'Before manually merging a SovMem implementation pull request, update `docs/sovmem-build-state.json`, regenerate `docs/SOVMEM-BUILD-AUDIT.md`, pass `npm run audit:sovmem:check` and `npm test` at the reviewed head, and review the implementation and audit together. This repository currently relies on manual pull-request merge discipline; documentation and audit evidence must not represent branch protection, required checks, rulesets, or merge queues as enforced unless independently verified later.';
const EXECUTOR_PUBLICATION_RULE = `9. ${MANUAL_PUBLICATION_RULE}`;

const AUDIT_REGISTRY_REQUIREMENTS = [
  {
    path: 'package.json',
    section: '### Project entry points and scaffold',
    status: 'JSON / scaffold and bounded command surface',
    purpose: /audit:sovmem.*audit:sovmem:check/,
  },
  {
    path: 'docs/archive/specs/2026-07-12-sovmem-build-audit-design.md',
    section: '### Project design and planning',
    status: 'Markdown / approved design',
    purpose: /reviewed state.*deterministic report.*authority boundaries/,
  },
  {
    path: 'docs/archive/plans/2026-07-12-sovmem-build-audit.md',
    section: '### Project design and planning',
    status: 'Markdown / approved execution plan',
    purpose: /test-first.*read-only pull-request gate.*final verification/,
  },
  {
    path: 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md',
    section: '### sovmem — memory and judgment',
    status: 'Markdown / derived generated evidence',
    purpose: /P0-P3 closure.*execution plan remains normative/,
  },
  {
    path: 'docs/sovmem-build-state.json',
    section: '### sovmem — memory and judgment',
    status: 'JSON / approved reviewed state',
    purpose: /human-reviewed P3 closure.*safe-clone evidence.*frozen P2 preservation/,
  },
  {
    path: 'scripts/lib/sovmem-build-audit.mjs',
    section: '### sovmem — memory and judgment',
    status: 'JavaScript / implemented audit tooling',
    purpose: /Validates strict P3 proof bindings.*raw-byte BLAKE3 evidence.*exact report freshness/,
  },
  {
    path: 'scripts/generate-sovmem-build-audit.mjs',
    section: '### sovmem — memory and judgment',
    status: 'JavaScript / implemented audit command',
    purpose: /write and read-only check modes.*root audit commands.*pull-request gate/,
  },
  {
    path: 'test/sovmem-build-audit.test.js',
    section: '### sovmem — memory and judgment',
    status: 'JavaScript / test',
    purpose: /strict state validation.*registry publication.*workflow constraints/,
  },
  {
    path: '.github/workflows/sovmem-audit.yml',
    section: '### sovmem — memory and judgment',
    status: 'YAML / read-only CI evidence',
    purpose: /audit freshness check.*full repository suite.*manual merge discipline/,
  },
];

function assertCanonicalWorkflow(workflow) {
  assert.equal(workflow, CANONICAL_AUDIT_WORKFLOW);
}

function markdownArtifactRows(index) {
  return index.split(/\r?\n/).filter(line => line.startsWith('| ['));
}

function markdownSection(markdown, expectedHeading) {
  const lines = markdown.split(/\r?\n/);
  const start = lines.indexOf(expectedHeading);
  assert.notEqual(start, -1, `${expectedHeading} section must exist`);
  const expectedLevel = /^(#+) /.exec(expectedHeading)?.[1].length;
  assert.ok(expectedLevel, `${expectedHeading} must be a Markdown heading`);
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    const heading = /^(#+) /.exec(lines[index]);
    if (heading && heading[1].length <= expectedLevel) {
      end = index;
      break;
    }
  }
  return lines.slice(start + 1, end).join('\n');
}

function parseArtifactRow(row) {
  assert.equal(row.endsWith('|'), true, 'artifact registry row must end with a cell delimiter');
  const cells = row.split('|').slice(1, -1).map(cell => cell.trim());
  assert.equal(cells.length, 3, 'artifact registry row must have exactly three cells');
  const link = /^\[[^\]]+\]\(([^)]+)\)$/.exec(cells[0]);
  assert.ok(link, 'artifact registry first cell must be one Markdown link');
  return { path: link[1], status: cells[1], purpose: cells[2] };
}

function assertAuditRegistry(index) {
  const allRows = markdownArtifactRows(index);
  for (const requirement of AUDIT_REGISTRY_REQUIREMENTS) {
    const matchesPath = row => {
      const firstDelimiter = row.indexOf('|', 1);
      const firstCell = row.slice(1, firstDelimiter).trim();
      const link = /^\[[^\]]+\]\(([^)]+)\)$/.exec(firstCell);
      return link?.[1] === requirement.path;
    };
    const globalMatches = allRows.filter(matchesPath);
    assert.equal(
      globalMatches.length,
      1,
      `${requirement.path} must appear in exactly one artifact row`,
    );
    const sectionRows = markdownArtifactRows(markdownSection(index, requirement.section));
    const sectionMatches = sectionRows.filter(matchesPath);
    assert.equal(
      sectionMatches.length,
      1,
      `${requirement.path} must appear in ${requirement.section}`,
    );
    const parsed = parseArtifactRow(sectionMatches[0]);
    assert.equal(parsed.status, requirement.status);
    assert.match(parsed.purpose, requirement.purpose);
  }
}

function countExact(text, value) {
  return text.split(value).length - 1;
}

function executorProtocolSection(plan) {
  const start = plan.indexOf('## 8. Executor protocol');
  assert.notEqual(start, -1, 'executor protocol section must exist');
  const remainder = plan.slice(start);
  const nextHeading = remainder.slice(1).search(/^## /m);
  return nextHeading === -1 ? remainder : remainder.slice(0, nextHeading + 1);
}

function assertExecutorPublicationRule(plan) {
  const section = executorProtocolSection(plan);
  assert.equal(countExact(section, EXECUTOR_PUBLICATION_RULE), 1);
  assert.equal(countExact(plan, EXECUTOR_PUBLICATION_RULE), 1);
}

function validState() {
  return {
    schema_version: 'sovmem-build-audit-v3',
    spec_version: 'v0.3',
    as_of: '2026-08-17',
    summary: 'P0, P1, P2, P3, and P4 are delivered. P0, P1, and P2 gates are closed with the canonical offline retrieval baseline passed Q3 at 23/25 top-five hits. P3 v2 (intake authority + lock capability amendments) is implemented and gate-closed. P4 MCP adapter implemented and merged. P5 remains unimplemented.',
    next_step: 'Remaining items: P5 operational hardening (trust command, sentry, audit sampling, runbook, dogfood).',
    blockers: [],
    evidence: {
      tests: { javascript: 85, rust: 151 },
      corpus: {
        accepted: 62,
        core: 8,
        research: 54,
        warnings: 226,
        exclusions: 1,
        errors: 0,
      },
      golden: {
        queries: 25,
        relevant_ids: 25,
        coverage_groups: 5,
        errors: 0,
        blake3: 'f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175',
      },
      thresholds: {
        schema_version: 'sovmem-retrieval-thresholds-v1',
        k: 5,
        minimum_queries_with_hit: 20,
        freeze_date: '2026-07-12',
      },
      verification: { level: 3, documents: 62, warnings: 226, errors: 0 },
      bundle_hash: '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
      retrieval: {
        baseline_blake3: 'c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6',
        bundle_hash: '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
        documents: 62,
        external_typed_links: 81,
        graph_edges: 19,
        graph_nodes: 62,
        index_config: '95a2237b666e0d6557709bb98eb2b4fe54a7478f6dca17df0bd3a14049e17f8d',
        k: 5,
        minimum_queries_with_hit: 20,
        queries: 25,
        queries_with_hit: 23,
      },
      implementation_commit: '614f02fbc9033913bf99a2f6561435526146b0a8',
    },
    phases: PHASES.map((phase, index) => ({
      ...phase,
      completion_percent: [100, 100, 100, 100, 100, 0][index],
      gate: index < 5 ? 'passed' : 'not-started',
      completed: index < 5 ? ['The formal execution-plan exit gate is passed.'] : [],
      remaining: index < 5 ? [] : ['The formal execution-plan exit gate is not passed.'],
      evidence: index === 0
        ? ['evals/audy-sovmem-v0/golden.jsonl']
        : index === 1
          ? ['sovmem/src/lib.rs']
          : index === 2 ? ['evals/audy-sovmem-v0/baseline.json']
          : index === 4
            ? ['sovmem/src/mcp/mod.rs', 'sovmem/src/mcp/tools.rs', 'sovmem/tests/mcp_integration.rs', 'sovmem/tests/mcp_stdio.rs']
          : ['sovmem/src/cli/p3.rs'],
    })),
    history: [
      {
        date: '2026-07-12',
        phase: 'P0/P1',
        summary: 'Closed the ground-truth and bundle-substrate gates and froze Q3.',
        commit: '614f02fbc9033913bf99a2f6561435526146b0a8',
      },
      {
        date: '2026-07-13',
        phase: 'P2',
        summary: 'Delivered the canonical P2 retrieval gate closure.',
        commit: '614f02fbc9033913bf99a2f6561435526146b0a8',
      },
      {
        date: '2026-07-27',
        phase: 'P3',
        summary: 'Delivered P3 v2 with intake-authority hardening, lock capability-boundary amendment, and safe-clone real-data proof. Master spec consolidation complete.',
        commit: '6d744194d82fb71850577996020c234cb90a3621',
      },
      {
        date: '2026-08-17',
        phase: 'P4',
        summary: 'Delivered P4 MCP adapter with raw JSON-RPC 2.0 over stdio, four tools (memory_search, memory_get, memory_propose, memory_status), 26 tests, feature-gated behind mcp feature.',
        commit: 'a9c8285c8431b7c9d2ee3dca20043ced067a7b47',
      },
    ],
  };
}

// The generator now cross-checks the reviewed state against the tracked
// evaluation artifacts, so fixtures must ship artifacts that agree with
// `validState()` rather than empty placeholders.
function validBaseline(state = validState()) {
  const { retrieval, corpus, golden } = state.evidence;
  const hits = retrieval.queries_with_hit;
  return {
    schema_version: 'sovmem-retrieval-baseline-v1',
    evaluation_set: {
      golden_blake3: golden.blake3,
      query_count: retrieval.queries,
      k: retrieval.k,
    },
    corpus: {
      bundle_hash: retrieval.bundle_hash,
      documents: corpus.accepted,
      core: corpus.core,
      research: corpus.research,
    },
    retrieval: { index_config: retrieval.index_config },
    index: {
      documents: retrieval.documents,
      graph_nodes: retrieval.graph_nodes,
      graph_edges: retrieval.graph_edges,
      external_typed_links: retrieval.external_typed_links,
    },
    metrics: {
      queries_with_hit: hits,
      gate: { minimum_queries_with_hit: retrieval.minimum_queries_with_hit, status: 'go' },
    },
    queries: Array.from({ length: retrieval.queries }, (_unused, index) => ({
      id: `q-${String(index + 1).padStart(3, '0')}`,
      hit_rank: index < hits ? 1 : null,
    })),
  };
}

function validThresholds(state = validState()) {
  const { thresholds, golden } = state.evidence;
  return {
    evaluation_set: { query_count: golden.queries, golden_blake3: golden.blake3 },
    retrieval: {
      k: thresholds.k,
      minimum_queries_with_hit: thresholds.minimum_queries_with_hit,
    },
    approval: { freeze_date: thresholds.freeze_date },
  };
}

function writeTrackedEvidence(root, { baseline = validBaseline(), thresholds = validThresholds() } = {}) {
  writeFileSync(
    join(root, 'evals/audy-sovmem-v0/baseline.json'),
    `${JSON.stringify(baseline, null, 2)}\n`,
  );
  writeFileSync(
    join(root, 'evals/audy-sovmem-v0/thresholds.json'),
    `${JSON.stringify(thresholds, null, 2)}\n`,
  );
}

function makeFingerprintFixture() {
  const root = mkdtempSync(join(tmpdir(), 'sovmem-build-audit-'));
  mkdirSync(join(root, 'sovmem/src'), { recursive: true });
  mkdirSync(join(root, 'sovmem/src/cli'), { recursive: true });
  mkdirSync(join(root, 'sovmem/src/mcp'), { recursive: true });
  mkdirSync(join(root, 'sovmem/target'), { recursive: true });
  mkdirSync(join(root, 'sovmem/tests'), { recursive: true });
  mkdirSync(join(root, 'evals/audy-sovmem-v0'), { recursive: true });
  mkdirSync(join(root, 'profiles/test'), { recursive: true });
  mkdirSync(join(root, 'var'), { recursive: true });
  writeFileSync(join(root, '.gitignore'), 'sovmem/target/\nvar/\n');
  writeFileSync(join(root, 'package.json'), '{}\n');
  writeFileSync(join(root, 'sovmem/Cargo.toml'), '[package]\nname = "fixture"\n');
  writeFileSync(join(root, 'sovmem/src/lib.rs'), 'pub fn initial() {}\n');
  writeFileSync(join(root, 'sovmem/src/cli/p3.rs'), '// P3 CLI fixture\n');
  writeFileSync(join(root, 'sovmem/src/mcp/mod.rs'), '// MCP module fixture\n');
  writeFileSync(join(root, 'sovmem/src/mcp/tools.rs'), '// MCP tools fixture\n');
  writeFileSync(join(root, 'sovmem/tests/mcp_integration.rs'), '// MCP integration test fixture\n');
  writeFileSync(join(root, 'sovmem/tests/mcp_stdio.rs'), '// MCP stdio test fixture\n');
  writeFileSync(join(root, 'evals/audy-sovmem-v0/golden.jsonl'), '{}\n');
  writeTrackedEvidence(root);
  writeFileSync(join(root, 'profiles/test/profile.yaml'), 'id: test\n');
  writeFileSync(join(root, 'sovmem/target/ignored'), 'generated\n');
  writeFileSync(join(root, 'var/ignored'), 'generated\n');
  symlinkSync('../var/ignored', join(root, 'sovmem/link'));
  execFileSync('git', ['init', '--quiet', root]);
  execFileSync('git', ['-C', root, 'add', '.']);
  execFileSync('git', ['-C', root, 'add', '-f', 'sovmem/target/ignored']);
  execFileSync('git', [
    '-C', root,
    '-c', 'user.name=SovMem Audit Test',
    '-c', 'user.email=audit@example.invalid',
    'commit', '--quiet', '-m', 'fixture',
  ]);
  return root;
}

function makeRepositoryFixture() {
  const root = makeFingerprintFixture();
  mkdirSync(join(root, 'docs/archive/audits'), { recursive: true });
  writeFileSync(
    join(root, 'docs/sovmem-build-state.json'),
    `${JSON.stringify(validState(), null, 2)}\n`,
  );
  return root;
}

function pathsFor(root) {
  return {
    root,
    statePath: join(root, 'docs/sovmem-build-state.json'),
    reportPath: join(root, 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md'),
  };
}

test('calculates the overall as 78 percent with P4 delivered and P5 unimplemented', () => {
  const state = validateState(validState());
  assert.equal(calculateOverall(state.phases), 78);
});

test('exposes root audit generation and check commands', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  assert.equal(pkg.scripts['audit:sovmem'], 'node scripts/generate-sovmem-build-audit.mjs');
  assert.equal(pkg.scripts['audit:sovmem:check'], 'node scripts/generate-sovmem-build-audit.mjs --check');
});

test('committed state and report match the approved v0.3 delivered state', () => {
  const state = validateState(JSON.parse(readFileSync('docs/sovmem-build-state.json', 'utf8')));
  assert.equal(calculateOverall(state.phases), 100);
  assert.deepEqual(
    state.phases.map(({ id, name, effort_weight, completion_percent, gate }) => ({
      id,
      name,
      effort_weight,
      completion_percent,
      gate,
    })),
    PHASES.map((phase, index) => ({
      ...phase,
      completion_percent: [100, 100, 100, 100, 100, 100][index],
      gate: index < 4 ? 'passed' : 'implementation_delivered',
    })),
  );
  assert.deepEqual(Object.keys(state.evidence.retrieval).sort(), [
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
  ].sort());
  assert.deepEqual(state.evidence.tests, { javascript: 85, rust: 690 });
  assert.deepEqual(state.evidence.corpus, {
    accepted: 62,
    core: 8,
    research: 54,
    warnings: 226,
    exclusions: 1,
    errors: 0,
  });
  assert.deepEqual(
    state.evidence.golden,
    {
      queries: 25,
      relevant_ids: 25,
      coverage_groups: 5,
      errors: 0,
      blake3: 'f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175',
    },
  );
  assert.deepEqual(state.evidence.thresholds, {
    schema_version: 'sovmem-retrieval-thresholds-v1',
    k: 5,
    minimum_queries_with_hit: 20,
    freeze_date: '2026-07-12',
  });
  assert.deepEqual(state.evidence.verification, {
    level: 3,
    documents: 62,
    warnings: 226,
    errors: 0,
  });
  assert.equal(
    state.evidence.bundle_hash,
    '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
  );
  assert.deepEqual(state.evidence.retrieval, {
    baseline_blake3: 'c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6',
    bundle_hash: '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
    documents: 62,
    external_typed_links: 81,
    graph_edges: 19,
    graph_nodes: 62,
    index_config: '95a2237b666e0d6557709bb98eb2b4fe54a7478f6dca17df0bd3a14049e17f8d',
    k: 5,
    minimum_queries_with_hit: 20,
    queries: 25,
    queries_with_hit: 23,
  });
  assert.match(state.evidence.implementation_commit, /^[0-9a-f]{40}$/);
  assert.deepEqual(state.phases.slice(3).map(phase => phase.completed.length > 0), [true, true, true]);
  assert.doesNotThrow(() => checkAudit({
    root: process.cwd(),
    statePath: 'docs/sovmem-build-state.json',
    reportPath: 'docs/archive/audits/SOVMEM-BUILD-AUDIT.md',
  }));
});

test('exports the literal normative phase contract', () => {
  assert.deepEqual(PHASES, EXPECTED_PHASES);
});

test('prevents consumers from mutating the exported phase contract', () => {
  const originalName = PHASES[0].name;
  const originalWeight = PHASES[1].effort_weight;
  try {
    assert.equal(Object.isFrozen(PHASES), true);
    assert.equal(PHASES.every(phase => Object.isFrozen(phase)), true);
    assert.throws(() => {
      PHASES[0].name = 'Consumer-defined phase';
    }, TypeError);
    assert.throws(() => {
      PHASES[1].effort_weight = 99;
    }, TypeError);
    assert.equal(PHASES[0].name, originalName);
    assert.equal(PHASES[1].effort_weight, originalWeight);
  } finally {
    if (PHASES[0].name !== originalName) {
      PHASES[0].name = originalName;
    }
    if (PHASES[1].effort_weight !== originalWeight) {
      PHASES[1].effort_weight = originalWeight;
    }
  }
});

test('returns the same state object after validation', () => {
  const state = validState();
  assert.equal(validateState(state), state);
});

test('throws AuditError with field-scoped diagnostics', () => {
  const state = validState();
  state.as_of = '12-07-2026';
  assert.throws(
    () => validateState(state),
    error => error instanceof AuditError
      && error.name === 'AuditError'
      && /^sovmem audit: as_of /.test(error.message),
  );
});

test('requires the exact ordered P0 through P5 contract', () => {
  const state = validState();
  state.phases.reverse();
  assert.throws(() => validateState(state), /phases must be ordered P0 through P5/);
});

test('rejects passed gates below 100 percent and passed gates without evidence', () => {
  const incomplete = validState();
  incomplete.phases[0].completion_percent = 99;
  assert.throws(() => validateState(incomplete), /passed gate requires 100 percent/);

  const unsupported = validState();
  unsupported.phases[0].completion_percent = 100;
  unsupported.phases[0].gate = 'passed';
  unsupported.phases[0].evidence = [];
  assert.throws(() => validateState(unsupported), /passed gate requires evidence/);
});

test('rejects unknown fields, malformed dates, invalid percentages, and unsorted blockers', () => {
  for (const mutate of [
    state => { state.unknown = true; },
    state => { state.as_of = '12-07-2026'; },
    state => { state.phases[0].completion_percent = 101; },
    state => { state.blockers = ['Z blocker', 'A blocker']; },
  ]) {
    const state = validState();
    mutate(state);
    assert.throws(() => validateState(state));
  }
});

test('rejects duplicate blockers and impossible calendar dates', () => {
  const duplicateBlockers = validState();
  duplicateBlockers.blockers = ['Duplicate blocker', 'Duplicate blocker'];
  assert.throws(() => validateState(duplicateBlockers), /blockers must be sorted and unique/);

  const impossibleDate = validState();
  impossibleDate.as_of = '2026-02-30';
  assert.throws(() => validateState(impossibleDate), /as_of must be a YYYY-MM-DD date/);
});

test('rejects invalid bundle and commit hashes', () => {
  for (const [mutate, field] of [
    [state => { state.evidence.bundle_hash = 'not-a-hash'; }, 'evidence.bundle_hash'],
    [state => { state.evidence.implementation_commit = 'not-a-commit'; }, 'evidence.implementation_commit'],
    [state => { state.history[0].commit = 'not-a-commit'; }, 'history[0].commit'],
  ]) {
    const state = validState();
    mutate(state);
    assert.throws(() => validateState(state), new RegExp(field.replaceAll(/[.[\]]/g, '\\$&')));
  }
});

test('requires exact test counts, frozen thresholds, and explicit level-three evidence', () => {
  for (const mutate of [
    state => { state.evidence.tests.javascript = 54; },
    state => { state.evidence.tests.rust = 5; },
    state => { state.evidence.thresholds.minimum_queries_with_hit = 19; },
    state => { state.evidence.verification.level = 2; },
    state => { state.evidence.verification.documents = 61; },
  ]) {
    const state = validState();
    mutate(state);
    assert.throws(() => validateState(state));
  }
});

test('rejects reviewed evidence that disagrees with the tracked evaluation artifacts', () => {
  // These values are no longer restated as literals in the validator; the
  // generator derives them from evals/audy-sovmem-v0/*.json instead. Each
  // mutation must therefore be caught by comparison against the real artifact.
  const mutations = [
    state => { state.evidence.golden.blake3 = '0'.repeat(64); },
    state => { state.evidence.retrieval.queries_with_hit = 24; },
    state => { state.evidence.retrieval.documents = 61; },
    state => { state.evidence.retrieval.graph_nodes = 61; },
    state => { state.evidence.retrieval.graph_edges = 20; },
    state => { state.evidence.retrieval.external_typed_links = 80; },
    state => { state.evidence.retrieval.bundle_hash = '0'.repeat(64); },
    state => { state.evidence.retrieval.index_config = '0'.repeat(64); },
    state => { state.evidence.corpus.core = 9; state.evidence.corpus.research = 53; },
    state => { state.evidence.retrieval.k = 6; },
  ];

  for (const mutate of mutations) {
    const root = makeRepositoryFixture();
    const state = validState();
    mutate(state);
    writeFileSync(
      join(root, 'docs/sovmem-build-state.json'),
      `${JSON.stringify(state, null, 2)}\n`,
    );
    assert.throws(() => writeAudit(pathsFor(root)), /sovmem audit:/);
  }
});

test('rejects a tracked baseline that contradicts its own query records', () => {
  const root = makeRepositoryFixture();
  const baseline = validBaseline();
  // Declared 23 hits, but every record reports a miss.
  baseline.queries = baseline.queries.map(record => ({ ...record, hit_rank: null }));
  writeTrackedEvidence(root, { baseline });

  assert.throws(() => writeAudit(pathsFor(root)), /sovmem audit:/);
});

test('rejects a missing or malformed tracked evaluation artifact', () => {
  for (const bytes of ['', 'not json', '{}\n']) {
    const root = makeRepositoryFixture();
    writeFileSync(join(root, 'evals/audy-sovmem-v0/baseline.json'), bytes);
    assert.throws(() => writeAudit(pathsFor(root)), /sovmem audit:/);
  }
});

test('requires exact canonical P2 retrieval evidence and the integer Q3 GO', () => {
  // Shape, format, and self-contradiction are decided without reading the
  // repository. Value drift against the tracked artifacts is covered by
  // 'rejects reviewed evidence that disagrees with the tracked evaluation
  // artifacts', which exercises the root-aware generator path.
  const mutations = [
    state => { state.evidence.retrieval.queries_with_hit = 19; },
    state => { state.evidence.retrieval.baseline_blake3 = 'not-a-blake3'; },
    state => { state.evidence.retrieval.index_config = 'not-a-config-hash'; },
    state => { state.phases[2].evidence = ['sovmem/src/profile/query.rs']; },
  ];

  for (const mutate of mutations) {
    const state = validState();
    mutate(state);
    assert.throws(() => validateState(state));
  }

  const provenanceMutations = [
    ['synchronized bundle hashes', state => {
      state.evidence.bundle_hash = '0'.repeat(64);
      state.evidence.retrieval.bundle_hash = '0'.repeat(64);
    }],
    ['P2 history commit', state => {
      state.history.find(entry => entry.phase === 'P2').commit = 'b'.repeat(40);
    }],
    ['missing P2 history entry', state => {
      state.history = state.history.filter(entry => entry.phase !== 'P2');
    }],
    ['duplicate P2 history entry', state => {
      state.history.push({ ...state.history.find(entry => entry.phase === 'P2') });
    }],
  ];
  const acceptedMutations = [];
  for (const [name, mutate] of provenanceMutations) {
    const state = validState();
    mutate(state);
    try {
      validateState(state);
      acceptedMutations.push(name);
    } catch (error) {
      assert.equal(error instanceof AuditError, true);
    }
  }
  assert.deepEqual(acceptedMutations, []);
});

test('rejects reviewed-state claims that GitHub controls are enforced', () => {
  const claims = [
    'Branch protection is enforced for this gate.',
    'Required checks are enforced for every merge.',
    'The merge queue is enforced for this repository.',
    'GitHub rulesets are enforced on main.',
    'Required status checks are enforced.',
    'Branch protections are enforced.',
    'Enforced GitHub rulesets protect main.',
    'No doubt branch protection is enforced.',
    'There is no question branch protection is enforced.',
    'Branch protection is not merely configured; it is enforced.',
    'Manual PR discipline only; branch protection, required status checks, rulesets, and merge queues are not enforced. Branch protection is enforced.',
    'GitHub controls are enforced.',
    'Manual PR discipline only; branch protection, required status checks, rulesets, and merge queues are not enforced. These controls are enforced.',
    'Those controls are enforced.',
    'Manual PR discipline only; branch protection, required status checks, rulesets, and merge queues are not enforced. The controls are enforced.',
    'Manual PR discipline only; branch protection, required status checks, rulesets, and merge queues are not enforced. This control is enforced.',
    'Controls are enforced.',
  ];
  const acceptedClaims = [];
  for (const claim of claims) {
    const state = validState();
    state.summary = claim;
    try {
      validateState(state);
      acceptedClaims.push(claim);
    } catch (error) {
      assert.match(error.message, /must not claim enforced GitHub controls/);
    }
  }
  assert.deepEqual(acceptedClaims, []);

  const disclaimer = validState();
  disclaimer.summary = 'Manual PR discipline only; branch protection, required status checks, rulesets, and merge queues are not enforced.';
  assert.doesNotThrow(() => validateState(disclaimer));
});

test('rejects nested unknown keys and invalid gates', () => {
  const nestedUnknown = validState();
  nestedUnknown.evidence.tests.unknown = 1;
  assert.throws(() => validateState(nestedUnknown), /evidence\.tests/);

  const invalidGate = validState();
  invalidGate.phases[0].gate = 'complete';
  assert.throws(() => validateState(invalidGate), /phases\[0\]\.gate must be a valid gate/);
});

test('rejects history entries that are not ordered by date', () => {
  const state = validState();
  state.history.push({
    date: '2026-07-11',
    phase: 'P0',
    summary: 'Published an earlier phase after the baseline.',
    commit: '426a8e7f5a648f8da48b45ce307d30660dd07e10',
  });
  assert.throws(() => validateState(state), /history must be ordered by date/);
});

test('fingerprints tracked SovMem inputs and ignores generated output', () => {
  const root = makeFingerprintFixture();
  assert.doesNotThrow(() => execFileSync(
    'git',
    ['-C', root, 'ls-files', '--error-unmatch', 'sovmem/target/ignored'],
    { stdio: 'ignore' },
  ));
  assert.deepEqual(
    listFingerprintFiles(root).map(path => path.toString('utf8')),
    [
      'evals/audy-sovmem-v0/baseline.json',
      'evals/audy-sovmem-v0/golden.jsonl',
      'evals/audy-sovmem-v0/thresholds.json',
      'package.json',
      'profiles/test/profile.yaml',
      'sovmem/Cargo.toml',
      'sovmem/link',
      'sovmem/src/cli/p3.rs',
      'sovmem/src/lib.rs',
      'sovmem/src/mcp/mod.rs',
      'sovmem/src/mcp/tools.rs',
      'sovmem/tests/mcp_integration.rs',
      'sovmem/tests/mcp_stdio.rs',
    ],
  );
  const first = fingerprintSourceTree(root);
  writeFileSync(join(root, 'evals/audy-sovmem-v0/baseline.json'), '{"changed":true}\n');
  const changedBaseline = fingerprintSourceTree(root);
  assert.notEqual(changedBaseline, first);
  writeFileSync(join(root, 'var/ignored'), 'changed');
  writeFileSync(join(root, 'sovmem/target/ignored'), 'changed');
  assert.equal(fingerprintSourceTree(root), changedBaseline);
  unlinkSync(join(root, 'sovmem/link'));
  symlinkSync('../var/changed', join(root, 'sovmem/link'));
  const changedLink = fingerprintSourceTree(root);
  assert.notEqual(changedLink, first);
  writeFileSync(join(root, 'sovmem/src/lib.rs'), 'pub fn changed() {}\n');
  assert.notEqual(fingerprintSourceTree(root), changedLink);
});

test('fingerprints and routes every SovMem input category, including supporting controls', t => {
  const root = makeFingerprintFixture();
  t.after(() => rmSync(root, { recursive: true, force: true }));
  // Independent examples, not values imported from the policy being tested.
  const protectedPaths = [
    'sovmem/src/lib.rs', 'sovmem/tests/new.rs', 'sovmem/tests/fixtures/new.json',
    'sovmem/examples/new.rs', 'sovmem/Cargo.toml', 'sovmem/Cargo.lock',
    'profiles/new/profile.yaml', 'evals/new/baseline.json',
    'test/sovmem-build-audit.test.js', 'test/sovmem-p3-safe-clone.test.js',
    'test/sovmem-new.test.js', 'test/retrieval-corpus-version.test.js',
    'package.json', 'package-lock.json', 'rust-toolchain.toml',
    'scripts/generate-sovmem-build-audit.mjs', 'scripts/lib/sovmem-build-audit.mjs',
    'scripts/lib/sovmem-inputs.mjs', 'scripts/sovmem-sentry.sh',
    'scripts/provision-sovmem-p3-safe-clone.mjs', 'scripts/verify-sovmem-p3-safe-clone.mjs',
    'scripts/run-js-tests.mjs', 
    '.github/scripts/sovmem-scope.mjs', '.github/scripts/sovmem-scope.test.mjs',
    '.github/workflows/sovmem-audit.yml', 'docs/sovmem-build-state.json',
    'docs/sovmem_v0.3_execution_plan.md', 'docs/spec/source-lineage-june-23-spec-bundle-verbatim.md',
  ];
  for (const path of protectedPaths) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), 'original\n');
  }
  execFileSync('git', ['-C', root, 'add', '-A']);
  const tracked = listFingerprintFiles(root).map(path => path.toString('utf8'));
  const before = fingerprintSourceTree(root);
  for (const path of protectedPaths) {
    assert.ok(tracked.includes(path), `${path} must be fingerprinted`);
    assert.equal(requiresSovmem([path]), true, `${path} must select regressions`);
    writeFileSync(join(root, path), 'changed\n');
    assert.notEqual(fingerprintSourceTree(root), before, `${path} content must affect the fingerprint`);
    writeFileSync(join(root, path), 'original\n');
  }
});

test('site and scaffold edits preserve audit freshness; mixed SovMem edits require regressions', t => {
  const root = makeRepositoryFixture();
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8' }).trim();
  const commit = () => {
    git('add', '-A');
    git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture');
    return git('rev-parse', 'HEAD');
  };
  const unrelated = [
    'test/site-migration.test.js', 'test/index.test.js', 'test/sovmem-fixtures/unrelated.test.js',
    'src/index.js', 'apps/site/src/app/page.tsx', 'apps/account/src/app/page.tsx',
    'packages/account-platform/src/index.ts', 'packages/runtime/src/index.ts',
    'packages/document-chat/src/index.js', 'scripts/verify-site.mjs',
    'docs/public-site/content-plan.md', 'README.md',
  ];
  for (const path of unrelated) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), 'original\n');
  }
  git('add', '-A');
  const options = pathsFor(root);
  writeAudit(options);
  const base = commit();
  const fingerprint = fingerprintSourceTree(root);
  const report = readFileSync(options.reportPath);
  for (const path of unrelated) writeFileSync(join(root, path), 'changed\n');
  // Exercise additions, deletions and renames outside the protected scope too.
  writeFileSync(join(root, 'test/new-site.test.js'), 'new website test\n');
  unlinkSync(join(root, 'test/index.test.js'));
  renameSync(join(root, 'README.md'), join(root, 'OVERVIEW.md'));
  const siteHead = commit();
  assert.equal(fingerprintSourceTree(root), fingerprint);
  assert.doesNotThrow(() => checkAudit(options));
  assert.deepEqual(readFileSync(options.reportPath), report);
  for (const event of ['pull_request', 'merge_group']) {
    assert.equal(selectRun({ event, base, head: siteHead, cwd: root }), false);
  }
  assert.equal(selectRun({ event: 'workflow_dispatch', base, head: siteHead, cwd: root }), true);

  writeFileSync(join(root, 'sovmem/src/lib.rs'), 'pub fn changed() {}\n');
  assert.throws(() => checkAudit(options), /stale/);
  const changedFingerprint = fingerprintSourceTree(root);
  writeAudit(options);
  assert.equal(fingerprintSourceTree(root), changedFingerprint, 'the report must not hash itself');
  const mixedHead = commit();
  assert.doesNotThrow(() => checkAudit(options));
  for (const event of ['pull_request', 'merge_group']) {
    assert.equal(selectRun({ event, base, head: mixedHead, cwd: root }), true);
  }
});

test('deleting or moving a SovMem test out of scope invalidates its audit and selects regressions', t => {
  for (const operation of ['delete', 'rename']) {
    const root = makeRepositoryFixture();
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const git = (...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8' }).trim();
    const commit = () => {
      git('add', '-A');
      git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture');
      return git('rev-parse', 'HEAD');
    };
    mkdirSync(join(root, 'test'));
    const source = join(root, 'test/sovmem-regression.test.js');
    writeFileSync(source, 'protected test\n');
    git('add', '-A');
    const options = pathsFor(root);
    writeAudit(options);
    const base = commit();
    if (operation === 'delete') unlinkSync(source);
    else renameSync(source, join(root, 'test/site-regression.test.js'));
    git('add', '-A');
    assert.throws(() => checkAudit(options), /stale/);
    writeAudit(options);
    const head = commit();
    for (const event of ['pull_request', 'merge_group']) {
      assert.equal(selectRun({ event, base, head, cwd: root }), true, operation);
    }
  }
});

test('sorts fingerprint paths as raw bytes and preserves non-UTF-8 names when supported', t => {
  const root = makeFingerprintFixture();
  writeFileSync(join(root, 'sovmem/é'), 'accent\n');
  writeFileSync(join(root, 'sovmem/Ω'), 'omega\n');

  const rawName = Buffer.from([0xff]);
  const rawPath = Buffer.concat([Buffer.from(`${root}/sovmem/`), rawName]);
  let rawNameSupported = true;
  try {
    writeFileSync(rawPath, 'raw\n');
  } catch {
    rawNameSupported = false;
    t.diagnostic('filesystem does not permit a non-UTF-8 fixture name');
  }
  execFileSync('git', ['-C', root, 'add', '-A']);

  const paths = listFingerprintFiles(root);
  assert.equal(paths.every(Buffer.isBuffer), true);
  for (let index = 1; index < paths.length; index += 1) {
    assert.ok(Buffer.compare(paths[index - 1], paths[index]) < 0);
  }
  assert.equal(paths.some(path => path.equals(Buffer.from('sovmem/é'))), true);
  assert.equal(paths.some(path => path.equals(Buffer.from('sovmem/Ω'))), true);
  if (rawNameSupported) {
    assert.equal(
      paths.some(path => path.equals(Buffer.concat([Buffer.from('sovmem/'), rawName]))),
      true,
    );
  }
});

test('renders stable Markdown with authority, evidence, history, and one newline', () => {
  const state = validateState(validState());
  const rendered = renderAudit(state, 'a'.repeat(64));
  assert.match(rendered, /Overall completion: 78%/);
  assert.match(rendered, /execution plan remains normative/);
  assert.match(rendered, /62 accepted documents/);
  assert.match(
    rendered,
    /## Verification evidence[\s\S]*### P0 — Scaffold & ground truth[\s\S]*evals\/audy-sovmem-v0\/golden\.jsonl/,
  );
  assert.match(rendered, /P0 \| Scaffold & ground truth \| 100%/);
  assert.match(rendered, /Level 3/);
  assert.match(rendered, /226 warnings/);
  assert.match(rendered, /1 exclusion/);
  assert.match(rendered, /sovmem-retrieval-thresholds-v1/);
  assert.match(rendered, /20 hit queries/);
  assert.match(rendered, /f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175/);
  assert.match(rendered, /23 of 25 queries.*k=5.*minimum 20/s);
  assert.match(rendered, /62 documents.*62 graph nodes.*19 graph edges.*81 external typed links/s);
  assert.match(rendered, /c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6/);
  assert.match(rendered, /95a2237b666e0d6557709bb98eb2b4fe54a7478f6dca17df0bd3a14049e17f8d/);
  assert.match(rendered, /Implementation commit/);
  assert.match(rendered, /Source-tree fingerprint: `a{64}`/);
  assert.equal(rendered.endsWith('\n'), true);
  assert.equal(rendered.endsWith('\n\n'), false);
  assert.doesNotMatch(rendered, /\/Users\/|\/Volumes\/|sovmem\/target|var\//);
});

test('labels test evidence as a fresh gate-closure baseline', () => {
  const rendered = renderAudit(validateState(validState()), 'a'.repeat(64));
  const gateClosureLabels = rendered.match(/Gate-closure verification baseline:/g);

  assert.equal(gateClosureLabels?.length, 2);
  assert.match(
    rendered,
    /## Snapshot[\s\S]*Gate-closure verification baseline:[^\n]*85 JavaScript[^\n]*151 Rust/,
  );
  assert.match(
    rendered,
    /## Verification evidence[\s\S]*Gate-closure verification baseline:[^\n]*85 JavaScript[^\n]*151 Rust/,
  );
});

test('renderer rejects state that has not passed the reviewed-state contract', () => {
  const state = validState();
  state.phases[0].completion_percent = 101;
  assert.throws(() => renderAudit(state, 'a'.repeat(64)), /completion_percent/);
});

test('rejects private absolute and environment-specific paths without echoing values', () => {
  const cases = [
    ['summary', state => { state.summary = 'Read /Users/alice/private/audy.md'; }, '/Users/alice/private/audy.md'],
    ['next_step', state => { state.next_step = 'Inspect /Volumes/vault/source'; }, '/Volumes/vault/source'],
    ['blockers[0]', state => { state.blockers = ['Blocked by C:\\Users\\alice\\vault']; }, 'C:\\Users\\alice\\vault'],
    ['phases[0].completed[0]', state => { state.phases[0].completed[0] = 'Copied from \\\\server\\share\\vault'; }, '\\\\server\\share\\vault'],
    ['phases[0].remaining[0]', state => { state.phases[0].remaining[0] = 'Inspect ~/vault'; }, '~/vault'],
    ['history[0].phase', state => { state.history[0].phase = '$HOME/vault'; }, '$HOME/vault'],
    ['summary', state => { state.summary = 'Source:/private/vault/a.md'; }, '/private/vault/a.md'],
    ['next_step', state => { state.next_step = 'AUDY_VAULT=/Users/name/vault'; }, '/Users/name/vault'],
    ['blockers[0]', state => { state.blockers = ['See [vault](/Volumes/audy/private.md)']; }, '/Volumes/audy/private.md'],
    ['phases[0].completed[0]', state => { state.phases[0].completed[0] = 'See <C:\\Users\\name\\vault>'; }, 'C:\\Users\\name\\vault'],
    ['history[0].summary', state => { state.history[0].summary = 'See <\\\\server\\share\\vault>'; }, '\\\\server\\share\\vault'],
    ['summary', state => { state.summary = 'Source:<file:///private/vault/a.md>'; }, 'file:///private/vault/a.md'],
    ['summary', state => { state.summary = '//Users/alice/private.md'; }, '//Users/alice/private.md'],
    ['next_step', state => { state.next_step = '//Volumes/audy/private.md'; }, '//Volumes/audy/private.md'],
  ];

  for (const [field, mutate, secret] of cases) {
    const state = validState();
    mutate(state);
    assert.throws(
      () => validateState(state),
      error => error instanceof AuditError
        && error.message.includes(field)
        && !error.message.includes(secret),
    );
  }
});

test('rejects generated-output references and unsafe control characters in rendered fields', () => {
  const cases = [
    ['phases[0].evidence[0]', state => { state.phases[0].evidence[0] = 'var/private-output.json'; }],
    ['history[0].summary', state => { state.history[0].summary = 'Built sovmem/target/debug/tool'; }],
    ['blockers[0]', state => { state.blockers = ['unsafe\u0000value']; }],
    ['next_step', state => { state.next_step = 'unsafe\u001bvalue'; }],
    ['summary', state => { state.summary = 'unsafe\tvalue'; }],
    ['summary', state => { state.summary = 'Output:./var/private.json'; }],
    ['next_step', state => { state.next_step = 'Evidence:./sovmem/target/debug/tool'; }],
    ['summary', state => { state.summary = '././var/private.json'; }],
    ['next_step', state => { state.next_step = '././sovmem/target/debug/tool'; }],
    ['history[0].summary', state => { state.history[0].summary = 'sovmem/./target/debug/tool'; }],
  ];

  for (const [field, mutate] of cases) {
    const state = validState();
    mutate(state);
    assert.throws(
      () => validateState(state),
      error => error instanceof AuditError && error.message.includes(field),
    );
  }
});

test('rejects parent-directory path segments without echoing traversing values', () => {
  const cases = [
    ['summary', state => { state.summary = '../var/private.json'; }, '../var/private.json'],
    ['next_step', state => { state.next_step = 'foo/../sovmem/target/debug/tool'; }, 'foo/../sovmem/target/debug/tool'],
    ['phases[0].evidence[0]', state => { state.phases[0].evidence[0] = 'foo/../evals/audy-sovmem-v0/untracked.jsonl'; }, 'foo/../evals/audy-sovmem-v0/untracked.jsonl'],
    ['blockers[0]', state => { state.blockers = ['..\\var\\private.json']; }, '..\\var\\private.json'],
    ['history[0].summary', state => { state.history[0].summary = 'foo\\..\\private\\vault.md'; }, 'foo\\..\\private\\vault.md'],
  ];

  for (const [field, mutate, privateValue] of cases) {
    const state = validState();
    mutate(state);
    assert.throws(
      () => validateState(state),
      error => error instanceof AuditError
        && error.message.includes(field)
        && !error.message.includes(privateValue),
    );
  }
});

test('allows ellipsis prose, version text, and dots inside filenames', () => {
  const state = validState();
  state.summary = 'Still reviewing... version v1..2 keeps report..draft.md unchanged.';
  state.next_step = 'Compare release..candidate and ordinary ... prose.';
  state.phases[0].evidence = ['evals/audy-sovmem-v0/golden.jsonl'];
  assert.doesNotThrow(() => validateState(state));
});

test('allows HTTPS URLs, ordinary slash prose, and approved relative evidence', () => {
  const state = validState();
  state.summary = 'See https://example.test/Users/public/P0/P1 and ordinary P0/P1 prose.';
  state.next_step = 'Compare P0/P1 before P2/P3.';
  state.phases[0].evidence = [
    'evals/audy-sovmem-v0/golden.jsonl',
    './profiles/test/profile.yaml',
  ];
  assert.doesNotThrow(() => validateState(state));

  const root = makeRepositoryFixture();
  writeFileSync(pathsFor(root).statePath, `${JSON.stringify(state, null, 2)}\n`);
  assert.doesNotThrow(() => writeAudit(pathsFor(root)));
});

test('normalizes redundant dot segments before comparing tracked phase evidence', () => {
  const passing = [
    '././evals/audy-sovmem-v0/golden.jsonl',
    'evals/./audy-sovmem-v0/./golden.jsonl',
  ];
  for (const evidence of passing) {
    const root = makeRepositoryFixture();
    const state = validState();
    state.phases[0].evidence = [evidence];
    writeFileSync(pathsFor(root).statePath, `${JSON.stringify(state, null, 2)}\n`);
    assert.doesNotThrow(() => writeAudit(pathsFor(root)));
  }

  const root = makeRepositoryFixture();
  const state = validState();
  state.phases[0].evidence = ['././evals/audy-sovmem-v0/untracked.jsonl'];
  writeFileSync(pathsFor(root).statePath, `${JSON.stringify(state, null, 2)}\n`);
  assert.throws(
    () => writeAudit(pathsFor(root)),
    /phases\[0\]\.evidence\[0\] must reference only Git-tracked fingerprint inputs/,
  );
});

test('rejects ambiguous Unicode and non-UTF-8 reviewed state strings', () => {
  const state = validState();
  state.summary = '\ud800';
  assert.throws(() => validateState(state), /summary must contain unambiguous UTF-8 text/);

  const root = makeRepositoryFixture();
  const options = pathsFor(root);
  const bytes = readFileSync(options.statePath);
  const marker = bytes.indexOf(Buffer.from('P0, P1, and P2 gates are closed'));
  assert.ok(marker >= 0);
  writeFileSync(options.statePath, Buffer.concat([
    bytes.subarray(0, marker),
    Buffer.from([0xff]),
    bytes.subarray(marker + 1),
  ]));
  assert.throws(() => writeAudit(options), /reviewed state file is not valid UTF-8/);
});

test('requires repository-path phase evidence to be tracked', () => {
  const root = makeRepositoryFixture();
  const state = validState();
  state.phases[0].evidence[0] = 'evals/audy-sovmem-v0/untracked.jsonl';
  writeFileSync(pathsFor(root).statePath, `${JSON.stringify(state, null, 2)}\n`);
  assert.throws(
    () => writeAudit(pathsFor(root)),
    /phases\[0\]\.evidence\[0\] must reference only Git-tracked fingerprint inputs/,
  );
});

test('requires each phase evidence entry to be exactly one tracked fingerprint path', () => {
  const passing = [
    'evals/audy-sovmem-v0/golden.jsonl',
    './evals/audy-sovmem-v0/golden.jsonl',
    'evals/./audy-sovmem-v0/./golden.jsonl',
    'evals/audy-sovmem-v0/baseline.json',
    'sovmem/src/lib.rs',
    'package.json',
    '././package.json',
    'sovmem/Cargo.toml',
    './sovmem/./Cargo.toml',
    'sovmem\\Cargo.toml',
  ];
  for (const evidence of passing) {
    const root = makeRepositoryFixture();
    const state = validState();
    state.phases[0].evidence = [evidence];
    writeFileSync(pathsFor(root).statePath, `${JSON.stringify(state, null, 2)}\n`);
    assert.doesNotThrow(() => writeAudit(pathsFor(root)));
  }

  const failing = [
    './evals/audy-sovmem-v0/untracked.jsonl',
    'Run node evals/audy-sovmem-v0/untracked.jsonl',
    'Compare evals/audy-sovmem-v0/golden.jsonl and profiles/test/untracked.yaml',
    'notes/untracked.md',
    'evals/audy-sovmem-v0/golden.jsonl and notes/untracked.md',
    'Cargo.toml',
    'package.json and notes.txt',
    'package.json and results.csv',
    'package.json and helper.py',
    'package.json and verify.sh',
    'LICENSE',
    'No repository artifact recorded',
    'Run node evals/audy-sovmem-v0/golden.jsonl',
    'Compare (evals/audy-sovmem-v0/golden.jsonl), ./profiles/test/profile.yaml.',
    'Inspect sovmem/src/lib.rs and package.json',
    'See https://example.test/notes/untracked.md and evals/audy-sovmem-v0/golden.jsonl',
  ];
  const acceptedEvidence = [];
  for (const evidence of failing) {
    const root = makeRepositoryFixture();
    const state = validState();
    state.phases[0].evidence = [evidence];
    writeFileSync(pathsFor(root).statePath, `${JSON.stringify(state, null, 2)}\n`);
    try {
      writeAudit(pathsFor(root));
      acceptedEvidence.push(evidence);
    } catch (error) {
      assert.match(
        error.message,
        /phases\[0\]\.evidence\[0\] must reference only Git-tracked fingerprint inputs/,
      );
    }
  }
  assert.deepEqual(acceptedEvidence, []);
});

test('history table safely escapes pipes after backslashes and normalizes line endings', () => {
  const state = validState();
  state.history[0].summary = 'Path \\| split\r\nnext\nlast';
  const rendered = renderAudit(state, 'a'.repeat(64));
  assert.match(rendered, /Path \\&#124; split<br>next<br>last/);
  assert.doesNotMatch(rendered, /Path \\\| split/);
});

test('check mode rejects a missing or stale report', () => {
  const root = makeRepositoryFixture();
  assert.throws(() => checkAudit(pathsFor(root)), /missing/);
  writeFileSync(pathsFor(root).reportPath, 'stale\n');
  assert.throws(() => checkAudit(pathsFor(root)), /stale/);
  writeAudit(pathsFor(root));
  assert.doesNotThrow(() => checkAudit(pathsFor(root)));
});

test('check mode compares exact bytes instead of replacement-decoded text', () => {
  const root = makeRepositoryFixture();
  const options = pathsFor(root);
  const state = validState();
  state.summary = 'Reviewed replacement marker: �';
  writeFileSync(options.statePath, `${JSON.stringify(state, null, 2)}\n`);
  writeAudit(options);

  const report = readFileSync(options.reportPath);
  const marker = Buffer.from('�');
  const markerIndex = report.indexOf(marker);
  assert.ok(markerIndex >= 0);
  writeFileSync(options.reportPath, Buffer.concat([
    report.subarray(0, markerIndex),
    Buffer.from([0xff]),
    report.subarray(markerIndex + marker.length),
  ]));
  assert.throws(() => checkAudit(options), /stale/);
});

test('wires a read-only pull-request audit workflow', () => {
  const workflow = readFileSync('.github/workflows/sovmem-audit.yml', 'utf8');
  assertCanonicalWorkflow(workflow);
});

test('registers the audit artifacts and phase publication rule', () => {
  const index = readFileSync('index.md', 'utf8');
  const plan = readFileSync('docs/sovmem_v0.3_execution_plan.md', 'utf8');
  const implementationPlan = readFileSync(
    'docs/archive/plans/2026-07-12-sovmem-build-audit.md',
    'utf8',
  );
  assertAuditRegistry(index);
  assertExecutorPublicationRule(plan);
  assert.match(implementationPlan, /Expected: 12 tests pass, 0 fail\./);
  const design = readFileSync(
    'docs/archive/specs/2026-07-12-sovmem-build-audit-design.md',
    'utf8',
  );
  assert.match(design, /2026-07-12 gate-closure supersession/);
  assert.match(
    design,
    new RegExp(MANUAL_PUBLICATION_RULE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
  );
});

test('workflow contract rejects write access, reordered steps, and extra jobs', () => {
  const workflow = readFileSync('.github/workflows/sovmem-audit.yml', 'utf8');
  assert.doesNotThrow(() => assertCanonicalWorkflow(workflow));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('  workflow_dispatch:\n', ''),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('    needs: scope', '    needs: []'),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace("    if: needs.scope.outputs.full == 'true'", '    if: false'),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('contents: read', 'contents: write'),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('  contents: read', '  contents: read\n  pull-requests: write'),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace(
      '      - run: npm ci\n      - run: npm run audit:sovmem:check',
      '      - run: npm run audit:sovmem:check\n      - run: npm ci',
    ),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('      - run: npm ci', '      # - run: npm ci'),
  ));
  // Formatting, lints, the pinned toolchain, and the cold-build budget are part
  // of the contract: silently dropping any of them must fail the check.
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace(
      '        run: cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --locked --features test-support -- -D warnings',
      '        run: cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --locked',
    ),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('cargo fmt --manifest-path sovmem/Cargo.toml --check', 'true'),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('timeout-minutes: 40', 'timeout-minutes: 20'),
  ));
  assert.throws(() => assertCanonicalWorkflow(
    workflow.replace('      - uses: Swatinem/rust-cache@v2\n', ''),
  ));
  assert.throws(() => assertCanonicalWorkflow(`${workflow}\n  publish:\n    permissions: write-all\n`));
});

test('registry contract rejects prose-only, incomplete, and duplicate artifact entries', () => {
  const index = readFileSync('index.md', 'utf8');
  assert.doesNotThrow(() => assertAuditRegistry(index));
  assert.throws(() => assertAuditRegistry(
    index.replace('| [SovMem build audit report]', 'SovMem build audit report'),
  ));
  assert.throws(() => assertAuditRegistry(
    index.replace('Markdown / derived generated evidence', 'Markdown / canonical'),
  ));
  assert.throws(() => assertAuditRegistry(
    index.replace(
      '| [SovMem build audit state](docs/sovmem-build-state.json) | JSON / approved reviewed state | Supplies the human-reviewed P3 closure, safe-clone evidence, and frozen P2 preservation record from which the v4 report is generated. |',
      '| [SovMem build audit state](docs/sovmem-build-state.json) | JSON / approved reviewed state |',
    ),
  ));
  const workflowRow = index.split('\n').find(line => (
    line.startsWith('| [') && line.includes('(.github/workflows/sovmem-audit.yml)')
  ));
  assert.throws(() => assertAuditRegistry(`${index}\n${workflowRow}\n`));
});

test('registry contract rejects audit artifacts moved outside their owning sections', () => {
  const index = readFileSync('index.md', 'utf8');
  const designRow = index.split('\n').find(line => (
    line.startsWith('| [')
      && line.includes('(docs/archive/specs/2026-07-12-sovmem-build-audit-design.md)')
  ));
  const libraryRow = index.split('\n').find(line => (
    line.startsWith('| [') && line.includes('(scripts/lib/sovmem-build-audit.mjs)')
  ));

  const designInSovmem = index
    .replace(`${designRow}\n`, '')
    .replace(
      '#### Build audit and publication gate',
      `#### Build audit and publication gate\n${designRow}`,
    );
  assert.throws(() => assertAuditRegistry(designInSovmem));

  const libraryInPlanning = index
    .replace(`${libraryRow}\n`, '')
    .replace(
      '\n### sovmem — memory and judgment',
      `\n${libraryRow}\n\n### sovmem — memory and judgment`,
    );
  assert.throws(() => assertAuditRegistry(libraryInPlanning));
});

test('executor protocol contract rejects rule 9 outside its owning section', () => {
  const plan = readFileSync('docs/sovmem_v0.3_execution_plan.md', 'utf8');
  assert.doesNotThrow(() => assertExecutorPublicationRule(plan));
  const withoutRule = plan.replace(`${EXECUTOR_PUBLICATION_RULE}\n`, '');
  assert.throws(() => assertExecutorPublicationRule(
    `${withoutRule}\n${EXECUTOR_PUBLICATION_RULE}\n`,
  ));
  assert.throws(() => assertExecutorPublicationRule(
    `${plan}\n${EXECUTOR_PUBLICATION_RULE}\n`,
  ));
});
