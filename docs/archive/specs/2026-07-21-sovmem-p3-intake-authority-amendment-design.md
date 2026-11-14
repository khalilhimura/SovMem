> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem P3 Intake Authority Amendment

**Status:** approved direction; design-only amendment

## 1. Decision and scope

This amendment corrects a P3 authority contradiction discovered during whole-branch review: a proposer could write, remove, or backdate the intake evidence that both proposal quotas and Level 4 historical replay treated as authoritative.

P3 adopts a separate, protected, append-only **intake authority**. It is distinct from the five-verb human judgment ledger. It records deterministic intake outcomes and defines their only authoritative order. A human-controlled, local, interactive intake-seal CLI is the sole writer of protected intake evidence; it never writes canon, a canonical manifest, the P2 index, `CURRENT`, or the verdict ledger.

This amendment is limited to the following P3 corrections:

- protected ordered retention of accepted and rejected intake outcomes;
- a verdict-bound intake high-water commitment and Level 4 prefix replay;
- policy-rotation parsing that is independent of the current policy's metadata limit;
- exact audit identity for the final reviewed source and proof heads.

It does not add MCP, network calls, a daemon, async code, a dependency, a source-vault write path, automatic acceptance, automatic promotion, trust routing, P4, or P5 behavior.

## 2. Rejected alternatives

### 2.1 Revalidate or content-address the existing staging receipts

Rejected. Recomputing a receipt ID and validating canonical receipt fields catches retained-file edits, but cannot prove a record was not deleted before a later proposal or verdict. A staging-root hash taken at verdict time has the same deletion blind spot.

### 2.2 Add failed intake records to `ledger/verdicts.jsonl`

Rejected. The verdict ledger remains the five-verb human judgment ledger. Failed intake is not a verdict and must not acquire verdict semantics, a human reason, or canonical authority.

### 2.3 Relax quotas or timestamp replay

Rejected. It would weaken the P3 exit gate. Timestamp strings remain evidence only; they cannot be used as the authority ordering for historical replay.

### 2.4 Protected intake authority with an interactive local seal command

Selected. It needs no daemon, no network transport, no new dependency, and no asynchronous runtime. An agent may submit an untrusted inbox item, but only a human at a real controlling terminal may turn it into protected intake evidence. This adds deterministic intake custody, not canon-commit authority.

## 3. Authority model

| Resource | Writer | Authority | Mutable by proposer |
|---|---|---|---|
| `staging/inbox/` | proposer | untrusted submission transport only | yes, until sealed or discarded |
| `intake/events.jsonl` | interactive intake seal | authoritative ordered intake history | no |
| `intake/proposals/` | interactive intake seal | authoritative immutable staged pairs | no |
| `ledger/verdicts.jsonl` | interactive human verdict | five-verb human judgment history and commit point | no |
| `canon/`, `evidence/`, `derived/` | interactive human verdict | canonical and derived P3 state | no |

The intake seal command requires a real `/dev/tty`, displays the selected inbox item's non-secret commitments, and requires the exact confirmation `SEAL INTAKE <submission-id>`. It is deterministic: the human chooses whether to run the command, but cannot choose its policy outcome, alter its candidate bytes, select policy rules, or cause a canonical write.

The existing human-only verdict ceremony remains the **only** path that commits canon. The intake seal is not a verdict, has no five-verb value, cannot apply a candidate, and cannot create a Human Judgment document.

Production capability provisioning changes as follows:

- proposers receive read access to profile, policy, seed, current commitments, and `intake/events.jsonl`;
- proposers may create only untrusted `staging/inbox/<submission-id>/` requests through `propose` and may not write `staging/.intake-lock`, `intake/`, `ledger/`, `canon/`, `policies/`, `evidence/`, `derived/`, `transactions/`, `locks/`, or `policy.toml`;
- interactive intake seal and verdict commands run outside proposer capability and reject a missing TTY before any store write;
- a path-only sandbox that gives recursive write access to `staging/` is insufficient and is forbidden by the updated capability fixture.

## 4. Storage, schemas, and ordering

The store gains a protected `intake/` directory:

```text
intake/
  events.jsonl
  proposals/<two-hex>/<proposal-id>/
    candidate.md
    proposal.meta.json
  transactions/<intake-transaction-id>/
    prepared.json
    committed.json
    published.json
```

`staging/` retains only untrusted inbox requests and short-lived request leases. Existing `staging/proposals/` and `staging/intake-rejections/` are removed by the migration; no P3 reader may treat them as authoritative after the new profile/store schema is active.

