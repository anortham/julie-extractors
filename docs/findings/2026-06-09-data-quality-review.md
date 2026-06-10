# Extraction Data Quality Review — 2026-06-09

Follow-up to `2026-06-09-project-review.md`, focused on the quality and depth
of the extracted data itself across the 36 supported languages. Method: golden
fixture comparison (`fixtures/extraction/<language>/*/expected.json`), schema
v3 domain inventory, and extractor source verification. One agent finding was
refuted during verification and is recorded in section 5.

## 1. Overall picture

The artifact schema is rich — 13 row domains (symbols, symbol_annotations,
identifiers, relationships, pending_relationships, type_facts,
type_argument_usages, type_arguments, literals, source_regions,
structural_facts, complexity_metrics, parse_diagnostics). Core symbol
extraction (spans, byte offsets, body spans, body hashes, signatures) is
uniformly strong. Depth, however, is a pyramid, and the capability matrix
(5 booleans per language) cannot express the difference.

### Quality tiers

| Tier | Languages | Characteristics |
| --- | --- | --- |
| 1 — rich (7) | Rust, C, C++, Go, TypeScript, JavaScript, Python | Full signatures, visibility, parent linkage, body hashes, complexity metrics, structural facts |
| 2 — solid core (~19) | Java, C#, Dart, Swift, Kotlin, Scala, PHP, Ruby, Elixir, Lua, QML, VB.NET, Bash, PowerShell, R, Regex, HTML, CSS, Markdown | Good symbols/relationships/identifiers; no complexity metrics; almost no structural facts; patchy extras |
| 3 — skeletal (~10) | JSON, TOML, YAML, Markdown (minimal), SQL, Vue, JSX, Razor, GDScript | Sparse fields; 1 identifier kind common; SQL body-span coverage ~44% with `extractedFromError` views/triggers |

JSON/TOML/YAML/Markdown shallowness is intentional (config/markup); SQL, Vue,
JSX, Razor, GDScript shallowness is not.

### Domain coverage facts (verified)

- Complexity metrics: emitted for 7/36 languages only (Rust, C, C++, Go,
  TypeScript, JavaScript, Python).
- Structural facts: emitted by 12 languages (rust.unsafe_block, c/cpp
  preprocessor_definition, go.goroutine_launch/defer_statement, js/ts/jsx/tsx
  await_expression, python.decorated_definition, html alpine/htmx, csharp
  aspnet minimal-api routes, razor mvc route attributes).
- symbol_annotations: only Dart and Elixir emit any; 34/36 languages emit
  zero rows despite the table being a first-class schema v3 domain.
- Identifier kinds: QML emits 4 kinds; Bash, Vue, JSX emit only `call`;
  YAML only `variable_ref`.

## 2. Verified gaps

### 2.1 Golden contract lags the schema (HIGH — test gap)

- `NormalizedExtraction` in
  `crates/julie-extractors/src/tests/golden.rs:32-41` covers symbols,
  relationships, pending/structured-pending relationships, identifiers,
  types, and parse_diagnostics — but omits structural_facts,
  complexity_metrics, literals, source_regions, and type_argument_usages.
- Even `fixtures/extraction/rust/structural_facts/expected.json` contains no
  structural facts — only the symbols from that fixture source.
- Impact: the newest flagship domains (structural facts, complexity metrics)
  have unit tests but no golden regression pin; a shape change would not be
  caught by the golden tier.

### 2.2 Doc-comment extraction exists but is unproven by fixtures (HIGH — test gap)

- Extraction code exists broadly (Rust, TypeScript, Java, C#, and most other
  languages have doc_comment handling in their extractors; ~360 references
  across 60+ files).
- But fixture sources contain no doc comments for most languages —
  `fixtures/extraction/rust/basic/source.rs` has zero `///` lines, and only
  14 languages have any non-null `doc_comment` in expected.json (52 values
  total, mostly toml/json/markdown/powershell/bash).
