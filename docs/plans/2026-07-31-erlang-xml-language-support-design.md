# Erlang and XML language support — design

**Date:** 2026-07-31 (revised same day after Codex adversarial review)
**Driver:** Miller issue #8 (calltelemetry-jason) — a mixed Elixir/Java/TypeScript workspace where
`.erl` files in BEAM deps and `.xsd` schemas get zero index coverage. Investigation showed the
issue's error output was fabricated (no "Erlang extractor" exists to fail); the real gap is that
Erlang and XML are **unsupported languages**, silently skipped. Decision: add both.

**Architecture impact:** Existing per-language seam, **medium contract impact**. No new
architecture, but registration touches more than a `LanguageSpec` row: module export in `lib.rs`,
registry extractor dispatch (`registry.rs` symbol/identifier/relationship routing), structural-fact
collector routing plus registered pattern specs and the generated
`docs/contracts/structural-fact-patterns.json`, an `EXTRACTION_CONTRACT_VERSION` review (the
changed-path guard enforces it), and the hard-coded 36-language assertions in
`registry.rs:676`, `factory.rs:60`, and `tests/capability_snapshot_test.rs:8` (36 → 38).
`docs/languages/new-language-checklist.md` is the authoritative registration checklist; this design
adds product scope on top of it, not a replacement for it.

## Scope

### Erlang — elixir-parity tier

- **Extensions:** `.erl`, `.hrl` (deferred: `.escript`, Erlang term/config formats)
- **Grammar:** `tree-sitter-erlang` 0.20.0 (WhatsApp-maintained; published to crates.io
  2026-07-28 — verified live, GitHub releases lag). Runtime pin here is `tree-sitter =0.26.11`;
  both grammars use `tree-sitter-language 0.1`, so compatibility is expected but Phase 0 proves it.
- **Capabilities:** `FULL_CAPABILITIES`, matching elixir — **conditional on real coverage**, not
  merely nonempty vectors (see acceptance list below)
- **Module:** `crates/julie-extractors/src/erlang/`, modeled on `src/elixir/` (mod, helpers,
  identifiers, relationships, calls, test_calls, attributes/definition forms as needed)
- **Symbols:** modules, functions grouped by name/arity across clauses, records, macros
  (`-define`), type declarations (`-type`/`-opaque`), behaviour callbacks
- **Visibility:** `-export`/`-export_type` lists drive exported-vs-private; handle
  `-compile(export_all)` and macro/conditional export lists explicitly
- **Identifiers/calls:** local calls, remote calls `M:F(Args)`, fun references `fun M:F/A`
  (distinct from calls), imported functions (`-import`), auto-imported BIFs
- **Doc comments:** extractor-level capture of EDoc (`%% @doc`) and OTP 27 `-doc`/`-moduledoc`
  attributes, attached to functions, types, and callbacks (all three tested);
  `doc_comment_styles` stays `EMPTY` unless implementation finds a clean line-style fit
- **Relationships:** `-behaviour(gen_server)` → implements; `-include`/`-include_lib` as pending
  relationships where cross-file; `.hrl` fragments extract standalone without failing
- **Types:** minimal inference from `-spec`, same weight class as elixir's `types_inference.rs`
- **Test roles:** EUnit (`*_tests.erl`, `eunit` include) and Common Test (`*_SUITE.erl`) containers
  and cases

### XML — data-language tier

- **Extensions:** `.xml`, `.xsd`, `.wsdl` (deliberately excluded for now: `.svg`, `.csproj`-family
  .NET build XML, `.plist` — each needs distinct noise/domain treatment; revisit on demand)
- **Grammar:** `tree-sitter-xml` 0.7.0 (2024). **Phase-0 risk check:** confirm the generated
  parser loads under tree-sitter 0.26.11 before any extractor work
- **Capabilities:** `DATA_ONLY_CAPABILITIES` (symbols + identifiers; no relationships, no types).
  Identifiers are genuinely emitted, not vacuously claimed: attribute-value QName references
  (`type=`, `ref=`, `base=`, `element=` in schema documents) become identifier rows — the raw
  material for later schema-type relationship resolution.
- **Module:** `crates/julie-extractors/src/xml/` — a **hybrid model**: yaml's parent-chain
  nesting (`src/yaml/mod.rs`) plus html's element-filtering discipline
  (`src/html/elements.rs`) so generic repeated elements (`<item>`, `<row>`) don't flood symbols
- **Symbols:** name-promoted elements only — `<xs:complexType name="AddPhone">` emits symbol
  `AddPhone`; elements with `name`/`id` attributes are referenceable, anonymous structural
  elements are not symbol-worthy
- **Structural facts:** document-structure facts following the JSON/YAML/TOML document-family
  registry conventions, with schema-aware facts for `.xsd`/`.wsdl`: types, elements,
  imports/includes, and WSDL services/operations/messages/bindings
- **Deferred with honest gap entries:** resolved schema-type relationships recorded as
  `open_gaps` entries with reason, closure, and planned task (capability matrix requires those
  fields)

### Large-XML decision (recorded)

The `MAX_SOURCE_FILE_BYTES` 1MB cap **stands unchanged, including for XML**. Rationale: >1MB XML is
almost always machine-generated (codegen-input schemas like the AXL XSDs, data dumps). The
reporter's own XSDs feed JAXB codegen whose generated Java is already fully indexed — symbol-
indexing the schema would duplicate those names without usage context, while bloating FTS, BM25,
and the semantic embedding pipeline (a 15MB XSD could emit ~10^5 symbols). Oversized files are
recorded visibly as skipped-too-large. No per-language cap override, no tiered extraction.

Corrections and additions from review:

