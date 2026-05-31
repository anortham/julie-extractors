# julie-extractors Development Guidelines

## Sync Contract

`AGENTS.md` and `CLAUDE.md` must stay byte-for-byte equivalent except for
future tool-specific sections that are explicitly labeled. When one changes,
update the other in the same commit.

## Product Boundary

This repo owns the extraction product:

```text
source tree -> versioned extraction artifact
```

The CLI and artifact contracts are first-class product APIs. The Rust crate is
important, but it is not the only interface. Assume downstream consumers may be
written in C#, Python, Go, JavaScript, or anything else that can spawn a binary
and read SQLite or JSONL.

## Core Rules

- SQLite is the primary durable output.
- JSONL is the secondary export/streaming output.
- `julie-extract` is the primary integration surface.
- Rust in-process APIs are secondary and must not force non-Rust callers to know
  tree-sitter or Julie internals.
- Do not add MCP server, daemon, search, embedding, watcher-service, dashboard,
  or editing-tool behavior here.
- `/Users/murphy/source/julie` is maintenance-only while this product is built.
  Do not back-port new extractor features into Julie unless explicitly asked.

## Test Discipline

The default test suite must stay fast enough for agents to run repeatedly.

- Default tests must not run real-world corpora, full parser certification, or
  slow release gates.
- Any slow test must be tagged or routed out of the default suite from the start.
- Add a wall-clock budget tripwire before the suite grows.
- Add convention tests that fail if slow gates leak into default.
- Per-language work must have narrow commands so agents can test one language
  without paying for all languages.

## Design Discipline

- Treat schemas, JSON reports, CLI exit codes, error codes, and capability rows
  as API contracts.
- Prefer clean new contracts over compatibility modes while this repo is not yet
  consumed in production.
- If a shortcut preserves old Julie coupling, reject it unless the user
  explicitly chooses it.
- Negative claims about unsupported languages or unavailable features require
  source verification.

## Documentation

Every major product or architecture decision should be captured in:

- `docs/decisions/`
- `docs/architecture/`
- `docs/plans/`

Keep docs concise but concrete. No placeholders, no "later" sections without a
named plan.

Before committing guideline changes, run:

```bash
scripts/check-agent-doc-sync.sh
```
