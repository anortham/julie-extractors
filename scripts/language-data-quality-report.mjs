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
];

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

const capabilities = JSON.parse(fs.readFileSync(CAPABILITIES_PATH, "utf8"));

function expectedFiles(language) {
  const root = path.join(ROOT, "fixtures/extraction", language);
  if (!fs.existsSync(root)) {
    return [];
  }
  const files = [];
  const stack = [root];
  while (stack.length > 0) {
    const dir = stack.pop();
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.name === "expected.json") {
        files.push(fullPath);
      }
    }
  }
  return files.sort();
}

function rows(value, field) {
  return Array.isArray(value?.[field]) ? value[field] : [];
}

function emptyCounts() {
  return Object.fromEntries(OBSERVED_DOMAINS.map((domain) => [domain, 0]));
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
    }
  }
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
  console.log("## Per-Language Summary");
  for (const { row, counts } of rowsByLanguage) {
    const populated = OBSERVED_DOMAINS.filter((domain) => counts[domain] > 0);
    console.log(`${row.language}: ${populated.join(", ") || "none"}`);
  }
}

const result = analyze();
printReport(result);

if (process.argv.includes("--strict") && result.silentCells.length > 0) {
  process.exitCode = 1;
}
