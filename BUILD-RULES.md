# Build Rules

Lessons learned from the P0–P3 delivery cycle and the P3 internal test refactor. These rules supplement `AGENTS.md` (agent behavior) and the SovMem master spec (domain contracts). Every spec, plan, PRD, and implementation session should review the applicable rules before starting work.

---

## 0. Test Architecture

### 0.1 Classify every test by cost and isolation at plan time

Before writing a single line of test code, the spec or implementation plan must classify each test into one of these categories:

| Category | Characteristics | Home |
|----------|----------------|------|
| **Unit** | No I/O, no filesystem, no threads, <100ms | `mod tests {}` in the source file |
| **Integration** | Filesystem I/O, Tantivy indexes, 5MB+ fixtures, threads, >1s | `tests/` directory as a separate binary |
| **End-to-end** | CLI invocation, process spawn, external binary | `tests/` directory |

**Rule:** If a test creates real files, builds a search index, spawns threads, or takes more than one second, it goes in `tests/` — not inside the library crate.

**Origin:** P3 internal test refactor, Aug 2026. ~25,000 lines of integration tests in `mod tests {}` blocks passed individually but failed at ~76% under concurrent execution. Extraction cost 8–11 hours that should have been zero.

### 0.2 Process isolation is the default for heavy tests

Integration test binaries each run in their own OS process. This single property eliminates:

- Shared `static AtomicU64` counter collisions
- Filesystem I/O thrashing from concurrent Tantivy index builds
- Condvar deadlocks across test boundaries
- `#[should_panic]` cross-contamination

**Rule:** If there is any question about whether a test needs its own process, put it in `tests/`. The cost is one extra Rust compilation. The benefit is eliminating entire categories of flaky failures.

**Origin:** Same as 0.1.

### 0.3 Shared global state in test fixtures requires explicit design

Every `static` variable used by test infrastructure must be documented with:

- What it guards (directory counters, environment variables, process-wide singletons)
- Whether it supports concurrent access (mutex, atomic, thread-local)
- Whether it can be initialized more than once per process

**Rule:** A `static AtomicU64` with `Ordering::Relaxed` is not a concurrency strategy. If two tests in the same process create a test fixture, the fixture must either (a) produce unique resources deterministically or (b) be safe to create multiple times with the same initial state.

**Origin:** P3 refactor — the `P2Fixture` Git environment shim is a process-wide singleton that can only be initialized once per process with one commit hash. Tests using it must run in separate `tests/` binaries.

### 0.4 Test-size budget in the spec

Every spec or PRD that calls for tests should budget for their infrastructure:

| Test type | Budget per test (dev time) | Budget per test (run time) |
|-----------|---------------------------|---------------------------|
| Unit | Minutes | <100ms |
| Integration (fixture copy) | Hours | 1–5s |
| Integration (index build) | Hours | 5–15s |
| End-to-end (CLI) | Hours | 10–60s |
| Real-data proof (safe clone) | Days | 5–30 min |

**Rule:** Do not approve a plan where the integration test infrastructure is "addressed later." Budget it upfront.

### 0.5 Every assertion in the test suite must be able to fail

A test that cannot fail is not a test — it is a comment with syntax. This includes:

- Hardcoded hash literals that are never cross-checked against the actual artifact.
- Thresholds that are checked only against themselves (no independent derivation).
- Coverage counts that are compile-time constants rather than being derived from the actual inputs.

**Rule:** Every test assertion must reference a moving part: either computed from the actual data, or cross-checked against an independent source. A test that can only pass is technical debt.

**Origin:** Independent P0–P2 gap analysis, Jul 2026 (Findings H1–H3). The build audit cited hashes and counts that were stored as hardcoded literals in three places with no cross-check. A corrupted baseline file would have left CI green. The fix was to derive every digest from source, not copy it.

---

## 1. Gate Closure

### 1.1 Run the full suite 3× consecutively before closing a phase gate

One passing run is not evidence of reliability. Before signing off on any phase gate:

1. Run the complete test suite 3 times in a row (or as many times as practical given total run time).
2. If any test fails in any run but passes in another, it is flaky — the gate is not closed.
3. Document the flaky test, its root cause, and the fix before re-running all 3.

**Origin:** P3 gate was declared closed, but ~76% of tests failed under concurrent execution. No one had run the full suite because the team had been running subsets during development.

