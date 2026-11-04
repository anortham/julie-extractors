#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(import.meta.dirname, "..");
const CAPABILITIES_PATH = path.join(ROOT, "fixtures/extraction/capabilities.json");

const DOMAINS = [
  "symbols",
  "relationships",
  "identifiers",
  "identifier_metadata",
  "body_spans",
  "structural_facts",
  "complexity_metrics",
  "annotations",
  "doc_comments",
  "literals",
  "source_regions",
  "test_detection",
];

const TEST_ROLE_UNITS = ["test_case", "test_container", "test_lifecycle"];

function roleTrue(symbol, field) {
  return symbol[field] === true || symbol.metadata?.[field] === true;
}

// Per test-evidence-v1: lifecycle hooks also set is_test, so test_case
// evidence requires is_test without test_lifecycle.
const TEST_ROLE_PREDICATES = {
  test_case: (symbol) => roleTrue(symbol, "is_test") && !roleTrue(symbol, "test_lifecycle"),
  test_container: (symbol) => roleTrue(symbol, "test_container"),
  test_lifecycle: (symbol) => roleTrue(symbol, "test_lifecycle"),
};

export const OBSERVED_DOMAINS = [
  ...DOMAINS,
  "pending_relationships",
  "types",
  "type_argument_usages",
];

const CODE_LANGUAGE_EXPECTATIONS = new Set([
  "annotations",
  "complexity_metrics",
  "doc_comments",
  "identifiers",
  "literals",
  "source_regions",
  "structural_facts",
  "test_detection",
]);

const DOMAIN_LANGUAGE_EXPECTATIONS = new Set([
  "identifiers",
  "literals",
  "source_regions",
  "structural_facts",
]);

const DOMAIN_LANGUAGES = new Set([
  "css",
  "gomod",
  "gosum",
  "html",
  "json",
  "markdown",
  "qmldir",
  "regex",
  "sql",
  "toml",
  "xml",
  "yaml",
]);

