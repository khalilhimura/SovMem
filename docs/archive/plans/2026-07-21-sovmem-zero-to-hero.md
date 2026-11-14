> Public-copy note: workstation paths are environment placeholders; the original private historical plan is retained in Nous. Set the named variables to your own locations before adapting any archived commands.

> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem: Zero to Hero Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a free, public, phase-aware SovMem learning product that takes a learner with no technical prerequisites through layered instruction, safe simulations, local progress, and The Drift Incident capstone.

**Architecture:** Create a separate static Astro 7 and TypeScript repository named `sovmem-learn`. Astro renders the permanent product front door and lesson routes; small React islands own stateful controls, checks, simulations, and local progress. Versioned JSON content, capability evidence, checks, and simulation fixtures are validated at build time and exported through a migration package without any learner-data backend.

**Tech Stack:** Node.js 22.12.0 or newer; Astro 7.1.x; TypeScript strict mode; React through the official `@astrojs/react` integration; Zod; Vitest; Testing Library; Playwright; `@axe-core/playwright`; local `@fontsource/archivo` and `@fontsource/hanken-grotesk` assets.

## Global Constraints

- Create the product in `${LEARNING_REPO}`; do not add application code to `${NOUS_ORIGIN}`.
- Treat the approved design at `docs/superpowers/specs/2026-07-21-sovmem-zero-to-hero-design.md` and PRD at `docs/product/2026-07-21-sovmem-zero-to-hero-prd.md` as the product contract.
- Confirm `pwd -P`, the new repository root, branch, and exact commit before every task.
- Do not modify the active SovMem P3 worktree or its dirty files. Read it only when refreshing reviewed capability evidence.
- Require Node.js `>=22.12.0`, matching current Astro support guidance.
- Resolve and commit exact dependency versions in `package-lock.json`; do not use uncommitted floating dependencies after bootstrap.
- Generate static output only. Do not add a database, server adapter, authentication, payment, analytics, telemetry, cookies, or runtime content API.
- Do not transmit lesson activity, answers, reasoning, or progress. Self-host fonts through npm packages so lesson rendering does not contact a font CDN.
- Parse imported progress and content strictly as data. Never evaluate imported code or HTML.
- General is the default depth. Developer is contract-first application integration. Technical is architecture-only.
- MCP is Planned until P4 is verified. P5 operational capabilities remain Planned until their evidence gate passes.
- Use only `verified`, `hardening`, `planned`, or `reserved` capability states.
- A capability may become `verified` only through an evidence-manifest change tied to a reviewed release or commit.
- Preserve stable course, module, lesson, check, simulation, and capstone IDs after publication.
- Reuse TFIS paper `#F2EDDF`, ink `#1C2A21`, pine `#28412F`, pine-deep `#1A2E22`, and sun `#E8722C`; reserve solid sun for primary actions and focus.
- Use Archivo for headings, labels, and actions; Hanken Grotesk for teaching copy.
- Use flat, unblurred ink shadows. A blurred shadow fails visual review.
- Meet WCAG 2.2 AA and keep every course and simulation action operable by keyboard at mobile and desktop widths.
- Use fixture-first TDD: demonstrate a non-zero failing test for the intended reason, implement one invariant, then run focused and cumulative checks.
- End every task with exact diff review, fresh verification, a focused commit, and an independent review gate before advancing.
- Do not create a GitHub repository, remote, deployment, DNS record, or public release without separate approval.

---

## File Structure

```text
${LEARNING_REPO}/
├── .github/workflows/ci.yml
├── .nvmrc
├── astro.config.mjs
├── package.json
├── package-lock.json
├── playwright.config.ts
├── tsconfig.json
├── vitest.config.ts
├── public/
│   ├── favicon.svg
│   ├── robots.txt
│   └── social-card.svg
├── scripts/
│   ├── export-course.ts
│   └── validate-content.ts
├── src/
│   ├── components/
│   │   ├── course/CourseNav.astro
│   │   ├── course/DepthSelector.tsx
│   │   ├── course/LessonBlocks.astro
│   │   ├── course/ProgressActions.tsx
│   │   ├── simulations/SimulationHost.tsx
│   │   ├── assessments/CheckHost.tsx
│   │   └── capstone/CapstoneHost.tsx
│   ├── content/
│   │   ├── capabilities.json
│   │   ├── evidence.json
│   │   ├── modules.json
│   │   ├── lessons/*.json
│   │   ├── checks/*.json
│   │   └── simulations/*.json
│   ├── domain/
│   │   ├── content.ts
│   │   ├── progress.ts
│   │   └── simulation.ts
│   ├── layouts/
│   │   ├── ProductLayout.astro
│   │   └── CourseLayout.astro
│   ├── lib/
│   │   ├── content/catalog.ts
│   │   ├── content/evidence.ts
│   │   ├── progress/storage.ts
│   │   ├── progress/transfer.ts
│   │   └── simulation/reducer.ts
│   ├── pages/
│   │   ├── index.astro
│   │   ├── learn/index.astro
│   │   └── learn/[module]/[lesson].astro
│   └── styles/
│       ├── tokens.css
│       └── global.css
└── tests/
    ├── fixtures/
    ├── setup.ts
    ├── unit/
    └── e2e/
```

## Shared Interfaces

```ts
export type AudienceDepth = "general" | "developer" | "technical";
export type CapabilityStatus = "verified" | "hardening" | "planned" | "reserved";

export type ContentBlock =
  | { type: "paragraph"; text: string }
  | { type: "heading"; level: 2 | 3; text: string }
  | { type: "callout"; tone: "principle" | "warning" | "horizon"; title: string; text: string }
  | { type: "diagram"; diagramId: string; alt: string }
  | { type: "artifact"; artifactId: string; label: string; body: string }
  | { type: "simulation"; simulationId: string }
  | { type: "check"; checkId: string };

export interface LessonRecord {
  schemaVersion: 1;
  id: string;
  moduleId: string;
  order: number;
  slug: string;
  title: string;
  summary: string;
  objectives: string[];
  estimatedMinutes: number;
  phase: "foundation" | "p0-p1" | "p2" | "p3" | "p3-v2" | "p4" | "p5";
  capabilityStatus: CapabilityStatus;
  evidenceRefs: string[];
  layers: Record<AudienceDepth, ContentBlock[]>;
  checkIds: string[];
  simulationIds: string[];
  migration: { courseKey: "sovmem-zero-to-hero"; moduleKey: string; lessonKey: string };
}

export interface ProgressRecord {
  schemaVersion: 1;
  courseId: "sovmem-zero-to-hero";
  courseVersion: string;
  depth: AudienceDepth;
  completedLessonIds: string[];
  passedCheckIds: string[];
  simulationStates: Record<string, string>;
  capstoneState: "not-started" | "in-progress" | "passed";
  createdAt: string;
  updatedAt: string;
}

export interface SimulationDefinition {
  schemaVersion: 1;
  id: string;
  kind: "classify" | "verify" | "rebuild" | "verdict" | "recover" | "integrate" | "capstone";
  title: string;
  initialState: string;
  states: Record<string, { message: string; terminal?: "pass" | "fail" }>;
  transitions: Array<{ from: string; action: string; to: string; explanation: string }>;
  protectedInvariants: string[];
}
```

