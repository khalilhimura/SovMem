> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem P3 lock capability-boundary amendment

**Status:** approved design choice; implementation remains paused pending review of this amendment.

**Parent authority:** [the P3 verified write-back-loop design](2026-07-14-sovmem-p3-verified-write-back-loop-design.md), especially sections 7.3, 24.3, and 26. This amendment resolves only the lease-path threat-model question raised while implementing P3 intake authority. All frozen P0–P2 and P3 authority decisions remain unchanged.

## Decision

P3 uses the parent design's local path-and-capability boundary for `locks/`:

1. `locks/` is outside every proposer, agent, evaluator, MCP, and untrusted harness write root.
2. Only the trusted local P3 verdict/intake/recovery process receives mutation capability for `locks/`.
3. A process able to run arbitrary filesystem operations as that same unrestricted trusted OS identity is not an in-scope production adversary.
4. A wrong-type, symlinked, missing, malformed, or incomplete lock artifact observed at a normal operation boundary still fails closed with the existing redacted typed error. It is evidence for recovery or operator investigation; it is never silently repaired by proposal or verdict application.

This choice does not grant any additional write authority. It states where the existing authority is enforced: deployment capability and Task 9 privilege fixtures, rather than an impossible promise that a same-identity peer with write permission can never rename an entry between POSIX namespace operations.

## Why this is required

Safe descriptor-relative POSIX operations can bind reads and parent-directory traversal, but deletion remains name-resolved: `unlinkat(parent, name)` cannot atomically prove it removes the exact entry previously inspected if an equally privileged peer may rename that entry concurrently. Attempting to simulate a stronger guarantee produced a growing, platform-specific lock protocol without satisfying the claim.

The parent design already excludes that peer and requires `locks/` to be outside untrusted write roots. Retaining the capability boundary is therefore consistent with the approved P3 threat model; claiming same-identity adversarial rename resistance would require a separate multi-user or kernel-assisted locking design.

## Lease protocol after this amendment

The intake lease coordinates **trusted P3 writers**, not hostile same-identity filesystem peers.

- Acquisition uses no-replace creation and canonical owner bytes. A pre-existing, malformed, or incomplete lease fails closed.
- Owner data binds the trusted operation identity and unique acquisition value; a stale in-process capability cannot authorize a later acquisition.
- Pair and intake-event publication revalidate the live lease immediately before their replace-capable publication boundary. A second trusted P3 writer receives the stable lock error rather than replacing evidence.
- Release follows the parent design's terminal liveness rule: while the lease is visible, verify ownership and sync its parent; then remove the lease as the final liveness-only deletion, with no later fallible operation. A failed or crashed release leaves visible incomplete evidence and makes reads/mutation fail closed until recovery.
- Recovery is the only route that may clear stale lock evidence. It preserves published immutable pairs, canonical generations, and ledger events.

No lease code may infer permission from a PID, lock age, evaluator content, confidence, a proposal, or a caller-supplied authority field.

## Capability enforcement and testing

Task 2 retains corruption, stale-capability, collision, crash, and incomplete-state fixtures. It does **not** claim a fixture that gives a same-identity attacker write access to `locks/` proves production safety.

Task 9 is responsible for the production authority proof:

- proposer/agent/evaluator/MCP test identities cannot create, rename, remove, or replace entries under `locks/`;
- they can write only their explicitly granted untrusted inbox/staging inputs;
- the trusted human/recovery identity can perform the reviewed lock lifecycle;
- a capability violation produces stable redacted diagnostics and never a partial canonical or intake publication.

Safe-clone and real-data proofs continue to use an approved separate store. Audy Vault, frozen Level 3/P2 artifacts, live canon, thresholds, golden queries, labels, and `baseline.json` remain untouched.

## Implementation consequences

1. Remove the direct `rustix = "=1.1.4"` dependency and restore the original pinned dependency set and lockfile relationship. The prior approval allowed an experiment; this selected design no longer needs it.
2. Replace the descriptor-race experiment with the parent-design, standard-library no-replace lease protocol and its explicit incomplete-state handling. Keep only behavior that remains valid under the capability boundary: raw-byte owner binding, non-clone stale-capability rejection, collision classification, canonical type/path checks at operation boundaries, UTF-8-safe temporary grammar, and successor cleanup after a pre-publication failure.
3. Do not add platform-specific filesystem claims, `unsafe`, new dependencies, privileged helper processes, automatic lock stealing, timeout-based liveness, or a new recovery command surface.
4. Keep the public and internal P3 authority model unchanged: `intake seal` remains a later human ceremony, verdict remains the only canonical write authority, and no MCP write authority is introduced.

## Required implementation evidence

Before Task 2 can close again, the revised fixture-first plan must prove:

- exact nonzero RED then GREEN coverage for duplicate trusted lease acquisition, stale in-process lease capability, live lock ownership mismatch, and release/crash incomplete-state visibility;
- no-replace acquisition does not overwrite an existing lease or owner;
- an unsafe or malformed observed lease fails closed with a typed redacted diagnostic;
- pair and event collision paths preserve prior durable bytes;
- candidate-event successor cleanup preserves the prior event head on failure;
- malformed multibyte temporary names return `store.layout_invalid` rather than panic;
- the direct dependency list returns to the approved frozen set; and
- Task 9 separately proves the actual filesystem capability boundary for untrusted identities.

The existing Task 10 static-v1 fixture migration dependency remains deferred to Task 10. This amendment neither changes it nor advances P3 audit state, merge readiness, a safe-clone proof, or any canonical write.

## Explicit non-scope

- No implementation code, profile/policy change, CLI behavior change, P2 artifact change, canonical mutation, source-vault synchronization, or audit regeneration occurs in this amendment.
- No claim of branch protection, OS sandbox guarantees outside the approved deployment capability, same-identity hostile-rename resistance, or automatic stale-lock recovery is made.
- A stronger multi-user, kernel-assisted, or hardware-backed lock authority is a separate future design requiring explicit review and dependency approval.
