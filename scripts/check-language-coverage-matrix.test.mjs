import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { afterEach, beforeEach, test } from "node:test";
import { OBSERVED_DOMAINS } from "./language-data-quality-report.mjs";

const { DOMAINS, validateMatrix, renderMarkdown, runCli } = await import("./check-language-coverage-matrix.mjs").catch(() => ({
  DOMAINS: [],
  validateMatrix: () => ["matrix validator is unavailable"],
  renderMarkdown: () => "",
  runCli: () => 1,
}));

let root;
let expectedPath;
let testPath;
let capabilities;
let matrix;

beforeEach(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), "coverage-matrix-"));
  fs.mkdirSync(path.join(root, "fixtures/extraction/rust"), { recursive: true });
  fs.mkdirSync(path.join(root, "crates/julie-extractors/src/tests"), { recursive: true });
  expectedPath = "fixtures/extraction/rust/expected.json";
  testPath = "crates/julie-extractors/src/tests/example.rs";
  fs.writeFileSync(path.join(root, "fixtures/extraction/rust/source.rs"), "x\n");
  fs.writeFileSync(path.join(root, expectedPath), JSON.stringify({
    symbols: [{
      kind: "function",
      file_path: "fixtures/extraction/rust/source.rs",
      start_line: 1,
      start_column: 0,
      end_line: 1,
      end_column: 1,
      start_byte: 0,
      end_byte: 1,
      body_span: { start_line: 1, start_column: 0, end_line: 1, end_column: 1, start_byte: 0, end_byte: 1 },
      body_hash: "0123456789abcdef0123456789abcdef",
    }],
    relationships: [],
    identifiers: [],
    structural_facts: [{ pattern_id: "rust.route.v1" }],
    complexity_metrics: [],
    literals: [],
    source_regions: [],
    pending_relationships: [],
    types: [],
    type_argument_usages: [],
  }));
  fs.writeFileSync(path.join(root, "fixtures/extraction/rust/other.json"), "{}\n");
  fs.writeFileSync(path.join(root, testPath), "#[test]\nfn proves_negative_case() {}\n#[test]\nfn proves_boundary_case() {}\n");
  capabilities = {
    languages: [{
      language: "rust",
      fixtures: [{ source: "fixtures/extraction/rust/source.rs", expected: expectedPath }],
      kind_coverage: {
        symbols: { supported: ["function"], not_applicable: [], open_gaps: [] },
        relationships: { supported: [], not_applicable: ["calls"], open_gaps: [] },
        identifiers: { supported: [], not_applicable: ["call"], open_gaps: [] },
        body_spans: { supported: ["function"], not_applicable: [], open_gaps: [] },
        structural_facts: {
          supported: ["rust.route.v1"], not_applicable: [],
          open_gaps: [{ kind: "rust.external_context", reason: "Requires project context." }],
        },
        complexity_metrics: { supported: [], not_applicable: ["file"], open_gaps: [] },
        annotations: { supported: [], not_applicable: ["function"], open_gaps: [] },
        doc_comments: { supported: [], not_applicable: ["function"], open_gaps: [] },
        literals: { supported: [], not_applicable: ["other"], open_gaps: [] },
        source_regions: { supported: [], not_applicable: ["comment"], open_gaps: [] },
        test_detection: { supported: [], not_applicable: ["test_case"], open_gaps: [] },
        identifier_metadata: { supported: [], not_applicable: ["identifiers.access"], open_gaps: [] },
      },
    }],
  };
  fs.mkdirSync(path.join(root, "fixtures/extraction"), { recursive: true });
  fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));
  matrix = {
    format_version: 1,
    cells: [
      ...["symbols", "body_spans", "structural_facts"].map((domain) => ({
        language: "rust",
        domain,
        classification: "supported",
        evidence: [{
          kind: "golden",
          path: expectedPath,
          pointer: domain === "symbols" ? "#/symbols/0/kind" : domain === "body_spans" ? "#/symbols/0/body_span" : "#/structural_facts/0/pattern_id",
          unit: domain === "symbols" ? "function" : domain === "body_spans" ? "function" : "rust.route.v1",
        }],
        negative_or_boundary_evidence: [
          { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
          { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
        ],
        rationale: "Golden backed native facts are supported.",
        local_defects: [],
        retained_gap_ids: domain === "structural_facts" ? ["rust:rust.external_context"] : [],
      })),
      ...["relationships", "identifiers", "identifier_metadata", "complexity_metrics", "annotations", "doc_comments", "literals", "source_regions", "test_detection", "pending_relationships", "types", "type_argument_usages"].map((domain) => ({
        language: "rust",
        domain,
        classification: "not_applicable",
        evidence: [{ kind: "source", path: testPath, symbol: "proves_negative_case" }],
        negative_or_boundary_evidence: [],
        rationale: "The bounded audit cites a source-backed applicability disposition.",
        local_defects: [],
        retained_gap_ids: [],
      })),
    ],
  };
});