`intake/events.jsonl` is prefix-preserving and BLAKE3 hash-chained. It has a distinct schema and domains from `ledger/verdicts.jsonl`. Its records have one of two exact outcomes:

- `STAGED`: binds `int-` sequence ID, previous intake hash, event hash, timestamp evidence, validated source, proposal ID, proposal metadata raw hash, candidate raw hash, current policy hash, and profile hash;
- `REJECTED`: binds `int-` sequence ID, previous intake hash, event hash, timestamp evidence, validated source or null, canonical sorted rule codes, current policy hash, and profile hash. It contains no candidate, metadata, target, reason, evaluator, or body hash.

`int-0000000000000001` is the first record. The sequence is strictly contiguous and is the only visibility order. A record ID, previous hash, event hash, canonical bytes, schema, timestamp syntax, source identity, outcome fields, rule-code ordering, and cross-file pair commitments are all verified on every read. Accepted pair files use create-new publication and are bound to their `STAGED` record; no pair exists without exactly one such record.

The intake head is the pair `(sequence, event_hash)`. A zero-record head is `(0, null)`. A human verdict binds both fields in its canonical verdict payload. This changes the verdict event schema version; all payload and event hashes are recomputed under a new explicit versioned domain. Old P3 v1 stores are not silently upgraded: an amendment-aware P3 profile/store version starts with the new schema and receives a separately reviewed migration only if retained real stores exist.

## 5. Intake state machine and atomic seal

1. `propose` validates only safe request framing and publishes an untrusted inbox directory with create-new, raw candidate bytes, request metadata, and a submission ID. It returns `submitted`, not `staged`.
2. `intake seal <submission-id>` acquires the protected intake lease after confirming no verdict or recovery lease exists. It reads the inbox bytes once and samples its clock once.
3. The command verifies the current protected intake chain, loads only the ordered protected prefix, prepares a canonical pair, and runs `PolicyEvaluator`.
4. On a pass, it stages the immutable pair under `intake/proposals/`; on a failure, it stages no pair. In both cases it constructs the next `STAGED` or `REJECTED` intake event.
5. An intake transaction journal binds the selected inbox raw hashes, expected sequence, previous intake head, policy/profile hashes, outcome, and exact output hashes. Pair publication, intake event append, and inbox cleanup are copy-on-write, fsynced, and recoverable. The intake event append is the intake transaction commit point.
6. Only after the event is durable does cleanup remove the untrusted inbox. A crash before append leaves no visible protected outcome; a crash after append leaves the committed outcome and only a classifiable cleanup tail. Public reads fail closed on any incomplete intake transaction.

`rejected-at-intake`, `staged`, `ruled`, `applied`, `superseded`, and `retained` remain the durable proposal lifecycle states. `submitted` is an untrusted transport status, not a proposal lifecycle state. A `REJECTED` intake event yields `rejected-at-intake`; a `STAGED` event yields `staged` until exactly one verdict event rules its proposal.

## 6. Policy, timestamps, and historical replay

`PolicyEvaluator` receives its retained pairs and rejected attempts only from an explicitly supplied protected intake prefix. At seal time the prefix ends at the current protected intake head; the candidate being evaluated is excluded from its own quota counts. At verdict time the prefix ends at the freshly observed head, and the selected proposal is excluded from its own counts.

For each verdict event, Level 4:

1. verifies the complete intake chain and all pair bindings;
2. verifies that the event's bound `(sequence, intake_head_hash)` names an exact chain prefix;
3. materializes only records in that prefix, excluding the event's own proposal from duplicate/open/daily comparisons;
4. reconstructs retained and rejected evaluator inputs from those records;
5. loads the event's archived policy, evaluates the exact proposal against that exact prefix, and rejects any mismatch with `ledger.intake_binding`.

Records after the bound high-water mark cannot alter an earlier verdict's verification. Equal timestamps are permitted because the sequence resolves visibility. A timestamp earlier than the last protected intake timestamp is rejected as `intake.clock_regression`; this prevents a rollback from moving an attempt into a different daily quota bucket. Timestamps remain evidence, not authority ordering.

The structural proposal-metadata read ceiling becomes a profile-versioned P3 constant. It is sufficient only to parse, canonicalize, and bind a pair safely. After a pair is structurally read, its applicable policy limit is enforced by the event's archived policy for ruled pairs and by the current policy for an actively sealed request. Therefore lowering current `max_meta_bytes` cannot make a previously valid sealed store unreadable; an unruled pair under an older policy is represented as stale rather than becoming a Level 4 parser deadlock.