### 1.2 Gate closure requires a fresh whole-suite run, not cached results

A gate signed off against cached compilation artifacts or a subset of tests is not closed. The final verification must:

- Compile from clean (or use `cargo clean` first).
- Run every test in every test binary.
- Use the same command that CI would use.

**Origin:** P0–P2 gate closure was checked against a local run with cached artifacts. The P0–P2 audit found that the CI timeout was too short for a cold build and that `test:sovmem` was missing `--locked` despite the spec requiring it.

### 1.3 Attestation evidence must be independently derivable

A gate is not closed if the evidence for closure is a hardcoded literal that no CI job can independently verify. Every attestation value must meet at least one of:

- **Derivable** — computed from source by a CI-only workflow.
- **Cross-checkable** — independently reproducible from an external reference.
- **Pinable** — frozen by a reviewed commit that includes a test verifying the pinned value matches the actual artifact.

**Rule:** Human-authored attestation values (hash literals, count literals, thresholds) must be accompanied by a test that verifies they correspond to the actual build output, OR be derived from source by the CI pipeline.

**Origin:** P0–P2 gap analysis, Finding H1. The 23/25 retrieval result, 226 warnings, and bundle hash depend on a private vault that no CI job can read. These values rest on the repository owner's attestation with no CI cross-check — an inherent limitation of a sovereign private corpus, not a bug, but one that must be documented and bounded.

### 1.4 The gate-signing commit must postdate the evidence it attests to

The commit that declares a gate closed must be an ancestor of (or equal to) the commit that produced the evidence. A gate signed against evidence from a future or unrelated commit is not valid.

**Origin:** P0–P2 gap analysis, Finding M4. The recorded implementation commit predated the baseline evidence it attests to. `audit:sovmem:check` did not verify ancestry — only document freshness and source fingerprint.

---

## 2. Spec and Plan Review

### 2.1 Every plan must have a "Test Architecture" subsection

Implementation plans must answer:

- Where do the tests live? (`mod tests {}` vs `tests/`)
- What I/O do they perform?
- How long does each test take?
- Do tests share global state?
- Can they run concurrently?
- What is the total estimated run time for the full suite?

**Rule:** If the plan does not answer these questions, the plan is incomplete. Do not approve it until it does.

### 2.2 Distinguish unit tests from integration tests in the spec

The spec should say: *"The following behaviors are verified by unit tests (no I/O, fast): ... The following behaviors are verified by integration tests (filesystem, index builds, threads): ..."*

This forces the author to think about test cost at design time rather than leaving it to implementation.

### 2.3 A containing proof is not a unit test

Do not label a CI proof as a "test" when it is actually a multi-minute, multi-gigabyte, cross-cutting end-to-end validation that exercises every layer of the stack. Call it what it is: a **containing proof** or **integration gate**. Containing proofs belong in `tests/`, run on demand or in CI, and have their own timeout and resource budget.

**Origin:** P2. The "determinism test" (`p2_determinism.rs`) takes minutes, builds two full Tantivy indexes, and exercises the entire export → verify → index → query → eval pipeline. It was structured as a test but it is an integration gate. The P0–P2 audit noted that the CI timeout was not budgeted for such proofs, causing a structural gap between what the spec required and what CI could actually run.

### 2.4 Every frozen constant needs exactly one source of truth

Thresholds, golden IDs, baseline hashes, corpus counts, and schema descriptors must be defined in exactly one place and derived or tested against that place everywhere else. Specifically:

- **Thresholds** go in a JSON file loaded by both the application and the test suite.
- **Configuration hashes** are derived from the object they describe (not hand-maintained as literal strings).
- **Baseline digests** are either computed by CI from the artifacts, or pinned once with a test that verifies the pinned value against the actual artifact.

**Rule:** If the same constant appears as a literal in three files and nothing checks that the three copies agree, at least one is already wrong and nobody knows which.

**Origin:** P0–P2 gap analysis, Findings H2, H3, M1, M2. The `baseline.json` corpus split was a compile-time constant in Rust, repeated as a JSON value, and asserted as a SHA-256 literal in a JS test — with no cross-check between any two copies. The `retrieval_config_hash()` hand-maintained a list of ~70 string literals describing `build_schema()` instead of deriving from the actual Schema object.

### 2.5 `--locked` must be consistent across all build commands

If the spec requires `cargo test --locked`, every CI and local build command must use `--locked`. A spec requirement that isn't enforced by the default command is a spec requirement that will be silently violated.