---

### Task 1: Bootstrap the Separate Static Application and Test Gate

**Files:**
- Create: `${LEARNING_REPO}/.nvmrc`
- Create: `${LEARNING_REPO}/package.json`
- Create: `${LEARNING_REPO}/astro.config.mjs`
- Create: `${LEARNING_REPO}/tsconfig.json`
- Create: `${LEARNING_REPO}/vitest.config.ts`
- Create: `${LEARNING_REPO}/playwright.config.ts`
- Create: `${LEARNING_REPO}/src/styles/tokens.css`
- Create: `${LEARNING_REPO}/src/styles/global.css`
- Create: `${LEARNING_REPO}/tests/setup.ts`
- Create: `${LEARNING_REPO}/tests/unit/design-tokens.test.ts`
- Create: `${LEARNING_REPO}/.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Approved design tokens and the Node/Astro constraints above.
- Produces: A local Git repository, static Astro build, strict TypeScript gate, unit-test gate, browser-test gate, and shared CSS variables used by every later task.

- [ ] **Step 1: Confirm the parent directory and create the repository scaffold**

Run:

```bash
cd "${PROJECTS_PARENT}"
npm create astro@latest sovmem-learn -- --template minimal --add react --install --git --yes --no-ai
cd "${LEARNING_REPO}"
```

Expected: a new local `sovmem-learn` Git repository using the current Astro 7 release and the official React integration. Do not add a remote.

- [ ] **Step 2: Pin the runtime and install the bounded dependency set**

Create `.nvmrc` with:

```text
22.12.0
```

Run:

```bash
npm install zod @fontsource/archivo @fontsource/hanken-grotesk
npm install --save-dev @astrojs/check typescript tsx vitest jsdom @testing-library/react @testing-library/jest-dom @playwright/test @axe-core/playwright
npx playwright install chromium
```

Expected: `package-lock.json` records exact versions; no database, analytics, auth, or server adapter appears in dependencies.

- [ ] **Step 3: Write the failing design-token test**

Create `tests/unit/design-tokens.test.ts`:

```ts
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("TFIS sibling tokens", () => {
  it("keeps the approved palette, typography, and flat-shadow rule", () => {
    const css = readFileSync("src/styles/tokens.css", "utf8");
    expect(css).toContain("--paper: #f2eddf");
    expect(css).toContain("--ink: #1c2a21");
    expect(css).toContain("--pine: #28412f");
    expect(css).toContain("--pine-deep: #1a2e22");
    expect(css).toContain("--sun: #e8722c");
    expect(css).toContain("--display: 'Archivo'");
    expect(css).toContain("--body: 'Hanken Grotesk'");
    expect(css).toContain("--shadow-card: 5px 5px 0 var(--ink)");
    expect(css).not.toMatch(/box-shadow:\s*[^;]*(blur|rgba)/i);
  });
});
```

- [ ] **Step 4: Run RED and confirm at least one test executes**

Run:

```bash
npx vitest run tests/unit/design-tokens.test.ts
```

Expected: one test fails because `src/styles/tokens.css` does not exist.

- [ ] **Step 5: Implement the shared tokens and scripts**

Create `src/styles/tokens.css`:

```css
:root {
  --paper: #f2eddf;
  --ink: #1c2a21;
  --pine: #28412f;
  --pine-deep: #1a2e22;
  --sun: #e8722c;
  --line: rgb(28 42 33 / 18%);
  --display: 'Archivo', sans-serif;
  --body: 'Hanken Grotesk', sans-serif;
  --radius: 12px;
  --shadow-card: 5px 5px 0 var(--ink);
  --shadow-action: 4px 4px 0 var(--ink);
  --content-max: 72rem;
}
```

Create `src/styles/global.css` with the font imports, box sizing, paper background, ink text, focus-visible sun outline, reduced-motion override, 44-pixel minimum interactive targets, and flat-shadow `.card` and `.primary-action` primitives. Do not add a dark-mode palette or blurred shadow.

```css
@import "@fontsource/archivo/700.css";
@import "@fontsource/hanken-grotesk/400.css";
@import "@fontsource/hanken-grotesk/600.css";
@import "./tokens.css";

*, *::before, *::after { box-sizing: border-box; }

html { background: var(--paper); color: var(--ink); font-family: var(--body); }
body { margin: 0; min-width: 20rem; }
h1, h2, h3 { font-family: var(--display); line-height: 1.05; }
a { color: inherit; }
button, a, input, select { min-block-size: 44px; }
:focus-visible { outline: 3px solid var(--sun); outline-offset: 3px; }
.card { border: 2px solid var(--ink); border-radius: var(--radius); box-shadow: var(--shadow-card); }
.primary-action { background: var(--sun); border: 2px solid var(--ink); box-shadow: var(--shadow-action); color: var(--ink); font-weight: 700; }

@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    scroll-behavior: auto !important;
    transition-duration: 0.01ms !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
  }
}
```

Set `astro.config.mjs` to static output with the official React integration:

```js
import { defineConfig } from "astro/config";
import react from "@astrojs/react";

