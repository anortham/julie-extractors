#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const ROOT = path.resolve(import.meta.dirname, "..");
const CAPABILITIES_PATH = path.join(ROOT, "fixtures/extraction/capabilities.json");
const REPORT_PATH = path.join(
  ROOT,
  "fixtures/extraction/reference-resolution-coverage.json",
);
const ORIGIN_KINDS = {
  identifier: ["call", "member_access", "type_usage", "variable_ref"],
  relationship: [
    "calls",
    "extends",
    "implements",
    "imports",
    "instantiates",
    "references",
    "uses",
  ],
  pending_relationship: [
    "calls",
    "extends",
    "implements",
    "imports",
    "instantiates",
    "references",
    "uses",
  ],
};
const CANONICAL_KINDS = [
  "calls",
  "extends",
  "implements",
  "imports",
  "instantiates",
  "references",
  "uses",
];

function canonicalKind(origin, rawKind) {
  if (origin === "identifier") {
    return {
      call: "calls",
      member_access: "references",
      type_usage: "uses",
      variable_ref: "references",
    }[rawKind];
  }
  return CANONICAL_KINDS.includes(rawKind) ? rawKind : undefined;
}

function fixtures(capabilities) {
  return capabilities.languages.flatMap((language) =>
    language.fixtures.map((fixture) => ({
      language,
      expectedPath: path.join(ROOT, fixture.expected),
    })),
  );
}

function sourceDigest(capabilities) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(CAPABILITIES_PATH));
  for (const fixture of fixtures(capabilities).sort((left, right) =>
    left.expectedPath.localeCompare(right.expectedPath),
  )) {
    hash.update(path.relative(ROOT, fixture.expectedPath));
    hash.update(fs.readFileSync(fixture.expectedPath));
  }
  return `sha256:${hash.digest("hex")}`;
}

function rows(value, field) {
  return Array.isArray(value?.[field]) ? value[field] : [];
}

function pendingRows(expected) {
  return [
    ...rows(expected, "structured_pending_relationships").map((row) => ({
      kind: row.pending.kind,
      terminalName: row.target.terminal_name,
      line: row.pending.line_number,
      span: row.span,
    })),
    ...rows(expected, "pending_relationships").map((row) => ({
      kind: row.kind,
      terminalName: row.callee_name,
      line: row.line_number,
      span: null,
    })),
  ];
}

function applicability(language, origin, rawKind, observed) {
  if (observed > 0) {
    return "observed";
  }
  if (origin === "pending_relationship") {
    return language.capabilities.pending_relationships ? "zero" : "not_applicable";
  }
  const domain = origin === "identifier" ? "identifiers" : "relationships";
  const coverage = language.kind_coverage[domain];
  if (coverage.supported.includes(rawKind)) {
    return "zero";
  }
  if (coverage.not_applicable.includes(rawKind)) {
    return "not_applicable";
  }
  if (coverage.open_gaps.some((gap) => gap.kind === rawKind)) {
    return "open_gap";
  }
  return language.capabilities[domain] ? "zero" : "not_applicable";
}

function cellKey(cell) {
  return [
    cell.language,
    cell.origin,
    cell.raw_kind,
    cell.canonical_kind,
    cell.outcome,
    cell.tier ?? "",
    cell.method ?? "",
    cell.span_present,
    cell.applicability,
  ].join("\u0000");
}

function addCell(cells, cell) {
  const key = cellKey(cell);
  const existing = cells.get(key);
  if (existing) {
    existing.count += cell.count;
  } else {
    cells.set(key, { ...cell });
  }
}