afterEach(() => fs.rmSync(root, { recursive: true, force: true }));

test("accepts exact registry cells with supported units and resolved evidence", () => {
  assert.deepEqual(validateMatrix(matrix, { projectRoot: root }), []);
});

test("uses the report's canonical observed domain list", () => {
  assert.deepEqual(DOMAINS, OBSERVED_DOMAINS);
});

test("rejects missing, duplicate, and unknown language-domain cells", () => {
  const missing = structuredClone(matrix);
  missing.cells.pop();
  assert.match(validateMatrix(missing, { projectRoot: root }).join("\n"), /missing cell/);

  const duplicate = structuredClone(matrix);
  duplicate.cells.push(structuredClone(duplicate.cells[0]));
  assert.match(validateMatrix(duplicate, { projectRoot: root }).join("\n"), /duplicate cell/);

  const unknown = structuredClone(matrix);
  unknown.cells[0].language = "made_up";
  assert.match(validateMatrix(unknown, { projectRoot: root }).join("\n"), /unknown language/);

  const unknownDomain = structuredClone(matrix);
  unknownDomain.cells[0].domain = "made_up";
  assert.match(validateMatrix(unknownDomain, { projectRoot: root }).join("\n"), /unknown domain/);
});

test("rejects a supported unit without matching golden proof", () => {
  const changed = structuredClone(matrix);
  changed.cells[0].evidence[0].unit = "method";
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /missing golden proof/);
});

test("rejects broken, null, empty, and malformed golden pointers", () => {
  for (const [pointer, expected] of [
    ["#/missing", /unresolved JSON pointer/],
    ["#/symbols/0/name", /unresolved JSON pointer|does not prove/],
    ["#/symbols/0/body_hash", /does not prove/],
    ["#/symbols/0/~2", /malformed JSON pointer/],
  ]) {
    const changed = structuredClone(matrix);
    changed.cells[0].evidence[0].pointer = pointer;
    assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), expected);
  }
});

test("rejects body spans without a paired body hash", () => {
  const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  expected.symbols[0].body_hash = null;
  fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
  assert.match(validateMatrix(matrix, { projectRoot: root }).join("\n"), /body spans|body span|claims differ/);
});

test("rejects reversed and out of source body spans", () => {
  const original = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  for (const span of [
    { ...original.symbols[0].body_span, start_byte: 1, end_byte: 1 },
    { ...original.symbols[0].body_span, end_byte: 20 },
  ]) {
    const expected = structuredClone(original);
    expected.symbols[0].body_span = span;
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
    assert.match(validateMatrix(matrix, { projectRoot: root }).join("\n"), /body spans|body span|claims differ/);
  }
});

test("rejects unregistered golden paths and files outside the project", () => {
  for (const [goldenPath, expected] of [
    ["fixtures/extraction/rust/other.json", /not registered/],
    ["../outside.json", /escapes project root/],
  ]) {
    const changed = structuredClone(matrix);
    changed.cells[0].evidence[0].path = goldenPath;
    assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), expected);
  }
});

test("rejects missing negative or boundary tests and nonexistent test names", () => {
  const changed = structuredClone(matrix);
  changed.cells[0].negative_or_boundary_evidence[1].name = "does_not_exist";
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /test not found/);

  changed.cells[0].negative_or_boundary_evidence.pop();
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /boundary evidence/);
});

