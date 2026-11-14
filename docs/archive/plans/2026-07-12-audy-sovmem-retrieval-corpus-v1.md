> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# Audy SovMem Retrieval Corpus v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the failed live v0 contract as evidence, introduce the approved one-path v1 manifest revision, and pass the pinned live corpus and offline verification gates without weakening privacy validation.

**Architecture:** `retrieval-corpus.yaml` remains the immutable v0 record. A new `retrieval-corpus-v1.yaml` becomes the only active fixed manifest; the loader and offline verifier bind to schema `audy-retrieval-corpus-v1` and embed that reviewed file. All corpus and golden-query fixtures consume v1, while a regression proves v0 is preserved but unsupported by the active runtime.

**Tech Stack:** Rust, serde/serde_yaml, existing SovMem profile/export/verifier modules, Cargo integration tests, npm test orchestration.

## Global Constraints

- Source commit remains exactly `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.
- Preserve `profiles/audy-ssa0-v0/retrieval-corpus.yaml` byte-for-byte as `audy-retrieval-corpus-v0` evidence.
- Create `profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml` with schema `audy-retrieval-corpus-v1`.
- V1 changes exactly one `governance-evidence` candidate: remove `04-Research/DSOM/source-pages/dsom-automation-encyclopedia-docs-tools__howto-privacy-guardian.md`; add `04-Research/DSOM/source-pages/2.-operational-rituals__digital-sovereignty-operational-model-palace.md`.
- Preserve exactly eight core documents, fifty-five research candidates, candidate distribution `12/15/14/14`, minimum forty-five accepted research documents, minimum fifty-three accepted total documents, and accepted theme minima `10/15/10/10`.
- Do not add a privacy exception, source-vault write, profile identity, retrieval/ranking behavior, dependency, or golden-query content.
- The active loader and offline verifier reject v0, mixed, and forged corpus reports instead of silently interpreting them as v1.

---

### Task 1: Bind the active corpus contract to v1

**Files:**
- Create: `profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml`
- Modify: `sovmem/src/profile/corpus.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/tests/corpus_config.rs`
- Modify: `sovmem/tests/corpus_report.rs`
- Modify: `sovmem/tests/corpus_export.rs`
- Modify: `sovmem/tests/cli.rs`
- Modify: `sovmem/tests/golden_queries.rs`
- Create: `test/retrieval-corpus-version.test.js`
- Modify: `docs/superpowers/plans/2026-07-12-audy-sovmem-retrieval-corpus.md`

**Interfaces:**
- Consumes: the immutable v0 YAML, `load_corpus(&Path)`, `CORPUS_SCHEMA`, `fixed_corpus_config()`, and the Task 1-5 export/verifier/golden contracts.
- Produces: the active `audy-retrieval-corpus-v1` fixed manifest and unchanged public Rust/CLI signatures.

- [ ] **Step 1: Write v1 contract regressions before production changes**

In `sovmem/tests/corpus_config.rs`, point the active helper to `retrieval-corpus-v1.yaml` and add constants for the exact removed and added paths. Update the approved-contract assertion to:

```rust
assert_eq!(corpus.schema, "audy-retrieval-corpus-v1");
assert_eq!(corpus.source_commit, PINNED_SOURCE_COMMIT);
assert_eq!(corpus.core.len(), 8);
assert_eq!(corpus.research.len(), 55);
assert!(corpus.entry(REMOVED_V0_CANDIDATE).is_none());
let replacement = corpus.entry(ADDED_V1_CANDIDATE).unwrap();
assert_eq!(replacement.lane, CorpusLane::Research);
assert_eq!(replacement.theme, Some(CorpusTheme::GovernanceEvidence));
```

Add a preservation/version test that reads `retrieval-corpus.yaml`, asserts its first line is `schema: audy-retrieval-corpus-v0`, asserts it still contains the removed path and not the replacement, and asserts `load_corpus(v0_path()).unwrap_err().code() == "corpus.schema"` under the active v1 runtime.

Create `test/retrieval-corpus-version.test.js` so the v0 file cannot be silently edited while both versions evolve:

```javascript
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('preserves the immutable Audy retrieval corpus v0 bytes', () => {
  const bytes = readFileSync('profiles/audy-ssa0-v0/retrieval-corpus.yaml');
  const digest = createHash('sha256').update(bytes).digest('hex');
  assert.equal(digest, 'ff7807ec0c9a06ece4d1ee1513a8fd6cd80ed9446f3fc0ee440fd2b137b6d132');
});
```

Keep the existing exact core, path uniqueness, mapping coverage, fifty-five candidate count, `12/15/14/14` distribution, and minimum assertions.

- [ ] **Step 2: Run the focused configuration test and verify RED**

```bash
cargo test --manifest-path sovmem/Cargo.toml --test corpus_config
```

Expected: FAIL because `retrieval-corpus-v1.yaml` is absent and the active schema is still v0.

- [ ] **Step 3: Add the exact v1 manifest**

Create `profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml` by copying the complete v0 YAML and making only these two content changes:

```yaml
schema: audy-retrieval-corpus-v1
```

```yaml
  - { path: "04-Research/DSOM/source-pages/2.-operational-rituals__digital-sovereignty-operational-model-palace.md", theme: governance-evidence }