**Origin:** P0–P2 gap analysis, Finding H4. The P2 design spec required `cargo test --locked`, but `test:sovmem` in `package.json` omitted `--locked`. Given the documented MSRV-resolver hazard, this was the one place `--locked` mattered most.

---

## 3. Evidence and Attestation

### 3.1 Distinguish between attested and verified evidence

**Attested evidence** depends on a human reviewer's word (private vault contents, local-only runs, unrepeatable external state). **Verified evidence** can be reproduced by CI from checked-in source.

Every build claim must be tagged with one or the other. A gate closure that rests on attested evidence must document what, specifically, is attested and why it cannot be verified.

**Origin:** P0–P2 gap analysis, §1 Verdict. The 23/25 retrieval result depends on a private vault that no CI job can read, produced by a single local run. Everything downstream of those numbers is machine-checked; the numbers themselves rest on the repository owner's attestation.

### 3.2 The evidence system must pass the "adversarial mutation test"

For every verification control, ask: "If the underlying artifact were silently corrupted, would this control detect it?" If the answer is no, the control is integrity theater (Risk R7 from the SovMem risk register).

**Rule:** A test that passes when its input is corrupted is worse than no test — it creates false confidence.

**Origin:** P0–P2 gap analysis, §1 Verdict. The build audit had controls that looked like verification but could not fail: hardcoded hash literals with no cross-check, a baseline file never checked against query records, corpus counts as compile-time constants. The independent auditor called this Risk R7 realized in the audit layer.

### 3.3 The output of one phase is the evidence for the next — keep it derivable

Each phase's exit gate produces evidence (baselines, hashes, counts, audit report) that the next phase consumes. If that evidence cannot be re-derived from source, every downstream phase inherits a trust debt.

**Rule:** Before closing a phase, ensure every evidence artifact it produces can be re-generated by running one command from a clean checkout. Artifacts that cannot (private vault outputs, local-machine-only results) must be documented as attested, not verified.

**Origin:** P0–P2. The P2 baseline and audit report consumed P0/P1 outputs (corpus, golden queries, thresholds). Because the corpus source was a private vault, the P2 gate inevitably carried attested evidence from P1 — which was acceptable but was never documented as a dependency.

---

## 4. Extraction review

### 4.3 An independent audit pass before gate closure catches structural gaps

The executor's own test suite verifies implementation correctness. An independent audit pass (human or agent) verifies the **evidence system** — the controls that vouch for the implementation. These are different skills and different failure modes.

**Rule:** Before closing any phase gate, schedule an independent review of the evidence system, not just the implementation. The reviewer's job is to find controls that cannot fail, not to re-run the tests.

**Origin:** P0–P2 independent gap analysis, Jul 2026. The review found that the executor's code was correct but the attestation layer (audit, baselines, cross-checks) was structurally weak — something the executor's own test suite could not have discovered because the tests themselves were part of the attestation system.

### 4.4 Verify the complete Git tree before publishing a repair

A commit tree must preserve the complete parent repository except for explicitly intended changes. When constructing commits through tree APIs or recovering partial trees, a list of changed files is not a complete repository tree.

Before publishing, inspect the exact staged diff and candidate tree: reject unexpected deletions, verify that required build/workflow files remain present, and compare frozen artifacts with the approved baseline using the required integrity semantics. After publishing, verify the remote tree and PR diff against the intended source snapshot. A commit message, successful push, or unrelated green check is not proof of repository preservation.

For recovery, preserve the damaged commits, start from a complete baseline and restore only reviewed paths. Record source recovery separately from code correctness and operational acceptance.

**Origin:** SovMem R0 DSH recovery, 16 September 2026. The complete 1,724-file tree became a 14-file tree in `c875bec`, then a lockfile-only tree in `4e00ef3`. The recovery proved the exact replay of surviving files while preserving all other baseline entries; partial repairs and an audit-schema failure remained open.


### Distinguish public schema markers from private identities in redaction tests

A host username can equal a public role word. Preserve exact public source-kind markers while checking dynamic source-ID suffixes, paths and other data for identity leaks. Add negative cases proving that private values still fail. Do not change frozen fixtures or runtime redaction to satisfy a host-dependent test.

**Origin:** Standalone extraction gate, 14 November 2026: USER=agent collided with the public src:agent: marker in an intake receipt.
