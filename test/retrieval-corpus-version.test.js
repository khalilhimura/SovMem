import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('pins the Rust 1.84 compatible P2 retrieval dependencies', () => {
  const cargo = readFileSync('sovmem/Cargo.toml', 'utf8');
  assert.match(cargo, /serde = \{ version = "=1\.0\.219", features = \["derive"\] \}/);
  assert.match(cargo, /serde_json = \{ version = "=1\.0\.140"/);
  assert.match(cargo, /tantivy = \{ version = "=0\.24\.2", default-features = false, features = \["mmap", "lz4-compression"\] \}/);
  assert.match(cargo, /petgraph = \{ version = "=0\.8\.3", default-features = false, features = \["std"\] \}/);
  assert.doesNotMatch(cargo, /qdrant|embedding|vector/i);
});

test('runs the Rust suite against the committed lockfile', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));

  // The P2 design requires an explicit clean Rust 1.84 `cargo test --locked`
  // proof. Without --locked a permissive transitive requirement can silently
  // resolve past the 1.84 MSRV instead of failing the build.
  assert.match(pkg.scripts['test:sovmem'], /cargo test --locked/);
  assert.match(pkg.scripts['test:sovmem'], /npm run test:sovmem:js/);
  assert.match(pkg.scripts['test:sovmem:full'], /npm run test:sovmem:js/);
  assert.match(pkg.scripts.test, /test:js/);
  assert.match(pkg.scripts.test, /test:sovmem/);
});

test('enforces formatting, lints, and a cached pinned toolchain in CI', () => {
  const workflow = readFileSync('.github/workflows/sovmem-audit.yml', 'utf8');

  assert.match(workflow, /rustup show active-toolchain \|\| rustup toolchain install/);
  assert.match(workflow, /Swatinem\/rust-cache@v2/);
  assert.match(workflow, /cargo fmt --manifest-path sovmem\/Cargo\.toml --check/);
  assert.match(
    workflow,
    /cargo clippy --manifest-path sovmem\/Cargo\.toml --all-targets --locked --features test-support -- -D warnings/,
  );
  assert.match(workflow, /npm run audit:sovmem:check/);
  assert.match(workflow, /npm test/);

  // A cold Tantivy build plus ~10 minutes of Rust tests overruns 20 minutes.
  const verifyJob = workflow.split('\n  verify:\n')[1];
  assert.ok(verifyJob, 'the full regression job must exist');
  const timeout = Number(/timeout-minutes: (\d+)/.exec(verifyJob)?.[1]);
  assert.ok(timeout >= 40, `CI timeout must allow a cold build, got ${timeout}`);

  // The workflow must not duplicate the rust-toolchain.toml pin.
  const toolchain = readFileSync('rust-toolchain.toml', 'utf8');
  assert.match(toolchain, /channel = "1\.84\.0"/);
  assert.doesNotMatch(workflow, /toolchain: *1\.84/);
});

test('preserves the immutable Audy retrieval corpus v0 bytes', () => {
  const bytes = readFileSync('profiles/audy-ssa0-v0/retrieval-corpus.yaml');
  const digest = createHash('sha256').update(bytes).digest('hex');
  assert.equal(digest, 'ff7807ec0c9a06ece4d1ee1513a8fd6cd80ed9446f3fc0ee440fd2b137b6d132');
});

test('locks the active Audy retrieval corpus v1 bytes', () => {
  const bytes = readFileSync('profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml');
  const digest = createHash('sha256').update(bytes).digest('hex');
  assert.equal(digest, 'a65404b1b7b043148917715a49777d90c3afdb6493df55534528a3b915fa3d4d');
});

test('pins the official Open Knowledge Format reference', () => {
  const path = 'docs/spec/okf-reference.json';
  const bytes = readFileSync(path, 'utf8');
  const expected = {
    schema_version: 'sovmem-okf-reference-v1',
    repository: 'GoogleCloudPlatform/knowledge-catalog',
    commit: 'd44368c15e38e7c92481c5992e4f9b5b421a801d',
    normative_file: 'okf/SPEC.md',
  };

  assert.deepEqual(JSON.parse(bytes), expected);
  assert.equal(bytes, `${JSON.stringify(expected, null, 2)}\n`);
});