function generate() {
  const capabilities = JSON.parse(fs.readFileSync(CAPABILITIES_PATH, "utf8"));
  const cells = new Map();

  for (const { language, expectedPath } of fixtures(capabilities)) {
    const expected = JSON.parse(fs.readFileSync(expectedPath, "utf8"));
    for (const relationship of rows(expected, "relationships")) {
      addCell(cells, {
        language: language.language,
        origin: "relationship",
        raw_kind: relationship.kind,
        canonical_kind:
          canonicalKind("relationship", relationship.kind) ?? "unmapped",
        outcome: "resolved",
        tier: 1,
        method: "extraction_direct",
        span_present:
          relationship.span !== null && relationship.span !== undefined,
        applicability: "observed",
        count: 1,
      });
    }
    for (const identifier of rows(expected, "identifiers")) {
      const resolved = identifier.target_key !== null && identifier.target_key !== undefined;
      addCell(cells, {
        language: language.language,
        origin: "identifier",
        raw_kind: identifier.kind,
        canonical_kind:
          canonicalKind("identifier", identifier.kind) ?? "unmapped",
        outcome: resolved ? "resolved" : "unattempted",
        tier: resolved ? 1 : null,
        method: resolved ? "tier1_local" : null,
        span_present: true,
        applicability: "observed",
        count: 1,
      });
    }
    for (const pending of pendingRows(expected)) {
      addCell(cells, {
        language: language.language,
        origin: "pending_relationship",
        raw_kind: pending.kind,
        canonical_kind:
          canonicalKind("pending_relationship", pending.kind) ?? "unmapped",
        outcome: ["imports", "references"].includes(pending.kind)
          ? "unattempted"
          : "unresolved_pending",
        tier: null,
        method: null,
        span_present: pending.span !== null && pending.span !== undefined,
        applicability: "observed",
        count: 1,
      });
    }
  }

  for (const language of capabilities.languages) {
    for (const [origin, rawKinds] of Object.entries(ORIGIN_KINDS)) {
      for (const rawKind of rawKinds) {
        const observed = [...cells.values()]
          .filter(
            (cell) =>
              cell.language === language.language &&
              cell.origin === origin &&
              cell.raw_kind === rawKind,
          )
          .reduce((sum, cell) => sum + cell.count, 0);
        if (observed === 0) {
          addCell(cells, {
            language: language.language,
            origin,
            raw_kind: rawKind,
            canonical_kind: canonicalKind(origin, rawKind),
            outcome: "zero",
            tier: null,
            method: null,
            span_present: false,
            applicability: applicability(language, origin, rawKind, observed),
            count: 0,
          });
        }
      }
    }
  }

  const orderedCells = [...cells.values()].sort((left, right) =>
    cellKey(left).localeCompare(cellKey(right)),
  );
  const summaries = capabilities.languages.map(({ language }) => {
    const languageCells = orderedCells.filter((cell) => cell.language === language);
    const count = (outcome) =>
      languageCells
        .filter((cell) => cell.outcome === outcome)
        .reduce((sum, cell) => sum + cell.count, 0);
    const total = languageCells.reduce((sum, cell) => sum + cell.count, 0);
    const noContext = count("no_context");
    const unattempted = count("unattempted");
    return {
      language,
      total,
      attempted: total - noContext - unattempted,
      resolved: count("resolved"),
      ambiguous: count("ambiguous"),
      missing: count("missing"),
      no_context: noContext,
      unresolved_pending: count("unresolved_pending"),
      unattempted: unattempted,
      span_present: languageCells
        .filter((cell) => cell.span_present)
        .reduce((sum, cell) => sum + cell.count, 0),
      span_missing: languageCells
        .filter((cell) => !cell.span_present)
        .reduce((sum, cell) => sum + cell.count, 0),
    };
  });

  return {
    schema_version: 1,
    source: "fixtures/extraction/capabilities.json registered canonical golden corpus",
    source_digest: sourceDigest(capabilities),
    languages: capabilities.languages.map((language) => language.language),
    canonical_kinds: CANONICAL_KINDS,
    summaries,
    cells: orderedCells,
  };
}

function validate(report) {
  const capabilities = JSON.parse(fs.readFileSync(CAPABILITIES_PATH, "utf8"));
  const expectedLanguages = capabilities.languages.map((language) => language.language);
  const problems = [];
  if (JSON.stringify(report.languages) !== JSON.stringify(expectedLanguages)) {
    problems.push("report languages must exactly equal the 36-language registry");
  }
  if (report.source_digest !== sourceDigest(capabilities)) {
    problems.push("report source_digest is stale; regenerate with --write");
  }
  if (JSON.stringify(report.canonical_kinds) !== JSON.stringify(CANONICAL_KINDS)) {
    problems.push("canonical kind vocabulary drifted");
  }
  for (const language of expectedLanguages) {
    if (!report.summaries.some((summary) => summary.language === language)) {
      problems.push(`${language}: missing summary`);
    }
    for (const [origin, rawKinds] of Object.entries(ORIGIN_KINDS)) {
      for (const rawKind of rawKinds) {
        if (
          !report.cells.some(
            (cell) =>
              cell.language === language &&
              cell.origin === origin &&
              cell.raw_kind === rawKind,
          )
        ) {
          problems.push(`${language}/${origin}/${rawKind}: silent cell`);
        }
      }
    }
  }
  for (const cell of report.cells) {
    if (!Number.isInteger(cell.count) || cell.count < 0) {
      problems.push(`${cell.language}/${cell.origin}/${cell.raw_kind}: invalid count`);
    }
    if (cell.canonical_kind === "unmapped") {
      problems.push(`${cell.language}/${cell.origin}/${cell.raw_kind}: unmapped kind`);
    }
    if (
      cell.origin === "pending_relationship" &&
      ["imports", "references"].includes(cell.raw_kind) &&
      cell.count > 0 &&
      cell.outcome !== "unattempted"
    ) {
      problems.push(
        `${cell.language}/${cell.origin}/${cell.raw_kind}: non-resolvable pending kind must be unattempted`,
      );
    }
    if (
      cell.origin === "identifier" &&
      cell.outcome === "resolved" &&
      cell.method !== "tier1_local"
    ) {
      problems.push(
        `${cell.language}/${cell.origin}/${cell.raw_kind}: resolved identifier method must be tier1_local`,
      );
    }
  }
  return problems;
}

const write = process.argv.includes("--write");
const strict = process.argv.includes("--strict");
if (write) {
  fs.writeFileSync(REPORT_PATH, `${JSON.stringify(generate(), null, 2)}\n`);
}
if (!fs.existsSync(REPORT_PATH)) {
  process.stderr.write(`missing ${path.relative(ROOT, REPORT_PATH)}; run with --write\n`);
  process.exit(1);
}
const report = JSON.parse(fs.readFileSync(REPORT_PATH, "utf8"));
const problems = validate(report);
const result = {
  languages: report.languages.length,
  cells: report.cells.length,
  silent_cells: problems.filter((problem) => problem.endsWith("silent cell")).length,
  quality_bar_debts: problems.length,
};
process.stdout.write(`${JSON.stringify(result)}\n`);
if (strict && problems.length > 0) {
  process.stderr.write(`${problems.join("\n")}\n`);
  process.exit(1);
}
