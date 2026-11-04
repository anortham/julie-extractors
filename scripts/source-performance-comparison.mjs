import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import crypto from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { parseArgs } from "node:util";
import { fileURLToPath, pathToFileURL } from "node:url";

export function verifyRebuiltReceipt(original, rebuilt) {
  for (const receipt of [original, rebuilt]) {
    assert.equal(receipt.format_version, 1, "unsupported baseline receipt version");
    assert.match(receipt.baseline_binary_sha256, /^[0-9a-f]{64}$/i, "invalid binary identity");
    assert.ok(Array.isArray(receipt.snapshots) && receipt.snapshots.length > 0, "baseline snapshots are empty");
  }
  const { baseline_binary_sha256: oldBinary, snapshots: oldSnapshots, ...oldHeader } = original;
  const { baseline_binary_sha256: newBinary, snapshots: newSnapshots, ...newHeader } = rebuilt;
  assert.deepEqual(newHeader, oldHeader, "baseline source, manifest or receipt contract changed");
  const index = (snapshots) => {
    const values = new Map();
    for (const snapshot of snapshots) {
      const key = JSON.stringify([snapshot.language, snapshot.level, snapshot.workload, snapshot.source_state_sha256]);
      assert.ok(!values.has(key), `duplicate baseline snapshot ${key}`);
      values.set(key, snapshot);
    }
    return values;
  };
  const expected = index(oldSnapshots);
  const actual = index(newSnapshots);
  assert.equal(actual.size, expected.size, "baseline snapshot count changed");
  for (const [key, snapshot] of expected) {
    assert.deepEqual(actual.get(key), snapshot, `baseline facts, diagnostics or health changed: ${key}`);
  }
  return {
    snapshot_count: expected.size,
    original_binary_sha256: oldBinary,
    rebuilt_binary_sha256: newBinary,
  };
}

export function parseCargoFeatures(tree) {
  const features = new Set();
  for (const line of tree.split(/\r?\n/)) {
    const edge = line.match(/^(\S+) feature "([^"]+)"(?: \(\*\))?$/);
    if (edge) features.add(`${edge[1]}/${edge[2]}`);
    const [packageText, enabled] = line.replaceAll("\\t", "\t").split("\t");
    if (enabled !== undefined) {
      const name = packageText.split(/\s+/)[0];
      for (const feature of enabled.replace(/ \(\*\)$/, "").split(",").filter(Boolean)) {
        features.add(`${name}/${feature}`);
      }
    }
  }
  return [...features].sort();
}

export function createEvidenceDirectory(output) {
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.mkdirSync(output);
}

const harnessRoot = fileURLToPath(new URL("..", import.meta.url));
const sha256 = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");
const readJson = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const writeJson = (file, value) => fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

function capture(argv, cwd, env = process.env) {
  return execFileSync(argv[0], argv.slice(1), { cwd, env, encoding: "utf8", maxBuffer: 32 * 1024 * 1024 }).trim();
}

function sourceIdentity(root, expectedCommit) {
  assert.equal(fs.realpathSync(capture(["git", "rev-parse", "--show-toplevel"], root)), fs.realpathSync(root), "source path must be a repository root");
  const commit = capture(["git", "rev-parse", "HEAD"], root);
  assert.equal(commit, expectedCommit, "source commit does not match the immutable pin");
  assert.equal(capture(["git", "status", "--porcelain", "--untracked-files=all"], root), "", `source checkout is dirty: ${root}`);
  const tree = execFileSync("git", ["ls-tree", "-rz", "--full-tree", commit], { cwd: root, maxBuffer: 32 * 1024 * 1024 });
  return { commit, tree_hash_algorithm: "git-ls-tree-r-v1-sha256", tree_sha256: sha256(tree), dirty: false, dirty_state_sha256: null };
}

