# SovMem artifact registry

Standalone ownership; historical rows describe origin evidence.

# Nous
## Project map
| --- | --- | --- | --- |
## SSA profile model
## How the subsystems relate
## Authority and status
| --- | --- |
## Artifact registry
### Project entry points and scaffold
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [index.md](index.md) | Markdown / navigational | Repository-wide map, artifact registry, reading guide, and maintenance contract. It points to authority but does not create it. |
| [README.md](README.md) | Markdown / project overview | Frames Nous within TFIS, the SSA, and Audy; declares current subsystem authority and source lineage; and separates the emerging architecture from the minimal development scaffold. |
| [.gitignore](.gitignore) | Git configuration / scaffold and generated-output boundary | Keeps Node dependencies, logs, local environment files, coverage, distribution output, macOS metadata, the reproducible `/var/` canary output, and the reproducible `/sovmem/target/` Rust build tree outside version control. |
| [package.json](package.json) | JSON / scaffold and bounded command surface | Declares the `nous` Node.js package, Node 20 floor, test commands, `audit:sovmem` generation and `audit:sovmem:check` freshness commands, unchanged canary commands, and guarded corpus export, offline verification, index, query, evaluation, and golden-validation commands. |
| [package-lock.json](package-lock.json) | JSON / generated lock | Reproducibly locks the current dependency-empty Node.js scaffold. |
| [Retrieval corpus version and root-command tests](test/retrieval-corpus-version.test.js) | JavaScript / contract test | Pins the historical v0 manifest bytes and requires portable, path-safe root commands bound to the active v1 manifest and committed golden set. |
### Project design and planning
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [Audy-SovMem canary exporter and validator implementation plan](docs/archive/plans/2026-07-10-audy-sovmem-canary-exporter.md) | Markdown / working execution plan | Sequences the test-first Rust implementation of a read-only, deterministic, profile-isolated 13-document Audy Vault export and validation canary. |
| [Audy SovMem retrieval corpus and golden queries implementation plan](docs/archive/plans/2026-07-12-audy-sovmem-retrieval-corpus.md) | Markdown / working implementation plan | Sequences exact corpus configuration, thematic gates, deterministic coverage evidence, shared atomic export, human-authored golden-query validation, documentation, and final live-vault verification without implementing retrieval. |
| [Audy retrieval corpus v1 design](docs/archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-v1-design.md) | Markdown / approved design addendum | Records the approved one-candidate v1 substitution after the v0 live preflight failed closed, while preserving the pinned source, counts, quotas, and privacy boundary. |
| [Audy retrieval corpus v1 implementation plan](docs/archive/plans/2026-07-12-audy-sovmem-retrieval-corpus-v1.md) | Markdown / approved execution plan addendum | Binds the active loader and verification workflow to v1 while retaining v0 as immutable evidence and repeating the live read-only gate. |
| [SovMem build audit design](docs/archive/specs/2026-07-12-sovmem-build-audit-design.md) | Markdown / approved design | Defines the reviewed state, deterministic report, fingerprint, pull-request freshness gate, and authority boundaries for build-status publication without changing SovMem technical authority. |
| [SovMem build audit implementation plan](docs/archive/plans/2026-07-12-sovmem-build-audit.md) | Markdown / approved execution plan | Sequences the test-first audit state contract, generator, baseline, read-only pull-request gate, registry integration, and final verification. |
| [SovMem P0/P1 gate closure and P2 thresholds design](docs/archive/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md) | Markdown / approved design | Defines the locked P0 decisions, OKF reference, cumulative verification levels, deterministic stats, corruption contracts, Q3 freeze, exit gates, audit impact, and explicit P2 implementation exclusion. |
| [SovMem P0/P1 gate closure and P2 thresholds implementation plan](docs/archive/plans/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds.md) | Markdown / approved implementation guidance | Sequences the reviewed TDD work for ground-truth locks, verification levels, corruption diagnostics, stats, documentation, audit schema v2, and final live evidence without implementing retrieval. |
| [SovMem P2 derived-index and query design](docs/archive/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md) | Markdown / approved design | Defines the disposable Level-3-bound Tantivy index, deterministic graph projection, explicit BM25 query, separate one-hop expansion, frozen evaluation, privacy boundary, canonical gate, and P3-P5 exclusions. |
| [SovMem P2 derived-index and query implementation plan](docs/archive/plans/2026-07-13-sovmem-p2-derived-index-and-query.md) | Markdown / approved implementation plan | Sequences fixture-first P2 dependency, schema, index, graph, query, evaluation, CLI, determinism, and canonical-gate work without permitting tuning through evaluation. |
### sovmem — memory and judgment
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [sovmem v0.3 execution plan](docs/sovmem_v0.3_execution_plan.md) | Markdown / normative development plan, P0-P3 closed | Controlling implementation sequence for the substrate. It records the resolved P0 decisions, implemented P1-P3 gates, frozen Q3 result, later phases, risks, executor protocol, metrics, kill criteria, and loop-facing contract. |
| [Nous SSA Context and Audy-SovMem Reference Profile v0 design](docs/archive/specs/2026-07-10-audy-sovmem-profile-v0-design.md) | Markdown / approved design | Defines Nous core, isolated SSA profiles, Audy/SSA-0 as the first reference instance, future default-profile extraction, and the OKF/SovMem export schema, authority, sensitivity, validation, and no-mutation boundaries. |
| [Audy SovMem retrieval corpus and golden queries design](docs/archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-design.md) | Markdown / approved design | Defines the corpus-first milestone: Audy's required canonical core, thematic reviewed-research quotas, pinned-source manifest, deterministic acceptance reporting, human-authored golden queries, privacy boundaries, and the gate before retrieval implementation. |
| [June 23 source-lineage recovery](docs/spec/source-lineage-june-23-spec-bundle-verbatim.md) | Markdown / accepted lineage record, secondary provenance | Preserves the final available recovery of the earlier architecture, stack, PRD, workstream, and enterprise bundle. Fidelity markers distinguish verbatim, partial, summarized, and unrecovered material; its delta register explains where v0.3 intentionally diverges. |
#### P3 safe-clone authority boundary
#### Build audit and publication gate
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [SovMem build audit report](docs/archive/audits/SOVMEM-BUILD-AUDIT.md) | Markdown / derived generated evidence | Presents the reviewed P0-P3 closure, frozen P2 Q3 evidence, next step, history, and source fingerprint; the v0.3 execution plan remains normative. |
| [SovMem build audit state](docs/sovmem-build-state.json) | JSON / approved reviewed state | Supplies the human-reviewed P3 closure, safe-clone evidence, and frozen P2 preservation record from which the v4 report is generated. |
| [SovMem build audit library](scripts/lib/sovmem-build-audit.mjs) | JavaScript / implemented audit tooling | Validates strict P3 proof bindings, raw-byte BLAKE3 evidence, the fixed weighted percentage, tracked inputs, and exact report freshness. |
| [SovMem build audit generator](scripts/generate-sovmem-build-audit.mjs) | JavaScript / implemented audit command | Exposes write and read-only check modes used by the root audit commands and pull-request gate. |
| [SovMem build audit tests](test/sovmem-build-audit.test.js) | JavaScript / test | Cover strict state validation, deterministic fingerprinting and rendering, privacy boundaries, exact freshness, command wiring, registry publication, and workflow constraints. |
| [SovMem audit workflow](.github/workflows/sovmem-audit.yml) | YAML / read-only CI evidence | Runs the audit freshness check and full repository suite on pull requests to `main` and merge groups with read-only repository contents permission; the repository still relies on manual merge discipline. |
| [P3 safe-clone proof](docs/evidence/sovmem-p3-safe-clone-proof.json) | JSON / reviewed redacted P3 safe-clone proof | Records the human-only applied verdict, Level 4 evidence, re-query/rebuild assertions, and frozen P2 preservation without publishing candidate, reason, or local-root data. |
### P3 verified write-back implementation
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [P3 profile](profiles/audy-ssa0-p3-v1/profile.yaml) | YAML / implemented P3 profile | Defines the versioned safe-clone canonical lane, separate from the frozen P2 profile and external Vault. |
| [Human verdict transaction](sovmem/src/profile/transaction.rs) | Rust / implemented human verdict transaction | Enforces the human-only confirmation, atomic canon publication, full rebuild, and ledger commitment path. |
| [Policy evaluator](sovmem/src/profile/policy.rs) | Rust / implemented deterministic policy evaluator | Applies versioned policy deterministically and fails closed; confidence and evaluator material remain advisory. |
| [Verdict ledger](sovmem/src/profile/ledger.rs) | Rust / implemented append-only verdict ledger | Implements the BLAKE3 hash chain, five-verb record format, and Level 4 verification bindings. |
| [P3 CLI contracts](sovmem/tests/p3_cli.rs) | Rust / P3 CLI contract test | Covers human verdict interfaces and redacted machine-readable diagnostics. |
| [P3 privacy contracts](sovmem/tests/p3_privacy.rs) | Rust / P3 privacy test | Covers candidate, reason, and advice redaction at public command boundaries. |
| [Safe-clone verifier](scripts/verify-sovmem-p3-safe-clone.mjs) | JavaScript / implemented safe-clone verifier | Verifies the safe clone, frozen P2 isolation, and redacted proof publication. |
| [Safe-clone verifier tests](test/sovmem-p3-safe-clone.test.js) | JavaScript / P3 safe-clone test | Covers corruption, safe-root separation, privacy, and proof-publication failure modes. |
#### Bounded Audy canary implementation
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [Rust toolchain pin](rust-toolchain.toml) | TOML / build configuration | Pins the Rust toolchain used to build and verify the bounded canary crate. |
| [sovmem crate manifest](sovmem/Cargo.toml) | TOML / implemented canary configuration | Declares the Rust package, binary, library, and dependencies for the exporter and validator slice. |
| [sovmem dependency lock](sovmem/Cargo.lock) | TOML / generated durable lock | Reproducibly locks the crate's resolved dependency graph. |
| [sovmem library root](sovmem/src/lib.rs) | Rust / implemented canary code | Exposes the profile module used by the CLI and integration tests. |
| [sovmem binary root](sovmem/src/main.rs) | Rust / implemented canary code | Provides the thin binary entry point for the command-line interface. |
| [sovmem CLI](sovmem/src/cli.rs) | Rust / implemented bounded interface | Implements export, offline verification, golden validation, and the fixed `index`, `query`, and `eval` JSON/text command contracts with privacy-safe P2 parse failures. |
| [Profile module root](sovmem/src/profile/mod.rs) | Rust / implemented canary code | Defines the internal module boundary for profile loading, committed reads, normalization, links, validation, and export. |
| [Profile configuration](sovmem/src/profile/config.rs) | Rust / implemented canary code | Loads and validates profile and canary configuration with deterministic rule precedence. |
| [Committed source snapshot](sovmem/src/profile/source.rs) | Rust / implemented canary code | Reads selected bytes and dates from the pinned Git commit while excluding selected dirty or untracked paths. |
| [Frontmatter parser](sovmem/src/profile/frontmatter.rs) | Rust / implemented canary code | Parses the bounded metadata surface used by deterministic normalization. |
| [Profile mapping](sovmem/src/profile/mapping.rs) | Rust / implemented canary code | Maps committed notes into the approved profile-scoped document model without inference-driven judgment promotion. |
| [Profile link conversion](sovmem/src/profile/links.rs) | Rust / implemented canary code | Resolves supported links deterministically and rejects cross-profile or structurally invalid references. |
| [Profile validation](sovmem/src/profile/validate.rs) | Rust / implemented canary code | Applies fail-closed schema, identity, evidence, date, link, sensitivity, and diagnostic validation. |
| [Deterministic exporter](sovmem/src/profile/export.rs) | Rust / implemented canary code | Builds validated bundles atomically, hashes evidence, and publishes only after all checks pass. |
| [Retrieval corpus contract](sovmem/src/profile/corpus.rs) | Rust / implemented corpus baseline | Loads and validates the fixed versioned core-and-research manifest, exact lanes and themes, pinned source, and minimum acceptance gates. |
| [Corpus acceptance report](sovmem/src/profile/corpus_report.rs) | Rust / implemented derived evidence | Builds and verifies deterministic per-candidate acceptance, exclusion, lane, theme, quota, and warning evidence for the fixed manifest. |
| [Golden query validator](sovmem/src/profile/golden.rs) | Rust / implemented evaluation gate | Validates human authorship, query structure, accepted relevance IDs, privacy-safe diagnostics, and required coverage groups without scoring retrieval. |
| [Offline bundle verifier](sovmem/src/profile/verify.rs) | Rust / implemented P1 gate | Implements compatibility verification and cumulative Level 1-3 structure, raw-byte integrity, semantic correspondence, complete-corpus, and stable corruption diagnostics without reading the live vault. |
| [Verified bundle statistics](sovmem/src/profile/stats.rs) | Rust / implemented P1 reporting | Produces deterministic document, provenance, type, layer, warning, exclusion, and bundle-hash summaries only from verified bundle evidence. |
| [Retrieval threshold contract](sovmem/src/profile/thresholds.rs) | Rust / implemented P0/Q3 lock | Strictly validates the frozen threshold schema, golden-file BLAKE3 identity, one-relevant-document contract, human approval, and integer 20-of-25 hit gate without scoring retrieval. |
| [Derived index builder](sovmem/src/profile/index.rs) | Rust / implemented P2 module | Builds, publishes, reopens, and strictly binds disposable Tantivy generations to the exact Level 3 bundle and fixed retrieval configuration. |
| [Derived graph projection](sovmem/src/profile/graph.rs) | Rust / implemented P2 module | Persists and validates the deterministic typed-link graph projection and reconstructs ordered one-hop expansion witnesses without external nodes. |
| [Offline BM25 query engine](sovmem/src/profile/query.rs) | Rust / implemented P2 module | Executes fixed boosted title/tag/body BM25 retrieval and optional separate one-hop expansion with deterministic metadata-only output. |
| [Frozen retrieval evaluator](sovmem/src/profile/eval.rs) | Rust / implemented P2 module | Evaluates all frozen queries at `k=5`, computes integer Q3 evidence, and atomically writes a complete GO or valid NO-GO baseline without accepting tuning overrides. |
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [Audy reference profile](profiles/audy-ssa0-v0/profile.yaml) | YAML / approved canary profile | Declares the profile identity, namespaces, context kinds, mappings, sensitivity rules, and public allowlist used by the bounded exporter. |
| [Pinned Audy canary selection](profiles/audy-ssa0-v0/canary.yaml) | YAML / approved canary selection | Selects the 13 committed documents, exclusion boundaries, and exact source commit for reproducible export. |
| [Audy retrieval corpus v0 manifest](profiles/audy-ssa0-v0/retrieval-corpus.yaml) | YAML / superseded immutable evidence | Preserves the original eight-core plus 55-candidate contract byte-for-byte after its live privacy failure; the active v1 runtime rejects it. |
| [Audy retrieval corpus v1 manifest](profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml) | YAML / approved active baseline | Defines exactly eight core and 55 reviewed research candidates, the pinned source commit, theme assignments, exclusions, and minimum 45-research / 53-total acceptance gate. |
| [Audy golden query set](evals/audy-sovmem-v0/golden.jsonl) | JSON Lines / approved human ground truth | Commits 25 human-approved queries, rationales, and relevance labels across the core and four research themes for strict evidence validation. |
| [Audy frozen retrieval thresholds](evals/audy-sovmem-v0/thresholds.json) | JSON / pinned reviewed ground truth | Freezes `k=5`, macro Precision@5 and Recall@5 floors, the exact golden-file identity, 20-of-25 integer hit gate, human approval, and the no-tuning-through-P2 boundary. |
| [Audy retrieval baseline](evals/audy-sovmem-v0/baseline.json) | JSON / canonical P2 Q3 GO evidence | Records the deterministic 23-of-25 integer GO, exact per-query top-five results, bundle and index bindings, and the 62-document / 62-node / 19-edge / 81-external-link derived shape. |
| [Official OKF reference pin](docs/spec/okf-reference.json) | JSON / pinned reviewed ground truth | Names the official `GoogleCloudPlatform/knowledge-catalog` repository, reviewed commit, normative `okf/SPEC.md`, and local reference schema used by P0. |
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
| [Shared Rust test support](sovmem/tests/support/mod.rs) | Rust / test support | Supplies isolated Git fixtures and helpers shared by the canary integration tests. |
| [Crate smoke test](sovmem/tests/smoke.rs) | Rust / test | Confirms the approved profile schema is exposed. |
| [Profile configuration tests](sovmem/tests/profile_config.rs) | Rust / test | Cover schema loading, identity isolation, glob behavior, precedence, and invalid configuration. |
| [Committed source snapshot tests](sovmem/tests/source_snapshot.rs) | Rust / test | Cover pinned reads, deterministic ordering and dates, missing paths or commits, and dirty or untracked exclusion. |
| [Normalization tests](sovmem/tests/normalization.rs) | Rust / test | Cover metadata precedence, deterministic fallbacks, approved mappings, sensitivity, and secret-pattern rejection. |
| [Link conversion tests](sovmem/tests/link_conversion.rs) | Rust / test | Cover supported Markdown and wiki links, stable warnings, canonical encoding, and cross-profile rejection. |
| [Validation tests](sovmem/tests/validation.rs) | Rust / test | Cover fail-closed identity, date, source-evidence, judgment, link, and diagnostic boundaries. |
| [Export determinism tests](sovmem/tests/export_determinism.rs) | Rust / test | Cover byte-identical export, atomic publication, source preservation, traversal and output boundaries, dirty-source reporting, and the 50-document expansion. |
| [CLI and bundle verification tests](sovmem/tests/cli.rs) | Rust / test | Cover pinned selection, deterministic summaries, output placement, bundle tampering, manifest mismatches, and verification failures. |
| [Bundle verification boundary tests](sovmem/tests/bundle_verification.rs) | Rust / test | Exercise the reusable verifier directly, including compatibility behavior, the Level 1/2 raw-byte boundary, and Level 3's complete-corpus requirement. |
| [Frozen threshold tests](sovmem/tests/thresholds.rs) | Rust / test | Cover exact threshold and golden identities, schema and evaluation-set binding, human approval, and integer threshold arithmetic. |
| [Default-profile isolation tests](sovmem/tests/default_profile_isolation.rs) | Rust / test | Exercise the shared exporter with a neutral profile and prove Audy or SSA-0 tokens cannot leak into its generated interface. |
| [Corpus configuration tests](sovmem/tests/corpus_config.rs) | Rust / test | Cover the exact versioned manifest, candidate counts, themes, quotas, exclusions, profile mapping, and preserved v0 boundary. |
| [Corpus export tests](sovmem/tests/corpus_export.rs) | Rust / test | Cover deterministic shared-path export, exact and minimum gates, exclusions, atomic failure, and derived report publication. |
| [Corpus report tests](sovmem/tests/corpus_report.rs) | Rust / test | Cover deterministic acceptance evidence, counts, warning summaries, theme minima, and manifest-bound offline verification. |
| [Golden query tests](sovmem/tests/golden_queries.rs) | Rust / test | Cover human authorship, IDs, relevance membership, balanced coverage, privacy-safe failures, and verified-bundle binding. |
| [P2 derived-index tests](sovmem/tests/index_build.rs) | Rust / test | Cover Level 3 gating, fixed schema and configuration, publication safety, strict reopen, bundle preservation, and invalid-current replacement. |
| [P2 graph tests](sovmem/tests/graph_index.rs) | Rust / test | Cover typed-link-only graph derivation, strict persisted graph validation, external-link counts, and deterministic one-hop witnesses. |
| [P2 query tests](sovmem/tests/query.rs) | Rust / test | Cover exact tokenization, boosts, stable ordering, validation, metadata-only results, and separate expansion behavior. |
| [P2 evaluation tests](sovmem/tests/eval.rs) | Rust / test | Cover integer GO/NO-GO arithmetic, deterministic strict baseline bytes, frozen identity, valid NO-GO evidence, and prior-file preservation. |
| [P2 CLI tests](sovmem/tests/p2_cli.rs) | Rust / test | Cover exact JSON/text summaries, exits, corruption diagnostics, parse redaction, fixed eval inputs, and no success output on invalid evaluation. |
| [P2 fixture support](sovmem/tests/support/p2.rs) | Rust / test support | Builds synthetic offline Level 3 bundles and deterministic retrieval/evaluation cases without reading Audy Vault or using the network. |
| [Neutral default-profile fixture root](sovmem/tests/fixtures/default-ssa-v0) | Test fixture / neutral and non-production | Groups the deliberately generic profile and source note used only for isolation testing. |
| [Neutral default-profile fixture](sovmem/tests/fixtures/default-ssa-v0/profile.yaml) | YAML / test fixture | Defines generic identity, namespace, mapping, and empty trust, ledger, and runtime fields without private-profile data. |
| [Neutral onboarding source fixture](sovmem/tests/fixtures/default-ssa-v0/source/Research/Welcome.md) | Markdown / test fixture | Provides a minimal generic source note for shared-path export and token-leak checks. |
### Cross-subsystem integration
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
### ttf-canary — trust and bounded autonomy
#### Core artifacts
| Artifact | Type / status | Purpose and relationship |
| --- | --- | --- |
#### Modular derivatives
| --- | --- |
## Suggested reading paths
## Registering future work
| --- | --- |
