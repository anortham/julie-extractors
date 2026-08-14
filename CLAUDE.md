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

## Data Quality Bar

The product goal is best-in-class tree-sitter extraction quality, not parity with
older Julie coverage. Do not lower the bar to existing weak coverage or mark a
real extractor gap as complete.

- Capability claims must be backed by golden fixture evidence and recorded in
  `fixtures/extraction/capabilities.json`.
- Positive support for a domain means the extractor emits useful rows for that
  domain, not just that the parser exposes matching syntax.
- `not_applicable` is valid only when the language genuinely lacks the
  construct. Missing implementation is `open_gaps` debt until fixed.
- `open_gaps` entries must include a concrete reason, required closure, and
  planned closure task.
- General-purpose languages should aim for rich symbols, body spans, body
  hashes, doc comments, relationships, identifiers, type facts, type argument
  usages, literals, source regions, complexity metrics, annotations, and
  structural facts where the grammar supports them.
- Data, markup, query, and domain-specific languages should extract their own
  semantics deeply: schema structure, links, selectors, bindings, routes,
  anchors, imports, queries, DDL/DML/procedure structure, embedded languages,
  and comparable domain-native facts.
- After capability or fixture changes, run
  `node scripts/language-data-quality-report.mjs --strict` and keep
  `silent_cells` and `quality_bar_debts` at `0`.

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

## Windows Compatibility

Windows is a first-class target. Every release ships a Windows binary, and a
long series of real Windows defects has already been fixed here. Check Windows
behavior explicitly whenever work touches paths, file lifecycles, process
supervision, durability, or platform-gated tests.

Pitfalls this repo has already hit:

- `Path::join` inserts the platform separator. Contract outputs that specify
  `/` (reports, JSONL, diagnostics paths) must join with an explicit `/`,
  never with `Path::join`.
- `std::fs::canonicalize` returns verbatim paths (`\\?\C:\...`,
  `\\?\UNC\...`) on Windows. Strip the prefix before building URIs or doing
  string work on paths. Tests must canonicalize fixture paths the way
  production code does, or they never see the prefix.
- Windows cannot delete or rename a file while another handle is open, and
  SQLite opens without `FILE_SHARE_DELETE`. Close connections and handles
  before unlink or rename. In `Drop` impls the `drop` body runs before the
  fields drop, so close explicitly before any cleanup that unlinks.
- Metadata writes need write-access handles. `File::open` is read-only, so
  `sync_all` (`FlushFileBuffers`) and timestamp updates fail with "Access is
  denied". Open with `OpenOptions::new().write(true)`.
- Directories cannot be opened as files, so Unix-style directory fsync does
  not exist on Windows. Use the shared store sync helpers instead of raw
  `File::open(dir)?.sync_all()`.
- Path text is not file identity. Case folding, hard links, and verbatim
  spellings all alias the same file. Compare files by handle identity
  (`same-file`), not by path string.
- There is no cheap pid liveness probe. `tasklist` costs ~100 ms per call, so
  memoize verdicts on hot retry loops. `--parent-pid` supervision is
  Unix-only by contract: accepted and ignored elsewhere.
- Gate Unix-only test assertions with `#[cfg(unix)]` and assert the
  documented per-platform contract on every platform. `cargo test` stops at
  the first failing target, so one Windows failure can mask others.

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
