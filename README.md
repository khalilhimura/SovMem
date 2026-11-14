# SovMem

Standalone Rust CLI and library, extracted as a snapshot from Nous.

Public snapshot of the owner-approved legacy SovMem inputs, licensed under [MIT](LICENSE). See [NOTICE](NOTICE) and [PROVENANCE.md](PROVENANCE.md).

The crate remains version 0.1.0; the product specification is v0.3.
Historical audit state and private-vault observations are retained origin evidence, not fresh standalone measurements.

## Build and verify

```sh
bash scripts/setup-codex.sh
source scripts/codex-env.sh
npm run audit:sovmem:check
cargo build --locked --manifest-path sovmem/Cargo.toml
cargo build --locked --manifest-path sovmem/Cargo.toml --no-default-features
cargo fmt --manifest-path sovmem/Cargo.toml --check
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --locked --features test-support -- -D warnings
npm test
```

Use the pinned checkout and `sovmem/target/debug/sovmem` binary. Pass absolute vault, profile and store paths explicitly; extraction never moves or reprovisions operator stores. CLI/MCP contracts and human verdict authority are unchanged. The embedded corpus is outside the crate directory: crates.io packaging is not supported by this extraction.

## SovMem specifications

The [v0.3 execution plan](docs/sovmem_v0.3_execution_plan.md) gives the development requirements.
The [master specification](docs/sovmem-master-spec.md) consolidates the designs, phase status, and remaining work.
The [build-state record](docs/sovmem-build-state.json) records phase evidence and the R0 repair.

The [June 23 recovery](docs/spec/source-lineage-june-23-spec-bundle-verbatim.md) records source history and design differences.
It identifies incomplete recovery and secondary sources.
The [IEEE design note](docs/sovmem%20v0.3%20documentation%20ieee.pdf) gives research context.

For implementation, use this order:

1. Follow the v0.3 execution plan.
2. Use the June 23 recovery to understand source history and recorded differences.
3. Use the IEEE note for design reasons.
4. If the sources disagree, stop for a human decision.

The recovery does not override v0.3 controls.
The execution plan resolves decisions D3, D5, D7, D8, D10 and questions Q1–Q4/Q10.
The recovery closes the source-recovery part of P0/R8.

### Audy canary

The canary exports 13 selected documents from this pinned Vault commit:

```text
43824cfd9caed7b260aa442128fd1130ddb6e2ff
```

It records BLAKE3 source hashes.
It checks profile identity, namespace, document hashes, source correspondence, and the bundle hash.
The exported bundle is derived output.
It does not replace canon or the Vault.

To export from a readable Git vault, supply its path:

```bash
AUDY_VAULT=/path/to/readable-git-vault npm run canary:audy
npm run verify:audy
```

The export command rejects an unset or empty `AUDY_VAULT`.
It writes only to the ignored `var/audy-sovmem-canary/` directory.
Rust writes build output to the ignored `sovmem/target/` directory.
The canary never writes to the source Vault.

### Retrieval corpus and verification

The v1 corpus contract contains 8 core documents and 55 reviewed research candidates.
The themes are `tfis-ssa`, `sovereign-context`, `agent-operations`, and `governance-evidence`.
The accepted export contains all 8 core documents and 54 research documents.
The export does not include one candidate.

The export keeps 226 `links.broken` and `links.ambiguous` warnings as evidence.
A new reviewed manifest is necessary to expand the corpus.
The original v0 and active v1 contracts remain unchanged.

The export and reports use the ignored `var/audy-sovmem-retrieval-corpus/` directory.
Audy Vault remains read-only at the canary commit above.
The OKF reference is `okf/SPEC.md` in `GoogleCloudPlatform/knowledge-catalog` at commit
`d44368c15e38e7c92481c5992e4f9b5b421a801d`.

Verification levels are cumulative:

| Level | Additional checks |
| --- | --- |
| 1 | Frontmatter, required types, profile identity, stable bindings, and provenance |
| 2 | Raw-byte BLAKE3 hashes, file-set integrity, source commitments, index, and recomputed bundle root |
| 3 | Typed links, source/report correspondence, and complete corpus evidence. This is the P1 gate. |

Bare `verify` keeps compatibility with existing canary callers.
The `stats` command uses verified offline evidence only.
It reports document counts, provenance, types, layers, warnings, exclusions, and the bundle hash.

To export and verify the corpus, run:

```bash
AUDY_VAULT=/path/to/readable-git-vault npm run corpus:audy
npm run verify:corpus:audy
npm run stats:audy
npm run verify:golden:audy
```

### P2 offline retrieval

The verified Level 3 Markdown bundle remains authoritative.
The Tantivy index, graph projection, and evaluation output are disposable derivatives.
The index uses the ignored `var/audy-sovmem-derived-index/` directory.
Removal or reconstruction of these derivatives cannot change the bundle or create canon.

To build the index, run:

```bash
npm run index:audy
```

To query the index, use one of these commands:

```bash
npm run query:audy -- --query "<text>"
npm run query:audy -- --query "<text>" --top-k 5 --expand one-hop
```