function runRecorded(argv, cwd, env, log) {
  const descriptor = fs.openSync(log, "wx");
  let result;
  try {
    result = spawnSync(argv[0], argv.slice(1), { cwd, env, stdio: ["ignore", descriptor, descriptor] });
  } finally {
    fs.closeSync(descriptor);
  }
  if (result.error || result.status !== 0) {
    throw Object.assign(new Error(`command failed (${result.status ?? result.signal}): ${argv.join(" ")}; log: ${log}`, { cause: result.error }), { exitCode: result.status ?? 2 });
  }
}

function buildEnvironment(target) {
  const env = { ...process.env };
  const overrides = new Set(["RUSTC", "CARGO_BUILD_RUSTC", "RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", "CARGO_INCREMENTAL", "CC", "CXX", "CFLAGS", "CXXFLAGS", "AR", "RANLIB"]);
  for (const key of Object.keys(env)) {
    if (overrides.has(key) || key.startsWith("CARGO_PROFILE_RELEASE_") || /^CARGO_TARGET_.*_RUSTFLAGS$/.test(key)) delete env[key];
  }
  env.CARGO_TARGET_DIR = target;
  env.CARGO_TERM_COLOR = "never";
  return env;
}

function buildExtractor(label, sourceRoot, expectedCommit, output, buildRoot) {
  const source = sourceIdentity(sourceRoot, expectedCommit);
  console.log(`Building ${label} from ${source.commit}.`);
  const directory = path.join(output, label);
  fs.mkdirSync(directory);
  const target = path.join(buildRoot, label);
  const env = buildEnvironment(target);
  const rustc = capture(["rustc", "--version"], sourceRoot, env);
  assert.match(rustc, /^rustc 1\.97\.1 /, "source comparison requires Rust 1.97.1");
  const cargo = capture(["cargo", "--version"], sourceRoot, env);
  const hostTarget = capture(["rustc", "-vV"], sourceRoot, env).split("\n").find((line) => line.startsWith("host: "))?.slice(6);
  assert.ok(hostTarget, "rustc did not report its host target");
  runRecorded(["cargo", "fetch", "--locked"], sourceRoot, env, path.join(directory, "fetch.log"));
  const recipe = ["cargo", "build", "--release", "--locked", "--offline", "--jobs", "1", "-p", "julie-extract-cli", "--bin", "julie-extract"];
  const buildLog = path.join(directory, "build.log");
  const started = new Date().toISOString();
  runRecorded(recipe, sourceRoot, env, buildLog);
  assert.deepEqual(sourceIdentity(sourceRoot, expectedCommit), source, "source changed during the build");
  const tree = capture(["cargo", "tree", "--locked", "--offline", "-p", "julie-extract-cli", "--target", hostTarget, "-e", "normal,build,features", "--prefix", "none", "--format", "{p}\t{f}"], sourceRoot, env);
  const features = parseCargoFeatures(tree);
  assert.ok(features.length > 0, "build feature graph is empty");
  fs.writeFileSync(path.join(directory, "feature-tree.txt"), `${tree}\n`);
  const binary = path.join(directory, "julie-extract");
  fs.copyFileSync(path.join(target, "release", "julie-extract"), binary);
  fs.chmodSync(binary, 0o755);
  const recordPath = path.join(directory, "build-record.json");
  const record = {
    format_version: 1, source, source_root: sourceRoot, target_directory: target,
    started_utc: started, finished_utc: new Date().toISOString(), recipe,
    recipe_sha256: sha256(JSON.stringify(recipe)), build_log_sha256: sha256(fs.readFileSync(buildLog)),
    binary_sha256: sha256(fs.readFileSync(binary)), rustc, cargo, host_target: hostTarget,
    compiler: capture(["cc", "--version"], sourceRoot, env), features,
    compiler_profile_overrides: "cleared in child environment; source-controlled Cargo release profile",
  };
  writeJson(recordPath, record);
  const metadata = {
    format_version: 1, binary_sha256: record.binary_sha256, source,
    build: {
      rustc, cargo, host_target: hostTarget, target: hostTarget, profile: "release", features,
      options: { jobs: "1", locked: "true", offline: "true" }, recipe,
      recipe_sha256: record.recipe_sha256,
      evidence: { kind: "build-record", reference: recordPath, sha256: sha256(fs.readFileSync(recordPath)) },
    },
  };
  const metadataPath = `${binary}.build.json`;
  writeJson(metadataPath, metadata);
  console.log(`${label} build recorded: ${metadata.binary_sha256}.`);
  return { binary, metadataPath, metadataSha256: sha256(fs.readFileSync(metadataPath)), metadata };
}

