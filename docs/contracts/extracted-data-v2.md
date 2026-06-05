# Extracted Data v2

This is the contributor-facing list of data `julie-extractors` tries to extract.
SQLite remains the primary contract; JSONL and reports expose the same facts in
consumer-friendly shapes.

## Capability Flags

Every language row has five capability flags:

| Flag | Meaning | Required evidence |
| --- | --- | --- |
| `symbols` | Named source entities such as functions, classes, modules, fields, tables, rules, headings, or data keys. | Golden fixture rows in `symbols`, plus kind coverage for supported symbol kinds. |
| `relationships` | Resolved source-to-target edges between extracted symbols. | Golden fixture rows in `relationships`, or an exception proving relationships are not a language-domain concept. |
| `pending_relationships` | Structured unresolved edges whose target may resolve outside the current file or after a later pass. | Golden fixture rows in `pending_relationships` and `structured_pending_relationships`, or an exception proving no deferred resolution exists. |
| `identifiers` | Usage sites such as calls, variable references, member access, type usage, aliases, or data references. | Golden fixture rows in `identifiers`, or an exception proving the language model uses symbols and relationships instead. |
| `types` | Type facts attached to symbols. | Golden fixture rows in `types`, or an exception proving the language has no static type surface worth modeling as type facts. |

`target_capabilities` says what the language should support. `capabilities`
says what the extractor currently emits and what tests must prove. A false
capability is not a failure when the matrix has a typed exception with evidence.
An open gap means the language is not complete for that capability.

## Artifact Row Domains

These row domains are part of the extraction product contract:

| Domain | What it captures | Produced when |
| --- | --- | --- |
| `artifact_metadata` | Artifact identity, schema versions, binary version, hashes, and fingerprints. | Every SQLite artifact. |
| `parser_inventory` | Parser package evidence by language. | Every scan/update artifact write. |
| `language_capabilities` | One row per language with target and actual capabilities. | Every scan/update artifact write and `languages --json`. |
| `language_capability_fixtures` | Golden fixture evidence for a capability snapshot row. | Every scan/update artifact write and JSONL export. |
| `language_capability_gaps` | Open gaps and exception rows with typed evidence. | Every scan/update artifact write when the snapshot declares rows. |
| `extraction_revisions` | Mutating operation history. | `scan`, `update`, and `delete`. |
| `revision_file_changes` | Per-file changes caused by a revision. | `scan`, `update`, and `delete`. |
| `files` | Root-relative file metadata, language, content hash, line count, and status. | Supported files represented in the artifact. |
| `symbols` | Named source entities with spans, optional body spans, signatures, docs, visibility, test-role flags, and metadata. | Languages with `symbols: true`. |
| `symbol_annotations` | Decorators, attributes, annotations, or equivalent markers attached to symbols. | Languages with syntax-level annotations or doc markers worth preserving. |
| `identifiers` | Usage sites with containing and resolved target symbol links when known. | Languages with `identifiers: true`. |
| `relationships` | Resolved symbol-to-symbol edges. | Languages with `relationships: true` when the target is known. |
| `pending_relationships` | Deferred relationship targets with terminal name, receiver, namespace, import context, and caller scope. | Languages with `pending_relationships: true` when resolution needs another file or pass. |
| `type_facts` | Resolved or inferred types for symbols. | Languages with `types: true`. |
| `type_argument_usages` | Generic or templated type argument usage sites attached to identifiers. | Languages with generic/type-argument syntax. |
| `type_arguments` | Normalized nested type argument names for a usage. | Each `type_argument_usage` with one or more arguments. |
| `literals` | String or scalar literals that carry URLs, SQL, or configured language-specific facts. Route is reserved until route carriers are explicitly configured. | Languages with configured literal carriers or useful literal semantics. |
| `source_regions` | Source spans for comments, doc comments, string literals, and embedded language regions. | Languages with supported source-region node kinds. |
| `parse_diagnostics` | Tree-sitter parse errors and missing-node diagnostics in stable row form. | Supported files with parser diagnostics that should be exposed to consumers. |

## Support Labels

Use these labels in docs, reviews, and capability audits:

- **Full for declared capabilities**: every `target_capabilities` true flag is
  also true in `capabilities`, has fixture evidence, has no open
  `capability_gaps`, and has kind coverage for the language constructs the
  extractor claims.
- **Domain-limited**: one or more capability flags are false because the language
  does not have that source concept. The row must have an `exception` with typed
  evidence and a concrete reason.
- **Partial**: at least one language-domain construct is intentionally open in
  `capability_gaps` or `kind_coverage.*.open_gaps`.
- **Broken**: a declared capability is true but the golden, capability, language,
  or contract gate fails.

Do not call a language "fully supported" unless it is full for declared
capabilities. Say "domain-limited" or "partial" when that is the honest claim.

## Evidence Rules

- A capability claim needs a golden fixture that exercises the claim.
- A cross-file or deferred-resolution claim needs `pending_relationships` and
  `structured_pending_relationships` fixture rows.
- An exception must describe an intrinsic language limitation or documented
  parser limitation. It cannot hide missing implementation.
- `kind_coverage.open_gaps` rows need a reason, required closure, and named
  closure task.
- A language variant is a separate row when it has separate parsing or extraction
  behavior. Current examples: `tsx`, `jsx`, and `vue`.
- Runtime smoke is not enough. If a function returns data, the test must assert
  the actual rows or fields.

## Source Of Truth

- Registry and parser facts: `crates/julie-extractors/src/language_spec/`
- Extractor wiring: `crates/julie-extractors/src/registry.rs`
- Capability evidence: `fixtures/extraction/capabilities.json`
- Golden fixture rules: `fixtures/extraction/README.md`
- SQLite row contract: `docs/contracts/sqlite-schema-v2.md`
- JSONL row contract: `docs/contracts/jsonl-v2.md`
- Report contract: `docs/contracts/reports.md`