```

The second line replaces, in the same governance-evidence position, the v0 line containing `dsom-automation-encyclopedia-docs-tools__howto-privacy-guardian.md`. Make no other manifest change.

- [ ] **Step 4: Bind runtime loading and verification to v1**

In `sovmem/src/profile/corpus.rs`, change only the active schema constant:

```rust
pub const CORPUS_SCHEMA: &str = "audy-retrieval-corpus-v1";
```

In `sovmem/src/cli.rs`, change the embedded fixed manifest to:

```rust
include_str!("../../profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml")
```

Update the forged-report fixture schema literal in `sovmem/tests/cli.rs` to `audy-retrieval-corpus-v1`.

- [ ] **Step 5: Move every active fixture to v1**

Change the corpus helper path from `retrieval-corpus.yaml` to `retrieval-corpus-v1.yaml` in exactly these files:

```text
sovmem/tests/corpus_config.rs
sovmem/tests/corpus_report.rs
sovmem/tests/corpus_export.rs
sovmem/tests/cli.rs
sovmem/tests/golden_queries.rs
```

Where exact-quota fixtures currently use the removed privacy-guardian path as a dirty research buffer, replace that fixture constant with the added v1 path. Preserve the same number of dirty buffers and every `53 exported / 10 excluded / 8 core / 45 research` assertion.

- [ ] **Step 6: Record v1 supersession in the parent execution plan**

Add a dated v1 supersession note near the top of `docs/superpowers/plans/2026-07-12-audy-sovmem-retrieval-corpus.md` linking the v1 design and this plan. State that completed v0 history remains unchanged, while all pending Task 6-8 commands and durable references must use `retrieval-corpus-v1.yaml` and schema `audy-retrieval-corpus-v1`.

Update only pending Task 6-8 command/reference text from `retrieval-corpus.yaml` to `retrieval-corpus-v1.yaml`; do not rewrite completed v0 task history.

- [ ] **Step 7: Run focused and full verification**

```bash
cargo test --manifest-path sovmem/Cargo.toml --test corpus_config
cargo test --manifest-path sovmem/Cargo.toml --test corpus_report
cargo test --manifest-path sovmem/Cargo.toml --test corpus_export
cargo test --manifest-path sovmem/Cargo.toml --test cli
cargo test --manifest-path sovmem/Cargo.toml --test golden_queries
npm test
cargo fmt --manifest-path sovmem/Cargo.toml -- --check
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings
git diff --check
```

Expected: all focused tests and the complete suite pass; formatting, lint, and diff checks report no errors.

- [ ] **Step 8: Commit the v1 contract**

```bash
git add profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml sovmem/src/profile/corpus.rs sovmem/src/cli.rs sovmem/tests/corpus_config.rs sovmem/tests/corpus_report.rs sovmem/tests/corpus_export.rs sovmem/tests/cli.rs sovmem/tests/golden_queries.rs test/retrieval-corpus-version.test.js docs/superpowers/plans/2026-07-12-audy-sovmem-retrieval-corpus.md
git commit -m "fix: version Audy retrieval corpus contract"
```

### Task 2: Repeat the pinned live corpus gate

**Files:**
- Generate ignored output: `var/audy-sovmem-retrieval-corpus/`
- Do not modify: `/Volumes/himura/audy-vault`

**Interfaces:**
- Consumes: the reviewed v1 manifest, the pinned read-only Audy Vault, `sovmem corpus`, and `sovmem verify`.
- Produces: an accepted v1 corpus and the safe ID/title/lane/theme catalog for the existing Task 6 human-authorship gate.

- [ ] **Step 1: Prove source identity and immutability**

```bash
git -C /Volumes/himura/audy-vault rev-parse HEAD
git -C /Volumes/himura/audy-vault status --short
```

Expected: HEAD is `43824cfd9caed7b260aa442128fd1130ddb6e2ff`. The live vault need not be globally clean: a selected dirty or untracked research buffer is eligible for the existing soft-exclusion rules, while any dirty or untracked canonical-core path remains a hard failure. Record the complete status so the same selected-path state can be confirmed after export.

- [ ] **Step 2: Export v1 from the pinned source**

Use the direct CLI until Task 7 adds root scripts:

```bash
cargo run --manifest-path sovmem/Cargo.toml -- corpus \
  --vault "$AUDY_VAULT" \
  --profile profiles/audy-ssa0-v0/profile.yaml \
  --corpus profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml \
  --output var/audy-sovmem-retrieval-corpus