function optionsFromArgs(args) {
  const names = ["baseline-root", "candidate-root", "out-dir", "baseline-commit", "candidate-commit", "harness-commit", "original-receipt", "original-receipt-sha256", "manifest", "manifest-sha256", "allowlist", "allowlist-sha256"];
  const options = Object.fromEntries(names.map((name) => [name, { type: "string" }]));
  options["run-comparison"] = { type: "boolean", default: false };
  const { values } = parseArgs({ args, options, strict: true, allowPositionals: false });
  for (const name of names) assert.ok(values[name], `missing required option --${name}`);
  for (const name of ["baseline-commit", "candidate-commit", "harness-commit"]) assert.match(values[name], /^[0-9a-f]{40}$/i, `${name} must be a full immutable 40-digit commit SHA`);
  for (const name of ["original-receipt-sha256", "manifest-sha256", "allowlist-sha256"]) assert.match(values[name], /^[0-9a-f]{64}$/i, `${name} must be a 64-digit SHA-256`);
  return values;
}

function prepareComparison(values) {
  assert.equal(process.platform, "linux", "source performance comparison requires a Linux runner");
  const baselineRoot = fs.realpathSync(values["baseline-root"]);
  const candidateRoot = fs.realpathSync(values["candidate-root"]);
  const output = path.resolve(values["out-dir"]);
  assert.ok(!fs.existsSync(output), "comparison output directory must be new");
  const harnessSource = sourceIdentity(harnessRoot, values["harness-commit"]);
  const baselineSource = sourceIdentity(baselineRoot, values["baseline-commit"]);
  sourceIdentity(candidateRoot, values["candidate-commit"]);
  for (const [file, pin] of [["original-receipt", "original-receipt-sha256"], ["manifest", "manifest-sha256"], ["allowlist", "allowlist-sha256"]]) {
    values[file] = path.resolve(values[file]);
    assert.equal(sha256(fs.readFileSync(values[file])), values[pin].toLowerCase(), `${file} SHA-256 mismatch`);
  }
  const original = readJson(values["original-receipt"]);
  assert.deepEqual(baselineSource, original.source, "baseline checkout differs from the frozen semantic baseline source");
  assert.equal(original.manifest_sha256, values["manifest-sha256"].toLowerCase(), "frozen receipt does not pin this corpus");
  createEvidenceDirectory(output);
  const buildRoot = fs.mkdtempSync(path.join(os.tmpdir(), "source-performance-build-"));
  fs.copyFileSync(values["original-receipt"], path.join(output, "original-baseline-expectations.json"));
  const baseline = buildExtractor("baseline", baselineRoot, values["baseline-commit"], output, buildRoot);
  const candidate = buildExtractor("candidate", candidateRoot, values["candidate-commit"], output, buildRoot);
  assert.deepEqual(candidate.metadata.build.features, baseline.metadata.build.features, "baseline/candidate build features differ");
  const harnessEnv = buildEnvironment(path.join(buildRoot, "harness"));
  console.log(`Building comparison harness from ${harnessSource.commit}.`);
  runRecorded(["cargo", "fetch", "--locked"], harnessRoot, harnessEnv, path.join(output, "harness-fetch.log"));
  runRecorded(["cargo", "build", "--locked", "--offline", "--jobs", "1", "-p", "xtask"], harnessRoot, harnessEnv, path.join(output, "harness-build.log"));
  assert.deepEqual(sourceIdentity(harnessRoot, values["harness-commit"]), harnessSource, "harness source changed during preparation");
  const harness = path.join(harnessEnv.CARGO_TARGET_DIR, "debug", "xtask");
  runRecorded([harness, "coverage", "--all", "--binary", candidate.binary], harnessRoot, harnessEnv, path.join(output, "candidate-coverage.log"));
  const receiptDirectory = path.join(output, "rebuilt-baseline-receipt");
  const captureCommand = [harness, "performance", "baseline-receipt", "--binary", baseline.binary, "--build-metadata", baseline.metadataPath, "--build-metadata-sha256", baseline.metadataSha256, "--manifest", values.manifest, "--manifest-sha256", values["manifest-sha256"], "--out-dir", receiptDirectory];
  runRecorded(captureCommand, harnessRoot, harnessEnv, path.join(output, "baseline-capture.log"));
  const rebuiltReceiptPath = path.join(receiptDirectory, "baseline-expectations.json");
  const proof = verifyRebuiltReceipt(original, readJson(rebuiltReceiptPath));
  console.log(`Verified ${proof.snapshot_count} rebuilt baseline snapshots against the unchanged frozen receipt.`);
  writeJson(path.join(output, "baseline-equivalence.json"), { ...proof, original_receipt_sha256: values["original-receipt-sha256"], rebuilt_receipt_sha256: sha256(fs.readFileSync(rebuiltReceiptPath)), semantic_expectations_changed: false });
  const comparison = [harness, "performance", "compare", "--baseline", baseline.binary, "--candidate", candidate.binary, "--baseline-expectations", rebuiltReceiptPath, "--baseline-expectations-sha256", sha256(fs.readFileSync(rebuiltReceiptPath)), "--baseline-build-metadata", baseline.metadataPath, "--baseline-build-metadata-sha256", baseline.metadataSha256, "--candidate-build-metadata", candidate.metadataPath, "--candidate-build-metadata-sha256", candidate.metadataSha256, "--manifest", values.manifest, "--allowlist", values.allowlist, "--profile", "release", "--out-dir", path.join(output, "comparison")];
  const preparation = { format_version: 1, harness_source: harnessSource, harness_binary_sha256: sha256(fs.readFileSync(harness)), baseline_binary_sha256: baseline.metadata.binary_sha256, candidate_binary_sha256: candidate.metadata.binary_sha256, frozen_receipt_sha256: values["original-receipt-sha256"], manifest_sha256: values["manifest-sha256"], allowlist_sha256: values["allowlist-sha256"], baseline_equivalence: proof, build_root: buildRoot, comparison_command: comparison, comparison_started: false };
  const preparationPath = path.join(output, "preparation.json");
  writeJson(preparationPath, preparation);
  assert.equal(sha256(fs.readFileSync(values["original-receipt"])), values["original-receipt-sha256"], "frozen receipt changed during preparation");
  if (values["run-comparison"]) {
    runRecorded(["sync"], harnessRoot, harnessEnv, path.join(output, "pre-timing-sync.log"));
    preparation.cache_procedure = "builds and receipt validation complete; sync before timing; OS page cache retained; harness warm-ups and calibration follow";
    preparation.comparison_started = true;
    writeJson(preparationPath, preparation);
    console.log("Starting the six-workload comparison with unchanged correctness and resource budgets.");
    runRecorded(comparison, harnessRoot, harnessEnv, path.join(output, "comparison.log"));
  }
  console.log(`Source builds and ${proof.snapshot_count} frozen baseline snapshots verified; comparison ${preparation.comparison_started ? "executed" : "not started"}. Evidence: ${output}`);
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    prepareComparison(optionsFromArgs(process.argv.slice(2)));
  } catch (error) {
    console.error(error.message);
    process.exitCode = error.exitCode ?? 2;
  }
}
