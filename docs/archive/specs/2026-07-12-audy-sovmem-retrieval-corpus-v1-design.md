> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# Audy SovMem Retrieval Corpus v1 Design

**Status:** Approved by Khalil on 2026-07-12 after the first live v0 corpus preflight.

## Purpose

The pinned live-vault preflight proved that the v0 candidate `04-Research/DSOM/source-pages/dsom-automation-encyclopedia-docs-tools__howto-privacy-guardian.md` fails closed with `document.restricted_content` and `document.restricted_scope`. The source contains a literal private-key header inside inline code and has no explicit private-scope frontmatter. The validator is behaving correctly and must not be weakened.

## Decision

Preserve `profiles/audy-ssa0-v0/retrieval-corpus.yaml` unchanged as the immutable `audy-retrieval-corpus-v0` record. Add `profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml` as the active fixed contract with schema `audy-retrieval-corpus-v1`.

V1 makes exactly one candidate substitution in the `governance-evidence` theme:

- Remove: `04-Research/DSOM/source-pages/dsom-automation-encyclopedia-docs-tools__howto-privacy-guardian.md`
- Add: `04-Research/DSOM/source-pages/2.-operational-rituals__digital-sovereignty-operational-model-palace.md`

The replacement passed a standalone committed-source export from the pinned vault commit. Its generated title is `Sovereign Markdown Palace Protocol`.

## Invariants

- The source commit remains `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.
- Canonical core remains exactly eight documents.
- Research remains exactly fifty-five candidates.
- Candidate theme distribution remains `tfis-ssa: 12`, `sovereign-context: 15`, `agent-operations: 14`, and `governance-evidence: 14`.
- Acceptance minima remain forty-five research documents, fifty-three total documents, and `10/15/10/10` by accepted theme.
- V0 is never edited or silently reinterpreted as v1.
- No source-vault write, privacy-detector exception, new profile identity, retrieval engine, ranking behavior, or query authorship enters this revision.

## Runtime Contract

The active fixed corpus schema becomes `audy-retrieval-corpus-v1`. Corpus loading, export, offline verification, golden-query evidence verification, fixtures, and future root commands must bind to `retrieval-corpus-v1.yaml`. The preserved v0 file is historical evidence and must be rejected by the active v1 loader rather than accepted under changed semantics.

The offline verifier must continue reconstructing the complete fixed manifest contract from the reviewed v1 file. It must bind every candidate path to its lane and theme and reject v0, mixed, or forged reports.

## Verification

Implementation follows TDD:

1. Tests first assert that the active contract is v1, the exact swap occurred, v0 remains byte-preserved, and all counts and theme distributions remain unchanged.
2. Focused corpus configuration, report, export, CLI, and golden-evidence tests pass against v1.
3. The complete JavaScript and Rust suite, formatting, lint, and diff checks pass.
4. A live pinned corpus export passes without weakening privacy validation, followed by offline verification with the same bundle hash.

Only after the accepted v1 corpus exists may the workflow proceed to the human-authored golden-query catalog gate.