test("rejects Rust test declarations inside literals while accepting lifetime parameters", () => {
  fs.writeFileSync(path.join(root, testPath), [
    'const NORMAL: &str = "#[test] fn fake_normal() {}";',
    'const RAW: &str = r###"#[test] fn fake_raw() {}"###;',
    "let hash = '#';",
    "let quote = '\\'';",
    "#[test]",
    "fn proves_negative_case() {}",
    "#[test]",
    "fn proves_boundary_case() {}",
    "#[test]",
    "fn actual_test<'a>() {}",
    "",
  ].join("\n"));

  for (const name of ["fake_normal", "fake_raw"]) {
    const changed = structuredClone(matrix);
    changed.cells[0].negative_or_boundary_evidence = [
      { kind: "test", path: testPath, name, role: "negative" },
      { kind: "test", path: testPath, name: "actual_test", role: "boundary" },
    ];
    const errors = validateMatrix(changed, { projectRoot: root }).join("\n");
    assert.match(errors, new RegExp(`test not found: ${testPath}::${name}`));
    assert.doesNotMatch(errors, /test not found: .*::actual_test/);
  }
});

test("requires source rationale for not applicable cells", () => {
  const changed = structuredClone(matrix);
  const cell = changed.cells.find(({ domain }) => domain === "relationships");
  cell.rationale = "  ";
  cell.evidence = [];
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /rationale|source evidence/);

  cell.evidence = [{ kind: "source", path: testPath, symbol: "absent_contract_heading" }];
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /source symbol not found/);
});

test("rejects unresolved defects and broken verification evidence", () => {
  const changed = structuredClone(matrix);
  changed.cells[0].local_defects = [{
    id: "body-span-1",
    status: "open",
    fix: "Add a body span.",
    verification: [{ kind: "test", path: testPath, name: "absent_test", role: "boundary" }],
  }];
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /unresolved defect/);
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /test not found/);
});

test("rejects duplicate local defect identifiers", () => {
  const changed = structuredClone(matrix);
  changed.cells[0].local_defects = ["first", "second"].map(() => ({
    id: "duplicate-id",
    status: "fixed",
    fix: "Correct the extracted fact.",
    verification: [{ kind: "test", path: testPath, name: "proves_negative_case" }],
  }));
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /duplicate local defect id/);
});

test("matches retained gap identifiers to the capability ledger exactly", () => {
  const changed = structuredClone(matrix);
  changed.cells[2].retained_gap_ids.push("rust:invented_gap");
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /unmatched retained gap/);
});

test("rejects symlinked evidence that escapes the project root", (t) => {
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), "coverage-outside-"));
  t.after(() => fs.rmSync(outside, { recursive: true, force: true }));
  fs.writeFileSync(path.join(outside, "expected.json"), "{}\n");
  fs.symlinkSync(outside, path.join(root, "escape"), process.platform === "win32" ? "junction" : "dir");
  capabilities.languages[0].fixtures.push({ expected: "escape/expected.json" });
  fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));
  const changed = structuredClone(matrix);
  changed.cells[0].evidence[0].path = "escape/expected.json";
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /escapes project root/);
});

test("accepts relationship kinds from direct, pending, and structured pending rows", () => {
  const families = [
    ["relationships", "#/relationships/0/kind", { kind: "calls", from_key: "source:caller", to_key: "source:callee", file_path: "fixtures/extraction/rust/source.rs", line_number: 1, confidence: "0.8" }],
    ["pending_relationships", "#/pending_relationships/0/kind", { kind: "calls", from_key: "source:caller", callee_name: "callee", file_path: "fixtures/extraction/rust/source.rs", line_number: 1, confidence: "0.8" }],
    ["structured_pending_relationships", "#/structured_pending_relationships/0/pending/kind", { pending: { kind: "calls", from_key: "source:caller", callee_name: "callee", file_path: "fixtures/extraction/rust/source.rs", line_number: 1, confidence: "0.8" }, target: { display_name: "callee", terminal_name: "callee" } }],
  ];

  for (const [field, pointer, row] of families) {
    const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
    expected.relationships = [];
    expected.pending_relationships = [];
    expected.structured_pending_relationships = [];
    expected[field] = [row];
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
    capabilities.languages[0].kind_coverage.relationships.supported = ["calls"];
    capabilities.languages[0].kind_coverage.relationships.not_applicable = [];
    fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));

    const changed = structuredClone(matrix);
    const cell = changed.cells.find(({ domain }) => domain === "relationships");
    cell.classification = "supported";
    cell.evidence = [{ kind: "golden", path: expectedPath, pointer, unit: "calls" }];
    cell.negative_or_boundary_evidence = [
      { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
      { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
    ];
    if (field === "pending_relationships") {
      const pendingCell = changed.cells.find(({ domain }) => domain === "pending_relationships");
      pendingCell.classification = "supported";
      pendingCell.evidence = [{ kind: "golden", path: expectedPath, pointer: "#/pending_relationships/0", unit: "pending_relationships" }];
      pendingCell.negative_or_boundary_evidence = structuredClone(cell.negative_or_boundary_evidence);
    }

    assert.deepEqual(validateMatrix(changed, { projectRoot: root }), [], `${field} should prove relationship calls`);

    expected[field] = [];
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
    assert.notDeepEqual(validateMatrix(changed, { projectRoot: root }), [], `${field} removal should invalidate relationship calls`);
  }
});