test('locks the approved golden and threshold bytes independently', () => {
  const golden = createHash('sha256')
    .update(readFileSync('evals/audy-sovmem-v0/golden.jsonl'))
    .digest('hex');
  const thresholds = createHash('sha256')
    .update(readFileSync('evals/audy-sovmem-v0/thresholds.json'))
    .digest('hex');

  assert.equal(golden, '698e56224d8c5358948557f8e4c6965eeeab7cff76afbdaf382f2faa3091af7d');
  assert.equal(thresholds, '33038018a4ca9fecaa960839f7bfb9e277f5306ac657a6ef30df62dbfa57f280');
});

test('locks the canonical P2 baseline and integer Q3 GO evidence', () => {
  const bytes = readFileSync('evals/audy-sovmem-v0/baseline.json');
  const digest = createHash('sha256').update(bytes).digest('hex');
  const baseline = JSON.parse(bytes);

  assert.equal(digest, '5d5025e1b3c3ea9ce1bb900dda5c031cff75f691eae05ea04fac834f58567689');
  assert.equal(baseline.schema_version, 'sovmem-retrieval-baseline-v1');
  assert.equal(
    baseline.corpus.bundle_hash,
    '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
  );
  assert.equal(
    baseline.retrieval.index_config,
    '95a2237b666e0d6557709bb98eb2b4fe54a7478f6dca17df0bd3a14049e17f8d',
  );
  assert.equal(baseline.evaluation_set.query_count, 25);
  assert.equal(baseline.evaluation_set.k, 5);
  assert.equal(baseline.queries.length, 25);
  assert.equal(baseline.metrics.gate.status, 'go');
  assert.ok(
    baseline.metrics.queries_with_hit >= baseline.metrics.gate.minimum_queries_with_hit,
  );
  assert.equal(baseline.metrics.gate.minimum_queries_with_hit, 20);
});

test('exposes portable Audy corpus commands at the repository root', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));

  assert.match(pkg.scripts['corpus:audy'], /AUDY_VAULT/);
  assert.match(pkg.scripts['corpus:audy'], /retrieval-corpus-v1\.yaml/);
  assert.match(pkg.scripts['verify:corpus:audy'], /var\/audy-sovmem-retrieval-corpus/);
  assert.match(pkg.scripts['verify:corpus:audy'], /--level 3/);
  assert.doesNotMatch(pkg.scripts['verify:corpus:audy'], /--require-corpus/);
  assert.doesNotMatch(pkg.scripts['verify:audy'], /--require-corpus/);
  assert.match(pkg.scripts['verify:golden:audy'], /evals\/audy-sovmem-v0\/golden\.jsonl/);
  assert.doesNotMatch(JSON.stringify(pkg), /\/Volumes\/himura|\/Users\/khalilhimura/);
});

test('wires formal level-three verification and deterministic stats', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));

  assert.match(pkg.scripts['verify:corpus:audy'], /--level 3/);
  assert.doesNotMatch(pkg.scripts['verify:corpus:audy'], /--require-corpus/);
  assert.match(pkg.scripts['stats:audy'], /sovmem\/Cargo\.toml -- stats/);
  assert.match(pkg.scripts['stats:audy'], /var\/audy-sovmem-retrieval-corpus/);
  assert.doesNotMatch(JSON.stringify(pkg), /\/Volumes\/himura|\/Users\/khalilhimura/);
});

