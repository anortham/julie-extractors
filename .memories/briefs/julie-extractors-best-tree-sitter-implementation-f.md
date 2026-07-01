---
id: julie-extractors-best-tree-sitter-implementation-f
title: julie-extractors Best Tree-Sitter Implementation Focus
status: completed
created: 2026-06-10T11:32:05.902Z
updated: 2026-07-01T22:46:01.292Z
tags:
  - julie-extractors
  - language-quality
  - tree-sitter
  - product-strategy
---

## Goal

Make `julie-extractors` the best available tree-sitter extraction implementation for the supported language set: deep, AST-backed, fixture-proven semantic data across the board, not skeleton support with documented caveats.

## Why Now

The repo already has downstream projects depending on it, and branch `feature/extraction-data-quality` exposed that a few languages are rich while many others still have shallow or missing domains. The next work must raise extractor quality broadly rather than normalize the current uneven bar.

## Constraints

- Product boundary remains `source tree -> versioned extraction artifact`.
- SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.
- Do not add MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior here.
- Do not use `not_applicable` to hide ordinary code-language gaps. It is valid only when the language genuinely lacks the construct.
- `open_gaps` are temporary implementation debt with named closure tasks.
- Positive capability claims require fixture evidence and should be backed by language tests.

## Success Criteria

- Code languages converge toward rich symbols, body spans, signatures, visibility, doc comments, relationships, identifiers, type data, literals, source regions, complexity metrics, annotations, and useful structural facts wherever the grammar supports them.
- Data, markup, query, and domain languages expose their own semantics deeply instead of being treated as low-value skeletons.
- Language quality is proven with golden fixtures, focused tests, capability matrix checks, and downstream dogfood scans.
- Remaining limitations have language-semantics justification or concrete closure tasks.

## References

- `docs/findings/2026-06-09-language-coverage-review.md`
- `docs/plans/2026-06-10-language-data-quality.md`
- `docs/decisions/0003-domain-coverage-via-kind-coverage.md`
