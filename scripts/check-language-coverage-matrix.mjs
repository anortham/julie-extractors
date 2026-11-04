#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { OBSERVED_DOMAINS } from "./language-data-quality-report.mjs";

const PROJECT_ROOT = path.resolve(import.meta.dirname, "..");
export const DOMAINS = OBSERVED_DOMAINS;

const CAPABILITIES_PATH = "fixtures/extraction/capabilities.json";
const MATRIX_PATH = "fixtures/extraction/validation-matrix.json";
const DOMAIN_FIELDS = {
  symbols: "symbols",
  relationships: "relationships",
  identifiers: "identifiers",
  identifier_metadata: "identifiers",
  body_spans: "symbols",
  structural_facts: "structural_facts",
  complexity_metrics: "complexity_metrics",
  annotations: "symbols",
  doc_comments: "symbols",
  literals: "literals",
  source_regions: "source_regions",
  test_detection: "symbols",
  pending_relationships: "pending_relationships",
  types: "types",
  type_argument_usages: "type_argument_usages",
};
const KIND_COVERAGE_DOMAINS = DOMAINS.slice(0, 12);

function nonEmptyText(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function array(value) {
  return Array.isArray(value) ? value : [];
}

function within(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
}

function canonicalPath(value) {
  if (process.platform !== "win32") return value;
  if (value.startsWith("\\\\?\\UNC\\")) return `\\\\${value.slice(8)}`;
  if (value.startsWith("\\\\?\\")) return value.slice(4);
  return value;
}

function safeAbsolutePath(root, value, errors, label) {
  if (!nonEmptyText(value) || value.includes("\\") || path.posix.isAbsolute(value) || path.win32.isAbsolute(value)) {
    errors.push(`${label}: path must be a nonempty project-relative path using / separators`);
    return undefined;
  }
  const absolute = path.resolve(root, value);
  if (!within(root, absolute)) {
    errors.push(`${label}: path escapes project root`);
    return undefined;
  }
  return absolute;
}

function resolveReference(root, value, errors, label) {
  const absolute = safeAbsolutePath(root, value, errors, label);
  if (!absolute) return undefined;
  try {
    const actual = canonicalPath(fs.realpathSync(absolute));
    const actualRoot = canonicalPath(fs.realpathSync(root));
    if (!within(actualRoot, actual)) {
      errors.push(`${label}: resolved path escapes project root`);
      return undefined;
    }
    if (!fs.statSync(actual).isFile()) {
      errors.push(`${label}: path does not name a file`);
      return undefined;
    }
    return actual;
  } catch {
    errors.push(`${label}: file does not exist`);
    return undefined;
  }
}

function readJson(file, errors, label) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    errors.push(`${label}: cannot read valid JSON (${error.message})`);
    return undefined;
  }
}

function parsePointer(pointer) {
  if (!nonEmptyText(pointer) || !pointer.startsWith("#")) {
    throw new Error("JSON pointer must be a URI fragment beginning with #");
  }
  let decoded;
  try {
    decoded = decodeURIComponent(pointer.slice(1));
  } catch {
    throw new Error("JSON pointer contains invalid percent encoding");
  }
  if (decoded !== "" && !decoded.startsWith("/")) {
    throw new Error("JSON pointer fragment must be empty or begin with /");
  }
  if (decoded === "") return [];
  return decoded.slice(1).split("/").map((part) => {
    if (/~(?![01])/.test(part)) throw new Error("JSON pointer has malformed escape");
    return part.replace(/~1/g, "/").replace(/~0/g, "~");
  });
}

function pointerValue(value, tokens) {
  let current = value;
  for (const token of tokens) {
    if (Array.isArray(current)) {
      if (!/^(0|[1-9][0-9]*)$/.test(token)) return undefined;
      current = current[Number(token)];
    } else if (current && typeof current === "object" && Object.hasOwn(current, token)) {
      current = current[token];
    } else {
      return undefined;
    }
  }
  return current;
}

function hasPositiveValue(value) {
  if (typeof value === "string") return value.trim().length > 0;
  if (typeof value === "number") return Number.isFinite(value) && value > 0;
  if (typeof value === "boolean") return value;
  if (Array.isArray(value)) return value.some(hasPositiveValue);
  if (value && typeof value === "object") return Object.values(value).some(hasPositiveValue);
  return false;
}

function expectedGoldenPaths(capabilities, language) {
  const row = capabilities.languages.find((candidate) => candidate.language === language);
  return new Set(array(row?.fixtures).map((fixture) => fixture.expected));
}

function sourcePathsForFixture(fixture) {
  const sources = array(fixture?.sources);
  return sources.length > 0 ? sources : nonEmptyText(fixture?.source) ? [fixture.source] : [];
}