- Impact: per-language doc-comment quality is unknown and unprotected. There
  is also no normalization policy across comment styles (rustdoc vs JSDoc vs
  XML-doc vs docstrings).

### 2.3 Complexity metrics limited to 7 languages (MEDIUM — capability gap)

- C#, Java, Kotlin, Swift, Dart, PHP, Ruby are mainstream consumer targets
  with no complexity rows. The metric machinery already exists for the
  Tier-1 languages.

### 2.4 symbol_annotations coverage is uneven and unproven (MEDIUM — capability gap)

- Verified wired: C# (`csharp/helpers.rs:34` `extract_annotations`, used
  across fields/members/types), Dart, Elixir, GDScript all call
  `base::normalize_annotations` and attach markers to symbols.
- Golden fixtures only show Dart/Elixir annotation markers because most
  fixture sources contain no attributes/decorators — the same fixture-gap
  pattern as doc comments (2.2).
- Apparently missing entirely (no `normalize_annotations` references): Java
  annotations, Python decorators, TypeScript/JavaScript decorators, Rust
  attributes, Kotlin annotations — confirm during implementation.
- Impact: framework-aware consumers (routing, DI, ORM mapping, test markers)
  must re-walk source for information the extractor already visits. Natural
  continuation of the ASP.NET/htmx/Alpine structural-facts work.

### 2.5 Capability matrix too coarse to express depth (MEDIUM — contract gap)

- `fixtures/extraction/capabilities.json` and the language_capabilities rows
  track 5 booleans (symbols, relationships, pending_relationships,
  identifiers, types). Nearly all languages claim all five, so Tier 1 and
  Tier 3 look identical to downstream consumers.
- Fix direction: extend capability rows to the full domain list (complexity
  metrics, structural facts, annotations, doc comments, literals, source
  regions) so depth becomes a tracked, queryable contract.

### 2.6 Identifier richness uneven in scripting/markup languages (MEDIUM)

- Bash: `call` only (no member/variable references — also flagged in the
  project review). Vue/JSX: `call` only; YAML: `variable_ref` only.
- Cheapest wins: bash variable/member references, JSX type usage.

### 2.7 SQL extraction quality (LOW-MEDIUM)

- Body-span coverage ~44%; views and triggers carry
  `"extractedFromError": true` markers in fixtures, indicating recovery-path
  extraction rather than clean parses.

## 3. Worth considering: one new domain

**File-level import edges.** Symbol-level relationships exist, but there are
no first-class file→file/module dependency rows. Downstream consumers
building module graphs must reconstruct them from import symbols. A small
`file_imports` domain would be cheap at extraction time and high leverage —
it is a schema addition, so it requires a tracked plan per the contract
rules.

## 4. Deliberate non-goals

- More languages: 36 is wide; marginal new languages add less value than
  closing Tier-2 depth gaps.
- Anything search/embedding/server-shaped: outside the product boundary.
- Deepening JSON/TOML/YAML/Markdown beyond config/markup semantics:
  intentional shallowness.

## 5. Refuted finding (do not re-report)

- **"Doc comments are 0% because extractors do not harvest them":** wrong.
  Extraction code exists across Rust, TypeScript, Java, and most languages;
  the golden fixtures simply contain no doc comments to extract. The real
  finding is the fixture/test gap recorded in 2.2.
- **"Only Dart and Elixir emit symbol_annotations":** wrong. C# and GDScript
  also wire annotations through to symbols; the fixtures just lack
  attribute/decorator examples for them. The corrected finding is in 2.4.

## 6. Recommended order

1. Golden contract expansion (2.1) and doc-comment fixtures (2.2) first —
   test-only, protect what has already shipped.
2. Complexity metrics for Tier-2 languages (2.3).
3. symbol_annotations population (2.4).
4. Capability matrix depth expansion (2.5) alongside, formalizing whatever
   is decided above.
5. Identifier richness (2.6) and SQL quality (2.7) as follow-ups.
6. File import edges (3) as a separately planned schema addition.