test("rejects structural facts used as relationship kind proof", () => {
  const changed = structuredClone(matrix);
  const cell = changed.cells.find(({ domain }) => domain === "relationships");
  cell.evidence = [{ kind: "golden", path: expectedPath, pointer: "#/structural_facts/0/pattern_id", unit: "calls" }];
  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /does not prove unit calls/);
});

test("rejects arbitrary objects labeled as type facts", () => {
  const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  expected.types = [{ kind: "types", arbitrary: "nonempty" }];
  fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));

  const changed = structuredClone(matrix);
  const cell = changed.cells.find(({ domain }) => domain === "types");
  cell.classification = "supported";
  cell.evidence = [{ kind: "golden", path: expectedPath, pointer: "#/types/0", unit: "types" }];
  cell.negative_or_boundary_evidence = [
    { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
    { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
  ];

  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /does not prove unit types/);
});

test("accepts actual-shaped observed-domain facts and detects removal of the last row", () => {
  const cases = [
    {
      domain: "pending_relationships",
      field: "pending_relationships",
      row: { from_key: "source:caller", callee_name: "callee", kind: "calls", file_path: "fixtures/extraction/rust/source.rs", line_number: 1, confidence: "0.8" },
    },
    {
      domain: "types",
      field: "types",
      row: { symbol_key: "source:symbol", resolved_type: "Vec<Item>", generic_params: null, constraints: null, is_inferred: true, language: "rust", metadata: null },
    },
    {
      domain: "types",
      field: "type_facts",
      row: { type_fact_id: "type-fact-1", symbol_id: "source:symbol", resolved_type: "Vec<Item>", generic_params_json: null, constraints_json: null, is_inferred: true, metadata_json: null },
    },
    {
      domain: "type_argument_usages",
      field: "type_argument_usages",
      row: { identifier_key: "source:identifier", language: "rust", file_path: "fixtures/extraction/rust/source.rs", arguments: [{ ordinal: 0, type_name: "Vec", children: [{ ordinal: 0, type_name: "Item", children: [] }] }] },
    },
  ];

  for (const { domain, field, row } of cases) {
    capabilities.languages[0].kind_coverage.relationships = { supported: [], not_applicable: ["calls"], open_gaps: [] };
    const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
    expected[field] = [row];
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));

    const changed = structuredClone(matrix);
    const relationshipCell = changed.cells.find((candidate) => candidate.domain === "relationships");
    if (domain === "pending_relationships") {
      capabilities.languages[0].kind_coverage.relationships = { supported: ["calls"], not_applicable: [], open_gaps: [] };
      relationshipCell.classification = "supported";
      relationshipCell.evidence = [{ kind: "golden", path: expectedPath, pointer: "#/pending_relationships/0/kind", unit: "calls" }];
      relationshipCell.negative_or_boundary_evidence = [
        { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
        { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
      ];
    }
    fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));
    const cell = changed.cells.find((candidate) => candidate.domain === domain);
    cell.classification = "supported";
    cell.evidence = [{ kind: "golden", path: expectedPath, pointer: `#/${field}/0`, unit: domain }];
    cell.negative_or_boundary_evidence = [
      { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
      { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
    ];

    assert.deepEqual(validateMatrix(changed, { projectRoot: root }), [], `${field} should prove ${domain}`);

    expected[field] = [];
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
    assert.notDeepEqual(validateMatrix(changed, { projectRoot: root }), [], `${field} removal should invalidate ${domain}`);
  }
});