## 7. Locks, recovery, and errors

The lock order is `verdict.lock` then `intake.lock`. Intake seal acquires only `intake.lock` and refuses to start if a verdict/recovery lock exists. Verdict holds both locks before observing the intake head, sealing policy evidence, or writing a verdict event. Recovery recognizes an intake transaction journal separately from verdict recovery and never deletes a published intake event or protected pair.

Stable public errors include:

- `intake.locked`, `intake.confirmation_failed`, `intake.incomplete`, `intake.clock_regression`;
- `intake.record_invalid`, `intake.chain_invalid`, `intake.head_mismatch`, `intake.pair_binding`;
- `ledger.intake_binding` for a verdict whose committed intake prefix cannot be verified or re-evaluated;
- existing policy codes for deterministic policy rejection.

All errors are static and redacted. No CLI success or error prints candidate bodies, reasons, evaluator bytes, source paths, inbox paths, operating-system identities, temporary names, or rule input details.

## 8. CLI and output contracts

`propose` continues to be the only proposer-facing command. Its successful output changes from `staged` to `submitted` and includes only a redacted submission ID, candidate hash, profile hash, policy hash, and untrusted submission time. It does not claim policy acceptance.

`intake seal` is a new local human-only CLI surface. It has no MCP mapping, has no batch or noninteractive flag, accepts one submission ID, writes no canon, and returns either one `staged` or `rejected-at-intake` summary with intake sequence/head commitments.

Proposal list, status, queue alarm, and `verdicts list` read protected intake and verdict evidence. They may render `submitted` requests separately only while an inbox directory is complete; an incomplete inbox is a store failure, not a queue item. Existing P2 bundle CLI output remains byte-identical.

## 9. Fixture-first verification

The amendment is not complete until fixtures prove all of the following:

- an agent cannot edit, delete, rename, or add a protected intake record or accepted pair;
- malformed receipt-equivalent fields, duplicate IDs, malformed source/timestamp, unsorted/duplicate rule codes, invalid prior hash, event hash, sequence, and pair commitment fail closed with exact redacted codes;
- an attempt to delete or mutate an old rejection cannot reset a daily quota;
- equal timestamps preserve deterministic replay by sequence; a backward timestamp fails `intake.clock_regression` without a durable outcome;
- a later record cannot change an older verdict's Level 4 result, and a forged verdict with a high-water that omits a visible blocking record fails `ledger.intake_binding`;
- acceptance/rejection sealing crashes at every boundary with no ambiguous outcome; recovery preserves published protected records and removes only bound unpublished remnants;
- a rotated policy with lower `max_meta_bytes` preserves Level 4 for retained historical pairs and represents incompatible open pairs as stale;
- all five verbs, Level 4 corruption, the 20-mixed-verdict chain, concurrency, privacy, and full propose → seal → human verdict → write → re-index → re-query proof remain covered;
- the real-data proof remains a safe clone and does not alter Audy Vault, the frozen P2 corpus, golden queries, thresholds, labels, or baseline.

Each correction starts with an executed nonzero RED fixture, then the smallest implementation change, focused GREEN, subsystem regression, serialized Rust suite, JavaScript suite, frozen-artifact guard, and final audit check.

## 10. Audit and release discipline

The final audit must distinguish:

- `implementation_commit`: the committed source SHA containing the complete amendment implementation;
- `safe_clone_proof_commit`: the exact source SHA used for the approved safe-clone ceremony;
- `audit_commit`: the later audit-only SHA, if a generated audit update necessarily follows source verification.

The audit review date, measured JavaScript/Rust counts, implementation history, and source-tree fingerprint must be regenerated only after the repaired source has passed every P3 gate. It may not cite a prior implementation SHA as proof for materially different transaction, policy, or Level 4 behavior.

## 11. Stop conditions

Stop for a human decision rather than weakening the gate if any required local capability mechanism cannot distinguish proposer transport writes from protected intake writes, if an existing retained P3 v1 store needs migration, if interactive intake sealing proves unusable for the safe-clone ceremony, or if high-water binding cannot be made atomic with the verdict transaction.

P3 remains unmergeable until this amendment is implemented, the review blockers are resolved, the full gates pass at the correct source SHA, fresh review reports no Critical or Important issue, and the audit is current at the final reviewed head.
