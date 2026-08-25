# Extracted Data Contract v4

Extraction contract 4 now pairs with [SQLite schema 7](sqlite-schema-v7.md)
and [JSONL contract 5](jsonl-v5.md). Schema 5/6 and JSONL 4 remain historical
pairings.

> **2026-08-18 retirement:** this product no longer writes workspace-global
> reference resolution. Identifier `target_symbol_id` is not stored on the
> artifact. Miller computes resolution at query time. See
> [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

## Reference evidence

- `identifier`, `relationship`, and `pending_relationship` evidence carries one
  `reference_site_id`.
- Exact sites use the producer-owned target-token byte span. Identifiers are parser-emitted token
  occurrences. Relationships and structured pending rows are exact only when their producer calls
  an explicit target-token constructor.
- Broad AST nodes are never upgraded to exact sites by name, line, overlap, containment, or nearest
  token. They emit row-specific `is_exact=false`, `provenance=spanless` sites.
- Current audited exact relationship/pending paths cover C calls and type uses, Bash command calls,
  PowerShell command calls, and Python calls. Other provider paths remain valid, explicit spanless
  evidence until their target-token nodes are audited.
- Same file and byte span means the same physical site across evidence row families. Different
  byte spans on one line remain different sites.

Assertions remain separate evidence rows. No assertion table is added. Consumers group exact
assertions by `(reference_site_id, target_symbol_id, canonical_kind)` and unresolved assertions by
`(reference_site_id, target_name, canonical_kind)`.

## Unsupported files are recorded, not parsed

A `scan` records every file the discovery walk reached and dropped because no
extractor claims the extension. Each one gets a `files` row with
`status = 'unsupported'`, `language = 'unsupported'`, a content hash, a byte
count, and no `line_count`. The file is read once for its hash and is never
decoded or parsed, so it produces no symbols and no other fact rows.

The change journal follows from that row: the first scan that sees the path
writes an `unsupported` change, a later scan writes another one only when the
content hash changes, and a scan that no longer sees the path writes a
`deleted` change. A consumer reading the journal can therefore account for
every path the walk visited instead of treating it as never seen.

Paths an ignore rule, a hard exclusion, or the source-size limit removed are
absent from the artifact, as before. So are the artifact's own SQLite
companion files (`-wal`, `-shm`, `-journal`).

## Capability gaps

Gap status is the closed vocabulary `open | exception`. Unknown statuses are
invalid. Workspace-global reference-resolution coverage is no longer a
julie-extract capability claim. Miller owns that policy.