export default defineConfig({
  output: "static",
  integrations: [react()],
});
```

Create `tests/setup.ts`:

```ts
import "@testing-library/jest-dom/vitest";
```

Create `vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/unit/**/*.test.{ts,tsx}"],
  },
});
```

Create `playwright.config.ts`:

```ts
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  webServer: {
    command: "npm run dev -- --host 127.0.0.1",
    url: "http://127.0.0.1:4321",
    reuseExistingServer: !process.env.CI,
  },
  use: {
    baseURL: "http://127.0.0.1:4321",
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
```

Set these scripts in `package.json`:

```json
{
  "scripts": {
    "dev": "astro dev",
    "build": "astro check && astro build",
    "check": "astro check",
    "test": "vitest run",
    "test:e2e": "playwright test",
    "validate:content": "tsx scripts/validate-content.ts",
    "export:course": "tsx scripts/export-course.ts"
  },
  "engines": { "node": ">=22.12.0" }
}
```

- [ ] **Step 6: Add the CI contract**

Create `.github/workflows/ci.yml` as future repository policy. It is not published or run remotely at this gate.

```yaml
name: ci

on:
  pull_request:
  push:
    branches: [main]

jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22.12.0
          cache: npm
      - run: npm ci
      - run: npx playwright install --with-deps chromium
      - run: npm run check
      - run: npm test
      - run: npm run validate:content
      - run: npm run build
      - run: npm run test:e2e
```

- [ ] **Step 7: Run GREEN and the scaffold gate**

Run:

```bash
npx vitest run tests/unit/design-tokens.test.ts
npm run check
npm run build
git diff --check
```

Expected: one token test passes; Astro check and static build exit zero; diff check is empty.

- [ ] **Step 8: Commit and stop for review**

```bash
git add .nvmrc .github astro.config.mjs package.json package-lock.json playwright.config.ts tsconfig.json vitest.config.ts src/styles tests/setup.ts tests/unit/design-tokens.test.ts
git commit -m "chore: bootstrap SovMem learning app"
```

Require independent review of the dependency boundary, static-output configuration, scripts, tokens, and test evidence before Task 2.

---

### Task 2: Define the Phase-Aware Content and Evidence Contracts

**Files:**
- Create: `src/domain/content.ts`
- Create: `src/content/modules.json`
- Create: `src/content/capabilities.json`
- Create: `src/content/evidence.json`
- Create: `src/lib/content/catalog.ts`
- Create: `src/lib/content/evidence.ts`
- Create: `scripts/validate-content.ts`
- Create: `tests/unit/content-schema.test.ts`
- Create: `tests/unit/evidence-status.test.ts`
- Create: `tests/fixtures/lesson.valid.json`

**Interfaces:**
- Consumes: `AudienceDepth`, `CapabilityStatus`, `ContentBlock`, and `LessonRecord` from Shared Interfaces.
- Produces: `LessonSchema`, `CapabilityManifestSchema`, `loadLessons(): LessonRecord[]`, `getLesson(id): LessonRecord`, and `validateEvidencePromotion()`.

- [ ] **Step 1: Write RED fixtures for valid content and unsafe promotion**

Create a valid lesson fixture with all three non-empty layers and a Hardening capability. Add tests that require:

```ts
expect(() => LessonSchema.parse(validLesson)).not.toThrow();
expect(() => LessonSchema.parse({ ...validLesson, layers: { general: [] } })).toThrow();
expect(() => validateEvidencePromotion({ status: "verified", evidenceRefs: [] })).toThrow(
  "verified capabilities require reviewed evidence",
);
expect(() => validateEvidencePromotion({ status: "planned", evidenceRefs: [] })).not.toThrow();
```

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/content-schema.test.ts tests/unit/evidence-status.test.ts
```

Expected: at least four tests execute and fail because the schemas and validators do not exist.

- [ ] **Step 3: Implement strict Zod schemas**

In `src/domain/content.ts`, define discriminated content-block schemas, the four status literals, all seven phase literals, stable ID patterns (`mNN-lNN`, `check-*`, `sim-*`), non-empty layer arrays, evidence references, and migration keys. Use `.strict()` on every object so unknown fields fail validation.

The promotion rule must be:

```ts
export function validateEvidencePromotion(input: {
  status: CapabilityStatus;
  evidenceRefs: string[];
}): void {
  if (input.status === "verified" && input.evidenceRefs.length === 0) {
    throw new Error("verified capabilities require reviewed evidence");
  }
}
```

- [ ] **Step 4: Seed the stable module and capability registries**

Create exactly nine module IDs:

```json
[
  { "id": "m01", "order": 1, "title": "Why an SSA Needs Sovereign Memory" },
  { "id": "m02", "order": 2, "title": "Canon, Evidence, and Human Authority" },
  { "id": "m03", "order": 3, "title": "Build a Trustworthy Memory Bundle" },
  { "id": "m04", "order": 4, "title": "Find Knowledge Without Changing It" },
  { "id": "m05", "order": 5, "title": "Agents Propose; Humans Decide" },
  { "id": "m06", "order": 6, "title": "From Verdict to Verified Canon" },
  { "id": "m07", "order": 7, "title": "Defend the Authority Boundary" },
  { "id": "m08", "order": 8, "title": "Integrate SovMem" },
  { "id": "m09", "order": 9, "title": "Operate Sovereign Memory" }
]
```

Seed capability states as Foundation/P0-P2 Verified, the P3 write-back surface Hardening while the intake-authority amendment remains active, P4 Planned, and P5 Planned. Evidence entries must distinguish a closure revision from an active hardening revision and may mark private sources as non-linkable.

- [ ] **Step 5: Implement catalog loading and full validation**

In browser/runtime code, `src/lib/content/catalog.ts` uses `import.meta.glob("../../content/lessons/*.json", { eager: true, import: "default" })`, parses every entry, rejects duplicate lesson IDs and duplicate migration keys, verifies module IDs, evidence IDs, check references, and simulation references, then sorts by module order and lesson order.

The Node-side `scripts/validate-content.ts` must not import the Vite-only catalog loader. It uses `node:fs/promises` to enumerate and parse the same JSON files, then calls the shared Zod schemas and pure cross-reference validator exported from `src/domain/content.ts`. This keeps browser loading and command-line validation consistent without depending on `import.meta.glob` outside Astro.

`scripts/validate-content.ts` must exit non-zero on the first aggregated validation report and print one stable summary on success:

```text
content valid: 9 modules · 17 lessons · 4 capability states · 0 unresolved markers
```

The lesson count is reached after Tasks 7-11; before then, accept the current count but always require exactly nine modules.

- [ ] **Step 6: Run GREEN and cumulative checks**

Run:

```bash
npx vitest run tests/unit/content-schema.test.ts tests/unit/evidence-status.test.ts
npm test
npm run check
git diff --check
```

Expected: focused and cumulative tests pass; type checking exits zero.

- [ ] **Step 7: Commit and stop for review**

```bash
git add src/domain/content.ts src/content/modules.json src/content/capabilities.json src/content/evidence.json src/lib/content scripts/validate-content.ts tests/unit tests/fixtures/lesson.valid.json
git commit -m "feat: define phase-aware course contracts"
```

Require review of strict parsing, evidence promotion, P3 Hardening language, and stable ID policy before Task 3.

---

### Task 3: Build the Permanent Product Front Door

**Files:**
- Create: `src/layouts/ProductLayout.astro`
- Create: `src/pages/index.astro`
- Create: `src/components/product/CapabilityStrip.astro`
- Create: `src/components/product/AuthorityLoop.astro`
- Create: `public/favicon.svg`
- Create: `public/social-card.svg`
- Create: `public/robots.txt`
- Create: `tests/e2e/product-home.spec.ts`

**Interfaces:**
- Consumes: capability status records and global TFIS tokens.
- Produces: the permanent `/` product page and a single primary `/learn` action.

- [ ] **Step 1: Write the failing product-page browser test**

Create `tests/e2e/product-home.spec.ts` asserting:

```ts
await page.goto("/");
await expect(page.getByRole("heading", { name: "SovMem: Zero to Hero" })).toBeVisible();
await expect(page.getByText("Sovereign Memory for the Solo Systems Architect")).toBeVisible();
await expect(page.getByRole("link", { name: "Start learning" })).toHaveAttribute("href", "/learn");
await expect(page.getByText("Agents propose. Humans decide.")).toBeVisible();
await expect(page.getByText("P4 · Planned")).toBeVisible();
await expect(page.getByText("P5 · Planned")).toBeVisible();
```

- [ ] **Step 2: Run RED**

Run:

```bash
npm run test:e2e -- tests/e2e/product-home.spec.ts
```

Expected: the test fails because the approved homepage content is absent.

- [ ] **Step 3: Implement the product layout and homepage**

Build a static page with these sections in order:

1. Hero: product name, subtitle, plain-language promise, one Start learning action.
2. SSA problem: agent leverage, drift, hidden authority, and why chat history is not canon.
3. Authority loop: Read → Propose → Human verdict → Verified canon → Rebuildable retrieval.
4. Capability strip: P0/P1 Verified, P2 Verified, P3 Hardening, P4 Planned, P5 Planned.
5. Three depth views: General, Developer, Technical.
6. Capstone promise: The Drift Incident.
7. Privacy promise: free, public, no account, progress stays local.
8. Final Start learning action using the same wording.

Use the AI Fluency sibling system but introduce ledger, provenance-thread, generation-stack, and verdict-seal motifs. Do not copy the AI Fluency course shell markup.

- [ ] **Step 4: Run GREEN, accessibility smoke, and mobile smoke**

Extend the test with Axe and a 375×812 viewport. Assert zero serious or critical Axe violations, no horizontal scroll, and a visible Start learning action.

Run:

```bash
npm run test:e2e -- tests/e2e/product-home.spec.ts
npm run build
git diff --check
```

Expected: product E2E and build pass.

- [ ] **Step 5: Commit and stop for review**

```bash
git add src/layouts/ProductLayout.astro src/pages/index.astro src/components/product public tests/e2e/product-home.spec.ts
git commit -m "feat: add SovMem product front door"
```

Require visual, copy, mobile, status-label, and single-action review before Task 4.

---

### Task 4: Build the Guided Course Shell and Layered Lesson Route

**Files:**
- Create: `src/layouts/CourseLayout.astro`
- Create: `src/pages/learn/index.astro`
- Create: `src/pages/learn/[module]/[lesson].astro`
- Create: `src/components/course/CourseNav.astro`
- Create: `src/components/course/DepthSelector.tsx`
- Create: `src/components/course/LessonBlocks.astro`
- Create: `tests/unit/depth-selector.test.tsx`
- Create: `tests/e2e/course-shell.spec.ts`

**Interfaces:**
- Consumes: `loadLessons()`, `LessonRecord`, `AudienceDepth`, nine module records, and shared styles.
- Produces: static lesson routes, General-default depth selection, responsive course navigation, and depth-preserving lesson position.

- [ ] **Step 1: Write RED tests for General default and depth persistence**

Test `DepthSelector` with:

```tsx
render(<DepthSelector initialDepth="general" storageKey="sovmem-depth" />);
expect(screen.getByRole("radio", { name: "General" })).toBeChecked();
await user.click(screen.getByRole("radio", { name: "Developer" }));
expect(localStorage.getItem("sovmem-depth")).toBe('"developer"');
expect(screen.getByRole("radio", { name: "Developer" })).toBeChecked();
```

Add E2E assertions for `/learn`, a visible nine-module trail, responsive navigation, and focus moving to the lesson heading after route navigation.

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/depth-selector.test.tsx
npm run test:e2e -- tests/e2e/course-shell.spec.ts
```

Expected: both focused suites fail because the shell and selector do not exist.

- [ ] **Step 3: Implement the depth selector**

Use a radio group with exactly three values. Read the saved preference only after hydration, fall back to General on invalid data, dispatch a `sovmem:depth-change` custom event, and never mark depth as completion.

- [ ] **Step 4: Implement the course layout and routes**

`/learn` renders orientation and the first available lesson action. Dynamic lesson routes come only from validated catalog entries. Desktop uses a sticky left rail. Mobile uses a button-controlled navigation dialog with focus return. Every page includes module position, capability state, objectives, estimated time, and the layered content region.

`LessonBlocks.astro` must render only the selected layer's structured blocks and must escape text and artifact bodies by default. Diagram and simulation IDs resolve through explicit component maps, never dynamic evaluation.

- [ ] **Step 5: Run GREEN and regression checks**

Run:

```bash
npx vitest run tests/unit/depth-selector.test.tsx
npm run test:e2e -- tests/e2e/course-shell.spec.ts
npm test
npm run check
npm run build
```

Expected: all commands exit zero; General remains the default; switching depth preserves the route.

- [ ] **Step 6: Commit and stop for review**

```bash
git add src/layouts/CourseLayout.astro src/pages/learn src/components/course tests/unit/depth-selector.test.tsx tests/e2e/course-shell.spec.ts
git commit -m "feat: add layered guided course shell"
```

Require navigation, keyboard, depth semantics, mobile drawer, and static-route review before Task 5.

---

### Task 5: Implement Private Local Progress and Safe Export/Import

**Files:**
- Create: `src/domain/progress.ts`
- Create: `src/lib/progress/storage.ts`
- Create: `src/lib/progress/transfer.ts`
- Create: `src/components/course/ProgressActions.tsx`
- Create: `tests/unit/progress-storage.test.ts`
- Create: `tests/unit/progress-transfer.test.ts`
- Create: `tests/fixtures/progress-v1.valid.json`
- Create: `tests/fixtures/progress-v1.corrupt.json`

**Interfaces:**
- Consumes: stable lesson/check/simulation IDs and `AudienceDepth`.
- Produces: `createProgress()`, `loadProgress()`, `saveProgress()`, `resetProgress()`, `exportProgress()`, `previewImport()`, and `commitImport()`.

- [ ] **Step 1: Write RED tests for non-destructive import**

Require these cases:

```ts
expect(loadProgress(emptyStorage)).toEqual(createProgress("1.0.0"));
expect(exportProgress(validProgress)).not.toContain("freeText");
expect(previewImport(corruptJson).ok).toBe(false);
expect(storage.getItem(PROGRESS_KEY)).toBe(existingSerializedProgress);
expect(previewImport(validJson)).toMatchObject({ ok: true, lessonCount: 2 });
commitImport(validJson, storage);
expect(loadProgress(storage).completedLessonIds).toEqual(["m01-l01", "m01-l02"]);
```

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/progress-storage.test.ts tests/unit/progress-transfer.test.ts
```

Expected: tests fail because the progress domain and transfer functions do not exist.

- [ ] **Step 3: Implement the strict progress schema and storage adapter**

Use one localStorage key: `tfis.sovmem-learn.progress`. Normalize arrays into stable course order, reject unknown IDs during import, preserve `createdAt`, update `updatedAt` on mutation, and return a recoverable empty record if stored JSON is corrupt while retaining the corrupt bytes under `tfis.sovmem-learn.progress.quarantine`.

- [ ] **Step 4: Implement preview-before-commit import**

`previewImport(text)` parses and validates without writing. `commitImport(text, storage)` calls preview again and writes only on success. Export uses MIME `application/json`, filename `sovmem-zero-to-hero-progress-v1.json`, and excludes free-text fields by schema.

- [ ] **Step 5: Implement learner controls**

`ProgressActions.tsx` provides Export progress, Import progress, and Reset progress. Import shows course version, completed lesson count, passed check count, and capstone state before a separate Confirm import action. Reset requires a local confirmation dialog and has no network effect.

- [ ] **Step 6: Run GREEN and cumulative checks**

Run:

```bash
npx vitest run tests/unit/progress-storage.test.ts tests/unit/progress-transfer.test.ts
npm test
npm run check
```

Expected: round-trip, quarantine, preview, non-destructive rejection, and reset tests pass.

- [ ] **Step 7: Commit and stop for review**

```bash
git add src/domain/progress.ts src/lib/progress src/components/course/ProgressActions.tsx tests/unit/progress-storage.test.ts tests/unit/progress-transfer.test.ts tests/fixtures/progress-v1.*.json
git commit -m "feat: add private portable course progress"
```

Require privacy, schema, timestamp, corrupt-state, and non-destructive import review before Task 6.

---

### Task 6: Implement the Deterministic Simulation and Check Engines

**Files:**
- Create: `src/domain/simulation.ts`
- Create: `src/lib/simulation/reducer.ts`
- Create: `src/components/simulations/SimulationHost.tsx`
- Create: `src/components/assessments/CheckHost.tsx`
- Create: `tests/unit/simulation-reducer.test.ts`
- Create: `tests/unit/check-host.test.tsx`
- Create: `tests/fixtures/simulation.valid.json`

**Interfaces:**
- Consumes: `SimulationDefinition`, validated check JSON, `AudienceDepth`, and progress callbacks.
- Produces: `transition(definition, state, action)`, reset/replay behavior, invariant results, immediate check feedback, and completion events.

- [ ] **Step 1: Write RED transition tests**

Use a fixture with `inspect`, `accept`, and `reject` actions. Require an invalid action to remain in the same state with a typed error, unsafe acceptance to end in `fail`, safe rejection to end in `pass`, and reset to return the exact initial state.

```ts
expect(transition(def, "submitted", "accept").state).toBe("authority-violation");
expect(transition(def, "submitted", "reject").terminal).toBe("pass");
expect(reset(def)).toBe(def.initialState);
expect(() => transition(def, "missing", "accept")).toThrow("unknown simulation state");
```

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/simulation-reducer.test.ts tests/unit/check-host.test.tsx
```

Expected: tests fail because the reducer and hosts do not exist.

- [ ] **Step 3: Implement a pure fail-closed reducer**

The reducer must not read time, randomness, storage, or network state. It validates the definition once, looks up exactly one `(from, action)` transition, and returns `{ state, message, explanation, terminal, protectedInvariants }`. A missing transition returns a typed invalid-action result without mutating state. An unknown current state throws a corruption error.

- [ ] **Step 4: Implement accessible simulation and check hosts**

Simulation actions are ordinary buttons in a labeled group. State changes announce through `aria-live="polite"`; pass/fail never relies on color alone. Reset is always available. Check feedback explains the relevant SovMem principle and permits unlimited retries. Hosts call progress callbacks only after a terminal pass.

- [ ] **Step 5: Run GREEN and cumulative checks**

Run:

```bash
npx vitest run tests/unit/simulation-reducer.test.ts tests/unit/check-host.test.tsx
npm test
npm run check
```

Expected: deterministic, invalid-action, reset, accessibility-label, and retry tests pass.

- [ ] **Step 6: Commit and stop for review**

```bash
git add src/domain/simulation.ts src/lib/simulation src/components/simulations src/components/assessments tests/unit/simulation-reducer.test.ts tests/unit/check-host.test.tsx tests/fixtures/simulation.valid.json
git commit -m "feat: add deterministic learning interactions"
```

Require fail-closed, determinism, keyboard, feedback, and progress-event review before Task 7.

---

### Task 7: Author Modules 1-2 and the Evidence-Classification Lab

**Files:**
- Create: `src/content/lessons/m01-l01.json`
- Create: `src/content/lessons/m01-l02.json`
- Create: `src/content/lessons/m02-l01.json`
- Create: `src/content/lessons/m02-l02.json`
- Create: `src/content/checks/check-ssa-memory-value.json`
- Create: `src/content/checks/check-authority-types.json`
- Create: `src/content/simulations/sim-classify-evidence.json`
- Create: `tests/unit/modules-foundation.test.ts`
- Create: `tests/e2e/foundation-journey.spec.ts`

**Interfaces:**
- Consumes: lesson, check, and simulation schemas plus `SimulationHost`.
- Produces: the no-prerequisite TFIS/SSA framing, canon/authority mental model, and the first complete learner journey slice.

- [ ] **Step 1: Write RED curriculum-contract tests**

Assert the four exact lesson titles and require the General layer to define `agent`, `SSA`, `memory`, `canon`, `proposal`, `judgment`, `advisory`, and `derived` before using each term in a check. Assert that the classification simulation contains all five state categories and that only a human judgment can authorize canon mutation.

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/modules-foundation.test.ts
```

Expected: the suite fails because the four lessons and fixtures are absent.

- [ ] **Step 3: Author the exact foundation lesson contracts**

Use these stable records:

| ID | Title | Required General promise | Developer addition | Technical addition |
| --- | --- | --- | --- | --- |
| `m01-l01` | From Leverage to Drift | An SSA gains leverage from agents but remains accountable for direction and judgment. | Applications must know whether they are reading, proposing, or requesting a human decision. | Authority is a separate system boundary from model capability. |
| `m01-l02` | Why Chat History Is Not Sovereign Memory | Chat history is convenient context, not verified canon. | Integration convenience must not silently become commit authority. | Persistence without provenance, integrity, and replay is not sovereign memory. |
| `m02-l01` | Canon Is a Human Decision | Canon is the accepted record governed by accountable judgment. | Applications submit proposals and consume verified results. | Advisory confidence cannot widen the authority boundary. |
| `m02-l02` | Five Kinds of State | Teach canon, proposal, judgment, advisory output, and derived state through one household-budget analogy and one agent-system example. | Map each state to a tool-neutral envelope. | Explain which state is authoritative, append-only, or rebuildable. |

Each General layer begins in plain language, then introduces the term. Each deeper layer adds content rather than replacing the General explanation.

- [ ] **Step 4: Implement `sim-classify-evidence`**

The learner classifies five artifacts. The simulation passes only when the human verdict is Judgment, the accepted document is Canon, the model recommendation is Advisory, the submitted edit is Proposal, and the search index is Derived. Marking the recommendation as Canon must explain that confidence is not authority.

- [ ] **Step 5: Run GREEN and the foundation E2E journey**

Run:

```bash
npx vitest run tests/unit/modules-foundation.test.ts
npm run validate:content
npm run test:e2e -- tests/e2e/foundation-journey.spec.ts
```

Expected: four lessons validate; the classification lab passes by keyboard at all depths.

- [ ] **Step 6: Commit and stop for review**

```bash
git add src/content/lessons/m01-*.json src/content/lessons/m02-*.json src/content/checks/check-ssa-memory-value.json src/content/checks/check-authority-types.json src/content/simulations/sim-classify-evidence.json tests/unit/modules-foundation.test.ts tests/e2e/foundation-journey.spec.ts
git commit -m "content: teach sovereign-memory foundations"
```

Require zero-prerequisite clarity, TFIS scope, terminology order, and authority-model review before Task 8.

---

### Task 8: Author Modules 3-4 and the Verify/Rebuild Labs

**Files:**
- Create: `src/content/lessons/m03-l01.json`
- Create: `src/content/lessons/m03-l02.json`
- Create: `src/content/lessons/m04-l01.json`
- Create: `src/content/lessons/m04-l02.json`
- Create: `src/content/checks/check-bundle-integrity.json`
- Create: `src/content/checks/check-derived-state.json`
- Create: `src/content/simulations/sim-verify-bundle.json`
- Create: `src/content/simulations/sim-rebuild-index.json`
- Create: `tests/unit/modules-read.test.ts`
- Create: `tests/e2e/read-journey.spec.ts`

**Interfaces:**
- Consumes: verified P0-P2 capability records and the simulation engine.
- Produces: bundle/provenance/integrity learning and deterministic read-only retrieval learning.

- [ ] **Step 1: Write RED tests for frozen and rebuildable boundaries**

Require the content to state that verified Markdown/bundle evidence is authoritative, indexes and graph projections are derived, deletion and rebuild cannot change canon, BM25 rank remains separate from one-hop expansion, and frozen evaluation inputs cannot be edited to make a gate pass.

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/modules-read.test.ts
```

Expected: failures identify the four missing lessons and two missing simulations.

- [ ] **Step 3: Author the exact read-path lesson contracts**

| ID | Title | Required concepts |
| --- | --- | --- |
| `m03-l01` | Profile, Bundle, and Provenance | Profile isolation, canonical source lineage, manifests, document identity, and why provenance is evidence rather than decoration. |
| `m03-l02` | Verify Before Retrieval | Cumulative verification, raw-byte integrity, complete file-set checks, stable diagnostics, and stopping on ambiguity. |
| `m04-l01` | Derived Indexes Are Disposable | Canon remains authoritative; index and graph are rebuildable projections; deleting an index is not deleting memory. |
| `m04-l02` | Search, Graphs, and Frozen Evaluation | Offline BM25, title/tag/body boosts, separate one-hop results, fixed golden queries, and no tuning frozen truth to force GO. |

Developer content describes read-only query responses and stable metadata. Technical content explains deterministic rebuilds and frozen-contract isolation without teaching Tantivy or Rust internals.

- [ ] **Step 4: Implement the verify and rebuild fixtures**

`sim-verify-bundle` presents one intact document, one byte-altered document, and one provenance mismatch. Only stopping on the first invalid authoritative input passes. `sim-rebuild-index` requires deleting a derived index, rebuilding from verified bundle evidence, and comparing semantic results while the canon hash remains unchanged.

- [ ] **Step 5: Run GREEN and P0-P2 journey checks**

Run:

```bash
npx vitest run tests/unit/modules-read.test.ts
npm run validate:content
npm run test:e2e -- tests/e2e/read-journey.spec.ts
npm test
```

Expected: content, simulations, and cumulative unit tests pass.

- [ ] **Step 6: Commit and stop for review**

```bash
git add src/content/lessons/m03-*.json src/content/lessons/m04-*.json src/content/checks/check-bundle-integrity.json src/content/checks/check-derived-state.json src/content/simulations/sim-verify-bundle.json src/content/simulations/sim-rebuild-index.json tests/unit/modules-read.test.ts tests/e2e/read-journey.spec.ts
git commit -m "content: teach verified retrieval foundations"
```

Require frozen-contract, canonical/derived, privacy, and determinism review before Task 9.

---

### Task 9: Author Modules 5-7 and the Verdict/Recovery Labs

**Files:**
- Create: `src/content/lessons/m05-l01.json`
- Create: `src/content/lessons/m05-l02.json`
- Create: `src/content/lessons/m06-l01.json`
- Create: `src/content/lessons/m06-l02.json`
- Create: `src/content/lessons/m07-l01.json`
- Create: `src/content/lessons/m07-l02.json`
- Create: `src/content/checks/check-human-verdict.json`
- Create: `src/content/checks/check-level4.json`
- Create: `src/content/checks/check-intake-authority.json`
- Create: `src/content/simulations/sim-human-verdict.json`
- Create: `src/content/simulations/sim-safe-recovery.json`
- Create: `tests/unit/modules-writeback.test.ts`
- Create: `tests/e2e/writeback-journey.spec.ts`

**Interfaces:**
- Consumes: P3 Verified-plus-Hardening evidence, verdict and recovery simulation kinds, and layered lesson rendering.
- Produces: the complete proposal-to-canon authority loop and the P3 intake-authority hardening explanation.

- [ ] **Step 1: Write RED tests for authority and fail-closed recovery**

Require content assertions that agents stage proposals only; the human gives a non-empty reason; ACCEPT and REWRITE can publish a new canonical generation; REJECT/TP/FP remain ledger judgments; Level 4 reconstructs and verifies the chain; proposer-controlled evidence is not authoritative intake; ambiguous lease, custody, integrity, or recovery state stops mutation.

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/modules-writeback.test.ts
```

Expected: the suite fails for the six absent lessons and two absent simulations.

- [ ] **Step 3: Author the exact write-back lesson contracts**

| ID | Title | Required concepts |
| --- | --- | --- |
| `m05-l01` | Agents Propose | Immutable submissions, policy as advisory constraint, and no agent commit authority. |
| `m05-l02` | Humans Decide | Public ACCEPT/REJECT/REWRITE teaching, internal five-verb ledger, meaningful reason, and verdict consequences. |
| `m06-l01` | Generations and the Verdict Ledger | Copy-on-write canonical generation, append-only verdict chain, commit point, and why Git is not judgment authority. |
| `m06-l02` | Rebuild and Level 4 Verification | Complete rebuild, ledger/canonical/index bindings, stable corruption outcomes, and exact replay. |
| `m07-l01` | Trusted Intake Custody | Untrusted inbox, interactive human sealing, protected intake events, immutable sealed pairs, and verdict high-water binding. |
| `m07-l02` | Crash, Concurrency, and Poisoning | Ordered leases, incomplete evidence, idempotency, recovery before mutation, privilege boundaries, and fail-closed behavior. |

Teach P3 v2 as a hardening amendment: larger than an edge case, smaller than a rewrite. Explicitly preserve human verdicts, canonical write-back, BLAKE3 commitments, ledgers, index rebuilds, safe-clone proof, and frozen P2 behavior.

- [ ] **Step 4: Implement the verdict and recovery fixtures**

`sim-human-verdict` presents a well-supported proposal, an unsafe proposal, and a proposal requiring rewrite. Every path requires a human reason. Model recommendation alone cannot pass. `sim-safe-recovery` injects one incomplete intake publication, one stale lease, one ledger mismatch, and one safe retry; only recovery that validates ownership and bindings before mutation passes.

- [ ] **Step 5: Run GREEN with serialized content verification**

Run:

```bash
npx vitest run tests/unit/modules-writeback.test.ts
npm run validate:content
npm run test:e2e -- tests/e2e/writeback-journey.spec.ts
npm test
```

Expected: all six lessons validate; verdict/recovery flows pass; unsafe authority shortcuts fail.

- [ ] **Step 6: Commit and stop for review**

```bash
git add src/content/lessons/m05-*.json src/content/lessons/m06-*.json src/content/lessons/m07-*.json src/content/checks/check-human-verdict.json src/content/checks/check-level4.json src/content/checks/check-intake-authority.json src/content/simulations/sim-human-verdict.json src/content/simulations/sim-safe-recovery.json tests/unit/modules-writeback.test.ts tests/e2e/writeback-journey.spec.ts
git commit -m "content: teach verified write-back authority"
```

Require distinct task-level review of the authority boundary, P3 v2 language, retained behavior, and recovery model before Task 10.

---

### Task 10: Author Module 8 and the Contract-First Integration Lab

**Files:**
- Create: `src/content/lessons/m08-l01.json`
- Create: `src/content/checks/check-integration-authority.json`
- Create: `src/content/simulations/sim-design-integration.json`
- Create: `tests/unit/module-integration.test.ts`
- Create: `tests/e2e/integration-journey.spec.ts`

**Interfaces:**
- Consumes: Planned P4 capability, Developer layer contract blocks, and integrate simulation kind.
- Produces: a tool-neutral integration lesson with MCP clearly marked as the future default.

- [ ] **Step 1: Write RED tests for honest P4 status**

Require the lesson to contain `capabilityStatus: "planned"`, reject any phrase claiming the MCP adapter is available, define read/query/propose/request-human-verdict/verify result boundaries, and state that transport never grants authority.

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/module-integration.test.ts
```

Expected: failures identify the missing lesson and integration simulation.

- [ ] **Step 3: Author `m08-l01` — Contract-First Integration**

General explains that applications use SovMem through permissions and explicit lanes. Developer defines tool-neutral envelopes for `query`, `propose`, `request_verdict`, `status`, and `verify`, while marking exact transport schemas as unresolved until P4 closes. Technical shows trust boundaries between application, adapter, store, human ceremony, and derived index. A horizon callout states: “MCP is the intended default adapter after P4 verification; this course does not present a live MCP contract yet.”

- [ ] **Step 4: Implement `sim-design-integration`**

The learner connects a fictional research application. The passing design grants read/query and propose permissions, routes verdicts to a human-only ceremony, treats evaluator scores as advisory, and verifies results before consumption. Any design granting the application `commit_canon` ends in fail with an authority explanation.

- [ ] **Step 5: Run GREEN and planned-status checks**

Run:

```bash
npx vitest run tests/unit/module-integration.test.ts
npm run validate:content
npm run test:e2e -- tests/e2e/integration-journey.spec.ts
```

Expected: P4 remains Planned; integration exercise passes only with the human authority boundary intact.

- [ ] **Step 6: Commit and stop for review**

```bash
git add src/content/lessons/m08-l01.json src/content/checks/check-integration-authority.json src/content/simulations/sim-design-integration.json tests/unit/module-integration.test.ts tests/e2e/integration-journey.spec.ts
git commit -m "content: add contract-first integration horizon"
```

Require P4 non-claim, tool-neutral vocabulary, MCP horizon, and application-authority review before Task 11.

---

### Task 11: Author Module 9, Completion Rules, and The Drift Incident

**Files:**
- Create: `src/content/lessons/m09-l01.json`
- Create: `src/content/lessons/m09-l02.json`
- Create: `src/content/checks/check-operational-horizon.json`
- Create: `src/content/simulations/sim-drift-incident.json`
- Create: `src/components/capstone/CapstoneHost.tsx`
- Create: `src/lib/progress/completion.ts`
- Create: `tests/unit/capstone.test.ts`
- Create: `tests/unit/completion.test.ts`
- Create: `tests/e2e/capstone.spec.ts`

**Interfaces:**
- Consumes: all prior lesson/check/simulation IDs and local progress.
- Produces: P5 horizon content, capstone state machine, `isCourseComplete(progress, catalog)`, and a complete local course record.

- [ ] **Step 1: Write RED capstone and completion tests**

Require that completion is false when any core lesson is incomplete, required check is unpassed, or capstone invariant fails. Require pass only after evidence classification, authority-shortcut rejection, human verdict with reason, verified publication state, and safe recovery.

```ts
expect(isCourseComplete(incomplete, catalog)).toBe(false);
expect(isCourseComplete(allLessonsNoCapstone, catalog)).toBe(false);
expect(runCapstone(unsafeAccept).terminal).toBe("fail");
expect(runCapstone(safeRewriteAndRecover).terminal).toBe("pass");
```

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/capstone.test.ts tests/unit/completion.test.ts
```

Expected: tests fail because the capstone and completion rules do not exist.

- [ ] **Step 3: Author the P5 horizon and capstone lessons**

`m09-l01`, **Operate and Observe**, teaches why a future SSA will need sentry checks, audit sampling, trust instrumentation, runbooks, and sustained dogfood evidence. Every P5 capability remains Planned and describes the evidence required for promotion.

`m09-l02`, **The Drift Incident**, briefs the learner on a plausible memory update containing conflicting sources, a high-confidence evaluator recommendation, an attempted direct canon write, and a crash after replacement generation creation but before ledger commit.

- [ ] **Step 4: Implement the capstone state machine**

The passing sequence is:

1. Inspect and classify the source bundle, proposal, evaluator output, existing canon, and prior verdict.
2. Reject the evaluator's direct-commit recommendation.
3. Select REWRITE because the proposal contains useful content with unsupported claims.
4. Enter a non-empty human reason and remove unsupported claims.
5. Seal the authoritative intake evidence.
6. Build and verify the replacement generation and derived index.
7. Detect the simulated pre-ledger crash.
8. Recover by validating ownership and bindings, then complete one idempotent ledger append.
9. Run Level 4 verification and confirm canon, ledger, intake prefix, and index bindings.

Any direct commit, empty reason, ignored crash, partial acceptance, or guessed recovery ends in fail with the violated invariant.

- [ ] **Step 5: Implement completion and learner handoff**

`isCourseComplete` requires all 17 lesson IDs, every required check, and `capstoneState === "passed"`. The completion view states what the learner can now explain, design, and evaluate; offers Export progress; and links back to the permanent product homepage. Do not issue a certificate or send completion data.

- [ ] **Step 6: Run GREEN and the complete capstone journey**

Run:

```bash
npx vitest run tests/unit/capstone.test.ts tests/unit/completion.test.ts
npm run validate:content
npm run test:e2e -- tests/e2e/capstone.spec.ts
npm test
```

Expected: the safe rewrite/recovery path passes; every unsafe path fails; validator reports 9 modules and 17 lessons.

- [ ] **Step 7: Commit and stop for review**

```bash
git add src/content/lessons/m09-*.json src/content/checks/check-operational-horizon.json src/content/simulations/sim-drift-incident.json src/components/capstone src/lib/progress/completion.ts tests/unit/capstone.test.ts tests/unit/completion.test.ts tests/e2e/capstone.spec.ts
git commit -m "feat: add Drift Incident capstone"
```

Require capstone invariant, P5 non-claim, completion, privacy, and recovery review before Task 12.

---

### Task 12: Prove Accessibility, Privacy, Migration, and Release Readiness

**Files:**
- Create: `scripts/export-course.ts`
- Create: `tests/unit/course-export.test.ts`
- Create: `tests/e2e/accessibility.spec.ts`
- Create: `tests/e2e/privacy.spec.ts`
- Create: `tests/e2e/mobile-complete.spec.ts`
- Create: `docs/CONTENT-GOVERNANCE.md`
- Create: `docs/MIGRATION.md`
- Create: `docs/RELEASE-CHECKLIST.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: validated catalog, capability/evidence registries, checks, simulations, progress schema, and all UI routes.
- Produces: `dist/course-package/sovmem-zero-to-hero.v1.json`, full local quality evidence, update governance, and an explicit non-publication handoff.

- [ ] **Step 1: Write RED export and privacy tests**

The export test must require course metadata, nine modules, 17 lessons, all three layers, checks, simulation metadata, stable migration IDs, capability states, and no learner progress. The privacy browser test must record requests after initial navigation and fail any request whose origin differs from `page.url()`'s origin.

- [ ] **Step 2: Run RED**

Run:

```bash
npx vitest run tests/unit/course-export.test.ts
npm run test:e2e -- tests/e2e/privacy.spec.ts
```

Expected: export test fails because the exporter is absent; privacy test identifies any remaining third-party asset until all assets are local.

- [ ] **Step 3: Implement the migration exporter**

Write one deterministic JSON object sorted by module order, lesson order, and stable ID. Include:

```ts
{
  schemaVersion: 1,
  course: { id: "sovmem-zero-to-hero", version: "1.0.0", title: "SovMem: Zero to Hero" },
  modules,
  lessons,
  checks,
  simulations,
  capabilities,
  evidence,
  progressSchema: { version: 1, stableIdPolicy: "preserve" }
}
```

Run the content validator first and refuse export on any validation failure. Evidence export is allowlist-based: include only `id`, public label, reviewed revision, status, and visibility. Strip filesystem paths, private repository URLs, worktree names, and local audit locations from every exported evidence record. Write to `dist/course-package/`; do not commit the generated package unless a later publication contract requires it.

- [ ] **Step 4: Add whole-product accessibility and responsive tests**

Cover the homepage, orientation, one lesson at each capability status, every simulation kind, import preview, and capstone. Require zero serious or critical Axe violations. Run keyboard-only journeys at 375×812 and 1440×900. Assert no horizontal scroll and no action dependent on drag or hover.

- [ ] **Step 5: Add the content-governance and migration runbooks**

`CONTENT-GOVERNANCE.md` records the four status definitions, reviewed-evidence promotion ceremony, P4/P5 activation checklist, stable-ID rule, and stale-audit prohibition. `MIGRATION.md` defines the package fields, progress conversion, permanent product-front-door behavior, and `/learn` handoff. `RELEASE-CHECKLIST.md` separates local verification, content review, deployment approval, DNS approval, publication, and migration.

- [ ] **Step 6: Run the complete local gate**

Run:

```bash
npm ci
npm run check
npm test
npm run validate:content
npm run build
npm run export:course
npm run test:e2e
git diff --check
git status --short
```

Expected:

- Type checking and static build exit zero.
- All unit tests pass with a non-zero count.
- Content summary reports `9 modules · 17 lessons` and no unresolved markers.
- Migration export validates and contains no learner progress.
- All browser, mobile, accessibility, privacy, and capstone tests pass.
- Worktree status contains only the intended Task 12 files before commit.

- [ ] **Step 7: Perform whole-product review without publication**

Review the complete branch against the design specification and PRD. Require explicit findings for content accuracy, P3 Hardening language, P4/P5 non-claims, authority boundaries, privacy, accessibility, responsive behavior, progress portability, and migration completeness. Convert every Critical or Important finding into a focused failing fixture, correct one invariant, rerun the affected layers, and obtain re-review.

- [ ] **Step 8: Commit the local release candidate and stop**

```bash
git add README.md docs scripts/export-course.ts tests/unit/course-export.test.ts tests/e2e/accessibility.spec.ts tests/e2e/privacy.spec.ts tests/e2e/mobile-complete.spec.ts
git commit -m "test: close SovMem learning product gate"
```

Stop after the reviewed local commit. Do not create a remote, push, deploy, change DNS, or publish without a separate user approval naming those actions.

---

## Plan Self-Review

- **Spec coverage:** Tasks 1-6 build the isolated static engine, content/evidence contracts, permanent product page, layered course shell, private progress, and deterministic interactions. Tasks 7-11 deliver all nine modules, six simulation patterns, formative checks, P4/P5 horizon scaffolding, completion rules, and The Drift Incident. Task 12 covers migration, privacy, accessibility, responsive operation, governance, full verification, and the publication stop boundary.
- **Scope:** The plan creates one cohesive product in `sovmem-learn`. It does not create a general TFIS learning platform, implement SovMem runtime phases, or mutate the Nous/P3 worktrees.
- **Type consistency:** `AudienceDepth`, `CapabilityStatus`, `LessonRecord`, `ProgressRecord`, and `SimulationDefinition` remain stable across tasks. Stable IDs use `mNN-lNN`, `check-*`, and `sim-*` consistently.
- **Phase truth:** P0-P2 begin Verified, P3 is taught with its original delivered behavior and active Hardening status, and P4/P5 remain Planned until a separate reviewed evidence update.
- **Privacy:** No server, analytics, learner-data request, remote font, or content API enters the dependency or runtime plan.
- **Release boundary:** The final task stops at a reviewed local commit. Remote creation, push, deployment, DNS, and publication remain separately approved actions.

## Official Framework References

- Astro installation and current release: `https://docs.astro.build/`
- Astro project structure: `https://docs.astro.build/en/basics/project-structure/`
- Astro TypeScript guidance: `https://docs.astro.build/en/guides/typescript/`
- Official React integration: `https://docs.astro.build/en/guides/integrations-guide/react/`
- Astro upgrade and Node support policy: `https://docs.astro.build/en/upgrade-astro/`
