import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const originalLog = console.log;
console.log = () => {};
const report = await import("./language-data-quality-report.mjs");
console.log = originalLog;

test("applicability validation rejects languages outside the registry", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: {},
      languages: ["rust"],
      domainApplicability: { symbols: { not_applicable: ["future"] } },
      capabilityRows: [],
    }),
    /unknown language: domain=symbols bucket=not_applicable language=future/,
  );
});

test("applicability validation rejects duplicate languages in a bucket", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: {},
      languages: ["rust"],
      domainApplicability: { symbols: { not_applicable: ["rust", "rust"] } },
      capabilityRows: [],
    }),
    /duplicate bucket: domain=symbols language=rust/,
  );
});

test("applicability validation rejects conflicting buckets", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: {},
      languages: ["rust"],
      domainApplicability: { symbols: { not_applicable: ["rust"], convention_only: ["rust"] } },
      capabilityRows: [],
    }),
    /duplicate bucket: domain=symbols language=rust/,
  );
});

test("applicability validation rejects fixture evidence classified as absent", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: { symbols: ["rust"] },
      languages: ["rust"],
      domainApplicability: { symbols: { not_applicable: ["rust"] } },
      capabilityRows: [],
    }),
    /fixture-proven conflict: domain=symbols bucket=not_applicable language=rust/,
  );
});

test("applicability validation rejects N/A classifications that hide ledger gaps", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: {},
      languages: ["rust"],
      domainApplicability: { symbols: { not_applicable: ["rust"] } },
      capabilityRows: [{
        language: "rust",
        kind_coverage: { symbols: { open_gaps: [{ kind: "function" }] } },
      }],
    }),
    /ledger conflict: domain=symbols bucket=not_applicable language=rust/,
  );
});

test("applicability validation rejects unclassified languages", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: {},
      languages: ["future"],
      domainApplicability: {},
      capabilityRows: [],
      requireComplete: true,
    }),
    /unclassified language: symbols\.future/,
  );
});

test("default applicability validation leaves completeness to the explicit gate", () => {
  assert.doesNotThrow(() => report.validateDomainApplicability({
    byDomain: {},
    languages: ["future"],
    domainApplicability: {},
    capabilityRows: [],
  }));
});

test("applicability validation rejects supported ledger claims without fixture evidence", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: {},
      languages: ["rust"],
      domainApplicability: {},
      capabilityRows: [{ language: "rust", kind_coverage: { symbols: { supported: ["function"] } } }],
      requireClaims: true,
    }),
    /unsupported claim: domain=symbols language=rust unit=function/,
  );
});

test("applicability validation rejects an unsupported unit beside a proven unit", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: { symbols: ["rust"] },
      languages: ["rust"],
      domainApplicability: {},
      capabilityRows: [{
        language: "rust",
        kind_coverage: { symbols: { supported: ["function", "imaginary_kind"] } },
      }],
      goldenFixturesByLanguage: {
        rust: [{ fixture: { source: "source.rs" }, expected: { symbols: [{ kind: "function" }] } }],
      },
      requireClaims: true,
    }),
    /unsupported claim: domain=symbols language=rust unit=imaginary_kind/,
  );
});

test("identifier access metadata needs a recognized nonempty value for fixture proof", () => {
  for (const access of ["", "execute"]) {
    assert.throws(
      () => report.validateDomainApplicability({
        byDomain: { identifier_metadata: ["rust"] },
        languages: ["rust"],
        domainApplicability: {},
        capabilityRows: [{
          language: "rust",
          kind_coverage: { identifier_metadata: { supported: ["identifiers.access"] } },
        }],
        goldenFixturesByLanguage: {
          rust: [{
            fixture: { source: "source.rs" },
            expected: { identifiers: [{ kind: "variable_ref", metadata: { access } }] },
          }],
        },
        requireClaims: true,
      }),
      /unsupported claim: domain=identifier_metadata language=rust unit=identifiers\.access/,
    );
  }
});

test("relationship kind proof accepts registered pending relationship families", () => {
  assert.doesNotThrow(() => report.validateDomainApplicability({
    byDomain: { relationships: [] },
    languages: ["rust"],
    domainApplicability: {},
    capabilityRows: [{
      language: "rust",
      kind_coverage: { relationships: { supported: ["call"] } },
    }],
    goldenFixturesByLanguage: {
      rust: [{ fixture: { source: "source.rs" }, expected: {
        pending_relationships: [{ kind: "call" }],
      } }],
    },
    requireClaims: true,
  }));
});

test("structural facts do not prove unrelated relationship kinds", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: { relationships: ["rust"] },
      languages: ["rust"],
      domainApplicability: {},
      capabilityRows: [{
        language: "rust",
        kind_coverage: { relationships: { supported: ["call"] } },
      }],
      goldenFixturesByLanguage: {
        rust: [{ fixture: { source: "source.rs" }, expected: {
          structural_facts: [{ pattern_id: "call" }],
        } }],
      },
      requireClaims: true,
    }),
    /unsupported claim: domain=relationships language=rust unit=call/,
  );
});

