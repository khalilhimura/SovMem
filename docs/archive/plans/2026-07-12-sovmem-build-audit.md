> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem Build Audit Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a deterministic SovMem v0.3 audit report whose reviewed phase state, weighted completion, evidence, and source-tree fingerprint must be current before a relevant pull request can merge.

**Architecture:** A dependency-free Node library validates reviewed JSON state, computes the execution-plan-weighted percentage, fingerprints the tracked SovMem surface, and renders stable Markdown. A thin CLI writes or checks the report; Node tests prove fail-closed behavior, while a GitHub pull-request workflow runs the freshness check and the full repository suite.

**Tech Stack:** Node.js 20 ESM and standard library, `node:test`, Git CLI, Markdown, JSON, GitHub Actions, existing Rust/Node verification commands.

## Global Constraints

- `docs/sovmem_v0.3_execution_plan.md` remains the normative build authority.
- The report is generated evidence and navigation; it cannot promote a phase or alter a SovMem runtime decision.
- Overall completion uses fixed weights P0=1, P1=2, P2=2, P3=3.5, P4=2, P5=3, total 13.5.
- Completion percentages are reviewed integers from 0 through 100; code never infers semantic completion.
- The report and diagnostics contain no Audy Vault body text, absolute private path, generated `var/` output, or `sovmem/target/` output.
- Use only Node standard-library modules; add no package dependency.
- Every production-code task follows red-green-refactor and ends with an independently reviewable commit.

---

## File structure

| File | Responsibility |
| --- | --- |
| `scripts/lib/sovmem-build-audit.mjs` | Validate state, compute completion, enumerate/fingerprint tracked inputs, render Markdown, compare/write exact bytes. |
| `scripts/generate-sovmem-build-audit.mjs` | Parse `--check`, locate repository files, call the library, print concise success/failure, and return stable exit codes. |
| `docs/sovmem-build-state.json` | Human-reviewed P0-P5 state, evidence, blockers, next step, and publication history. |
| `docs/SOVMEM-BUILD-AUDIT.md` | Deterministically generated primary audit reference. |
| `test/sovmem-build-audit.test.js` | Unit, integration, privacy, wiring, and stale-report tests. |
| `.github/workflows/sovmem-audit.yml` | Read-only pull-request and merge-group freshness and regression gate. |
| `package.json` | Root generation and check commands. |
| `index.md` | Artifact registry entries and usage routing. |
| `docs/sovmem_v0.3_execution_plan.md` | Phase-publication audit rule in the executor protocol. |

---

### Task 1: State contract and weighted completion

**Files:**
- Create: `scripts/lib/sovmem-build-audit.mjs`
- Create: `test/sovmem-build-audit.test.js`

**Interfaces:**
- Produces: `PHASES`, `validateState(value)`, and `calculateOverall(phases)`.
- `validateState` returns the same validated object and throws `AuditError` with a public field-scoped message.
- `calculateOverall` returns `Math.round(sum(weight * percent) / 13.5)`.

- [ ] **Step 1: Write failing state and calculation tests**

Create `test/sovmem-build-audit.test.js` with a `validState()` fixture containing exact P0-P5 rows and these tests:

```js
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  PHASES,
  calculateOverall,
  validateState,
} from '../scripts/lib/sovmem-build-audit.mjs';

function validState() {
  return {
    schema_version: 'sovmem-build-audit-v1',
    spec_version: 'v0.3',
    as_of: '2026-07-12',
    summary: 'A tested read-only foundation exists; retrieval and the sovereign write-back loop remain unbuilt.',
    next_step: 'Close P0/P1 contract gaps, freeze Q3 thresholds, then implement P2.',
    blockers: ['Q3 retrieval thresholds are not frozen.'],
    evidence: {
      tests: { javascript: 6, rust: 185 },
      corpus: { accepted: 62, core: 8, research: 54, errors: 0 },
      golden: { queries: 25, relevant_ids: 25, coverage_groups: 5, errors: 0 },
      bundle_hash: '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
      merge_commit: '326a8e7f5a648f8da48b45ce307d30660dd07e10',
    },
    phases: PHASES.map((phase, index) => ({
      ...phase,
      completion_percent: [85, 55, 10, 0, 0, 0][index],
      gate: index < 3 ? 'in-progress' : 'not-started',
      completed: index === 0 ? ['Ground-truth corpus and human golden queries exist.'] : [],
      remaining: ['The formal execution-plan exit gate is not passed.'],
      evidence: index === 0 ? ['evals/audy-sovmem-v0/golden.jsonl'] : [],
    })),
    history: [{
      date: '2026-07-12',
      phase: 'baseline',
      summary: 'Recorded the first merged corpus and golden-query baseline.',
      commit: '326a8e7f5a648f8da48b45ce307d30660dd07e10',
    }],
  };
}

test('calculates the approved weighted baseline as 16 percent', () => {
  const state = validateState(validState());
  assert.equal(calculateOverall(state.phases), 16);
});

test('requires the exact ordered P0 through P5 contract', () => {
  const state = validState();
  state.phases.reverse();
  assert.throws(() => validateState(state), /phases must be ordered P0 through P5/);
});

test('rejects passed gates below 100 percent and passed gates without evidence', () => {
  const incomplete = validState();
  incomplete.phases[0].gate = 'passed';
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
```