```

Expected: exit `0`, schema v1 evidence, `exported >= 53`, `core = 8`, `research >= 45`, and `errors = 0`.

For SMB performance, an audited sparse clone or mirror is acceptable only when its HEAD equals the pinned commit, all exact 63 v1 bodies are checked out, and its selected-path porcelain status is byte-identical to the canonical live vault's selected-path status. Dirty or untracked selected research must therefore be explicitly mirrored as dirty or untracked status; omission is not equivalent evidence. Core dirtiness remains a hard failure. With `AUDY_SPARSE` set to the proposed mirror, run this exact comparison before export:

```bash
node --input-type=module -e 'import fs from "node:fs"; import path from "node:path"; import {execFileSync} from "node:child_process"; const manifest=fs.readFileSync("profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml","utf8"); const paths=[...manifest.matchAll(/path: "([^"]+)"/g)].map(match=>match[1]); if(paths.length!==63||new Set(paths).size!==63) throw new Error("expected 63 exact v1 paths"); for(const sourcePath of paths){if(!fs.existsSync(path.join(process.env.AUDY_SPARSE,sourcePath))) throw new Error(`missing sparse body: ${sourcePath}`)} const selectedStatus=root=>execFileSync("git",["-C",root,"status","--porcelain=v1","--",...paths]); const live=selectedStatus(process.env.AUDY_VAULT); const sparse=selectedStatus(process.env.AUDY_SPARSE); if(!live.equals(sparse)) throw new Error("selected-path status differs from canonical live vault"); console.log("All 63 bodies and selected-path status match.");'
```

- [ ] **Step 3: Verify the published corpus offline**

```bash
cargo run --manifest-path sovmem/Cargo.toml -- verify \
  --profile profiles/audy-ssa0-v0/profile.yaml \
  --bundle var/audy-sovmem-retrieval-corpus \
  --require-corpus
```

Expected: exit `0`, `documents` equals the export count, `errors = 0`, and `bundle_hash` exactly matches the export summary.

- [ ] **Step 4: Confirm no source mutation and stop at the human gate**

```bash
git -C /Volumes/himura/audy-vault rev-parse HEAD
git -C /Volumes/himura/audy-vault status --short
```

Expected: pinned HEAD unchanged and the full status matches the preflight record. If a sparse clone or mirror was used, repeat the exact 63-body and selected-path porcelain comparison from Step 2 and require the same successful result. Any new core dirtiness, omitted selected research status, or other source-state change fails the gate. Generate the accepted catalog with only ID, generated title, lane, and theme. Do not create query text, rationales, or relevance labels. Present the catalog to Khalil for the existing Task 6 authorship gate.