// Script-local applicability metadata. Domains without an entry report
// unclassified gaps for languages lacking fixture-proven rows.
const DOMAIN_APPLICABILITY = {
  type_argument_usages: {
    not_applicable: [
      "c",
      "javascript",
      "jsx",
      "html",
      "css",
      "gomod",
      "gosum",
      "qmldir",
      "xml",
      "r",
      "bash",
      "sql",
      "regex",
      "markdown",
      "json",
      "toml",
      "yaml",
    ],
    convention_only: ["php", "ruby", "lua"],
    native_debt: [],
    quality_debt: [],
  },
  relationships: {
    not_applicable: ["qmldir"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  identifiers: {
    not_applicable: ["json", "markdown", "toml"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  identifier_metadata: {
    not_applicable: [
      "gomod",
      "gosum",
      "qmldir",
      "css",
      "regex",
      "markdown",
      "json",
      "toml",
      "yaml",
    ],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  body_spans: {
    not_applicable: [],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  source_regions: {
    not_applicable: [],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  pending_relationships: {
    not_applicable: ["gosum", "qmldir", "regex"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  types: {
    not_applicable: [
      "css",
      "gomod",
      "gosum",
      "json",
      "markdown",
      "qmldir",
      "toml",
      "xml",
      "yaml",
    ],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  complexity_metrics: {
    not_applicable: ["css", "json", "markdown", "toml", "yaml"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  annotations: {
    not_applicable: [
      "bash",
      "css",
      "html",
      "json",
      "markdown",
      "r",
      "regex",
      "sql",
      "toml",
      "yaml",
    ],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  doc_comments: {
    not_applicable: ["regex"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
};

const APPLICABILITY_BUCKETS = [
  "not_applicable",
  "convention_only",
  "native_debt",
  "quality_debt",
];

const capabilities = JSON.parse(fs.readFileSync(CAPABILITIES_PATH, "utf8"));
const ALL_LANGUAGES = capabilities.languages.map((row) => row.language);

// Every expected.json on disk must be registered in capabilities.json, and
// every registered golden must exist on disk. Unregistered goldens are
// invisible to every evidence gate; registered-but-missing goldens would
// otherwise crash observedCounts with a bare ENOENT.
export function reconcileGoldenRegistry({
  root = ROOT,
  capabilities: matrix = capabilities,
} = {}) {
  const registered = new Set(
    matrix.languages.flatMap((row) =>
      (row.fixtures ?? []).map((fixture) => path.join(root, fixture.expected)),
    ),
  );
  const problems = [];

  for (const file of registered) {
    if (!fs.existsSync(file)) {
      problems.push(
        `registered golden missing on disk: ${reportPath(root, file)} (fix the capabilities.json fixtures entry or restore the file)`,
      );
    }
  }

  const stack = [path.join(root, "fixtures/extraction")];
  while (stack.length > 0) {
    const dir = stack.pop();
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.name === "expected.json" && !registered.has(fullPath)) {
        problems.push(
          `golden on disk is not registered in capabilities.json: ${reportPath(root, fullPath)}`,
        );
      }
    }
  }

  return problems;
}

function reportPath(root, file) {
  return path.relative(root, file).split(path.sep).join("/");
}

function rows(value, field) {
  return Array.isArray(value?.[field]) ? value[field] : [];
}

function addUnit(units, domain, value) {
  if (typeof value === "string" && value.trim().length > 0) {
    units[domain].add(value);
  }
}

function identifierAccessIsUseful(identifier) {
  return (
    ["variable_ref", "member_access"].includes(identifier.kind) &&
    ["read", "write", "read_write"].includes(identifier.metadata?.access)
  );
}

function annotationIsUseful(annotation) {
  return typeof annotation?.annotation === "string" && annotation.annotation.trim().length > 0;
}

function symbolHasUsefulAnnotation(symbol) {
  return rows(symbol, "annotations").some(annotationIsUseful);
}

function symbolHasUsefulDocComment(symbol) {
  return typeof symbol.doc_comment === "string" && symbol.doc_comment.trim().length > 0;
}

function spanFieldsAreValid(span) {
  return (
    span &&
    typeof span === "object" &&
    !Array.isArray(span) &&
    [
      "start_line",
      "start_column",
      "end_line",
      "end_column",
      "start_byte",
      "end_byte",
    ].every((field) => Number.isSafeInteger(span[field]) && span[field] >= 0) &&
    span.start_byte < span.end_byte
  );
}

function sourceProofData(source) {
  const lineBreakOffsets = [];
  for (let offset = 0; offset < source.length; offset += 1) {
    if (source[offset] === 0x0a) {
      lineBreakOffsets.push(offset);
    }
  }
  return {
    lineBreakOffsets,
    source,
    validUtf8: Buffer.from(source.toString("utf8")).equals(source),
  };
}

function spanMatchesSource(sourceData, span) {
  const { lineBreakOffsets, source, validUtf8 } = sourceData;
  if (!validUtf8 || !spanFieldsAreValid(span) || span.end_byte > source.length) {
    return false;
  }

  for (const [name, offset] of [
    ["start", span.start_byte],
    ["end", span.end_byte],
  ]) {
    if (offset < source.length && source[offset] >= 0x80 && source[offset] <= 0xbf) {
      return false;
    }

    let low = 0;
    let high = lineBreakOffsets.length;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      if (lineBreakOffsets[middle] < offset) {
        low = middle + 1;
      } else {
        high = middle;
      }
    }
    const line = low + 1;
    const column = offset - (low === 0 ? 0 : lineBreakOffsets[low - 1] + 1);
    if (span[`${name}_line`] !== line || span[`${name}_column`] !== column) {
      return false;
    }
  }

  return true;
}

function bodySpanHasGoldenProof(symbol, fixture, root, sourceCache) {
  const bodySpan = symbol.body_span;
  const declaration = symbol;
  const filePath = symbol.file_path;
  const registeredSources = Array.isArray(fixture.sources) && fixture.sources.length > 0
    ? fixture.sources
    : [fixture.source];
  if (
    typeof filePath !== "string" ||
    !registeredSources.includes(filePath) ||
    !spanFieldsAreValid(bodySpan) ||
    !spanFieldsAreValid(declaration) ||
    typeof symbol.language !== "string" ||
    symbol.language.trim().length === 0 ||
    typeof symbol.body_hash !== "string" ||
    !/^[a-f0-9]{32}$/i.test(symbol.body_hash) ||
    bodySpan.start_byte < declaration.start_byte ||
    bodySpan.end_byte > declaration.end_byte
  ) {
    return false;
  }

  const rootPath = path.resolve(root);
  const sourcePath = path.resolve(rootPath, filePath);
  const relativePath = path.relative(rootPath, sourcePath);
  if (
    relativePath === "" ||
    relativePath === ".." ||
    relativePath.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relativePath)
  ) {
    return false;
  }

  let sourceData = sourceCache.get(sourcePath);
  if (!sourceData) {
    try {
      sourceData = sourceProofData(fs.readFileSync(sourcePath));
      sourceCache.set(sourcePath, sourceData);
    } catch {
      return false;
    }
  }

  return spanMatchesSource(sourceData, bodySpan) && spanMatchesSource(sourceData, declaration);
}

function observedSupportedUnits(goldenFixtures, root) {
  const observed = Object.fromEntries(DOMAINS.map((domain) => [domain, new Set()]));
  for (const { fixture, expected } of goldenFixtures ?? []) {
    const sourceCache = new Map();
    for (const symbol of rows(expected, "symbols")) {
      addUnit(observed, "symbols", symbol.kind);
      if (bodySpanHasGoldenProof(symbol, fixture, root, sourceCache)) {
        addUnit(observed, "body_spans", symbol.kind);
      }
      if (symbolHasUsefulAnnotation(symbol)) {
        addUnit(observed, "annotations", symbol.kind);
      }
      if (symbolHasUsefulDocComment(symbol)) {
        addUnit(observed, "doc_comments", symbol.kind);
      }
      for (const role of TEST_ROLE_UNITS) {
        if (TEST_ROLE_PREDICATES[role](symbol)) {
          addUnit(observed, "test_detection", role);
        }
      }
    }

    for (const field of ["relationships", "pending_relationships"]) {
      for (const relationship of rows(expected, field)) {
        addUnit(observed, "relationships", relationship.kind);
      }
    }
    for (const relationship of rows(expected, "structured_pending_relationships")) {
      addUnit(observed, "relationships", relationship.pending?.kind);
    }
    for (const identifier of rows(expected, "identifiers")) {
      addUnit(observed, "identifiers", identifier.kind);
      if (identifierAccessIsUseful(identifier)) {
        addUnit(observed, "identifier_metadata", "identifiers.access");
      }
    }
    for (const fact of rows(expected, "structural_facts")) {
      addUnit(observed, "structural_facts", fact.pattern_id);
    }
    for (const metric of rows(expected, "complexity_metrics")) {
      addUnit(observed, "complexity_metrics", metric.scope);
    }
    for (const literal of rows(expected, "literals")) {
      addUnit(observed, "literals", literal.kind);
    }
    for (const region of rows(expected, "source_regions")) {
      addUnit(observed, "source_regions", region.kind);
    }
  }

  return observed;
}

function emptyCounts() {
  return Object.fromEntries(
    [...OBSERVED_DOMAINS, ...TEST_ROLE_UNITS].map((domain) => [domain, 0]),
  );
}

function observedCounts(language) {
  const counts = emptyCounts();
  const row = capabilities.languages.find((candidate) => candidate.language === language);
  const goldenFixtures = [];
  for (const fixture of row?.fixtures ?? []) {
    const file = path.join(ROOT, fixture.expected);
    const expected = JSON.parse(fs.readFileSync(file, "utf8"));
    goldenFixtures.push({ fixture, expected });
    counts.symbols += rows(expected, "symbols").length;
    counts.relationships += rows(expected, "relationships").length;
    counts.pending_relationships +=
      rows(expected, "pending_relationships").length +
      rows(expected, "structured_pending_relationships").length;
    counts.identifiers += rows(expected, "identifiers").length;
    counts.identifier_metadata += rows(expected, "identifiers").filter((identifier) =>
      identifier.metadata && Object.hasOwn(identifier.metadata, "access"),
    ).length;
    counts.types += rows(expected, "types").length + rows(expected, "type_facts").length;
    counts.structural_facts += rows(expected, "structural_facts").length;
    counts.complexity_metrics += rows(expected, "complexity_metrics").length;
    counts.literals += rows(expected, "literals").length;
    counts.source_regions += rows(expected, "source_regions").length;
    counts.type_argument_usages += rows(expected, "type_argument_usages").length;

    for (const symbol of rows(expected, "symbols")) {
      if (symbol.body_span) {
        counts.body_spans += 1;
      }
      if (symbol.doc_comment !== undefined && symbol.doc_comment !== null) {
        counts.doc_comments += 1;
      }
      if (Array.isArray(symbol.annotations) && symbol.annotations.length > 0) {
        counts.annotations += 1;
      }
      for (const unit of TEST_ROLE_UNITS) {
        if (TEST_ROLE_PREDICATES[unit](symbol)) {
          counts[unit] += 1;
        }
      }
    }
  }
  counts.test_detection = TEST_ROLE_UNITS.reduce(
    (total, role) => total + counts[role],
    0,
  );
  return { counts, goldenFixtures };
}

function coverageState(row, domain) {
  const coverage = row.kind_coverage?.[domain] ?? {};
  const supported = Array.isArray(coverage.supported) ? coverage.supported : [];
  const notApplicable = Array.isArray(coverage.not_applicable)
    ? coverage.not_applicable
    : [];
  const openGaps = Array.isArray(coverage.open_gaps) ? coverage.open_gaps : [];
  return { coverage, supported, notApplicable, openGaps };
}

function isSilent(row, domain) {
  const state = coverageState(row, domain);
  return (
    state.supported.length + state.notApplicable.length + state.openGaps.length === 0
  );
}

export function expectedDomainsFor(language) {
  return DOMAIN_LANGUAGES.has(language)
    ? DOMAIN_LANGUAGE_EXPECTATIONS
    : CODE_LANGUAGE_EXPECTATIONS;
}

function countOpenGaps(rowsByLanguage) {
  const byDomain = Object.fromEntries(DOMAINS.map((domain) => [domain, 0]));
  let total = 0;
  for (const { row } of rowsByLanguage) {
    for (const domain of DOMAINS) {
      const n = coverageState(row, domain).openGaps.length;
      byDomain[domain] += n;
      total += n;
    }
  }
  return { total, byDomain };
}

function analyze() {
  const byDomain = Object.fromEntries(OBSERVED_DOMAINS.map((domain) => [domain, []]));
  const rowsByLanguage = [];
  const silentCells = [];
  const qualityDebts = [];
  const goldenFixturesByLanguage = {};

  for (const row of capabilities.languages) {
    const { counts, goldenFixtures } = observedCounts(row.language);
    goldenFixturesByLanguage[row.language] = goldenFixtures;
    rowsByLanguage.push({ row, counts });

    for (const domain of OBSERVED_DOMAINS) {
      if (counts[domain] > 0) {
        byDomain[domain].push(row.language);
      }
    }

    for (const domain of DOMAINS) {
      if (isSilent(row, domain)) {
        silentCells.push(`${row.language}.${domain}`);
      }
    }

    for (const domain of expectedDomainsFor(row.language)) {
      const state = coverageState(row, domain);
      if (state.supported.length === 0 && state.notApplicable.length === 0) {
        qualityDebts.push({
          language: row.language,
          domain,
          state: state.openGaps.length > 0 ? "open_gap" : "silent",
        });
      }
    }
  }

  return {
    byDomain,
    qualityDebts,
    rowsByLanguage,
    silentCells,
    openGapBacklog: countOpenGaps(rowsByLanguage),
    goldenFixturesByLanguage,
  };
}

function metadataBuckets(domain, metadata) {
  return metadata[domain] ?? Object.fromEntries(
    APPLICABILITY_BUCKETS.map((bucket) => [bucket, []]),
  );
}

function ledgerBucket(row, domain) {
  if (!row) {
    return undefined;
  }
  const state = coverageState(row, domain);
  if (state.supported.length > 0) {
    return "supported";
  }
  if (state.openGaps.length > 0) {
    return "native_debt";
  }
  if (state.notApplicable.length > 0) {
    return "not_applicable";
  }
  return undefined;
}

export function validateDomainApplicability({
  byDomain,
  languages = ALL_LANGUAGES,
  domainApplicability = DOMAIN_APPLICABILITY,
  capabilityRows = capabilities.languages,
  goldenFixturesByLanguage = {},
  requireComplete = false,
  requireClaims = false,
}) {
  if (languages.length !== new Set(languages).size) {
    throw new Error("DOMAIN_APPLICABILITY duplicate language in registry");
  }
  const allLanguages = new Set(languages);
  const capabilityByLanguage = new Map(capabilityRows.map((row) => [row.language, row]));

  for (const [domain, meta] of Object.entries(domainApplicability)) {
    if (!APPLICABILITY_VIEW_DOMAINS.includes(domain)) {
      throw new Error(`DOMAIN_APPLICABILITY unknown domain: ${domain}`);
    }
    if (!meta || typeof meta !== "object" || Array.isArray(meta)) {
      throw new Error(`DOMAIN_APPLICABILITY invalid domain metadata: domain=${domain}`);
    }
    const unknownBuckets = Object.keys(meta).filter(
      (bucket) => !APPLICABILITY_BUCKETS.includes(bucket),
    );
    if (unknownBuckets.length > 0) {
      throw new Error(
        `DOMAIN_APPLICABILITY unknown bucket: domain=${domain} bucket=${unknownBuckets[0]}`,
      );
    }

    const fixtureProven = new Set(byDomain[domain] ?? []);
    const bucketForLanguage = new Map();

    for (const bucket of APPLICABILITY_BUCKETS) {
      const entries = meta[bucket] ?? [];
      if (!Array.isArray(entries)) {
        throw new Error(`DOMAIN_APPLICABILITY invalid bucket: domain=${domain} bucket=${bucket}`);
      }

      for (const language of entries) {
        if (!allLanguages.has(language)) {
          throw new Error(
            `DOMAIN_APPLICABILITY unknown language: domain=${domain} bucket=${bucket} language=${language}`,
          );
        }
        if (bucketForLanguage.has(language)) {
          const priorBucket = bucketForLanguage.get(language);
          throw new Error(
            `DOMAIN_APPLICABILITY duplicate bucket: domain=${domain} language=${language} buckets=${priorBucket},${bucket}`,
          );
        }
        bucketForLanguage.set(language, bucket);

        const row = capabilityByLanguage.get(language);
        if (
          row &&
          DOMAINS.includes(domain) &&
          ["not_applicable", "convention_only"].includes(bucket) &&
          coverageState(row, domain).openGaps.length > 0
        ) {
          throw new Error(
            `DOMAIN_APPLICABILITY ledger conflict: domain=${domain} bucket=${bucket} language=${language}`,
          );
        }

        if (
          ["not_applicable", "convention_only", "native_debt"].includes(bucket) &&
          fixtureProven.has(language)
        ) {
          throw new Error(
            `DOMAIN_APPLICABILITY fixture-proven conflict: domain=${domain} bucket=${bucket} language=${language}`,
          );
        }
      }
    }
  }

  if (requireClaims) {
    for (const language of languages) {
      const row = capabilityByLanguage.get(language);
      if (!row) {
        continue;
      }
      const observed = observedSupportedUnits(goldenFixturesByLanguage[language], ROOT);
      for (const domain of DOMAINS) {
        for (const unit of coverageState(row, domain).supported) {
          if (!observed[domain].has(unit)) {
            throw new Error(
              `DOMAIN_APPLICABILITY unsupported claim: domain=${domain} language=${language} unit=${unit}`,
            );
          }
        }
      }
    }
  }

  if (requireComplete) {
    const missing = applicabilityView(byDomain, languages, domainApplicability, capabilityRows)
      .flatMap((view) => view.unclassifiedGaps.map((language) => `${view.domain}.${language}`));
    if (missing.length > 0) {
      throw new Error(`DOMAIN_APPLICABILITY unclassified language: ${missing[0]}`);
    }
  }
}

function sortedUnique(languages) {
  return [...new Set(languages)].sort();
}

// test_detection is excluded: its classification is per unit, lives in
// capabilities.json (enforced exactly-once by capability_matrix tests), and is
// reported in the Test-Detection Role Coverage section. A language-level
// bucket view would misreport those classified gaps as unclassified.
const APPLICABILITY_VIEW_DOMAINS = OBSERVED_DOMAINS.filter(
  (domain) => domain !== "test_detection",
);

export function applicabilityView(
  byDomain,
  languages = ALL_LANGUAGES,
  domainApplicability = DOMAIN_APPLICABILITY,
  capabilityRows = capabilities.languages,
) {
  const capabilityByLanguage = new Map(capabilityRows.map((row) => [row.language, row]));

  return APPLICABILITY_VIEW_DOMAINS.map((domain) => {
    const meta = metadataBuckets(domain, domainApplicability);
    const fixtureProvenNative = sortedUnique(byDomain[domain]);
    const classifications = Object.fromEntries(
      APPLICABILITY_BUCKETS.map((bucket) => [bucket, new Set(meta[bucket] ?? [])]),
    );

    for (const language of languages) {
      if (
        fixtureProvenNative.includes(language) ||
        APPLICABILITY_BUCKETS.some((bucket) => classifications[bucket].has(language))
      ) {
        continue;
      }

      if (DOMAINS.includes(domain) && domain !== "test_detection") {
        const bucket = ledgerBucket(capabilityByLanguage.get(language), domain);
        if (bucket && bucket !== "supported") {
          classifications[bucket].add(language);
        }
      }
    }

    const classified = new Set([
      ...fixtureProvenNative,
      ...APPLICABILITY_BUCKETS.flatMap((bucket) => [...classifications[bucket]]),
    ]);
    const unclassifiedGaps = sortedUnique(
      languages.filter((language) => !classified.has(language)),
    );
    const applicableTotal =
      languages.length -
      classifications.not_applicable.size -
      classifications.convention_only.size;
    const applicableCovered = fixtureProvenNative.length;
    const notApplicable = sortedUnique([...classifications.not_applicable]);
    const conventionOnly = sortedUnique([...classifications.convention_only]);
    const nativeDebt = sortedUnique([...classifications.native_debt]);
    const qualityDebt = sortedUnique([...classifications.quality_debt]);
    const closureComplete =
      unclassifiedGaps.length === 0 &&
      nativeDebt.length === 0 &&
      qualityDebt.length === 0 &&
      applicableCovered === applicableTotal;

    return {
      domain,
      fixtureProvenNative,
      notApplicable,
      conventionOnly,
      nativeDebt,
      qualityDebt,
      unclassifiedGaps,
      applicableTotal,
      applicableCovered,
      closureComplete,
    };
  });
}

function formatLanguageList(languages) {
  return languages.length === 0 ? "none" : languages.join(", ");
}

function printApplicabilityView(views) {
  console.log("## Applicability-Aware Domain View");
  for (const view of views) {
    console.log(`${view.domain}:`);
    if (view.closureComplete) {
      console.log(
        `  applicable_closure: ${view.applicableCovered}/${view.applicableTotal} complete`,
      );
    } else {
      console.log(
        `  applicable_closure: ${view.applicableCovered}/${view.applicableTotal} incomplete`,
      );
    }
    console.log(
      `  fixture_proven_native: ${view.fixtureProvenNative.length}/${ALL_LANGUAGES.length} ${formatLanguageList(view.fixtureProvenNative)}`,
    );
    if (view.notApplicable.length > 0) {
      console.log(
        `  not_applicable: ${view.notApplicable.length} ${formatLanguageList(view.notApplicable)}`,
      );
    }
    if (view.conventionOnly.length > 0) {
      console.log(
        `  convention_only: ${view.conventionOnly.length} ${formatLanguageList(view.conventionOnly)}`,
      );
    }
    if (view.nativeDebt.length > 0) {
      console.log(
        `  native_debt: ${view.nativeDebt.length} ${formatLanguageList(view.nativeDebt)}`,
      );
    }
    if (view.qualityDebt.length > 0) {
      console.log(
        `  quality_debt: ${view.qualityDebt.length} ${formatLanguageList(view.qualityDebt)}`,
      );
    }
    if (view.unclassifiedGaps.length > 0) {
      console.log(
        `  unclassified_gaps: ${view.unclassifiedGaps.length} ${formatLanguageList(view.unclassifiedGaps)}`,
      );
    } else {
      console.log("  unclassified_gaps: 0");
    }
    console.log("");
  }
}

function printReport({
  byDomain,
  qualityDebts,
  rowsByLanguage,
  silentCells,
  openGapBacklog,
}) {
  console.log("# Language Data Quality Scorecard");
  console.log("");
  console.log(`languages: ${capabilities.languages.length}`);
  console.log(`silent_cells: ${silentCells.length}`);
  console.log(`quality_bar_debts: ${qualityDebts.length}`);
  // open_gap_backlog is informational: classified open_gaps satisfy silent_cells,
  // so this metric tracks remaining product debt separately from gate health.
  console.log(`open_gap_backlog: ${openGapBacklog.total}`);
  console.log("");
  console.log("## Open Gap Backlog By Domain");
  for (const domain of DOMAINS) {
    const n = openGapBacklog.byDomain[domain] ?? 0;
    if (n > 0) {
      console.log(`${domain.padEnd(24)} ${n}`);
    }
  }
  if (openGapBacklog.total === 0) {
    console.log("none");
  }
  console.log("");
  console.log("## Fixture-Proven Domain Counts");
  for (const domain of OBSERVED_DOMAINS) {
    console.log(
      `${domain.padEnd(24)} ${String(byDomain[domain].length).padStart(2)}/${
        capabilities.languages.length
      } ${byDomain[domain].join(", ")}`,
    );
  }
  console.log("");
  console.log("## Observed Domains Outside kind_coverage");
  console.log(
    "types, type_argument_usages, and pending_relationships are fixture-counted",
  );
  console.log(
    "via OBSERVED_DOMAINS + DOMAIN_APPLICABILITY in this script. They are not",
  );
  console.log(
    "kind_coverage cells in capabilities.json (see docs/decisions/2026-07-17-capability-ledger-policy.md).",
  );
  console.log("");
  printApplicabilityView(applicabilityView(byDomain));
  console.log("## Silent Cells");
  if (silentCells.length === 0) {
    console.log("none");
  } else {
    for (const cell of silentCells) {
      console.log(cell);
    }
  }
  console.log("");
  console.log("## Quality-Bar Debt");
  if (qualityDebts.length === 0) {
    console.log("none");
  } else {
    for (const debt of qualityDebts) {
      console.log(`${debt.language}.${debt.domain} ${debt.state}`);
    }
  }
  console.log("");
  console.log("## Test-Detection Role Coverage");
  for (const { row, counts } of rowsByLanguage) {
    const state = coverageState(row, "test_detection");
    const supported = state.supported.join(", ") || "none";
    const open = state.openGaps.map((gap) => gap.kind).join(", ") || "none";
    console.log(
      `${row.language}: observed test_case=${counts.test_case} test_container=${counts.test_container} test_lifecycle=${counts.test_lifecycle}; supported=${state.supported.length} [${supported}]; open=${state.openGaps.length} [${open}]`,
    );
  }
  console.log("");
  console.log("## Per-Language Summary");
  for (const { row, counts } of rowsByLanguage) {
    const populated = OBSERVED_DOMAINS.filter((domain) => counts[domain] > 0);
    console.log(`${row.language}: ${populated.join(", ") || "none"}`);
  }
}

export function runReport(args = process.argv.slice(2)) {
  const registryProblems = reconcileGoldenRegistry();
  if (registryProblems.length > 0) {
    console.error("golden fixture registry is out of sync with disk:");
    for (const problem of registryProblems) {
      console.error(`  ${problem}`);
    }
    return 1;
  }

  const result = analyze();
  try {
    validateDomainApplicability({
      byDomain: result.byDomain,
      goldenFixturesByLanguage: result.goldenFixturesByLanguage,
      requireComplete: args.includes("--check-applicability"),
      requireClaims: args.includes("--check-applicability"),
    });
  } catch (error) {
    console.error(error.message);
    return 1;
  }
  printReport(result);

  if (
    args.includes("--strict") &&
    (result.silentCells.length > 0 || result.qualityDebts.length > 0)
  ) {
    return 1;
  }

  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = runReport();
}
