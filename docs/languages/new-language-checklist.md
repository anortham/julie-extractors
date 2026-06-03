# New Language Implementation Checklist

Use this checklist when adding a language, adding a language variant, or
upgrading a language claim from partial/domain-limited to fuller support.

## 1. Define The Public Claim

- Pick the canonical language name. Use lower-case snake case or the existing
  registry style.
- Decide whether this is a separate language row or an alias. Separate parsing or
  extraction behavior needs a separate row.
- Record parser package, file extensions, doc-comment styles, and dependency
  status.
- Decide the five capability flags: `symbols`, `relationships`,
  `pending_relationships`, `identifiers`, and `types`.
- Write down any domain-limited false capabilities before coding. Each one needs
  a typed exception in `fixtures/extraction/capabilities.json`.

## 2. Wire The Parser And Registry

- Add the parser dependency in `crates/julie-extractors/Cargo.toml`.
- Add the parser function and `LanguageSpec` row in
  `crates/julie-extractors/src/language_spec/`.
- Add the extractor module under `crates/julie-extractors/src/<language>/`.
- Export the module from `crates/julie-extractors/src/lib.rs` if the local module
  pattern requires it.
- Add the extraction entry point in `crates/julie-extractors/src/registry.rs`.
- Make sure `supported_languages()` and `capabilities_for_language()` see the new
  language.

## 3. Implement Extraction By Data Domain

Implement every domain the language claims:

- `symbols`: names, kinds, spans, signatures, doc comments, visibility, parent
  symbols, body spans, body hashes, semantic groups, content type, and test-role
  flags when applicable.
- `relationships`: resolved calls, imports, extends/implements, uses, embeds, or
  language-specific edges that point to known symbols.
- `pending_relationships`: structured unresolved edges for cross-file or
  deferred resolution, including terminal name, display name, namespace, receiver,
  import context, and caller scope.
- `identifiers`: calls, references, member access, type usage, aliases, selectors,
  and data references when the language has them.
- `types`: explicit and inferred type facts for symbols when the language has a
  type surface.
- `type_argument_usages` and `type_arguments`: generic/template/type-parameter
  uses when the language has them.
- `symbol_annotations`: decorators, attributes, annotations, or equivalent
  markers.
- `literals`: route, URL, SQL, or other configured literal carrier facts.
- `parse_diagnostics`: stable diagnostics for parse errors or missing nodes when
  they should be visible to downstream consumers.

Do not set a capability to true because the extractor returns an empty vector for
that domain. True means useful rows are emitted and verified.

## 4. Add Capability Matrix Evidence

Update `fixtures/extraction/capabilities.json`:

- Add exactly one row for the registry language.
- Keep `extensions` and `parser_crate` identical to `LanguageSpec`.
- Set `target_capabilities` to the honest product target.
- Set `capabilities` to the current tested extractor behavior.
- Add fixture entries for every advertised capability.
- Fill `kind_coverage.symbols`, `kind_coverage.relationships`,
  `kind_coverage.identifiers`, and `kind_coverage.body_spans`.
- Add `capability_gaps` only for real open gaps or true domain/parser
  exceptions. Every row needs typed evidence.

## 5. Add Golden Fixtures

Add fixtures under `fixtures/extraction/<language>/`.

Required cases:

- `basic`: proves the core symbol model and the highest-value identifiers,
  relationships, types, annotations, literals, and type arguments that apply.
- `cross_file`: required when `relationships` or `pending_relationships` can
  cross file boundaries. It must prove both resolved and structured pending
  shapes when the language claims deferred resolution.
- Variant-specific cases: required for parser variants such as JSX, TSX, Vue, or
  embedded-language formats.

Each fixture needs:

- `source`: source input used by the golden harness.
- `expected`: normalized output with assertions for the claimed rows.
- No smoke-only expected data. Empty arrays are valid only when the capability is
  false or the specific fixture is proving absence.

## 6. Add Focused Tests

Add narrow tests under `crates/julie-extractors/src/tests/<language>/`.

Required tests when applicable:

- symbol extraction by kind and parent/child shape
- doc comments and annotations
- body spans and body hashes
- relationship extraction
- pending relationship shape
- identifier extraction
- type facts
- type argument usage
- literals and literal carriers
- test detection
- parser error behavior
- edge cases that previously broke or are common in the language

Every test must assert returned values. Do not add no-assertion tests, smoke-only
tests, placeholder tests, or tests that only prove the extractor does not panic.

## 7. Verify Before Claiming Support

Run these gates:

```bash
cargo xtask test language <language>
cargo xtask test golden
cargo xtask test capability
cargo xtask test changed crates/julie-extractors/src/language_spec/specs.rs fixtures/extraction/capabilities.json
cargo run -p julie-extract-cli -- languages --json
```

If parser dependencies changed, run the certification gate:

```bash
cargo xtask test certification
```

If the language is important for release confidence or has weak fixture coverage,
run the real-world smoke gate:

```bash
cargo xtask test real-world-smoke
```

## 8. Update Public Docs

- Update `README.md` only when the human-facing summary needs to change.
- Update `docs/contracts/extracted-data-v2.md` when a new row domain or support
  label is introduced.
- Update `docs/contracts/sqlite-schema-v2.md`, `docs/contracts/jsonl-v2.md`, and
  `docs/contracts/reports.md` before changing artifact or report shape.
- Update `docs/testing-strategy.md` before changing test-tier behavior.
- Record major architecture or product decisions under `docs/decisions/`,
  `docs/architecture/`, or `docs/plans/`.

## 9. Review The Claim

Before merging, answer these review questions:

- Does `languages --json` match the intended capability claim?
- Does every true capability have fixture evidence?
- Does every false capability have either a domain reason or a planned closure?
- Are variant languages tested independently?
- Are parser-specific limitations documented as evidence, not hidden in prose?
- Would a non-Rust consumer understand the artifact rows without knowing
  tree-sitter internals?
- Did default tests stay fast?