function domainRows(domain, expected) {
  if (domain === "relationships") {
    return [
      ...array(expected?.relationships),
      ...array(expected?.pending_relationships),
      ...array(expected?.structured_pending_relationships).map((item) => item?.pending),
    ];
  }
  if (domain === "types") return [...array(expected?.types), ...array(expected?.type_facts)];
  return array(expected?.[DOMAIN_FIELDS[domain]]);
}

function roleTrue(symbol, field) {
  return symbol?.[field] === true || symbol?.metadata?.[field] === true;
}

function isSourceLocalRelationship(row, registeredSourcePaths) {
  const filePath = row?.file_path ?? row?.filePath;
  const target = row?.callee_name ?? row?.calleeName ?? row?.to_key ?? row?.to_symbol_id ?? row?.toSymbolId;
  return nonEmptyText(row?.kind)
    && nonEmptyText(target)
    && nonEmptyText(filePath)
    && registeredSourcePaths.includes(filePath)
    && Number.isInteger(row?.line_number ?? row?.lineNumber);
}

function isTypeArgument(argument) {
  return Boolean(argument)
    && typeof argument === "object"
    && !Array.isArray(argument)
    && Number.isInteger(argument.ordinal)
    && argument.ordinal >= 0
    && nonEmptyText(argument.type_name)
    && Array.isArray(argument.children)
    && argument.children.every(isTypeArgument);
}

function isUsefulDomainFact(domain, row, registeredSourcePaths) {
  if (!row || typeof row !== "object" || Array.isArray(row)) return false;
  if (domain === "pending_relationships") return isSourceLocalRelationship(row, registeredSourcePaths);
  if (domain === "types") {
    return nonEmptyText(row.symbol_key ?? row.symbol_id ?? row.symbolId)
      && nonEmptyText(row.resolved_type ?? row.resolvedType ?? row.type_name);
  }
  if (domain === "type_argument_usages") {
    const filePath = row.file_path ?? row.filePath;
    return nonEmptyText(row.identifier_key ?? row.identifier_id ?? row.identifierId)
      && nonEmptyText(row.language)
      && registeredSourcePaths.includes(filePath)
      && Array.isArray(row.arguments)
      && row.arguments.length > 0
      && row.arguments.every(isTypeArgument);
  }
  return false;
}

function hasVariableAccess(row) {
  return ["variable_ref", "member_access"].includes(row?.kind) &&
    ["read", "write", "read_write"].includes(row.metadata?.access);
}

function rowUnits(domain, expected, root, registeredSourcePaths) {
  const result = new Set();
  const rows = domainRows(domain, expected);
  if (["pending_relationships", "types", "type_argument_usages"].includes(domain)) {
    if (rows.some((row) => isUsefulDomainFact(domain, row, registeredSourcePaths))) result.add(domain);
    return result;
  }
  for (const row of rows) {
    if (!row || typeof row !== "object") continue;
    if (domain === "identifier_metadata") {
      if (hasVariableAccess(row)) result.add("identifiers.access");
    } else if (domain === "body_spans") {
      const span = row.body_span;
      if (isBodySpan(row, root, registeredSourcePaths) && nonEmptyText(row.kind)) result.add(row.kind);
    } else if (domain === "annotations") {
      if (array(row.annotations).some((annotation) => nonEmptyText(annotation?.annotation))) result.add(row.kind);
    } else if (domain === "doc_comments") {
      if (nonEmptyText(row.doc_comment)) result.add(row.kind);
    } else if (domain === "test_detection") {
      if (roleTrue(row, "is_test") && !roleTrue(row, "test_lifecycle")) result.add("test_case");
      if (roleTrue(row, "test_container")) result.add("test_container");
      if (roleTrue(row, "test_lifecycle")) result.add("test_lifecycle");
    } else if (domain === "relationships") {
      if (isSourceLocalRelationship(row, registeredSourcePaths)) result.add(row.kind);
    } else if (domain === "structural_facts") {
      if (nonEmptyText(row.pattern_id)) result.add(row.pattern_id);
    } else if (domain === "complexity_metrics") {
      if (nonEmptyText(row.scope)) result.add(row.scope);
    } else if (nonEmptyText(row.kind)) {
      result.add(row.kind);
    }
  }
  return result;
}

function advertisedUnits(languageRow, domain) {
  if (!KIND_COVERAGE_DOMAINS.includes(domain)) return [];
  const coverageDomain = domain === "identifier_metadata" ? "identifier_metadata" : domain;
  return array(languageRow.kind_coverage?.[coverageDomain]?.supported).filter(nonEmptyText).sort();
}