To evaluate retrieval, run:

```bash
npm run eval:audy
```

Each command returns one compact JSON object on stdout by default.
For text output from `index:audy` or `eval:audy`, append `-- --format text`.
For text output from `query:audy`, append `--format text` after the query arguments.
Query output contains metadata only.
It does not show the query or source text.

Retrieval uses offline Tantivy BM25 with these boosts: `title` = `3`, `tags` = `2`, and `body` = `1`.
Optional one-hop Petgraph expansion uses a separate result channel.
It does not change the BM25 rank or score.
The retrieval binary makes no embeddings, model calls, or network calls.

The golden set contains 25 human-approved queries, reasons, and relevance judgments.
Each query has one relevant document.
Model drafts cannot replace the approved evaluation data.
The file `evals/audy-sovmem-v0/thresholds.json` sets these limits:

| Setting | Value |
| --- | --- |
| `k` | `5` |
| Macro Precision@5 | At least `0.16` |
| Macro Recall@5 | At least `0.80` |
| `minimum_queries_with_hit` | `20` |

Both metric limits need at least 20 of 25 queries to find the relevant document within the first five results.
The evaluator uses the fixed golden set, thresholds, index path, and `k = 5`.
It disables expansion and accepts no override of queries, labels, thresholds, tokenizers, boosts, or expansion.
The P2 golden queries and thresholds remain unchanged.

| Result | Behavior |
| --- | --- |
| GO | Writes the complete baseline and exits with code `0` |
| Valid NO-GO | Writes complete evidence, prints the result, emits `eval.q3_no_go`, and exits with code `1` |
| Invalid input | Gives no success output and keeps the previous baseline |

After a NO-GO result, stop P2 progression.
Do not adjust the frozen inputs to obtain a pass.

The recorded baseline reports 23 hits from 25 queries within the first five results.
Queries `q-005` and `q-011` missed.
It records 62 documents, 62 graph nodes, 19 internal edges, and 81 external typed links.

| Record | BLAKE3 |
| --- | --- |
| Bundle | `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65` |
| Baseline | `c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6` |

These corpus results depend on attested private Vault inputs.
CI can check the committed evidence but cannot run the private export again.
The CI baseline readability check does not constitute a fresh retrieval comparison.

### P3 safe clone and human verdicts

P3 is delivered for the separate `audy-sovmem-writeback-v2` safe-clone workflow.
The frozen P2/Q3 export, golden queries, thresholds, relevance labels, and baseline remain unchanged.
Audy Vault remains read-only.

The portable commands need explicit roots and executable paths.
They contain no machine-specific paths and do not accept a Vault-to-store import.
The raw-hash helper checks opened-file identity on supported Unix platforms, such as macOS and Linux.
Unsupported platforms reject the operation.

First, build the raw-hash helper:

```bash
cargo build --locked --manifest-path sovmem/Cargo.toml --example sovmem-p3-raw-hash
```

Then, supply the reviewed paths to provision the separate store:

```bash
node scripts/provision-sovmem-p3-safe-clone.mjs --binary <sovmem> --hash-binary <sovmem-p3-raw-hash> --seed <verified-level3-export> --p2-profile <frozen-profile> --p3-profile <p3-profile> --policy <policy-template> --store <approved-empty-root> --protected-root <live-vault>
```

Separate authorization is necessary for the proof command with real data.
After that authorization, supply the reviewed paths:

```bash
node scripts/verify-sovmem-p3-safe-clone.mjs --binary <sovmem> --hash-binary <sovmem-p3-raw-hash> --store <approved-store> --seed <verified-level3-export> --p2-profile <frozen-profile> --p3-profile <p3-profile> --golden <golden-jsonl> --thresholds <thresholds-json> --frozen-baseline <canonical-tracked-baseline> --baseline-output <approved-absent-disposable-baseline> --frozen-index <approved-disposable-index> --vault <read-only-vault> --target-query-file <private-target-query> --judgment-query-file <private-judgment-query> --output <proof-output>
```

The proof command rejects publication if verification fails.
Its tracked baseline is read-only evidence.
The `--baseline-output` destination must be separate, disposable, and absent before evaluation.

Agents can stage proposals only.
The interactive CLI verdict transaction commits canon.
Git supplies optional snapshot history and does not grant verdict authority.
A separate reviewed human workflow is necessary for a future transfer back to the Vault.
Public instructions use ACCEPT, REJECT, and REWRITE.
The internal ledger keeps all five verbs, with TP and FP.

### P4 and P5 delivery limits

SovMem v0.3 includes the delivered P4 and P5 components.
P4 supplies the MCP adapter over stdio.
Its tools are `memory_search`, `memory_get`, `memory_propose`, and `memory_status`.
MCP does not give verdict, canon-write, intake-seal, or policy-change authority.

P5 supplies trust measurements, the sentry pipeline, and operational checks.
Full `audit --sample`, blindspot detection, and the remaining operational evidence are follow-up work.
Honcho synchronization and the integrated Nous runtime remain incomplete.
Live-Vault writes remain outside the delivered safe-clone workflow.