- **Escape hatch, stated precisely:** Miller's *automatic* source corpus also skips >1MB files
  (`ContentCorpusWriter`), so ordinary `search mode=source` cannot see a 15MB XSD. The real
  escape hatch is Miller's **manual `content import`** (25MB default cap,
  `ContentCorpusExternalStore`). The issue-#8 reply must say that, not "text search covers it."
- **Oversized-transition policy:** today, if an indexed file later grows past 1MB, `update`
  reports `no_change` and **preserves its stale rows** (`julie-extract-cli/src/commands.rs`
  no_change path). This design changes that policy for all languages: a tracked file that
  transitions to oversized has its rows removed and is recorded as skipped-too-large. Boundary
  tests at exactly 1MB, 1MB+1, and the indexed-then-oversized update case.
- **Cardinality control:** the byte cap alone doesn't bound symbol noise — a 999KB minified XML
  file can still hold tens of thousands of elements. The element-filtering rule (name-promoted
  only) is the primary control; add a worst-case fixture asserting bounded symbol cardinality on
  a dense sub-1MB document.

## Validation gates

1. **Phase 0:** both grammar crates compile and a smoke parse succeeds under
   `tree-sitter =0.26.11`.
2. **Erlang acceptance:** scan the issue's exact corpus — hex.pm `telemetry` 1.3.0, `certifi`
   2.15.0, `unicode_util_compat` 0.7.1 sources. All `.erl`/`.hrl` files extract (0 unsupported,
   0 failed) with exported functions, records, and behaviours present. Golden fixtures require
   zero parse diagnostics; the real-world corpus gets an explicit committed diagnostic baseline
   (checksummed inputs, exact assertions) rather than a bare "0 failed" — the existing
   `real-world-smoke` tier does not cover this and is not relied on.
3. **XML acceptance:** representative sub-1MB `.xml`, `.xsd`, and `.wsdl` goldens (separate
   goldens per extension) extract with name-promoted symbols and QName identifiers; zero parse
   diagnostics on goldens; a >1MB file is skipped and recorded as too-large; boundary and
   oversized-transition tests pass; dense-document cardinality fixture passes.
4. **Repo gates — the full new-language checklist** (`docs/languages/new-language-checklist.md`):
   per-language tests, goldens, capability matrix, changed-path certification, `languages --json`,
   strict data-quality, default/contract/certification tiers, `cargo deny`, fmt, clippy,
   package-list, release preflight. "Default suite green" alone is insufficient — default tests
   exclude goldens/capability/certification tiers by design (`docs/testing-strategy.md`).
   `capabilities.json` regenerated; gap entries honest; grammar-freshness report clean
   (supplemental drift check, not a correctness gate).

## Delivery chain (full, in order)

1. **julie-extractors:** implement both languages, fixtures, goldens, capability snapshot,
   contract updates (structural-fact contract JSON, `EXTRACTION_CONTRACT_VERSION` review,
   36→38 assertions); merge.
2. **julie-extract release:** version bump (expected 2.21.0), publish the 4-target release matrix.
3. **Miller consumption:** bump `scripts/julie-pins.json`, re-run restore (build guard
   `VerifyPinnedJulieExtractVersion` enforces), fast + scale suites green.
4. **Docs, both repos:** supported-language count 36 → 38 — Miller README language list, public
   site, julie-extractors README/site; keep MCP tool-description char budgets intact.
5. **Issue #8 reply:** Erlang + XML shipped; AXL XSDs intentionally excluded as oversized
   generated artifacts (visible skip; manual `content import` is the escape hatch); note the
   original error strings don't originate from this toolchain.

Steps 2, 3 (pin-bump push), and 5 pause for explicit user approval per release/push boundaries.

## Effort estimate (agent terms)

- Erlang extractor + fixtures: 3–4 focused sessions (largest slice; the FULL-capabilities
  acceptance list is the cost driver)
- XML extractor + fixtures + oversized-transition policy change: 1–2 sessions
- Release + Miller pin bump + docs + issue reply: 1 session (plus human approvals and CI waits)

## Risks

- `tree-sitter-xml` is a 2024 crate; ABI or `tree-sitter-language` dependency mismatch with
  0.26.11 would force vendoring or a fork (Phase 0 catches this before deeper investment).
- Erlang preprocessor-heavy `.hrl` files may parse with ERROR nodes; extractor must degrade
  gracefully (emit what parses, record parse diagnostics) rather than fail-closed.
- The oversized-transition policy change touches the shared update path for all languages —
  small blast radius but needs its own contract tests.
- Capability snapshot discipline: every declared capability needs a registered fixture or an
  honest gap entry — budget fixture time accordingly.

## Acceptance criteria

- [ ] Phase-0 grammar compatibility proven for both crates
- [ ] `.erl`/`.hrl` route to Erlang at `FULL_CAPABILITIES` with the full coverage list
      (export_all, BIFs, fun refs vs calls, clause grouping, doc attributes on
      functions/types/callbacks); issue-corpus scan clean with committed baseline (gate 2)
- [ ] `.xml`/`.xsd`/`.wsdl` route to XML at `DATA_ONLY_CAPABILITIES` with name-promoted symbols
      AND QName identifiers; schema/WSDL structural facts registered end-to-end (gate 3)
- [ ] 1MB cap unchanged; oversized-transition removes stale rows; boundary + cardinality tests
- [ ] Full new-language checklist green for both languages; contract version reviewed;
      36→38 assertions updated
- [ ] julie-extract 2.21.0 released (4 targets), Miller pin bumped, both suites green
- [ ] Language-count docs updated in both repos (36 → 38)
- [ ] Issue #8 answered and closed