test("requires an observed-only unitless pointer to use the matching output family", () => {
  const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  expected.types = [{ symbol_key: "source:symbol", resolved_type: "Vec<Item>", is_inferred: true, language: "rust" }];
  fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));

  const changed = structuredClone(matrix);
  const cell = changed.cells.find(({ domain }) => domain === "types");
  cell.classification = "supported";
  cell.evidence = [{ kind: "golden", path: expectedPath, pointer: "#/structural_facts/0/pattern_id" }];
  cell.negative_or_boundary_evidence = [
    { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
    { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
  ];

  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /does not prove domain types/);
});

test("accepts test-role booleans at the top level or under metadata", () => {
  for (const [unit, field] of [["test_case", "is_test"], ["test_container", "test_container"], ["test_lifecycle", "test_lifecycle"]]) {
    for (const storage of ["top-level", "metadata"]) {
      const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
      for (const roleField of ["is_test", "test_container", "test_lifecycle"]) delete expected.symbols[0][roleField];
      delete expected.symbols[0].metadata;
      if (storage === "top-level") expected.symbols[0][field] = true;
      else expected.symbols[0].metadata = { [field]: true };
      fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
      capabilities.languages[0].kind_coverage.test_detection = { supported: [unit], not_applicable: [], open_gaps: [] };
      fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));

      const changed = structuredClone(matrix);
      const cell = changed.cells.find(({ domain }) => domain === "test_detection");
      cell.classification = "supported";
      cell.evidence = [{ kind: "golden", path: expectedPath, pointer: `#/symbols/0/${storage === "metadata" ? `metadata/${field}` : field}`, unit }];
      cell.negative_or_boundary_evidence = [
        { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
        { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
      ];
      assert.deepEqual(validateMatrix(changed, { projectRoot: root }), [], `${unit} at ${storage} should be valid`);
    }
  }
});

test("does not count lifecycle hooks as test cases", () => {
  const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  expected.symbols[0].is_test = true;
  expected.symbols[0].metadata = { test_lifecycle: true };
  fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
  capabilities.languages[0].kind_coverage.test_detection = { supported: ["test_case"], not_applicable: [], open_gaps: [] };
  fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));

  const changed = structuredClone(matrix);
  const cell = changed.cells.find(({ domain }) => domain === "test_detection");
  cell.classification = "supported";
  cell.evidence = [{ kind: "golden", path: expectedPath, pointer: "#/symbols/0/is_test", unit: "test_case" }];
  cell.negative_or_boundary_evidence = [
    { kind: "test", path: testPath, name: "proves_negative_case", role: "negative" },
    { kind: "test", path: testPath, name: "proves_boundary_case", role: "boundary" },
  ];

  assert.match(validateMatrix(changed, { projectRoot: root }).join("\n"), /does not prove unit test_case/);
});

test("rejects body spans with incorrect UTF-8 coordinates or declaration ownership", () => {
  const sourcePath = path.join(root, "fixtures/extraction/rust/source.rs");
  fs.writeFileSync(sourcePath, "éx\n");
  const original = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  original.symbols[0] = {
    ...original.symbols[0],
    start_line: 1,
    start_column: 0,
    end_line: 1,
    end_column: 3,
    start_byte: 0,
    end_byte: 3,
    body_span: { start_line: 1, start_column: 0, end_line: 1, end_column: 2, start_byte: 0, end_byte: 2 },
  };

  for (const mutation of [
    (symbol) => { symbol.body_span.end_column = 1; },
    (symbol) => { symbol.body_span.start_byte = 1; symbol.body_span.start_column = 1; },
    (symbol) => { symbol.end_byte = 1; symbol.end_column = 1; },
  ]) {
    const expected = structuredClone(original);
    mutation(expected.symbols[0]);
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
    assert.notDeepEqual(validateMatrix(matrix, { projectRoot: root }), [], "invalid byte coordinates and owner bounds must reject the body span");
  }
});

test("matches UTF-8 byte columns across a BOM and CRLF source", () => {
  fs.writeFileSync(path.join(root, "fixtures/extraction/rust/source.rs"), "\uFEFFé\r\nx\n");
  const expected = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  expected.symbols[0] = {
    ...expected.symbols[0],
    start_line: 1,
    start_column: 0,
    end_line: 1,
    end_column: 5,
    start_byte: 0,
    end_byte: 5,
    body_span: { start_line: 1, start_column: 3, end_line: 1, end_column: 5, start_byte: 3, end_byte: 5 },
  };
  fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));

  assert.deepEqual(validateMatrix(matrix, { projectRoot: root }), []);
});