- [ ] **Step 2: Run the test and verify RED**

Run: `node --test test/sovmem-build-audit.test.js`

Expected: FAIL with `ERR_MODULE_NOT_FOUND` for `scripts/lib/sovmem-build-audit.mjs`.

- [ ] **Step 3: Implement the state contract**

Create `scripts/lib/sovmem-build-audit.mjs` with:

```js
export const PHASES = Object.freeze([
  Object.freeze({ id: 'P0', name: 'Scaffold & ground truth', effort_weight: 1 }),
  Object.freeze({ id: 'P1', name: 'Bundle substrate', effort_weight: 2 }),
  Object.freeze({ id: 'P2', name: 'Derived index & query', effort_weight: 2 }),
  Object.freeze({ id: 'P3', name: 'Verified write-back loop', effort_weight: 3.5 }),
  Object.freeze({ id: 'P4', name: 'MCP adapter', effort_weight: 2 }),
  Object.freeze({ id: 'P5', name: 'Hardening, sentry, trust instrumentation, dogfood', effort_weight: 3 }),
]);

export class AuditError extends Error {
  constructor(message) {
    super(`sovmem audit: ${message}`);
    this.name = 'AuditError';
  }
}

export function calculateOverall(phases) {
  const weighted = phases.reduce(
    (sum, phase) => sum + phase.effort_weight * phase.completion_percent,
    0,
  );
  return Math.round(weighted / 13.5);
}
```

Add strict helpers `assertObject`, `assertKeys`, `assertString`, `assertStringArray`, `assertInteger`, `assertDate`, and `assertCommit`. `validateState` must enforce the exact top-level, evidence, phase, and history keys from the approved design; exact phase constants; valid gates; sorted unique blockers; SHA-256 bundle hash; 40-character commit hashes; and passed-gate consistency. Diagnostics name only the invalid public field.

- [ ] **Step 4: Run tests and verify GREEN**

Run: `node --test test/sovmem-build-audit.test.js`

Expected: 12 tests pass, 0 fail.

- [ ] **Step 5: Commit Task 1**

```bash
git add scripts/lib/sovmem-build-audit.mjs test/sovmem-build-audit.test.js
git commit -m "test: lock SovMem audit state contract"
```

---

### Task 2: Fingerprint, renderer, and stale-report CLI

**Files:**
- Modify: `scripts/lib/sovmem-build-audit.mjs`
- Create: `scripts/generate-sovmem-build-audit.mjs`
- Modify: `test/sovmem-build-audit.test.js`

**Interfaces:**
- Produces: `listFingerprintFiles(root)`, `fingerprintSourceTree(root)`, `renderAudit(state, fingerprint)`, `writeAudit(options)`, and `checkAudit(options)`.
- Fingerprint inputs are Git-tracked files in the exact approved surface.
- CLI exit code is 0 for success and 1 for validation, generation, or stale-report failure.

- [ ] **Step 1: Write failing fingerprint and rendering tests**

Add tests that create a temporary Git repository with `package.json`, `sovmem/src/lib.rs`, ignored `sovmem/target/ignored`, and `var/ignored`; commit the tracked inputs; then assert:

