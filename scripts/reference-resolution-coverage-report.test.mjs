import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  normalizeContractPath,
  sourceDigest,
} from "./reference-resolution-coverage-report.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const CAPABILITIES_PATH = path.join(ROOT, "fixtures/extraction/capabilities.json");
const REPORT_PATH = path.join(
  ROOT,
  "fixtures/extraction/reference-resolution-coverage.json",
);

test("normalizes Windows-style paths before hashing the coverage report", () => {
  const capabilities = JSON.parse(fs.readFileSync(CAPABILITIES_PATH, "utf8"));
  const report = JSON.parse(fs.readFileSync(REPORT_PATH, "utf8"));

  assert.equal(
    normalizeContractPath(String.raw`fixtures\extraction\qml\test_roles\expected.json`),
    "fixtures/extraction/qml/test_roles/expected.json",
  );
  assert.equal(sourceDigest(capabilities), report.source_digest);
});
