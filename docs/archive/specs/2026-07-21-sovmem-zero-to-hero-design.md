> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem: Zero to Hero — Design Specification

**Date:** 2026-07-21

**Status:** Approved design

**Product repository:** `sovmem-learn` (to be created during implementation)

**Permanent product URL:** `https://sovmem.thefutureissolo.com`

**Future course destination:** `https://course.thefutureissolo.com`

## 1. Summary

SovMem: Zero to Hero is a free, public, interactive learning product that takes a learner with no technical prerequisites from first principles to a defensible understanding of sovereign memory for a Solo Systems Architect (SSA).

The experience is a guided, self-paced course lasting approximately three to four hours when all depth layers are explored. Every lesson supports one layered journey with three views:

- **General:** concepts, analogies, value, and human decisions.
- **Developer:** tool-neutral application-integration contracts, with MCP presented as the future default transport once P4 is verified.
- **Technical:** architecture, invariants, authority boundaries, determinism, recovery, and threat models without source-code instruction.

The first release runs entirely in the browser. It has no accounts, payments, analytics, or server-side learner data. Progress is stored locally and can be exported or imported through a versioned, validated record.

## 2. Product context

The Future Is Solo (TFIS) frames the Solo Systems Architect as a sovereign individual who creates leverage through agents, automation, context engineering, and disciplined execution. That leverage is unsafe when agent-generated memory can silently become authority, when derived state is mistaken for canon, or when judgments cannot be reconstructed and verified.

SovMem is the sovereign memory subsystem within Nous. Its governing principle is that agents may read and propose, but only accountable human judgment may change canon. The course must make that value legible immediately without becoming a general TFIS orientation course.

The curriculum baseline is phase-aware:

- P0 and P1 establish the profile, bundle, provenance, and verification substrate.
- P2 provides verified, offline derived retrieval without changing canon.
- The original P3 write-back loop has delivered human verdicts, canonical generations, ledgers, rebuilds, recovery, and safe-clone evidence.
- P3 intake-authority hardening is active and must be labeled according to reviewed evidence rather than treated as silently complete.
- P4 MCP adapter and P5 operational hardening remain future phases until their gates pass.

The learning product must never flatten this distinction. Current behavior is taught as current; hardening work is labeled; planned behavior remains a horizon.

## 3. Goals

1. Explain why an SSA requires sovereign memory rather than ordinary chat history or an ungoverned knowledge base.
2. Teach SovMem from zero technical knowledge through progressively deeper views.
3. Let learners practise the authority model through deterministic, safe browser simulations.
4. Preserve fidelity to verified SovMem behavior and clearly separate delivered, hardening, planned, and reserved capabilities.
5. Make P4 and P5 content easy to activate after their build gates pass without changing stable lesson IDs or invalidating learner progress.
6. Create a recognizable sibling of TFIS AI Fluency while giving SovMem its own ledger, provenance, graph, and authority motifs.
7. Keep the curriculum and progress format portable to the future TFIS course platform.

## 4. Non-goals

- Running a real SovMem store or connecting to Audy Vault.
- Executing arbitrary code, shell commands, or learner-supplied programs.
- Teaching Rust implementation or contributor workflows.
- Duplicating the future TFIS Orientation curriculum.
- Implementing the P4 MCP adapter or P5 sentry and trust systems.
- Providing authentication, payment, certificates, cohorts, instructor dashboards, or learner analytics.
- Presenting speculative P4/P5 commands or schemas as settled product contracts.
- Creating the broader multi-course TFIS learning platform in this project.

## 5. Product boundary and routes

The product lives in a separate repository named `sovmem-learn`. During implementation, this approved design and its PRD are transferred into that repository as founding product documents.

The permanent public surface has two initial routes:

- `/` — the SovMem product front door, value proposition, capability status, and primary learning action.
- `/learn` — the standalone guided course and its stable lesson routes.

After course migration, `/` remains the product front door. Its primary learning action opens the migrated SovMem course on `course.thefutureissolo.com`. The `/learn` route becomes a clear migration handoff rather than a dead link or an invisible redirect loop.

## 6. Learner journey

1. The homepage frames agent leverage, drift, canon, and accountable human judgment.
2. The learner selects **Start learning** without creating an account.
3. Course orientation explains the three depth layers, local progress, and the current capability-status vocabulary.
4. Lessons default to General and permit depth changes without losing location or completion state.
5. Short checks and deterministic simulations reinforce each major invariant.
6. The learner completes **The Drift Incident** capstone.
7. The learner can export a private progress record for restoration or future course-platform migration.

Completion requires all core lessons, successful required checks, and protection of every critical capstone invariant. Retries are unlimited.

## 7. Curriculum