```js
test('fingerprints tracked SovMem inputs and ignores generated output', () => {
  const root = makeFingerprintFixture();
  const first = fingerprintSourceTree(root);
  writeFileSync(join(root, 'var/ignored'), 'changed');
  writeFileSync(join(root, 'sovmem/target/ignored'), 'changed');
  assert.equal(fingerprintSourceTree(root), first);
  writeFileSync(join(root, 'sovmem/src/lib.rs'), 'pub fn changed() {}
');
  assert.notEqual(fingerprintSourceTree(root), first);
});

test('renders stable Markdown with authority, evidence, history, and one newline', () => {
  const state = validateState(validState());
  const rendered = renderAudit(state, 'a'.repeat(64));
  assert.match(rendered, /Overall completion: 16%/);
  assert.match(rendered, /execution plan remains normative/);
  assert.match(rendered, /62 accepted documents/);
  assert.match(rendered, /P0 \| Scaffold & ground truth \| 85%/);
  assert.match(rendered, /Source-tree fingerprint: `a{64}`/);
  assert.equal(rendered.endsWith('
'), true);
  assert.equal(rendered.endsWith('

'), false);
  assert.doesNotMatch(rendered, /\/Users\/|\/Volumes\/|sovmem\/target|var\//);
});

test('check mode rejects a missing or stale report', () => {
  const root = makeRepositoryFixture();
  assert.throws(() => checkAudit(pathsFor(root)), /missing/);
  writeFileSync(join(root, 'docs/SOVMEM-BUILD-AUDIT.md'), 'stale
');
  assert.throws(() => checkAudit(pathsFor(root)), /stale/);
  writeAudit(pathsFor(root));
  assert.doesNotThrow(() => checkAudit(pathsFor(root)));
});
```

- [ ] **Step 2: Run focused tests and verify RED**

Run: `node --test test/sovmem-build-audit.test.js`

Expected: FAIL because fingerprint, render, write, and check exports do not exist.

- [ ] **Step 3: Implement fingerprint and rendering**

Use `execFileSync('git', ['-C', root, 'ls-files', '-z', '--', ...pathspecs])`, split NUL-delimited paths, filter exact approved roots/files, sort with bytewise `<`/`>` comparison, and reject an empty input set. Hash framed path and content bytes with `createHash('sha256')`.

Render these fixed sections in order:

1. `# SovMem v0.3 Build Audit`
2. generated and authority notices;
3. `## Snapshot` with overall percentage, review date, test total, and fingerprint;
4. `## Phase breakdown` table;
5. `## Current state`;
6. `## Completed capabilities` grouped by phase;
7. `## Remaining work` grouped by phase;
8. `## Verification evidence`;
9. `## Active blockers`;
10. `## Recommended next step`;
11. `## Phase publication history`;
12. `## Update contract` with both npm commands.

Escape pipe characters and newlines in table cells. Render `None recorded.` when a reviewed list is empty. Never derive semantic phase claims beyond the state file.

- [ ] **Step 4: Implement the CLI**

Create `scripts/generate-sovmem-build-audit.mjs`:

```js
#!/usr/bin/env node
import { resolve } from 'node:path';
import { checkAudit, writeAudit } from './lib/sovmem-build-audit.mjs';

const root = resolve(import.meta.dirname, '..');
const options = {
  root,
  statePath: resolve(root, 'docs/sovmem-build-state.json'),
  reportPath: resolve(root, 'docs/SOVMEM-BUILD-AUDIT.md'),
};

try {
  if (process.argv.length === 3 && process.argv[2] === '--check') {
    checkAudit(options);
    process.stdout.write('SovMem build audit is current.
');
  } else if (process.argv.length === 2) {
    writeAudit(options);
    process.stdout.write('Updated docs/SOVMEM-BUILD-AUDIT.md.
');
  } else {
    throw new Error('usage: generate-sovmem-build-audit.mjs [--check]');
  }
} catch (error) {
  process.stderr.write(`${error.message}
`);
  process.exitCode = 1;
}
```

- [ ] **Step 5: Run tests and verify GREEN**

Run: `node --test test/sovmem-build-audit.test.js`

Expected: all focused tests pass with no absolute-path or generated-output leakage.

- [ ] **Step 6: Commit Task 2**

```bash
git add scripts/lib/sovmem-build-audit.mjs scripts/generate-sovmem-build-audit.mjs test/sovmem-build-audit.test.js
git commit -m "feat: generate deterministic SovMem audit report"
```

---

### Task 3: Baseline state, report, and root commands

**Files:**
- Create: `docs/sovmem-build-state.json`
- Create: `docs/SOVMEM-BUILD-AUDIT.md`
- Modify: `package.json:7-17`
- Modify: `test/sovmem-build-audit.test.js`

**Interfaces:**
- Produces root commands `audit:sovmem` and `audit:sovmem:check`.
- Establishes the approved 16% baseline and exact live evidence.

- [ ] **Step 1: Add failing repository-wiring tests**