test("rejects unknown command line options before reading the matrix", () => {
  const script = fileURLToPath(new URL("./check-language-coverage-matrix.mjs", import.meta.url));
  const result = spawnSync(process.execPath, [script, "--unrecognized"], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /unknown option/);
});

test("fails closed when an overridden matrix file is missing", () => {
  const script = fileURLToPath(new URL("./check-language-coverage-matrix.mjs", import.meta.url));
  const result = spawnSync(process.execPath, [script, "--matrix", "fixtures/extraction/missing-validation-matrix.json"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /cannot read valid JSON/);
});

test("fails closed when the default matrix file is absent", () => {
  assert.equal(runCli([], { projectRoot: root }), 1);
  assert.equal(fs.existsSync(path.join(root, "fixtures/extraction/validation-matrix.json")), false);
});

test("renders deterministic Markdown ordered by language and domain", () => {
  const markdown = renderMarkdown(matrix);
  assert.ok(markdown.indexOf("| rust | annotations |") < markdown.indexOf("| rust | body_spans |"));
  assert.ok(markdown.includes("crates/julie-extractors/src/tests/example.rs::proves_negative_case (negative)"));
  assert.equal(markdown, renderMarkdown(structuredClone(matrix)));
});

test("writes the Markdown view only after the matrix validates", () => {
  const matrixPath = path.join(root, "matrix.json");
  const markdownPath = path.join(root, "matrix.md");
  fs.writeFileSync(matrixPath, JSON.stringify(matrix));
  assert.equal(runCli(["--matrix", "matrix.json", "--markdown", "matrix.md"], { projectRoot: root }), 0);
  assert.ok(fs.readFileSync(markdownPath, "utf8").startsWith("# Language coverage validation matrix"));

  fs.rmSync(markdownPath);
  const invalid = structuredClone(matrix);
  invalid.cells.pop();
  fs.writeFileSync(matrixPath, JSON.stringify(invalid));
  assert.equal(runCli(["--matrix", "matrix.json", "--markdown", "matrix.md"], { projectRoot: root }), 1);
  assert.equal(fs.existsSync(markdownPath), false);
});

test("refuses a Markdown output path that aliases the matrix file", () => {
  const matrixPath = path.join(root, "matrix.json");
  const markdownPath = path.join(root, "matrix-alias.json");
  const original = JSON.stringify(matrix);
  fs.writeFileSync(matrixPath, original);
  fs.linkSync(matrixPath, markdownPath);

  assert.equal(runCli(["--matrix", "matrix.json", "--markdown", "matrix-alias.json"], { projectRoot: root }), 2);
  assert.equal(fs.readFileSync(matrixPath, "utf8"), original);
});


test("call and type-usage identifiers cannot prove variable access", () => {
  const original = JSON.parse(fs.readFileSync(path.join(root, expectedPath), "utf8"));
  for (const [kind, valid] of [["call", false], ["type_usage", false], ["variable_ref", true], ["member_access", true]]) {
    const expected = structuredClone(original);
    expected.identifiers = [{ kind, name: "probe", metadata: { access: "read" } }];
    fs.writeFileSync(path.join(root, expectedPath), JSON.stringify(expected));
    capabilities.languages[0].kind_coverage.identifiers = { supported: [kind] };
    capabilities.languages[0].kind_coverage.identifier_metadata = { supported: ["identifiers.access"] };
    fs.writeFileSync(path.join(root, "fixtures/extraction/capabilities.json"), JSON.stringify(capabilities));
    const changed = structuredClone(matrix);
    for (const [domain, pointer, unit] of [
      ["identifiers", "#/identifiers/0/kind", kind],
      ["identifier_metadata", "#/identifiers/0/metadata/access", "identifiers.access"],
    ]) {
      Object.assign(changed.cells.find((cell) => cell.domain === domain), {
        classification: "supported",
        evidence: [{ kind: "golden", path: expectedPath, pointer, unit }],
        negative_or_boundary_evidence: structuredClone(matrix.cells[0].negative_or_boundary_evidence),
      });
    }
    const errors = validateMatrix(changed, { projectRoot: root });
    if (valid) assert.deepEqual(errors, []);
    else assert.match(errors.join("\n"), /does not prove unit identifiers\.access|supported classification has no positive golden facts/);
  }
});
