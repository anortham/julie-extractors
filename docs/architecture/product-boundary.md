# Product Boundary

## Boundary

`julie-extractors` owns extraction only:

```text
filesystem input -> parser/extractor engine -> versioned artifact
```

The artifact can be consumed by any runtime. Downstream tools own search,
ranking, graph traversal, editing, UI, daemon lifecycle, and application policy.

## Caller-Facing Interfaces

- `julie-extract` CLI commands.
- SQLite schema and metadata rows.
- JSONL export records.
- JSON report shape for command results.
- Machine-readable error codes.
- Capability snapshot by language.
- Rust crate API for in-process use.

## Internal Implementation

- Tree-sitter parser dependencies.
- Per-language extractor modules.
- Normalization pipeline.
- Fixture and capability gates.
- Schema writer and migration code.
- Exporters.

Callers should not need to know parser crate names, grammar node names,
per-language extractor quirks, or Julie's old database internals.

## Data Flow

```text
scan/update/delete request
  -> canonicalize root and paths
  -> detect supported files
  -> parse with tree-sitter
  -> extract canonical rows
  -> normalize spans, paths, IDs, diagnostics
  -> write SQLite transaction
  -> report status and counts
```

JSONL export reads the same canonical data. It is not a separate extraction
path.

## Versioning

Artifacts must record:

- extractor binary version
- extraction contract version
- SQLite schema version
- JSONL schema version when exported
- parser crate versions
- hash algorithm
- canonical root
- supported language/capability snapshot version

## Architecture Quality

- **Affected modules:** extractor crate, extraction CLI, schema writer, export
  formats, fixtures, parser certification, release packaging.
- **Caller-facing interface:** CLI plus versioned artifacts.
- **Depth/locality:** parser complexity stays inside this repo; downstream
  tools consume stable data.
- **Test surface:** CLI contracts, artifact schemas, fixture gates, downstream
  smoke consumers.
- **Rejected shortcut:** copying Julie's current schema as the permanent product
  contract.
- **Architecture risk:** medium-high. The code boundary is clear; the risk is
  release discipline and test-suite control.