| Module | Title | Primary learning outcome | Phase status |
| --- | --- | --- | --- |
| 1 | Why an SSA Needs Sovereign Memory | Explain why agent leverage requires governed memory and human authority. | Foundation |
| 2 | Canon, Evidence, and Human Authority | Distinguish canon, proposals, judgments, advisory output, and derived state. | Foundation |
| 3 | Build a Trustworthy Memory Bundle | Explain profiles, provenance, manifests, raw-byte integrity, and verification levels. | P0/P1 verified |
| 4 | Find Knowledge Without Changing It | Explain offline retrieval, disposable indexes, BM25, graph expansion, and frozen evaluation. | P2 verified |
| 5 | Agents Propose; Humans Decide | Trace proposal staging, policy checks, and human-only verdict authority. | P3 verified plus active hardening labels |
| 6 | From Verdict to Verified Canon | Explain canonical generations, ledgers, rebuilds, and Level 4 verification. | P3 verified plus active hardening labels |
| 7 | Defend the Authority Boundary | Reason about intake custody, replay, concurrency, crash recovery, poisoning, and fail-closed behavior. | P3 hardening |
| 8 | Integrate SovMem | Design against stable, tool-neutral integration contracts; understand MCP as the future default adapter. | P4 planned |
| 9 | Operate Sovereign Memory | Understand the P5 horizon and complete the final authority-boundary capstone. | P5 planned plus current capstone |

The total target is three to four hours for a learner who explores all three layers. A General-only pass is shorter but still forms a complete conceptual journey.

## 8. Capability status model

Every capability claim uses one of four explicit states:

- **Verified:** supported by reviewed implementation and named evidence.
- **Hardening:** the original capability exists but a reviewed authority, security, or recovery amendment is active.
- **Planned:** accepted direction for an unimplemented phase; no current availability claim.
- **Reserved:** curriculum or product structure retained for later definition; no contract claim.

Status is content, not decoration. It appears in lesson metadata, learner-facing labels, and the evidence manifest. Planned and Reserved sections must not resemble paid locks or completed product UI.

## 9. Content model

Lessons, checks, simulations, capability states, and migration metadata live in versioned structured files. Content is not embedded directly into page components.

Each lesson record contains:

- Stable course, module, and lesson IDs.
- Title, summary, objectives, and estimated time.
- General, Developer, and Technical content layers.
- Phase and capability status.
- Evidence references and the reviewed source revision.
- Required checks and simulation IDs.
- Content-schema version and revision date.
- Future course-platform migration metadata.

Each phase has an independently replaceable content package. P4 and P5 packages may include scaffolded objectives and honest horizon content, but cannot include invented final contracts. Activating a future phase updates the capability manifest and its package without changing stable navigation or progress identifiers.

## 10. Application architecture

The recommended implementation is a static Astro and TypeScript application with interactive islands for simulations and assessments.

### 10.1 Product shell

Renders the permanent homepage, current capability overview, evidence-aware phase status, and learning entry point.

### 10.2 Course shell

Owns lesson routing, responsive navigation, depth selection, progress, accessibility behavior, and completion state.

### 10.3 Content registry

Loads and validates versioned lesson files during the build. Duplicate IDs, missing layers, invalid statuses, incomplete evidence references, and unresolved content markers fail the build.

### 10.4 Capability and evidence manifest

Binds learner-facing claims to named reviewed releases or commits. It is the only mechanism that may promote a capability from Planned or Hardening to Verified.

### 10.5 Simulation and assessment engine

Runs deterministic state machines from versioned scenario fixtures. Lesson files select scenarios by stable ID. Scenario logic remains independent of lesson prose.

### 10.6 Local progress manager

Persists progress in the browser, validates imported records, exports portable records, and applies explicit course-version migrations.

### 10.7 Migration exporter

Produces a validated course package containing curriculum structure, layered content, checks, simulation metadata, asset references, and stable IDs for `course.thefutureissolo.com`.

## 11. Data flow

```text
Versioned lessons + capability manifest + simulation fixtures
                              |
                     Build-time validation
                              |
                 Static product and course bundle
                              |
                  Browser rendering and simulation
                              |
             Local progress ↔ export/import record
```

There is no learner-data server, analytics pipeline, authentication service, or runtime dependency on the private SovMem repository.

## 12. Simulations and checks

The course uses six reusable simulation patterns:

1. Classify canon, proposals, judgments, advisory output, and derived state.
2. Detect altered provenance or raw-byte corruption in a bundle.
3. Query, delete, and rebuild a derived index without changing canon.
4. Trace a proposal and apply ACCEPT, REJECT, or REWRITE with a meaningful human reason.
5. Diagnose a simulated crash, incomplete publication, stale lease, corrupted ledger, or authority violation.
6. Connect a fictional application to tool-neutral SovMem contracts and inspect the future MCP boundary.

The same scenario exposes different information by depth:

- General explains consequences and judgment.
- Developer reveals application contracts and integration effects.
- Technical reveals boundaries, invariants, recovery logic, and threat-model reasoning.

Checks provide immediate explanatory feedback and unlimited retries. Their purpose is mastery, not ranking.

## 13. Capstone: The Drift Incident

An agent submits a plausible but unsafe memory update containing conflicting evidence and an attempted authority shortcut. The learner must:

1. Identify canon, untrusted evidence, advisory output, and the human authority point.
2. Detect the unsafe shortcut and any integrity or custody failure.
3. Choose ACCEPT, REJECT, or REWRITE.
4. Provide a non-empty, meaningful reason.
5. Verify the resulting canonical and ledger state.
6. Respond correctly to a simulated failure during publication or recovery.

The capstone passes only when every critical authority invariant remains protected. The learner may retry or reset the scenario without penalty.

## 14. Progress and portability

Progress is stored locally using a versioned schema. The record contains:

- Course and schema versions.
- Stable lesson and simulation IDs.
- Completion states and check outcomes.
- Capstone state.
- Depth preference.
- Created and updated timestamps.

Free-text reasoning and learner-authored content are excluded from exported progress by default. Import shows a validation summary before replacing local progress. A malformed or unsupported record is rejected and never overwrites valid state.

## 15. Visual system

The product is a recognizable sibling of TFIS AI Fluency and reuses:

- Paper `#F2EDDF`, ink `#1C2A21`, pine `#28412F`, pine-deep `#1A2E22`, and sun `#E8722C`.
- Archivo for structural headings, labels, and actions.
- Hanken Grotesk for explanation and teaching.
- Strong borders and flat, unblurred sticker shadows.
- Orange reserved for primary actions and focus.
- Paper and pine section rhythm.

SovMem-specific motifs include chain-of-custody timelines, provenance threads, ledger records, canonical generations, rebuildable graph projections, and explicit verdict seals.

Desktop uses a persistent left course rail. Mobile uses a compact header and accessible navigation drawer. Simulations reflow to stacked controls and never require hover, dragging, or precise pointing.

## 16. Accessibility

The product targets WCAG 2.2 AA and requires:

- Complete keyboard operation.
- Visible focus states.
- High contrast on paper and pine surfaces.
- No information conveyed by color alone.
- Screen-reader labels and text alternatives for diagrams.
- Reduced-motion support.
- Usable touch targets.
- Logical focus order after route, layer, and simulation-state changes.

## 17. Error handling

- Invalid content, duplicate IDs, missing layers, unresolved markers, and malformed fixtures fail the build.
- A missing or invalid evidence reference prevents a capability from becoming Verified.
- Planned phase content remains readable but cannot enter a current-capability simulation.
- Every simulation provides reset and replay controls.
- Corrupt local progress is isolated and recoverable through reset or a valid import.
- Unsupported progress versions produce a readable error and preserve the current record.
- Content-import and progress-import data is parsed as data and never executed.

## 18. Verification strategy

1. **Content:** schema, links, IDs, statuses, layers, evidence, and placeholder scans.
2. **Simulation:** expected success, failure, fail-closed, reset, and replay paths.
3. **Progress:** save, resume, reset, export/import, corruption rejection, and version migration.
4. **Interface:** keyboard, semantics, contrast, reduced motion, and responsive layouts.
5. **End to end:** complete the journey at each depth, change depth mid-lesson, finish the capstone, and restore progress in a fresh browser.
6. **Visual regression:** protect TFIS tokens, typography, sticker shadows, responsive structure, and scarce use of orange.
7. **Privacy:** verify that lesson activity, answers, and progress cause no network transmission.

## 19. Content governance and release updates

For every SovMem release or phase closure, maintainers update together:

1. Capability status and reviewed evidence.
2. Affected lesson layers.
3. Checks and simulation fixtures.
4. Course version and migration notes.
5. Migration export.

CI rejects contradictory or incomplete updates. A generated audit or stale checkout cannot independently promote a teaching claim. The evidence manifest must point to the reviewed implementation authority for that release.

## 20. Long-term migration

The standalone app is the first delivery surface, not the permanent course platform. Migration preserves:

- Stable course, module, lesson, check, and simulation IDs.
- Layered content.
- Capability and evidence metadata.
- Progress schema or an explicit conversion mapping.
- TFIS visual tokens and SovMem-specific motifs.

After migration, `sovmem.thefutureissolo.com` remains the public product front door. It points learners to the SovMem course on `course.thefutureissolo.com` alongside AI Fluency and TFIS Orientation.

## 21. Approved decisions

- Layered mastery is the definition of Hero.
- One layered journey replaces separate audience tracks.
- Current truth plus a clearly labeled future horizon anchors the curriculum.
- The course is interactive-first and uses safe browser simulations.
- Completion uses checks plus a capstone.
- Entry requires no technical prerequisites.
- TFIS and SSA receive brief framing; SovMem remains the focus.
- Content is versioned and phase-aware.
- The app is responsive everywhere.
- Progress is local with export/import.
- The app is a recognizable sibling of TFIS AI Fluency.
- Technical depth is architecture-only.
- Developer depth is application integration, contract-first and tool-neutral, with MCP as the future default.
- The app collects no analytics.
- Access is free and public.
- The product name is SovMem: Zero to Hero.
- The permanent URL is `sovmem.thefutureissolo.com`.
- The future repository is `sovmem-learn`.