function retainedGaps(languageRow, language) {
  const result = new Set();
  for (const [domain, coverage] of Object.entries(languageRow.kind_coverage ?? {})) {
    for (const gap of array(coverage?.open_gaps)) {
      if (nonEmptyText(gap?.kind)) result.add(`${language}:${gap.kind}:${domain}`);
    }
  }
  return result;
}

function gapIdsForDomain(languageRow, language, domain) {
  return new Set([...retainedGaps(languageRow, language)].filter((id) => id.endsWith(`:${domain}`)).map((id) => id.slice(0, -domain.length - 1)));
}

function sameStrings(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function normalizedSpanForByteRange(bytes, startByte, endByte) {
  if (!Number.isInteger(startByte) || !Number.isInteger(endByte) || startByte < 0 || startByte >= endByte || endByte > bytes.length) return undefined;
  let startPrefix;
  let endPrefix;
  try {
    const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
    decoder.decode(bytes);
    startPrefix = decoder.decode(bytes.subarray(0, startByte));
    endPrefix = decoder.decode(bytes.subarray(0, endByte));
  } catch {
    return undefined;
  }
  const lineColumn = (prefix) => {
    const newline = prefix.lastIndexOf("\n");
    const tail = newline < 0 ? prefix : prefix.slice(newline + 1);
    return {
      line: 1 + [...prefix].filter((character) => character === "\n").length,
      column: Buffer.byteLength(tail, "utf8"),
    };
  };
  const start = lineColumn(startPrefix);
  const end = lineColumn(endPrefix);
  return {
    start_line: start.line,
    start_column: start.column,
    end_line: end.line,
    end_column: end.column,
    start_byte: startByte,
    end_byte: endByte,
  };
}

function matchesSpan(bytes, span) {
  if (!span || typeof span !== "object" || Array.isArray(span)) return false;
  const actual = normalizedSpanForByteRange(bytes, span.start_byte, span.end_byte);
  return actual !== undefined && [
    "start_line",
    "start_column",
    "end_line",
    "end_column",
    "start_byte",
    "end_byte",
  ].every((field) => Number.isInteger(span[field]) && span[field] === actual[field]);
}

function isBodySpan(row, root, registeredSourcePaths) {
  const span = row?.body_span;
  if (!span || !Number.isInteger(span.start_byte) || !Number.isInteger(span.end_byte) || span.start_byte < 0 || span.start_byte >= span.end_byte || !/^[0-9a-f]{32}$/.test(row.body_hash ?? "")) return false;
  if (!registeredSourcePaths.includes(row.file_path)) return false;
  const errors = [];
  const sourceFile = resolveReference(root, row.file_path, errors, "body span source");
  if (!sourceFile) return false;
  let bytes;
  try {
    bytes = fs.readFileSync(sourceFile);
  } catch {
    return false;
  }
  const declaration = row;
  return matchesSpan(bytes, span)
    && matchesSpan(bytes, declaration)
    && span.start_byte >= declaration.start_byte
    && span.end_byte <= declaration.end_byte
    && bytes.subarray(span.start_byte, span.end_byte).some((byte) => ![9, 10, 13, 32].includes(byte));
}

function matchesUnitProof(domain, unit, tokens, expected, target, root, registeredSourcePaths) {
  const field = DOMAIN_FIELDS[domain];
  if (["pending_relationships", "types", "type_argument_usages"].includes(domain)) {
    const fields = domain === "types" ? ["types", "type_facts"] : [field];
    return unit === domain && tokens.length === 2 && fields.includes(tokens[0]) && array(expected?.[tokens[0]]).some((row, index) => tokens[1] === String(index) && isUsefulDomainFact(domain, row, registeredSourcePaths) && target === row);
  }
  if (domain === "body_spans") {
    if (tokens.length !== 3 || tokens[0] !== "symbols" || tokens[2] !== "body_span") return false;
    const row = expected.symbols?.[Number(tokens[1])];
    return tokens[1] !== undefined && Number.isInteger(Number(tokens[1])) && row?.kind === unit && target === row.body_span && isBodySpan(row, root, registeredSourcePaths);
  }
  if (domain === "relationships") {
    if (tokens.length === 3 && ["relationships", "pending_relationships"].includes(tokens[0]) && tokens[2] === "kind") {
      const row = expected[tokens[0]]?.[Number(tokens[1])];
      return row?.kind === unit && target === unit && isSourceLocalRelationship(row, registeredSourcePaths);
    }
    if (tokens.length === 4 && tokens[0] === "structured_pending_relationships" && tokens[2] === "pending" && tokens[3] === "kind") {
      const row = expected.structured_pending_relationships?.[Number(tokens[1])]?.pending;
      return row?.kind === unit && target === unit && isSourceLocalRelationship(row, registeredSourcePaths);
    }
    return false;
  }
  if (domain === "identifier_metadata") {
    if (tokens.length !== 4 || tokens[0] !== "identifiers" || tokens[2] !== "metadata" || tokens[3] !== "access") return false;
    const row = expected.identifiers?.[Number(tokens[1])];
    return unit === "identifiers.access" && hasVariableAccess(row) && row.metadata.access === target;
  }
  if (domain === "test_detection") {
    if (tokens[0] !== "symbols" || ![3, 4].includes(tokens.length)) return false;
    const row = expected.symbols?.[Number(tokens[1])];
    if (!row || !Number.isInteger(Number(tokens[1]))) return false;
    const topLevel = tokens.length === 3;
    const metadata = tokens.length === 4 && tokens[2] === "metadata";
    if (!topLevel && !metadata) return false;
    const field = tokens.at(-1);
    const roleMatches = (name) => topLevel ? row[name] === true : row.metadata?.[name] === true;
    if (unit === "test_case") return target === true && field === "is_test" && roleMatches("is_test") && !roleTrue(row, "test_lifecycle");
    if (unit === "test_container") return target === true && field === "test_container" && roleMatches("test_container");
    if (unit === "test_lifecycle") return target === true && field === "test_lifecycle" && roleMatches("test_lifecycle");
    return false;
  }
  const indexes = {
    symbols: domain === "symbols" || domain === "annotations" || domain === "doc_comments",
    relationships: domain === "relationships",
    identifiers: domain === "identifiers",
    structural_facts: domain === "structural_facts",
    complexity_metrics: domain === "complexity_metrics",
    literals: domain === "literals",
    source_regions: domain === "source_regions",
  };
  const arrayName = Object.entries(indexes).find(([, matches]) => matches)?.[0];
  if (!arrayName || tokens.length !== 3 || tokens[0] !== arrayName || tokens[2] !== (domain === "structural_facts" ? "pattern_id" : domain === "complexity_metrics" ? "scope" : domain === "doc_comments" ? "doc_comment" : "kind")) return false;
  const row = expected[arrayName]?.[Number(tokens[1])];
  if (!row || !Number.isInteger(Number(tokens[1]))) return false;
  if (domain === "symbols" || domain === "relationships" || domain === "identifiers" || domain === "literals" || domain === "source_regions") return row.kind === unit && target === unit;
  if (domain === "structural_facts") return row.pattern_id === unit && target === unit;
  if (domain === "complexity_metrics") return row.scope === unit && target === unit;
  if (domain === "annotations") return row.kind === unit && target === unit && array(row.annotations).some((annotation) => nonEmptyText(annotation?.annotation));
  if (domain === "doc_comments") return row.kind === unit && target === row.doc_comment && nonEmptyText(target);
  return false;
}

function matchesDomainProof(domain, tokens, expected, target, registeredSourcePaths) {
  if (!["pending_relationships", "types", "type_argument_usages"].includes(domain) || tokens.length !== 2 || target === null || typeof target !== "object" || Array.isArray(target)) return false;
  const fields = domain === "types" ? ["types", "type_facts"] : [DOMAIN_FIELDS[domain]];
  return fields.includes(tokens[0]) && array(expected?.[tokens[0]]).some((row, index) => tokens[1] === String(index) && target === row && isUsefulDomainFact(domain, row, registeredSourcePaths));
}

function blankRustRange(characters, start, end) {
  for (let index = start; index < end; index += 1) {
    if (characters[index] !== "\n" && characters[index] !== "\r") characters[index] = " ";
  }
}

function rawStringEnd(source, start) {
  let rawStart = start;
  if (["b", "c"].includes(source[start]) && source[start + 1] === "r") rawStart = start + 1;
  else if (source[start] !== "r") return undefined;
  if (/[\p{ID_Continue}]/u.test(source[start - 1] ?? "")) return undefined;
  let quote = rawStart + 1;
  while (source[quote] === "#") quote += 1;
  if (source[quote] !== '"') return undefined;
  const delimiter = `"${"#".repeat(quote - rawStart - 1)}`;
  const close = source.indexOf(delimiter, quote + 1);
  return close < 0 ? source.length : close + delimiter.length;
}

function stringEnd(source, quote) {
  let index = quote + 1;
  while (index < source.length) {
    if (source[index] === "\\") index += 2;
    else if (source[index] === '"') return index + 1;
    else index += 1;
  }
  return source.length;
}

function characterEnd(source, quote) {
  let index = quote + 1;
  if (index >= source.length || source[index] === "\n" || source[index] === "\r") return undefined;
  if (source[index] === "\\") {
    index += 1;
    if (source[index] === "u" && source[index + 1] === "{") {
      const close = source.indexOf("}", index + 2);
      if (close < 0 || close - index > 16) return undefined;
      index = close + 1;
    } else if (source[index] === "x") index += 3;
    else index += 1;
  } else {
    const point = source.codePointAt(index);
    index += point > 0xffff ? 2 : 1;
  }
  return source[index] === "'" ? index + 1 : undefined;
}

function maskRustNonCode(source) {
  const characters = source.split("");
  let index = 0;
  while (index < source.length) {
    const rawEnd = rawStringEnd(source, index);
    if (rawEnd !== undefined) {
      blankRustRange(characters, index, rawEnd);
      index = rawEnd;
      continue;
    }
    if (source.startsWith("//", index)) {
      const newline = source.indexOf("\n", index + 2);
      const end = newline < 0 ? source.length : newline;
      blankRustRange(characters, index, end);
      index = end;
      continue;
    }
    if (source.startsWith("/*", index)) {
      let depth = 1;
      let end = index + 2;
      while (end < source.length && depth > 0) {
        if (source.startsWith("/*", end)) {
          depth += 1;
          end += 2;
        } else if (source.startsWith("*/", end)) {
          depth -= 1;
          end += 2;
        } else end += 1;
      }
      blankRustRange(characters, index, end);
      index = end;
      continue;
    }
    if (source[index] === '"') {
      const end = stringEnd(source, index);
      blankRustRange(characters, index, end);
      index = end;
      continue;
    }
    if (source[index] === "'") {
      const end = characterEnd(source, index);
      if (end !== undefined) {
        blankRustRange(characters, index, end);
        index = end;
        continue;
      }
    }
    index += 1;
  }
  return characters.join("");
}

function functionDeclaration(source, name) {
  const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`\\bfn\\s+${escapedName}\\b`, "g");
  let match;
  while ((match = pattern.exec(source)) !== null) {
    let index = match.index + match[0].length;
    while (/\s/.test(source[index] ?? "")) index += 1;
    if (source[index] === "<") {
      let depth = 0;
      for (; index < source.length; index += 1) {
        if (source[index] === "<") depth += 1;
        else if (source[index] === ">" && source[index - 1] !== "-") {
          depth -= 1;
          if (depth === 0) {
            index += 1;
            break;
          }
        }
      }
      if (depth !== 0) continue;
    }
    while (/\s/.test(source[index] ?? "")) index += 1;
    if (source[index] === "(") return match.index;
  }
  return -1;
}

