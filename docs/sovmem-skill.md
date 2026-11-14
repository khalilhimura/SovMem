# SovMem Harness Agent Runbook

> **Status:** P5 operational hardening — advisory trust output, read-only MCP.
> **Protocol:** MCP revision 2026-07-28 (stateless, stdio, JSON-RPC 2.0).

## 1. Propose protocol

- `memory_search { "query": "...", "top_k": 5 }` — Search the verified derived index.
- `memory_get { "id": "ssa-0/..." }` — Retrieve a canonical document by ID.
- `memory_propose { "source": "...", "operation": "create|replace", "confidence": 0.85 }` — Submit an untrusted proposal.

## 2. Confidence elicitation

State your confidence **before drafting**. Scale:

- 0.9–1.0: Certain — direct evidence
- 0.7–0.9: Confident — good evidence, minor uncertainty
- 0.5–0.7: Uncertain — partial evidence
- 0.0–0.5: Speculative — weak evidence

Confidence is never required and never gates a proposal.

## 3. Skeptic stance

Default to adversarial bug-finding. Assume the proposal is wrong. Protocol: `adversarial-bug-finding-v1`.

## 4. Verdict loop

1. Proposal enters the untrusted inbox.
2. Human runs `sovmem intake seal <id>` to stage it.
3. Human runs `sovmem verdict <id> ACCEPT --reason "..."` to commit to canon.
4. The index rebuilds automatically.

## 5. Trust report

`sovmem trust` computes per-source rates, calibration gap, optimal threshold, and blindspot cells.

Trust is advisory only and never accepts proposals to canon.
