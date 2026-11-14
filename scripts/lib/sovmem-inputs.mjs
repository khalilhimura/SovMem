// Shared by the audit fingerprint and CI routing. Generated reports are outputs,
// not inputs: routing handles them separately to avoid a circular fingerprint.
const directories = Object.freeze(['sovmem/', 'profiles/', 'evals/']);
const files = Object.freeze([
  'package.json', 'package-lock.json', 'rust-toolchain.toml',
  'scripts/generate-sovmem-build-audit.mjs', 'scripts/lib/sovmem-build-audit.mjs',
  'scripts/lib/sovmem-inputs.mjs', 'scripts/sovmem-sentry.sh',
  'scripts/provision-sovmem-p3-safe-clone.mjs', 'scripts/verify-sovmem-p3-safe-clone.mjs',
  'scripts/run-js-tests.mjs', 
  'test/retrieval-corpus-version.test.js',
  'docs/sovmem_v0.3_execution_plan.md', 'docs/spec/source-lineage-june-23-spec-bundle-verbatim.md',
  'docs/sovmem-build-state.json',
  '.github/workflows/sovmem-audit.yml',
  '.github/scripts/sovmem-scope.mjs', '.github/scripts/sovmem-scope.test.mjs',
]);

export const SOVMEM_INPUT_PATHS = Object.freeze([
  ...directories,
  ':(glob)test/sovmem*.test.js',
  ...files,
]);

const prefixes = directories.map(path => Buffer.from(path));
const exact = files.map(path => Buffer.from(path));
const generated = Buffer.from('sovmem/target/');
const testPrefix = Buffer.from('test/sovmem');
const testSuffix = Buffer.from('.test.js');

function startsWith(path, prefix) {
  return path.length >= prefix.length && path.subarray(0, prefix.length).equals(prefix);
}

export function isSovmemInput(value) {
  // Preserve non-UTF-8 Git filenames; decoding can replace distinct byte names.
  const path = Buffer.isBuffer(value) ? value : Buffer.from(value);
  if (startsWith(path, generated)) return false;
  return prefixes.some(prefix => startsWith(path, prefix))
    || exact.some(file => path.equals(file))
    || (startsWith(path, testPrefix)
      && !path.subarray(testPrefix.length).includes(0x2f)
      && path.subarray(-testSuffix.length).equals(testSuffix));
}