function readTestSource(file, name) {
  const source = maskRustNonCode(fs.readFileSync(file, "utf8"));
  const declaration = functionDeclaration(source, name);
  if (declaration < 0) return false;
  const tests = [...source.slice(0, declaration).matchAll(/#\s*\[\s*test\s*\]/g)];
  const latestTest = tests.at(-1);
  if (!latestTest) return false;
  const intervening = source.slice(latestTest.index + latestTest[0].length, declaration);
  return /^\s*(?:#\s*\[[^\]]+\]\s*)*$/.test(intervening);
}

function validateReference(reference, context) {
  const { root, capabilities, language, cell, errors, cache, role } = context;
  const label = `${language}.${cell.domain} evidence`;
  if (!reference || typeof reference !== "object" || Array.isArray(reference)) {
    errors.push(`${label}: evidence reference must be an object`);
    return undefined;
  }
  if (reference.kind === "golden") {
    if (!safeAbsolutePath(root, reference.path, errors, label)) return undefined;
    if (!expectedGoldenPaths(capabilities, language).has(reference.path)) {
      errors.push(`${label}: golden path is not registered for ${language}: ${reference.path}`);
      return undefined;
    }
  }
  const file = resolveReference(root, reference.path, errors, label);
  if (!file) return undefined;
  if (reference.kind === "source") {
    if (!nonEmptyText(reference.symbol)) {
      errors.push(`${label}: source evidence requires a symbol or contract heading`);
      return undefined;
    }
    if (!fs.readFileSync(file, "utf8").includes(reference.symbol)) errors.push(`${label}: source symbol not found`);
    return { kind: "source", file };
  }
  if (reference.kind === "test") {
    if (!nonEmptyText(reference.name)) {
      errors.push(`${label}: test evidence requires a name`);
      return undefined;
    }
    if (role !== undefined && reference.role !== role) errors.push(`${label}: test evidence role must be ${role}`);
    if (reference.role !== undefined && !["negative", "boundary"].includes(reference.role)) errors.push(`${label}: test role must be negative or boundary`);
    if (!readTestSource(file, reference.name)) errors.push(`${label}: test not found: ${reference.path}::${reference.name}`);
    return { kind: "test", file, name: reference.name, role: reference.role };
  }
  if (reference.kind !== "golden") {
    errors.push(`${label}: evidence kind must be golden, test, or source`);
    return undefined;
  }
  let tokens;
  try {
    tokens = parsePointer(reference.pointer);
  } catch (error) {
    errors.push(`${label}: malformed JSON pointer (${error.message})`);
    return undefined;
  }
  if (!cache.has(file)) cache.set(file, readJson(file, errors, `${label} golden`));
  const expected = cache.get(file);
  const value = pointerValue(expected, tokens);
  const languageRow = capabilities.languages.find((candidate) => candidate.language === language);
  const fixture = array(languageRow?.fixtures).find((candidate) => candidate.expected === reference.path);
  const registeredSourcePaths = sourcePathsForFixture(fixture);
  if (value === undefined) errors.push(`${label}: unresolved JSON pointer ${reference.pointer}`);
  else if (!hasPositiveValue(value)) errors.push(`${label}: JSON pointer has null or empty proof ${reference.pointer}`);
  else if (nonEmptyText(reference.unit) && !matchesUnitProof(cell.domain, reference.unit, tokens, expected, value, root, registeredSourcePaths)) errors.push(`${label}: JSON pointer does not prove unit ${reference.unit}`);
  else if (!nonEmptyText(reference.unit) && KIND_COVERAGE_DOMAINS.includes(cell.domain)) errors.push(`${label}: golden evidence requires a unit`);
  else if (!nonEmptyText(reference.unit) && !matchesDomainProof(cell.domain, tokens, expected, value, registeredSourcePaths)) errors.push(`${label}: JSON pointer does not prove domain ${cell.domain}`);
  return { kind: "golden", file, tokens, value, unit: reference.unit };
}

function actualUnitsFor(languageRow, language, domain, root, errors, cache) {
  const units = new Set();
  for (const fixture of array(languageRow.fixtures)) {
    const expectedFile = resolveReference(root, fixture.expected, errors, `${language} fixture`);
    if (!expectedFile) continue;
    if (!cache.has(expectedFile)) cache.set(expectedFile, readJson(expectedFile, errors, `${language} fixture`));
    const expected = cache.get(expectedFile);
    for (const unit of rowUnits(domain, expected, root, sourcePathsForFixture(fixture))) units.add(unit);
  }
  return [...units].sort();
}

function validateCell(cell, languageRows, root, errors, cache, defectIds) {
  if (!cell || typeof cell !== "object" || Array.isArray(cell)) {
    errors.push("cell must be an object");
    return;
  }
  const { language, domain } = cell;
  if (!nonEmptyText(language) || !languageRows.has(language)) {
    errors.push(`unknown language: ${language ?? "<missing>"}`);
    return;
  }
  if (!DOMAINS.includes(domain)) {
    errors.push(`${language}: unknown domain: ${domain ?? "<missing>"}`);
    return;
  }
  const key = `${language}.${domain}`;
  for (const field of ["classification", "evidence", "negative_or_boundary_evidence", "rationale", "local_defects", "retained_gap_ids"]) {
    if (!Object.hasOwn(cell, field)) errors.push(`${key}: missing ${field}`);
  }
  if (![
    "supported",
    "not_applicable",
    "convention_only",
  ].includes(cell.classification)) errors.push(`${key}: invalid classification`);
  if (!nonEmptyText(cell.rationale)) errors.push(`${key}: rationale must be nonempty`);
  if (!Array.isArray(cell.evidence)) errors.push(`${key}: evidence must be an array`);
  if (!Array.isArray(cell.negative_or_boundary_evidence)) errors.push(`${key}: negative_or_boundary_evidence must be an array`);
  if (!Array.isArray(cell.local_defects)) errors.push(`${key}: local_defects must be an array`);
  if (!Array.isArray(cell.retained_gap_ids)) errors.push(`${key}: retained_gap_ids must be an array`);

  const languageRow = languageRows.get(language);
  const actualUnits = actualUnitsFor(languageRow, language, domain, root, errors, cache);
  const advertised = advertisedUnits(languageRow, domain);
  if (KIND_COVERAGE_DOMAINS.includes(domain) && !sameStrings(advertised, actualUnits)) {
    errors.push(`${key}: capability claims differ from golden evidence (advertised=${advertised.join(",") || "none"}; observed=${actualUnits.join(",") || "none"})`);
  }
  if (cell.classification === "supported" && actualUnits.length === 0) errors.push(`${key}: supported classification has no positive golden facts`);
  if (["not_applicable", "convention_only"].includes(cell.classification) && actualUnits.length > 0) errors.push(`${key}: classification conflicts with positive golden facts`);

  const wantedGaps = [...gapIdsForDomain(languageRow, language, domain)].sort();
  const foundGaps = array(cell.retained_gap_ids).filter(nonEmptyText).sort();
  if (!sameStrings(foundGaps, [...new Set(foundGaps)])) errors.push(`${key}: duplicate retained gap identifier`);
  for (const gap of foundGaps.filter((id) => !wantedGaps.includes(id))) errors.push(`${key}: unmatched retained gap identifier ${gap}`);
  for (const gap of wantedGaps.filter((id) => !foundGaps.includes(id))) errors.push(`${key}: missing retained gap identifier ${gap}`);
  if (wantedGaps.length > 0 && cell.classification !== "supported") errors.push(`${key}: retained gaps must coexist with supported local facts`);

  const allEvidence = array(cell.evidence);
  const validatedEvidence = allEvidence.map((reference) => validateReference(reference, { root, capabilities: { languages: [...languageRows.values()] }, language, cell, errors, cache }));
  if (cell.classification === "supported") {
    for (const unit of actualUnits) {
      if (!validatedEvidence.some((reference) => reference?.kind === "golden" && reference.unit === unit)) errors.push(`${key}: missing golden proof for supported unit ${unit}`);
    }
    const roles = new Set();
    for (const reference of array(cell.negative_or_boundary_evidence)) {
      const validated = validateReference(reference, { root, capabilities: { languages: [...languageRows.values()] }, language, cell, errors, cache });
      if (validated?.kind === "test") roles.add(validated.role);
    }
    for (const role of ["negative", "boundary"]) if (!roles.has(role)) errors.push(`${key}: missing ${role} evidence`);
  } else {
    for (const reference of array(cell.negative_or_boundary_evidence)) validateReference(reference, { root, capabilities: { languages: [...languageRows.values()] }, language, cell, errors, cache });
    if (!validatedEvidence.some((reference) => reference?.kind === "source")) errors.push(`${key}: source evidence is required for ${cell.classification}`);
  }

  for (const defect of array(cell.local_defects)) {
    if (!defect || typeof defect !== "object" || Array.isArray(defect)) {
      errors.push(`${key}: local defect must be an object`);
      continue;
    }
    if (!nonEmptyText(defect.id)) errors.push(`${key}: local defect requires an id`);
    else if (defectIds.has(defect.id)) errors.push(`${key}: duplicate local defect id ${defect.id}`);
    else defectIds.add(defect.id);
    if (defect.status !== "fixed") errors.push(`${key}: unresolved defect ${defect.id ?? "<missing id>"}`);
    if (!nonEmptyText(defect.fix)) errors.push(`${key}: local defect requires a fix`);
    if (!Array.isArray(defect.verification) || defect.verification.length === 0) {
      errors.push(`${key}: local defect requires verification evidence`);
      continue;
    }
    for (const reference of defect.verification) validateReference(reference, { root, capabilities: { languages: [...languageRows.values()] }, language, cell, errors, cache });
  }
}

export function validateMatrix(matrix, { projectRoot = PROJECT_ROOT } = {}) {
  const root = path.resolve(projectRoot);
  const errors = [];
  let capabilities;
  try {
    capabilities = JSON.parse(fs.readFileSync(path.join(root, CAPABILITIES_PATH), "utf8"));
  } catch (error) {
    return [`cannot read capability registry (${error.message})`];
  }
  if (!Array.isArray(capabilities.languages) || capabilities.languages.some((row) => !nonEmptyText(row?.language))) return ["capability registry has an invalid languages array"];
  const languageRows = new Map();
  for (const row of capabilities.languages) {
    if (languageRows.has(row.language)) errors.push(`duplicate language in capability registry: ${row.language}`);
    languageRows.set(row.language, row);
  }
  if (!matrix || typeof matrix !== "object" || Array.isArray(matrix)) return ["matrix must be an object"];
  if (matrix.format_version !== 1) errors.push("format_version must equal 1");
  if (!Array.isArray(matrix.cells)) return [...errors, "cells must be an array"];
  const seen = new Set();
  const cache = new Map();
  const defectIds = new Set();
  for (const cell of matrix.cells) {
    if (!cell || typeof cell !== "object" || Array.isArray(cell)) {
      errors.push("cell must be an object");
      continue;
    }
    const key = `${cell.language}.${cell.domain}`;
    if (seen.has(key)) errors.push(`duplicate cell: ${key}`);
    seen.add(key);
    validateCell(cell, languageRows, root, errors, cache, defectIds);
  }
  for (const language of languageRows.keys()) {
    for (const domain of DOMAINS) {
      const key = `${language}.${domain}`;
      if (!seen.has(key)) errors.push(`missing cell: ${key}`);
    }
  }
  for (const key of seen) {
    const separator = key.lastIndexOf(".");
    const language = key.slice(0, separator);
    const domain = key.slice(separator + 1);
    if (languageRows.has(language) && !DOMAINS.includes(domain)) errors.push(`${language}: unknown domain: ${domain}`);
  }
  return errors;
}

function safeMarkdown(value) {
  return String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll("|", "\\|").replaceAll("\r", " ").replaceAll("\n", " ").trim();
}

function describeReferences(references) {
  return array(references).map((reference) => {
    if (reference.kind === "golden") return `${reference.path} ${reference.pointer}${reference.unit ? ` (${reference.unit})` : ""}`;
    if (reference.kind === "test") return `${reference.path}::${reference.name}${reference.role ? ` (${reference.role})` : ""}`;
    return `${reference.path}${reference.symbol ? ` (${reference.symbol})` : ""}`;
  }).sort().join("; ");
}

export function renderMarkdown(matrix) {
  const compare = (left, right) => left < right ? -1 : left > right ? 1 : 0;
  const cells = [...matrix.cells].sort((left, right) => compare(left.language, right.language) || compare(left.domain, right.domain));
  const rows = cells.map((cell) => [
    cell.language,
    cell.domain,
    cell.classification,
    describeReferences(cell.evidence),
    describeReferences(cell.negative_or_boundary_evidence),
    cell.rationale,
    array(cell.local_defects).map((defect) => `${defect.id}: ${defect.status}`).sort().join("; "),
    array(cell.retained_gap_ids).sort().join("; "),
  ].map(safeMarkdown));
  return [
    "# Language coverage validation matrix",
    "",
    `Generated from validation matrix format version ${matrix.format_version}.`,
    "",
    "| Language | Domain | Classification | Positive evidence | Negative or boundary evidence | Rationale | Local defects | Retained gaps |",
    "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ...rows.map((row) => `| ${row.join(" | ")} |`),
    "",
  ].join("\n");
}

function parseArgs(args, projectRoot) {
  let matrixPath = path.resolve(projectRoot, MATRIX_PATH);
  let markdownPath;
  const seen = new Set();
  for (let index = 0; index < args.length; index += 1) {
    const flag = args[index];
    if (!["--matrix", "--markdown"].includes(flag)) throw new Error(`unknown option: ${flag}`);
    if (seen.has(flag)) throw new Error(`${flag} may be provided only once`);
    if (!args[index + 1] || args[index + 1].startsWith("--")) throw new Error(`missing value for ${flag}`);
    seen.add(flag);
    const value = path.resolve(projectRoot, args[index + 1]);
    if (flag === "--matrix") matrixPath = value;
    else markdownPath = value;
    index += 1;
  }
  if (markdownPath && markdownPath === matrixPath) throw new Error("--markdown must not overwrite the matrix JSON");
  return { matrixPath, markdownPath };
}

function comparablePath(value) {
  const resolved = path.resolve(canonicalPath(value));
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

function outputAliasesMatrix(outputPath, matrixPath) {
  try {
    const outputStat = fs.statSync(outputPath, { bigint: true });
    const matrixStat = fs.statSync(matrixPath, { bigint: true });
    if (outputStat.isFile() && matrixStat.isFile() && outputStat.dev === matrixStat.dev && outputStat.ino !== 0n && outputStat.ino === matrixStat.ino) return true;
  } catch {}
  try {
    if (comparablePath(fs.realpathSync(outputPath)) === comparablePath(fs.realpathSync(matrixPath))) return true;
  } catch {}
  try {
    const outputParent = fs.realpathSync(path.dirname(outputPath));
    const destination = path.join(outputParent, path.basename(outputPath));
    return comparablePath(destination) === comparablePath(fs.realpathSync(matrixPath));
  } catch {
    return false;
  }
}

export function runCli(args, { projectRoot = PROJECT_ROOT } = {}) {
  let paths;
  try {
    paths = parseArgs(args, path.resolve(projectRoot));
  } catch (error) {
    console.error(error.message);
    return 2;
  }
  const errors = [];
  const matrix = readJson(paths.matrixPath, errors, "matrix");
  const results = matrix === undefined ? errors : [...errors, ...validateMatrix(matrix, { projectRoot })];
  if (results.length > 0) {
    console.error("language coverage matrix validation failed:");
    for (const error of results) console.error(`  ${error}`);
    return 1;
  }
  if (paths.markdownPath) {
    if (outputAliasesMatrix(paths.markdownPath, paths.matrixPath)) {
      console.error("--markdown must not overwrite the matrix JSON");
      return 2;
    }
    try {
      fs.writeFileSync(paths.markdownPath, renderMarkdown(matrix), "utf8");
    } catch (error) {
      console.error(`cannot write Markdown view (${error.message})`);
      return 1;
    }
  }
  console.log(`language coverage matrix valid: ${matrix.cells.length} cells`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = runCli(process.argv.slice(2));
}