test("body span proof requires source-matching byte and coordinate spans", () => {
  const capabilities = JSON.parse(fs.readFileSync(new URL(
    "../fixtures/extraction/capabilities.json",
    import.meta.url,
  )));
  const language = capabilities.languages.find((row) => row.language === "javascript");
  const fixture = language.fixtures.find((row) =>
    row.expected === "fixtures/extraction/javascript/backend_http_boundaries/expected.json",
  );
  const expected = JSON.parse(fs.readFileSync(new URL(`../${fixture.expected}`, import.meta.url)));
  const symbol = structuredClone(expected.symbols.find((row) => row.body_span));
  symbol.body_span.start_column += 1;

  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: { body_spans: ["javascript"] },
      languages: ["javascript"],
      domainApplicability: {},
      capabilityRows: [{
        language: "javascript",
        kind_coverage: { body_spans: { supported: [symbol.kind] } },
      }],
      goldenFixturesByLanguage: {
        javascript: [{ fixture, expected: { symbols: [symbol] } }],
      },
      requireClaims: true,
    }),
    new RegExp(`unsupported claim: domain=body_spans language=javascript unit=${symbol.kind}`),
  );
});

test("annotation proof needs a nonempty annotation name", () => {
  for (const annotation of [null, "", " \t "]) {
    assert.throws(
      () => report.validateDomainApplicability({
        byDomain: { annotations: ["rust"] },
        languages: ["rust"],
        domainApplicability: {},
        capabilityRows: [{
          language: "rust",
          kind_coverage: { annotations: { supported: ["function"] } },
        }],
        goldenFixturesByLanguage: {
          rust: [{ fixture: { source: "source.rs" }, expected: {
            symbols: [{ kind: "function", annotations: [{ annotation }] }],
          } }],
        },
        requireClaims: true,
      }),
      /unsupported claim: domain=annotations language=rust unit=function/,
    );
  }
});

test("doc-comment proof needs non-whitespace text", () => {
  assert.throws(
    () => report.validateDomainApplicability({
      byDomain: { doc_comments: ["rust"] },
      languages: ["rust"],
      domainApplicability: {},
      capabilityRows: [{
        language: "rust",
        kind_coverage: { doc_comments: { supported: ["function"] } },
      }],
      goldenFixturesByLanguage: {
        rust: [{ fixture: { source: "source.rs" }, expected: {
          symbols: [{ kind: "function", doc_comment: " \t\n " }],
        } }],
      },
      requireClaims: true,
    }),
    /unsupported claim: domain=doc_comments language=rust unit=function/,
  );
});

test("golden registry validation detects stale registrations in both directions", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "language-coverage-"));
  const registered = path.join(root, "fixtures", "extraction", "registered", "expected.json");
  const unregistered = path.join(root, "fixtures", "extraction", "unregistered", "expected.json");
  fs.mkdirSync(path.dirname(unregistered), { recursive: true });
  fs.writeFileSync(unregistered, "{}");

  try {
    assert.deepEqual(
      report.reconcileGoldenRegistry({
        root,
        capabilities: { languages: [{ fixtures: [{ expected: "fixtures/extraction/registered/expected.json" }] }] },
      }),
      [
        "registered golden missing on disk: fixtures/extraction/registered/expected.json (fix the capabilities.json fixtures entry or restore the file)",
        "golden on disk is not registered in capabilities.json: fixtures/extraction/unregistered/expected.json",
      ],
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("qmldir dependencies and XML schema types retain their native fact evidence", () => {
  const qmldir = JSON.parse(fs.readFileSync(new URL(
    "../fixtures/extraction/qmldir/basic/expected.json",
    import.meta.url,
  )));
  const xmlSchema = JSON.parse(fs.readFileSync(new URL(
    "../fixtures/extraction/xml/xsd/expected.json",
    import.meta.url,
  )));

  assert.ok(qmldir.structural_facts.some((fact) => fact.pattern_id === "qmldir.depends.v1"));
  assert.ok(qmldir.structural_facts.some((fact) => fact.pattern_id === "qmldir.object_type.v1"));
  assert.ok(xmlSchema.structural_facts.some((fact) => fact.pattern_id === "xml.xsd.type.v1"));
});

test("qmldir uses domain-language coverage expectations", () => {
  assert.deepEqual(report.expectedDomainsFor("qmldir"), new Set([
    "identifiers",
    "literals",
    "source_regions",
    "structural_facts",
  ]));
});

test("applicability gate passes after every observed domain is classified", () => {
  const result = spawnSync(process.execPath, [
    fileURLToPath(new URL("./language-data-quality-report.mjs", import.meta.url)),
    "--strict",
    "--check-applicability",
  ], { encoding: "utf8" });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /unclassified_gaps: 0/);
  assert.equal((result.stdout.match(/unclassified_gaps: 0/g) ?? []).length, 14);
  const relationships = result.stdout.split("relationships:\n")[1].split("\n\n")[0];
  const types = result.stdout.split("types:\n")[1].split("\n\n")[0];
  const typeArguments = result.stdout.split("type_argument_usages:\n")[1].split("\n\n")[0];
  const structuralFacts = result.stdout.split("structural_facts:\n")[1].split("\n\n")[0];
  assert.match(relationships, /not_applicable:[^\n]*qmldir/);
  assert.match(types, /not_applicable:[^\n]*gomod[^\n]*gosum[^\n]*qmldir[^\n]*xml/);
  assert.match(typeArguments, /not_applicable:[^\n]*gomod[^\n]*gosum[^\n]*qmldir[^\n]*xml/);
  assert.match(structuralFacts, /fixture_proven_native:[^\n]*qmldir[^\n]*xml/);
});
