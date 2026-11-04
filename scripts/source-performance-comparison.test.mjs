import assert from "node:assert/strict";
import fs from "node:fs";
import zlib from "node:zlib";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const { verifyRebuiltReceipt, parseCargoFeatures, createEvidenceDirectory } = await import("./source-performance-comparison.mjs").catch(() => ({
  verifyRebuiltReceipt: () => null,
  parseCargoFeatures: () => [],
  createEvidenceDirectory: () => null,
}));

function receipt() {
  return {
    format_version: 1,
    baseline_binary_sha256: "a".repeat(64),
    manifest_sha256: "b".repeat(64),
    source: {
      commit: "c".repeat(40),
      tree_hash_algorithm: "git-ls-tree-r-v1-sha256",
      tree_sha256: "d".repeat(64),
      dirty: false,
      dirty_state_sha256: null,
    },
    snapshots: ["references", "full"].map((level) => ({
      language: "all",
      level,
      workload: "fresh_scan",
      source_state_sha256: "e".repeat(64),
      semantic_dump_sha256: "f".repeat(64),
      diagnostic_counts_by_file: { "rust/recovery.rs": 1 },
      health: {
        files_scanned: 1,
        files_failed: 0,
        files_unsupported: 0,
        errors: 0,
        skipped: {},
        warnings: {},
        parse_diagnostics: { reported_total: 1 },
      },
    })),
  };
}

test("rebuilt receipt accepts only a different binary identity and snapshot ordering", () => {
  const original = receipt();
  const before = JSON.stringify(original);
  const rebuilt = structuredClone(original);
  rebuilt.baseline_binary_sha256 = "1".repeat(64);
  rebuilt.snapshots.reverse();
  assert.deepEqual(verifyRebuiltReceipt(original, rebuilt), {
    snapshot_count: 2,
    original_binary_sha256: "a".repeat(64),
    rebuilt_binary_sha256: "1".repeat(64),
  });
  assert.equal(JSON.stringify(original), before);
});

for (const [name, mutate] of [
  ["changed canonical facts", (value) => { value.snapshots[0].semantic_dump_sha256 = "2".repeat(64); }],
  ["lost diagnostic detail", (value) => { value.snapshots[0].diagnostic_counts_by_file = {}; }],
  ["lost scanned files", (value) => { value.snapshots[0].health.files_scanned = 0; }],
  ["additional skips", (value) => { value.snapshots[0].health.skipped = { slow_file_skipped: 1 }; }],
  ["changed source state", (value) => { value.snapshots[0].source_state_sha256 = "3".repeat(64); }],
  ["missing snapshot", (value) => { value.snapshots.pop(); }],
  ["duplicate snapshot", (value) => { value.snapshots[1] = structuredClone(value.snapshots[0]); }],
  ["changed manifest", (value) => { value.manifest_sha256 = "4".repeat(64); }],
  ["changed source commit", (value) => { value.source.commit = "5".repeat(40); }],
  ["unexplained new receipt field", (value) => { value.extra = true; }],
  ["malformed binary identity", (value) => { value.baseline_binary_sha256 = "invalid"; }],
]) {
  test(`rebuilt receipt rejects ${name}`, () => {
    const original = receipt();
    const rebuilt = structuredClone(original);
    mutate(rebuilt);
    assert.throws(() => verifyRebuiltReceipt(original, rebuilt));
  });
}

test("empty matching receipts cannot establish semantic equivalence", () => {
  const original = receipt();
  original.snapshots = [];
  assert.throws(() => verifyRebuiltReceipt(original, structuredClone(original)));
});

test("feature extraction retains default edges and deduplicates package features", () => {
  const tree = 'a feature "default"\na v1.0.0\tstd (*)\nb v2.0.0\talloc,std\nb feature "alloc" (*)\n';
  assert.deepEqual(parseCargoFeatures(tree), ["a/default", "a/std", "b/alloc", "b/std"]);
});

test("feature extraction agrees with the independently pinned build graph", () => {
  const tree = zlib.gunzipSync(fs.readFileSync(new URL("../docs/evidence/2026-11-03-language-coverage/candidate-feature-tree.txt.gz", import.meta.url))).toString();
  const expected = JSON.parse(fs.readFileSync(new URL("../docs/evidence/2026-11-03-language-coverage/candidate-features.json", import.meta.url)));
  assert.deepEqual(parseCargoFeatures(tree), expected);
});

test("source comparison refuses missing pins before attempting a build", () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("./source-performance-comparison.mjs", import.meta.url))], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /missing required option/);
});

test("mutable source references are rejected before creating output", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "source-comparison-contract-"));
  try {
    const output = path.join(directory, "output");
    const values = {
      "baseline-root": directory,
      "candidate-root": directory,
      "out-dir": output,
      "baseline-commit": "main",
      "candidate-commit": "c".repeat(40),
      "harness-commit": "d".repeat(40),
      "original-receipt": path.join(directory, "receipt.json"),
      "original-receipt-sha256": "e".repeat(64),
      manifest: path.join(directory, "manifest.json"),
      "manifest-sha256": "f".repeat(64),
      allowlist: path.join(directory, "allowlist.json"),
      "allowlist-sha256": "1".repeat(64),
    };
    const args = Object.entries(values).flatMap(([key, value]) => [`--${key}`, value]);
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./source-performance-comparison.mjs", import.meta.url)), ...args], { encoding: "utf8" });
    assert.equal(result.status, 2);
    assert.match(result.stderr, /baseline-commit must be a full/);
    assert.equal(fs.existsSync(output), false);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("fresh runner evidence can create its missing target parent", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "source-comparison-directory-"));
  try {
    const output = path.join(directory, "target", "comparison");
    assert.doesNotThrow(() => createEvidenceDirectory(output));
    assert.equal(fs.statSync(output).isDirectory(), true);
    assert.deepEqual(fs.readdirSync(output), []);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("existing evidence is preserved instead of reused", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "source-comparison-existing-"));
  try {
    const sentinel = path.join(directory, "receipt.json");
    fs.writeFileSync(sentinel, "reviewed evidence\n");
    assert.throws(() => createEvidenceDirectory(directory));
    assert.equal(fs.readFileSync(sentinel, "utf8"), "reviewed evidence\n");
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
