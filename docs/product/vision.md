# Product Vision

`julie-extractors` should become the open-source reference implementation for
practical tree-sitter extraction across Julie's 34+ language set.

## Promise

Given a source tree, produce a versioned artifact that downstream tools can use
for code intelligence without owning language parser details.

## Users

- Code search and navigation tools.
- AI-agent indexing systems.
- Test impact and dependency analysis tools.
- Documentation and code audit tools.
- Any project that wants cross-language symbols, relationships, identifiers,
  type facts, annotations, literals, diagnostics, and capability metadata.

## Product Shape

- Primary output: SQLite database.
- Secondary output: JSONL export.
- Primary API: `julie-extract` CLI.
- Secondary API: Rust crate.
- Release output: versioned binaries, checksums, schema docs, contract docs, and
  migration notes.

## Product Quality Bar

- Language-agnostic design across the full supported set.
- Documented capability matrix with evidence.
- Stable error and status model.
- Fast default development loop.
- Slow gates isolated and intentional.
- No dependence on Julie MCP/server/daemon internals.

## Non-Goals

- Replacing Miller, Eros, or Julie's higher-level tools.
- Owning search ranking or embeddings.
- Owning long-running watcher services.
- Supporting every possible tree-sitter grammar before the current language set
  is clean and maintainable.