test('exposes exact offline P2 root commands', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));

  assert.equal(
    pkg.scripts['index:audy'],
    'cargo run --locked --manifest-path sovmem/Cargo.toml -- index --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/audy-sovmem-derived-index',
  );
  assert.equal(
    pkg.scripts['query:audy'],
    'cargo run --locked --manifest-path sovmem/Cargo.toml -- query --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/audy-sovmem-derived-index',
  );
  assert.equal(
    pkg.scripts['eval:audy'],
    'cargo run --locked --manifest-path sovmem/Cargo.toml -- eval --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/audy-sovmem-derived-index --golden evals/audy-sovmem-v0/golden.jsonl --thresholds evals/audy-sovmem-v0/thresholds.json --baseline evals/audy-sovmem-v0/baseline.json',
  );
});

test('documents and registers the bounded offline P2 interface', () => {
  const readme = readFileSync('README.md', 'utf8');
  const registry = readFileSync('index.md', 'utf8');
  for (const command of ['npm run index:audy', 'npm run query:audy -- --query', 'npm run eval:audy']) {
    assert.match(readme, new RegExp(command.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
  }
  assert.match(readme, /title.*3.*tags.*2.*body.*1/s);
  assert.match(readme, /one-hop.*separate|separate.*one-hop/s);
  assert.match(readme, /no embeddings.*model calls.*network calls/s);
  assert.match(readme, /NO-GO.*stop/s);
  assert.match(readme, /P3 is delivered/s);
  assert.match(readme, /SovMem v0\.3 includes the delivered P4 and P5 components/);

  const artifacts = [
    'docs/archive/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md',
    'docs/archive/plans/2026-07-13-sovmem-p2-derived-index-and-query.md',
    'sovmem/src/profile/index.rs',
    'sovmem/src/profile/graph.rs',
    'sovmem/src/profile/query.rs',
    'sovmem/src/profile/eval.rs',
    'sovmem/tests/index_build.rs',
    'sovmem/tests/graph_index.rs',
    'sovmem/tests/query.rs',
    'sovmem/tests/eval.rs',
    'sovmem/tests/p2_cli.rs',
    'sovmem/tests/support/p2.rs',
    'evals/audy-sovmem-v0/baseline.json',
  ];
  for (const artifact of artifacts) {
    assert.equal(
      registry.split(artifact).length - 1,
      1,
      `${artifact} must appear exactly once in the durable registry`,
    );
  }
  assert.match(
    registry,
    /evals\/audy-sovmem-v0\/baseline\.json[^\n]*canonical P2 Q3 GO evidence/,
  );
});

test('records the P2 gate closure without claiming enforced GitHub controls', () => {
  const plan = readFileSync('docs/sovmem_v0.3_execution_plan.md', 'utf8');

  assert.match(plan, /62 accepted documents.*8 core.*54 research/s);
  assert.match(
    plan,
    /GoogleCloudPlatform\/knowledge-catalog.*d44368c15e38e7c92481c5992e4f9b5b421a801d.*okf\/SPEC\.md/s,
  );
  assert.match(plan, /minimum_queries_with_hit.*20/);
  assert.match(plan, /manual pull-request merge discipline/);
  assert.doesNotMatch(
    plan,
    /branch protection is enforced|required merge queue is enforced/,
  );
  assert.match(plan, /P2.*passed.*23 of 25/s);
  assert.match(plan, /All five phases.*P0.*P5.*delivered/s);
});

test('registers every P0/P1 gate-closure artifact exactly once', () => {
  const registry = readFileSync('index.md', 'utf8');
  const artifacts = [
    'docs/archive/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md',
    'docs/archive/plans/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds.md',
    'docs/spec/okf-reference.json',
    'evals/audy-sovmem-v0/thresholds.json',
    'sovmem/src/profile/verify.rs',
    'sovmem/src/profile/stats.rs',
    'sovmem/src/profile/thresholds.rs',
    'sovmem/tests/thresholds.rs',
    'sovmem/tests/bundle_verification.rs',
  ];

  for (const artifact of artifacts) {
    assert.equal(
      registry.split(artifact).length - 1,
      1,
      `${artifact} must appear exactly once in the durable registry`,
    );
  }
});