```js
test('exposes root audit generation and check commands', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  assert.equal(pkg.scripts['audit:sovmem'], 'node scripts/generate-sovmem-build-audit.mjs');
  assert.equal(pkg.scripts['audit:sovmem:check'], 'node scripts/generate-sovmem-build-audit.mjs --check');
});

test('committed state and report match the approved baseline', () => {
  const state = validateState(JSON.parse(readFileSync('docs/sovmem-build-state.json', 'utf8')));
  assert.equal(calculateOverall(state.phases), 16);
  assert.equal(state.evidence.tests.javascript, 6);
  assert.equal(state.evidence.tests.rust, 185);
  assert.doesNotThrow(() => checkAudit({
    root: process.cwd(),
    statePath: 'docs/sovmem-build-state.json',
    reportPath: 'docs/SOVMEM-BUILD-AUDIT.md',
  }));
});
```

- [ ] **Step 2: Verify RED**

Run: `node --test test/sovmem-build-audit.test.js`

Expected: FAIL because root scripts, state, and report do not exist.

- [ ] **Step 3: Add reviewed baseline state**

Create the state with P0=85, P1=55, P2=10, P3-P5=0; gates P0-P2 `in-progress`, P3-P5 `not-started`; the approved 62/8/54 corpus, 25-query/five-group golden set, 6/185 tests, bundle hash, merge commit, Q3 blocker, and baseline history. Populate each phase's completed, remaining, and evidence arrays from the live audit summary and execution-plan gate.

- [ ] **Step 4: Wire commands and generate the report**

Add to `package.json`:

```json
"audit:sovmem": "node scripts/generate-sovmem-build-audit.mjs",
"audit:sovmem:check": "node scripts/generate-sovmem-build-audit.mjs --check"
```

Run: `npm run audit:sovmem && npm run audit:sovmem:check`

Expected: report updated, then `SovMem build audit is current.`

- [ ] **Step 5: Run focused and JavaScript tests**

Run: `npm run test:js`

Expected: all JavaScript tests pass.

- [ ] **Step 6: Commit Task 3**

```bash
git add docs/sovmem-build-state.json docs/SOVMEM-BUILD-AUDIT.md package.json test/sovmem-build-audit.test.js
git commit -m "docs: establish SovMem build audit baseline"
```

---

### Task 4: Pull-request gate and publication contract

**Files:**
- Create: `.github/workflows/sovmem-audit.yml`
- Modify: `test/sovmem-build-audit.test.js`
- Modify: `index.md:75-175`
- Modify: `docs/sovmem_v0.3_execution_plan.md:235-251`

**Interfaces:**
- Produces read-only GitHub check `SovMem audit / verify` for pull requests to `main` and merge groups.
- Registers all material artifacts and adds the phase-publication rule without changing technical phase gates.

- [ ] **Step 1: Add failing workflow and documentation tests**

```js
test('wires a read-only pull-request audit workflow', () => {
  const workflow = readFileSync('.github/workflows/sovmem-audit.yml', 'utf8');
  assert.match(workflow, /pull_request:/);
  assert.match(workflow, /branches: \[main\]/);
  assert.match(workflow, /contents: read/);
  assert.match(workflow, /npm ci/);
  assert.match(workflow, /npm run audit:sovmem:check/);
  assert.match(workflow, /npm test/);
  assert.doesNotMatch(workflow, /git push|contents: write/);
});

test('registers the audit artifacts and phase publication rule', () => {
  const index = readFileSync('index.md', 'utf8');
  const plan = readFileSync('docs/sovmem_v0.3_execution_plan.md', 'utf8');
  for (const path of [
    'docs/SOVMEM-BUILD-AUDIT.md',
    'docs/sovmem-build-state.json',
    'scripts/generate-sovmem-build-audit.mjs',
    '.github/workflows/sovmem-audit.yml',
  ]) assert.match(index, new RegExp(path.replaceAll('.', '\.')));
  assert.match(plan, /audit:sovmem:check/);
  assert.match(plan, /before publishing a completed build phase/);
});
```

- [ ] **Step 2: Verify RED**

Run: `node --test test/sovmem-build-audit.test.js`

Expected: FAIL because the workflow and registry/protocol entries are absent.

- [ ] **Step 3: Add the read-only workflow**

Create `.github/workflows/sovmem-audit.yml`:

```yaml
name: SovMem audit

on:
  pull_request:
    branches: [main]
  merge_group:

permissions:
  contents: read

jobs:
  verify:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
      - run: npm ci
      - run: npm run audit:sovmem:check
      - run: npm test
```

