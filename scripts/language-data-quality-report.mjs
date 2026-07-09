#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const ROOT = path.resolve(import.meta.dirname, "..");
const CAPABILITIES_PATH = path.join(ROOT, "fixtures/extraction/capabilities.json");

const DOMAINS = [
  "symbols",
  "relationships",
  "identifiers",
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

const OBSERVED_DOMAINS = [
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
]);

const DOMAIN_LANGUAGE_EXPECTATIONS = new Set([
  "identifiers",
  "literals",
  "source_regions",
  "structural_facts",
]);

const DOMAIN_LANGUAGES = new Set([
  "css",
  "html",
  "json",
  "markdown",
  "regex",
  "sql",
  "toml",
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
    not_applicable: [],
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
  body_spans: {
    not_applicable: ["yaml"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  source_regions: {
    not_applicable: ["regex"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  pending_relationships: {
    not_applicable: ["css", "markdown", "razor", "regex", "toml", "yaml"],
    convention_only: [],
    native_debt: [],
    quality_debt: [],
  },
  types: {
    not_applicable: ["css", "json", "markdown", "toml", "yaml"],
    convention_only: ["lua", "r"],
    native_debt: [],
    quality_debt: [],
  },
  complexity_metrics: {
    not_applicable: ["css", "html", "json", "markdown", "toml", "yaml"],
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
      "lua",
      "markdown",
      "qml",
      "r",
      "regex",
      "ruby",
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

function expectedFiles(language) {
  const row = capabilities.languages.find((candidate) => candidate.language === language);
  return (row?.fixtures ?? [])
    .map((fixture) => path.join(ROOT, fixture.expected))
    .sort();
}

function rows(value, field) {
  return Array.isArray(value?.[field]) ? value[field] : [];
}

function emptyCounts() {
  return Object.fromEntries(
    [...OBSERVED_DOMAINS, ...TEST_ROLE_UNITS].map((domain) => [domain, 0]),
  );
}

function observedCounts(language) {
  const counts = emptyCounts();
  for (const file of expectedFiles(language)) {
    const expected = JSON.parse(fs.readFileSync(file, "utf8"));
    counts.symbols += rows(expected, "symbols").length;
    counts.relationships += rows(expected, "relationships").length;
    counts.pending_relationships +=
      rows(expected, "pending_relationships").length +
      rows(expected, "structured_pending_relationships").length;
    counts.identifiers += rows(expected, "identifiers").length;
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
      if (symbol.is_test === true || symbol.metadata?.is_test === true) {
        counts.test_case += 1;
      }
      if (
        symbol.test_container === true ||
        symbol.metadata?.test_container === true
      ) {
        counts.test_container += 1;
      }
      if (
        symbol.test_lifecycle === true ||
        symbol.metadata?.test_lifecycle === true
      ) {
        counts.test_lifecycle += 1;
      }
    }
  }
  counts.test_detection = TEST_ROLE_UNITS.reduce(
    (total, role) => total + counts[role],
    0,
  );
  return counts;
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

function expectedDomainsFor(language) {
  return DOMAIN_LANGUAGES.has(language)
    ? DOMAIN_LANGUAGE_EXPECTATIONS
    : CODE_LANGUAGE_EXPECTATIONS;
}

function analyze() {
  const byDomain = Object.fromEntries(OBSERVED_DOMAINS.map((domain) => [domain, []]));
  const rowsByLanguage = [];
  const silentCells = [];
  const qualityDebts = [];

  for (const row of capabilities.languages) {
    const counts = observedCounts(row.language);
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

  return { byDomain, qualityDebts, rowsByLanguage, silentCells };
}

function validateDomainApplicability(byDomain) {
  const allLanguages = new Set(ALL_LANGUAGES);

  for (const [domain, meta] of Object.entries(DOMAIN_APPLICABILITY)) {
    const fixtureProven = new Set(byDomain[domain] ?? []);
    const bucketForLanguage = new Map();

    for (const bucket of APPLICABILITY_BUCKETS) {
      for (const language of meta[bucket] ?? []) {
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

        if (
          bucket === "not_applicable" ||
          bucket === "convention_only" ||
          bucket === "native_debt"
        ) {
          if (fixtureProven.has(language)) {
            throw new Error(
              `DOMAIN_APPLICABILITY fixture-proven conflict: domain=${domain} bucket=${bucket} language=${language}`,
            );
          }
        }
      }
    }
  }
}

function sortedUnique(languages) {
  return [...new Set(languages)].sort();
}

function applicabilityView(byDomain) {
  return OBSERVED_DOMAINS.map((domain) => {
    const meta = DOMAIN_APPLICABILITY[domain] ?? {
      not_applicable: [],
      convention_only: [],
      native_debt: [],
      quality_debt: [],
    };
    const fixtureProvenNative = sortedUnique(byDomain[domain]);
    const classified = new Set([
      ...fixtureProvenNative,
      ...meta.not_applicable,
      ...meta.convention_only,
      ...meta.native_debt,
      ...meta.quality_debt,
    ]);
    const unclassifiedGaps = sortedUnique(
      ALL_LANGUAGES.filter((language) => !classified.has(language)),
    );
    const applicableTotal =
      ALL_LANGUAGES.length -
      meta.not_applicable.length -
      meta.convention_only.length;
    const applicableCovered = fixtureProvenNative.length;
    const nativeDebt = sortedUnique(meta.native_debt);
    const qualityDebt = sortedUnique(meta.quality_debt);
    const closureComplete =
      unclassifiedGaps.length === 0 &&
      nativeDebt.length === 0 &&
      qualityDebt.length === 0 &&
      applicableCovered === applicableTotal;

    return {
      domain,
      fixtureProvenNative,
      notApplicable: sortedUnique(meta.not_applicable),
      conventionOnly: sortedUnique(meta.convention_only),
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

function printReport({ byDomain, qualityDebts, rowsByLanguage, silentCells }) {
  console.log("# Language Data Quality Scorecard");
  console.log("");
  console.log(`languages: ${capabilities.languages.length}`);
  console.log(`silent_cells: ${silentCells.length}`);
  console.log(`quality_bar_debts: ${qualityDebts.length}`);
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

const result = analyze();
validateDomainApplicability(result.byDomain);
printReport(result);

if (
  process.argv.includes("--strict") &&
  (result.silentCells.length > 0 || result.qualityDebts.length > 0)
) {
  process.exitCode = 1;
}