- [ ] **Step 4: Register artifacts and publication rule**

Add registry rows for the design, plan, report, state, generator/library, audit test, and workflow. Update the `package.json` registry description to include the audit commands.

Add executor-protocol rule 9:

```markdown
9. Before publishing a completed build phase through push, pull request, and merge, update `docs/sovmem-build-state.json`, run `npm run audit:sovmem`, and pass `npm run audit:sovmem:check`; the generated audit report and implementation must merge together. Merging requires either (a) required `SovMem audit / verify` with strict up-to-date enforcement and no applicable bypass, or (b) a required merge queue using the `merge_group` check. A loose required status check is insufficient. The controller must configure and verify this repository rule after the first check exists; if permissions prevent configuration, feature publication is blocked rather than represented as enforced.
```

- [ ] **Step 5: Regenerate after fingerprinted spec change and verify GREEN**

Run: `npm run audit:sovmem && npm run test:js && npm run audit:sovmem:check`

Expected: all JavaScript tests pass and the report is current.

- [ ] **Step 6: Commit Task 4**

```bash
git add .github/workflows/sovmem-audit.yml index.md docs/sovmem_v0.3_execution_plan.md docs/SOVMEM-BUILD-AUDIT.md test/sovmem-build-audit.test.js
git commit -m "ci: require current SovMem build audit"
```

---

### Task 5: Whole-branch verification and publication readiness

**Files:**
- Verify all files changed since `origin/main`
- Modify only files required by a failing gate

**Interfaces:**
- Produces final evidence for report freshness, deterministic regeneration, privacy, complete tests, and clean Git state.

- [ ] **Step 1: Verify deterministic regeneration**

Run:

```bash
npm run audit:sovmem
cp docs/SOVMEM-BUILD-AUDIT.md /tmp/sovmem-build-audit-first.md
npm run audit:sovmem
diff -u /tmp/sovmem-build-audit-first.md docs/SOVMEM-BUILD-AUDIT.md
```

Expected: `diff` exits 0 with no output.

- [ ] **Step 2: Run the full repository gate**

Run:

```bash
npm run audit:sovmem:check
npm test
cargo fmt --manifest-path sovmem/Cargo.toml --check
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings
git diff --check origin/main...HEAD
```

Expected: audit current; all JavaScript and Rust tests pass; formatting, clippy, and diff checks exit 0.

- [ ] **Step 3: Verify report privacy and registration**

Run:

```bash
rg -n '/Users/|/Volumes/|sovmem/target|var/' docs/SOVMEM-BUILD-AUDIT.md
rg -n 'SOVMEM-BUILD-AUDIT|sovmem-build-state|generate-sovmem-build-audit|sovmem-audit.yml' index.md
```

Expected: first command has no matches; second lists every registered artifact.

- [ ] **Step 4: Review the complete diff and status**

Run:

```bash
git diff --stat origin/main...HEAD
git diff --check origin/main...HEAD
git status --short --branch
```

Expected: only approved audit/spec/index/workflow changes; no unstaged or untracked files; branch is `codex/sovmem-build-audit`.

- [ ] **Step 5: Commit any verification-only correction**

If a gate required a correction, rerun the full gate and commit only the correction:

```bash
git add -u
git diff --cached --name-only
git commit -m "fix: close SovMem audit verification gaps"
```

If no correction was required, do not create an empty commit.

- [ ] **Step 6: Publish and enforce**

Push `codex/sovmem-build-audit`, open a ready pull request against `main`, and wait for `SovMem audit / verify`. The controller must then configure and verify one of the strict controls below before publication; lacking permissions is a publication blocker, not a remaining administrative note.

## Acceptance criteria

Merging requires either (a) required `SovMem audit / verify` with strict up-to-date enforcement and no applicable bypass, or (b) a required merge queue using the `merge_group` check. A loose required status check is insufficient. The controller must configure and verify this repository rule after the first check exists; if permissions prevent configuration, feature publication is blocked rather than represented as enforced.

---

## Plan self-review result

- Every approved design requirement maps to Tasks 1-5.
- The generator, state, report, tests, commands, workflow, registry, and execution-protocol rule have explicit owners and paths.
- The plan contains no dependency additions, post-merge write automation, private-path output, or autonomous phase promotion.
- Interface names and fixed weights are consistent across tasks.
- Publication remains blocked on fresh verification and a green pull-request check.
